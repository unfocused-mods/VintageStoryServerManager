//! Public Azure retail compute estimates, independent of subscription/ARM discovery.
use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Price {
    pub hourly: f64,
    pub quoted_at: String,
}
type Entry = Arc<tokio::sync::Mutex<Option<(Instant, std::result::Result<Price, String>)>>>;
static CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, Entry>>> = OnceLock::new();
static LIMIT: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub async fn lookup(region: &str, size: &str, fresh: bool) -> Result<Price> {
    let normalized_region = region.to_ascii_lowercase();
    let region = normalized_region.as_str();
    anyhow::ensure!(
        [region, size].iter().all(|s| !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')),
        "Invalid Azure region or size"
    );
    if fresh {
        return bounded_fetch(region, size).await;
    }
    let key = format!("{region}:{size}");
    let entry = {
        let mut cache = CACHE.get_or_init(Default::default).lock().await;
        // Bound memory even when callers request arbitrary (validly shaped) SKUs.
        if cache.len() >= 512 && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.entry(key).or_default().clone()
    };
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut entry = entry.lock().await;
        if let Some((at, result)) = entry.as_ref() {
            let ttl = if result.is_ok() { 900 } else { 30 };
            if at.elapsed() < Duration::from_secs(ttl) {
                return result.clone().map_err(anyhow::Error::msg);
            }
        }
        let result = bounded_fetch(region, size).await.map_err(|e| e.to_string());
        *entry = Some((Instant::now(), result.clone()));
        result.map_err(anyhow::Error::msg)
    })
    .await
    .context("Azure price lookup timed out; retry shortly")?
}

async fn bounded_fetch(region: &str, size: &str) -> Result<Price> {
    tokio::time::timeout(Duration::from_secs(12), async {
        let _permit = LIMIT.get_or_init(|| tokio::sync::Semaphore::new(8)).acquire().await?;
        let client = CLIENT.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::none()).build().expect("valid retail HTTP client"));
        let filter=format!("serviceName eq 'Virtual Machines' and armRegionName eq '{region}' and armSkuName eq '{size}' and priceType eq 'Consumption'");
        let mut url=reqwest::Url::parse("https://prices.azure.com/api/retail/prices")?;
        url.query_pairs_mut().append_pair("$filter", &filter).append_pair("currencyCode", "'USD'");
        let mut price: Option<f64> = None;
        for _ in 0..5 {
            let mut response = client.get(url.clone()).send().await?.error_for_status()?;
            anyhow::ensure!(response.status().is_success(), "Azure price endpoint redirected unexpectedly");
            let mut bytes=Vec::new();
            while let Some(chunk)=response.chunk().await? {
                anyhow::ensure!(bytes.len()+chunk.len() <= 2*1024*1024,"Azure price response too large");
                bytes.extend_from_slice(&chunk);
            }
            let data:Value=serde_json::from_slice(&bytes)?;
            for row in data["Items"].as_array().context("Invalid Azure price response")? {
                if let Some(value)=regular_linux_price(row,region,size) { price=Some(price.map_or(value,|old|old.min(value))); }
            }
            let next=data["NextPageLink"].as_str().unwrap_or("");
            if next.is_empty() {return Ok(Price {hourly:price.context("No regular Linux retail price available for this size and region")?,quoted_at:crate::domain::now()});}
            url=reqwest::Url::parse(next)?;
            anyhow::ensure!(url.scheme()=="https" && url.host_str()==Some("prices.azure.com") && url.port().is_none() && url.username().is_empty() && url.password().is_none() && url.path()=="/api/retail/prices" && url.fragment().is_none(),"Untrusted Azure pricing continuation URL");
        }
        anyhow::bail!("Azure pricing pagination exceeded its limit; retry shortly")
    }).await.context("Azure price lookup timed out; retry shortly")?
}

fn regular_linux_price(row: &Value, region: &str, size: &str) -> Option<f64> {
    let product = row["productName"].as_str()?.to_ascii_lowercase();
    let sku = row["skuName"].as_str()?.to_ascii_lowercase();
    if row["armRegionName"] != region
        || row["armSkuName"] != size
        || row["type"] != "Consumption"
        || row["currencyCode"] != "USD"
        || row["unitOfMeasure"] != "1 Hour"
        || row["isPrimaryMeterRegion"] != true
        || !product.starts_with("virtual machines")
        || ["windows", "sql", "red hat", "rhel", "suse", "ubuntu pro"]
            .iter()
            .any(|s| product.contains(s))
        || sku.contains("spot")
        || sku.contains("low priority")
    {
        return None;
    }
    row["retailPrice"]
        .as_f64()
        .filter(|p| p.is_finite() && *p >= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_regular_linux_meter_only() {
        let row = serde_json::json!({"productName":"Virtual Machines Dsv5 Series","skuName":"D2s v5","armSkuName":"Standard_D2s_v5","armRegionName":"eastus","type":"Consumption","currencyCode":"USD","unitOfMeasure":"1 Hour","isPrimaryMeterRegion":true,"retailPrice":0.096});
        assert_eq!(
            regular_linux_price(&row, "eastus", "Standard_D2s_v5"),
            Some(0.096)
        );
        for (key, value) in [
            ("armRegionName", serde_json::json!("westus")),
            ("armSkuName", serde_json::json!("Standard_D4s_v5")),
            ("type", serde_json::json!("Reservation")),
            ("currencyCode", serde_json::json!("EUR")),
            ("skuName", serde_json::json!("D2s v5 Spot")),
            ("skuName", serde_json::json!("D2s v5 Low Priority")),
            (
                "productName",
                serde_json::json!("Virtual Machines Dsv5 Series Windows"),
            ),
            ("retailPrice", serde_json::json!(-1)),
            ("isPrimaryMeterRegion", serde_json::json!(false)),
        ] {
            let mut invalid = row.clone();
            invalid[key] = value;
            assert!(regular_linux_price(&invalid, "eastus", "Standard_D2s_v5").is_none());
        }
    }
    #[tokio::test]
    async fn cached_price_keeps_timestamp_and_does_not_request_credentials() {
        let key = "testregion:Standard_Test".to_string();
        CACHE.get_or_init(Default::default).lock().await.insert(
            key,
            Arc::new(tokio::sync::Mutex::new(Some((
                Instant::now(),
                Ok(Price {
                    hourly: 0.1,
                    quoted_at: "original".into(),
                }),
            )))),
        );
        let price = lookup("testregion", "Standard_Test", false).await.unwrap();
        assert_eq!(price.quoted_at, "original");
        assert_eq!(price.hourly, 0.1);
        assert!(lookup("eastus' or true", "Standard_Test", false)
            .await
            .is_err());
    }
    #[tokio::test]
    #[ignore = "Read-only public Azure retail API timing; no cloud credentials"]
    async fn live_public_price() {
        let start = Instant::now();
        let first = lookup("eastus", "Standard_D2s_v5", false).await.unwrap();
        println!(
            "Public retail uncached: {:?}, USD {} per hour",
            start.elapsed(),
            first.hourly
        );
        let start = Instant::now();
        let cached = lookup("eastus", "Standard_D2s_v5", false).await.unwrap();
        println!("Cached retail: {:?}", start.elapsed());
        assert_eq!(first.quoted_at, cached.quoted_at);
        assert_eq!(first.hourly, cached.hourly);
    }
}
