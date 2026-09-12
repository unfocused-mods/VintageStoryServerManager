use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::LazyLock;
type Cache = Option<(std::time::Instant, Value)>;
static CATALOG: LazyLock<tokio::sync::Mutex<Cache>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));
fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 4
                && attempt.url().scheme() == "https"
                && matches!(
                    attempt.url().host_str(),
                    Some("mods.vintagestory.at" | "moddbcdn.vintagestory.at")
                )
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()?)
}
async fn limited(url: &str, limit: usize) -> Result<Vec<u8>> {
    let mut r = client()?.get(url).send().await?.error_for_status()?;
    let mut bytes = vec![];
    while let Some(c) = r.chunk().await? {
        anyhow::ensure!(
            bytes.len() + c.len() <= limit,
            "ModDB response exceeds size limit"
        );
        bytes.extend_from_slice(&c);
    }
    Ok(bytes)
}
pub async fn search(query: &str) -> Result<Value> {
    anyhow::ensure!(query.len() <= 100, "Search query too long");
    let mut cache = CATALOG.lock().await;
    if cache
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed().as_secs() > 3600)
    {
        let data: Value = serde_json::from_slice(
            &limited("https://mods.vintagestory.at/api/mods", 16 * 1024 * 1024).await?,
        )?;
        anyhow::ensure!(data["mods"].is_array(), "ModDB catalog unavailable");
        *cache = Some((std::time::Instant::now(), data));
    }
    let query = query.to_lowercase();
    let data = &cache.as_ref().unwrap().1;
    let results: Vec<_> = data["mods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            query.is_empty()
                || format!(
                    "{} {}",
                    m["name"].as_str().unwrap_or(""),
                    m["summary"].as_str().unwrap_or("")
                )
                .to_lowercase()
                .contains(&query)
        })
        .take(60)
        .cloned()
        .collect();
    Ok(json!({"mods":results,"limit":60}))
}
pub async fn detail(id: &str) -> Result<Value> {
    anyhow::ensure!(
        !id.is_empty()
            && id.len() < 100
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Invalid mod identifier"
    );
    let data: Value = serde_json::from_slice(
        &limited(
            &format!("https://mods.vintagestory.at/api/mod/{id}"),
            4 * 1024 * 1024,
        )
        .await?,
    )?;
    anyhow::ensure!(data["mod"].is_object(), "Mod not found");
    Ok(data["mod"].clone())
}
pub async fn archive(id: &str, release: u64) -> Result<(Value, Vec<u8>)> {
    let data = detail(id).await?;
    let release = data["releases"]
        .as_array()
        .context("No releases")?
        .iter()
        .find(|r| r["releaseid"] == release)
        .context("Release not found")?
        .clone();
    let url = reqwest::Url::parse(release["mainfile"].as_str().context("No release file")?)?;
    anyhow::ensure!(
        url.scheme() == "https"
            && matches!(
                url.host_str(),
                Some("moddbcdn.vintagestory.at" | "mods.vintagestory.at")
            ),
        "Untrusted mod download origin"
    );
    let bytes = limited(url.as_str(), 64 * 1024 * 1024).await?;
    Ok((release, bytes))
}
