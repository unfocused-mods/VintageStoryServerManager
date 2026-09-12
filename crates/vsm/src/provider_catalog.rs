// Interactive catalogs use a short-lived per-account cache. Deployment checks bypass it.
type CatalogEntry = std::sync::Arc<tokio::sync::Mutex<Option<(std::time::Instant, Value)>>>;
static CATALOG_CACHE: std::sync::OnceLock<
    tokio::sync::Mutex<std::collections::HashMap<String, CatalogEntry>>,
> = std::sync::OnceLock::new();
pub async fn invalidate_catalog(account_id: &str) {
    let prefix = format!(
        "{}:",
        serde_json::to_string(account_id).expect("string serialization")
    );
    if let Some(cache) = CATALOG_CACHE.get() {
        cache
            .lock()
            .await
            .retain(|key, _| !key.starts_with(&prefix));
    }
}

impl Cloud {
    pub async fn regions(&self) -> Result<Value> {
        anyhow::ensure!(
            self.account.provider == Provider::Azure,
            "Regional catalogs are available for Azure"
        );
        self.interactive_catalog(None).await
    }
    pub async fn sizes(&self, region: &str) -> Result<Value> {
        anyhow::ensure!(
            self.account.provider == Provider::Azure,
            "Regional catalogs are available for Azure"
        );
        segment(region)?;
        anyhow::ensure!(region.len() <= 128, "Invalid Azure region");
        self.interactive_catalog(Some(region)).await
    }
    async fn interactive_catalog(&self, region: Option<&str>) -> Result<Value> {
        // Include account settings/credential reference so edits cannot reuse another subscription's data.
        let mut key = format!(
            "{}:{}:{region:?}",
            serde_json::to_string(&self.account.id)?,
            serde_json::to_string(&self.account)?
        );
        #[cfg(test)]
        {
            key.push_str(&format!("{:?}", self.endpoint));
        }
        let entry = {
            let mut cache = CATALOG_CACHE.get_or_init(Default::default).lock().await;
            if cache.len() >= 128 && !cache.contains_key(&key) {
                cache.clear();
            }
            cache.entry(std::mem::take(&mut key)).or_default().clone()
        };
        tokio::time::timeout(std::time::Duration::from_secs(25), async {
            let mut entry = entry.lock().await;
            if let Some((at, value)) = entry.as_ref() {
                if at.elapsed() < std::time::Duration::from_secs(300) {
                    return Ok(value.clone());
                }
            }
            let sub = self.subscription()?;
            let value = if let Some(region) = region {
                // Azure documents location as the only supported Resource SKUs filter.
                let path = regional_skus_path(sub, region);
                let rows = self.azure_pages(&path).await?;
                let sizes: Vec<_> = rows
                    .into_iter()
                    .filter(|s| s["resourceType"] == "virtualMachines" && sku_in_region(s, region))
                    .collect();
                json!({"sizes":sizes,"region":region,"fetched_at":now()})
            } else {
                let response = self
                    .request(
                        Method::GET,
                        &format!("/subscriptions/{sub}/locations?api-version=2022-12-01"),
                        None,
                    )
                    .await?;
                let regions = response["value"]
                    .as_array()
                    .context("Invalid Azure regions response")?;
                json!({"regions":regions,"fetched_at":now()})
            };
            *entry = Some((std::time::Instant::now(), value.clone()));
            Ok(value)
        })
        .await
        .context("Azure catalog timed out; retry loading this region")?
    }
}
fn regional_skus_path(subscription: &str, region: &str) -> String {
    format!("/subscriptions/{subscription}/providers/Microsoft.Compute/skus?api-version=2021-07-01&$filter=location%20eq%20%27{region}%27")
}

fn sku_in_region(sku: &Value, region: &str) -> bool {
    sku["locations"].as_array().is_some_and(|locations| {
        locations.iter().any(|location| {
            location
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(region))
        })
    })
}
fn sku_region_restricted(sku: &Value, region: &str) -> bool {
    let Some(restrictions) = sku["restrictions"].as_array() else {
        return true;
    };
    restrictions.iter().any(|restriction| {
        match restriction["type"].as_str() {
            Some("Zone") => false, // These deployments do not request an availability zone.
            Some("Location") => {
                let locations: Vec<_> = [
                    restriction.get("values"),
                    restriction
                        .get("restrictionInfo")
                        .and_then(|info| info.get("locations")),
                ]
                .into_iter()
                .flatten()
                .filter_map(Value::as_array)
                .flatten()
                .collect();
                locations.is_empty()
                    || locations.iter().any(|value| {
                        value
                            .as_str()
                            .is_none_or(|name| name.eq_ignore_ascii_case(region))
                    })
            }
            _ => true,
        }
    })
}
