//! Indicative ECB daily reference rates, never account billing or transaction quotes.
use anyhow::{Context, Result};
use chrono::NaiveDate;
use quick_xml::{events::Event, Reader};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::OnceLock,
    time::{Duration, Instant},
};

const SOURCE: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
#[derive(Clone, Serialize)]
pub struct FxRates {
    pub base: String,
    pub rates: BTreeMap<String, f64>,
    pub as_of: Option<String>,
    pub fetched_at: String,
    pub source: String,
    pub status: String,
    pub error: Option<String>,
}
type Cache = tokio::sync::Mutex<Option<(Instant, FxRates)>>;
static CACHE: OnceLock<Cache> = OnceLock::new();

pub async fn rates() -> FxRates {
    let mut cache = CACHE.get_or_init(Default::default).lock().await;
    if let Some((time, value)) = cache.as_ref() {
        let ttl = if value.status == "current" { 3600 } else { 60 };
        if time.elapsed() < Duration::from_secs(ttl) {
            let mut result = value.clone();
            if result.status == "current"
                && !recent_date(result.as_of.as_deref(), chrono::Utc::now().date_naive())
            {
                result.status = "stale".into();
                result.error =
                    Some("The latest exchange rates are more than seven days old.".into());
            }
            return result;
        }
    }
    let result = match fetch().await {
        Ok(value) => value,
        Err(_) => unavailable(cache.as_ref().map(|(_, value)| value)),
    };
    *cache = Some((Instant::now(), result.clone()));
    result
}
fn unavailable(previous: Option<&FxRates>) -> FxRates {
    if let Some(previous) = previous.filter(|value| !value.rates.is_empty()) {
        let mut result = previous.clone();
        result.status = "stale".into();
        result.error = Some(
            "Exchange rates could not be refreshed. Showing original provider currencies.".into(),
        );
        return result;
    }
    FxRates {
        base: "EUR".into(),
        rates: BTreeMap::new(),
        as_of: None,
        fetched_at: crate::domain::now(),
        source: SOURCE.into(),
        status: "unavailable".into(),
        error: Some("Exchange rates are unavailable. Showing original provider currencies.".into()),
    }
}
async fn fetch() -> Result<FxRates> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut response = client.get(SOURCE).send().await?.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= 64 * 1024,
            "Exchange-rate response exceeds size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    parse_ecb(
        std::str::from_utf8(&bytes)?,
        chrono::Utc::now().date_naive(),
    )
}
fn recent_date(date: Option<&str>, today: NaiveDate) -> bool {
    date.and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        .is_some_and(|date| (0..=7).contains(&today.signed_duration_since(date).num_days()))
}
fn parse_ecb(xml: &str, today: NaiveDate) -> Result<FxRates> {
    let mut reader = Reader::from_str(xml);
    let mut date = None;
    let mut rates = BTreeMap::from([("EUR".into(), 1.0)]);
    loop {
        match reader.read_event()? {
            Event::Start(element) | Event::Empty(element)
                if element.local_name().as_ref() == b"Cube" =>
            {
                let mut currency = None;
                let mut rate = None;
                for attribute in element.attributes() {
                    let attribute = attribute?;
                    let value = std::str::from_utf8(attribute.value.as_ref())?;
                    match attribute.key.as_ref() {
                        b"time" => {
                            anyhow::ensure!(date.is_none(), "Multiple exchange-rate dates");
                            let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                                .context("Invalid exchange-rate date")?;
                            anyhow::ensure!(parsed <= today, "Exchange-rate date is in the future");
                            date = Some(value.to_owned());
                        }
                        b"currency" => currency = Some(value.to_owned()),
                        b"rate" => rate = Some(value.to_owned()),
                        _ => {}
                    }
                }
                if let Some(currency) = currency
                    .filter(|currency| ["USD", "AUD", "NZD", "EUR"].contains(&currency.as_str()))
                {
                    let rate: f64 = rate.context("Missing exchange rate")?.parse()?;
                    anyhow::ensure!(rate.is_finite() && rate > 0.0, "Invalid exchange rate");
                    anyhow::ensure!(!rates.contains_key(&currency), "Duplicate exchange rate");
                    rates.insert(currency, rate);
                }
            }
            Event::DocType(_) => anyhow::bail!("Unexpected XML document type"),
            Event::Eof => break,
            _ => {}
        }
    }
    anyhow::ensure!(date.is_some(), "Missing exchange-rate date");
    anyhow::ensure!(
        ["USD", "AUD", "NZD"]
            .iter()
            .all(|currency| rates.contains_key(*currency)),
        "Missing supported exchange rates"
    );
    let recent = recent_date(date.as_deref(), today);
    Ok(FxRates { base: "EUR".into(), rates, as_of: date, fetched_at: crate::domain::now(), source: SOURCE.into(), status: if recent {"current"} else {"stale"}.into(), error: (!recent).then(|| "The latest exchange rates are more than seven days old. Showing original provider currencies.".into()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(date: &str, usd: &str) -> String {
        format!("<Envelope><Cube><Cube time='{date}'><Cube currency='USD' rate='{usd}'/><Cube currency='AUD' rate='1.6'/><Cube currency='NZD' rate='2.0'/></Cube></Cube></Envelope>")
    }
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 6).unwrap()
    }
    #[test]
    fn friday_rates_remain_current_on_sunday_and_keep_reference_date() {
        let value = parse_ecb(&sample("2026-09-04", "1.2"), today()).unwrap();
        assert_eq!(value.status, "current");
        assert_eq!(value.as_of.as_deref(), Some("2026-09-04"));
        assert_eq!(value.rates["EUR"], 1.0);
        assert!((24.0 * value.rates["NZD"] / value.rates["USD"] - 40.0).abs() < 1e-10);
        assert!((10.0 * value.rates["AUD"] / value.rates["EUR"] - 16.0).abs() < 1e-10);
    }
    #[test]
    fn invalid_missing_duplicate_future_and_nonfinite_rates_are_rejected() {
        for rate in ["0", "-1", "NaN", "inf", "oops"] {
            assert!(parse_ecb(&sample("2026-09-04", rate), today()).is_err());
        }
        for date in ["invalid", "2026-09-07"] {
            assert!(parse_ecb(&sample(date, "1.2"), today()).is_err());
        }
        assert!(parse_ecb(
            &sample("2026-09-04", "1.2").replace("currency='NZD'", "currency='USD'"),
            today()
        )
        .is_err());
        assert!(parse_ecb(
            &sample("2026-09-04", "1.2").replace("currency='NZD'", "currency='JPY'"),
            today()
        )
        .is_err());
    }
    #[test]
    fn stale_and_unavailable_rates_never_become_current_fallbacks() {
        let old = parse_ecb(&sample("2026-08-20", "1.2"), today()).unwrap();
        assert_eq!(old.status, "stale");
        let failed = unavailable(Some(&old));
        assert_eq!(failed.status, "stale");
        assert_eq!(failed.as_of, old.as_of);
        assert_eq!(failed.fetched_at, old.fetched_at);
        let absent = unavailable(None);
        assert_eq!(absent.status, "unavailable");
        assert!(absent.rates.is_empty());
        assert!(absent.as_of.is_none());
    }
    #[tokio::test]
    #[ignore = "Fetches authoritative live ECB rates"]
    async fn live_ecb_rates() {
        let result = fetch().await.unwrap();
        assert_eq!(result.status, "current");
        assert_eq!(result.rates.len(), 4);
        println!(
            "ECB {}: EUR=1 USD={} AUD={} NZD={}",
            result.as_of.unwrap(),
            result.rates["USD"],
            result.rates["AUD"],
            result.rates["NZD"]
        );
    }
}
