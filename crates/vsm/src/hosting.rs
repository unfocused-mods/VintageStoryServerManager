//! Account-free planning prices. These never authorize provisioning or confirm capacity.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

type Cache = tokio::sync::Mutex<Option<(Instant, Value)>>;
static CACHE: OnceLock<Cache> = OnceLock::new();

pub async fn catalog() -> Result<Value> {
    let mut cache = CACHE.get_or_init(Default::default).lock().await;
    if let Some((time, value)) = cache.as_ref() {
        let ttl = if value["notices"].as_array().is_some_and(|n| n.is_empty())
            && value["fx"]["status"] == "current"
        {
            900
        } else {
            60
        };
        if time.elapsed() < Duration::from_secs(ttl) {
            return Ok(value.clone());
        }
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()?;
    let bounded = |future| async {
        tokio::time::timeout(Duration::from_secs(30), future)
            .await
            .context("Catalog timed out")?
    };
    let (vultr, azure, fx) = tokio::join!(
        bounded(Box::pin(vultr(&client))
            as std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Vec<Value>>> + Send>,
            >),
        bounded(Box::pin(azure(&client))
            as std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Vec<Value>>> + Send>,
            >),
        crate::fx::rates()
    );
    let mut offers = snapshots();
    let mut notices = vec![];
    for (provider, result) in [("Vultr", vultr), ("Microsoft Azure", azure)] {
        match result {
            Ok(rows) => offers.extend(rows),
            Err(_) => notices.push(format!(
                "{provider} live prices could not be loaded. Try again shortly."
            )),
        }
    }
    let value = json!({"offers": offers, "notices": notices, "fx":fx});
    *cache = Some((Instant::now(), value.clone()));
    Ok(value)
}

async fn get(client: &reqwest::Client, url: &str) -> Result<Value> {
    Ok(client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

// This constructor mirrors the fixed fields in the public catalog record.
#[allow(clippy::too_many_arguments)]
fn offer(
    provider: &str,
    region: &str,
    area: &str,
    size: &str,
    cpu: u64,
    memory: f64,
    price: f64,
    currency: &str,
    total: Option<f64>,
    source: &str,
    date: &str,
    live: bool,
    shared: bool,
    details: &str,
) -> Value {
    json!({"id":format!("{provider}:{region}:{size}"),"provider":provider,"region":region,"area":area,"size":size,"vcpus":cpu,"memory_gib":memory,"compute":price,"total":total,"currency":currency,"source":source,"checked_at":date,"live":live,"shared_cpu":shared,"details":details})
}

fn snapshots() -> Vec<Value> {
    let mut offers = vec![];
    for (area, region) in [
        ("europe", "Amsterdam / London / Frankfurt"),
        ("north_america", "New York / San Francisco / Toronto"),
        ("asia", "Singapore / Bangalore"),
        ("oceania", "Sydney"),
    ] {
        for (size, cpu, memory, price, disk, transfer) in [
            ("s-2vcpu-4gb", 2, 4., 24., 80, 4000),
            ("s-4vcpu-8gb", 4, 8., 48., 160, 5000),
            ("s-8vcpu-16gb", 8, 16., 96., 320, 6000),
        ] {
            offers.push(offer("digital_ocean",region,area,size,cpu,memory,price,"USD",Some(price),"https://www.digitalocean.com/pricing/droplets","2026-09-06",false,true,&format!("Published Basic Regular rates. Includes {disk} GiB disk, IPv4 and {transfer} GiB transfer; extra traffic and optional backups excluded. Region availability is checked after sign-in.")));
        }
    }
    for (size, cpu, memory, price, disk) in [
        ("cx23", 2, 4., 5.49, 40),
        ("cx33", 4, 8., 8.49, 80),
        ("cx43", 8, 16., 15.99, 160),
    ] {
        offers.push(offer("hetzner","Nuremberg / Helsinki","europe",size,cpu,memory,price,"EUR",Some(price+0.5),"https://docs.hetzner.com/general/infrastructure-and-availability/price-adjustment/","2026-09-06",false,true,&format!("Published Germany/Finland rates, {disk} GB disk and 20 TB transfer included; adds €0.50 for IPv4. Extra traffic and optional backups excluded. Capacity may be unavailable.")));
    }
    offers
}

async fn vultr(client: &reqwest::Client) -> Result<Vec<Value>> {
    let (plans, regions) = tokio::try_join!(
        get(client, "https://api.vultr.com/v2/plans?per_page=500"),
        get(client, "https://api.vultr.com/v2/regions?per_page=500")
    )?;
    let mut offers = vec![];
    let date = crate::domain::now();
    for region in regions["regions"].as_array().context("No regions")? {
        let area = match region["continent"].as_str().unwrap_or("") {
            "Europe" => "europe",
            "North America" => "north_america",
            "South America" => "south_america",
            "Asia" => "asia",
            "Australia" | "Oceania" => "oceania",
            "Africa" => "africa",
            _ => continue,
        };
        let id = region["id"].as_str().unwrap_or("");
        for plan in plans["plans"].as_array().context("No plans")? {
            let cpu = plan["vcpu_count"].as_u64().unwrap_or(0);
            let memory = plan["ram"].as_f64().unwrap_or(0.) / 1024.;
            if !(2..=16).contains(&cpu)
                || memory <= 0.
                || !["vc2", "vhf", "vhp", "voc"].contains(&plan["type"].as_str().unwrap_or(""))
                || plan["deploy_ondemand"] != true
                || !plan["locations"]
                    .as_array()
                    .is_some_and(|v| v.contains(&json!(id)))
            {
                continue;
            }
            let price = plan["location_cost"][id]["monthly_cost"]
                .as_f64()
                .or_else(|| plan["monthly_cost"].as_f64())
                .context("No price")?;
            if !price.is_finite() || price <= 0. {
                continue;
            }
            offers.push(offer("vultr",region["city"].as_str().unwrap_or(id),area,plan["id"].as_str().unwrap_or(""),cpu,memory,price,"USD",Some(price),"https://api.vultr.com/v2/plans",&date,true,plan["type"] != "voc", &format!("Includes {} GB disk, IPv4 and {} GB transfer. Extra traffic and optional backups excluded. Capacity is checked before deployment.",plan["disk"],plan["bandwidth"])));
        }
    }
    Ok(offers)
}

async fn azure(client: &reqwest::Client) -> Result<Vec<Value>> {
    // Exact x64 nonburstable SKU specs, not inferred from arbitrary retail product names.
    let mut offers = vec![];
    let filter = "serviceName eq 'Virtual Machines' and priceType eq 'Consumption' and (armSkuName eq 'Standard_F2s_v2' or armSkuName eq 'Standard_F4s_v2' or armSkuName eq 'Standard_D2s_v5' or armSkuName eq 'Standard_D4s_v5' or armSkuName eq 'Standard_D8s_v5')";
    let mut page: Value = client
        .get("https://prices.azure.com/api/retail/prices")
        .query(&[("$filter", filter)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let date = crate::domain::now();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..10 {
        for p in page["Items"].as_array().context("No retail prices")? {
            if !regular_linux(p) {
                continue;
            }
            let (region, area) = match p["armRegionName"].as_str().unwrap_or("") {
                "eastus" => ("Virginia", "north_america"),
                "westus2" => ("Washington", "north_america"),
                "westeurope" => ("Netherlands", "europe"),
                "uksouth" => ("London", "europe"),
                "southeastasia" => ("Singapore", "asia"),
                "japaneast" => ("Tokyo", "asia"),
                "australiaeast" => ("Sydney", "oceania"),
                "brazilsouth" => ("São Paulo", "south_america"),
                "southafricanorth" => ("Johannesburg", "africa"),
                _ => continue,
            };
            let size = p["armSkuName"].as_str().unwrap_or("");
            if !seen.insert((region.to_owned(), size.to_owned())) {
                continue;
            }
            let Some((cpu, memory)) = azure_size(size) else {
                continue;
            };
            let price = p["retailPrice"].as_f64().context("No price")? * 730.;
            offers.push(offer("azure",region,area,size,cpu,memory,price,"USD",None,"https://prices.azure.com/api/retail/prices",&date,true,false,"730 hours of regular Linux compute. Persistent disk, static IPv4, outbound traffic and optional backup storage are additional; total is not yet available. Subscription quota and capacity are checked after sign-in."));
        }
        let next = page["NextPageLink"].as_str().unwrap_or("");
        if next.is_empty() {
            break;
        }
        anyhow::ensure!(safe_retail_page(next), "Unexpected retail pagination host");
        page = get(client, next).await?;
    }
    Ok(offers)
}
fn safe_retail_page(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("prices.azure.com")
            && url.port_or_known_default() == Some(443)
            && url.path() == "/api/retail/prices"
            && url.username().is_empty()
            && url.password().is_none()
    })
}
fn azure_size(size: &str) -> Option<(u64, f64)> {
    match size {
        "Standard_F2s_v2" => Some((2, 4.)),
        "Standard_F4s_v2" => Some((4, 8.)),
        "Standard_D2s_v5" => Some((2, 8.)),
        "Standard_D4s_v5" => Some((4, 16.)),
        "Standard_D8s_v5" => Some((8, 32.)),
        _ => None,
    }
}
fn regular_linux(p: &Value) -> bool {
    let product = p["productName"].as_str().unwrap_or("");
    let sku = p["skuName"].as_str().unwrap_or("");
    !product.contains("Windows")
        && !sku.contains("Spot")
        && !sku.contains("Low Priority")
        && p["type"] == "Consumption"
        && p["unitOfMeasure"] == "1 Hour"
        && p["isPrimaryMeterRegion"] == true
        && p["currencyCode"] == "USD"
        && p["retailPrice"]
            .as_f64()
            .is_some_and(|v| v.is_finite() && v > 0.)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Fetches live public provider catalogs; run manually"]
    async fn live_public_catalogs() {
        let started = Instant::now();
        let result = catalog().await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(35));
        assert_eq!(result["notices"], json!([]));
        let offers = result["offers"].as_array().unwrap();
        for provider in ["azure", "vultr", "digital_ocean", "hetzner"] {
            assert!(
                offers.iter().any(|o| o["provider"] == provider),
                "Missing {provider}"
            );
        }
        for size in ["Standard_F2s_v2", "Standard_F4s_v2", "Standard_D2s_v5"] {
            assert!(offers.iter().any(|o| o["size"] == size), "Missing {size}");
        }
        assert!(offers
            .iter()
            .any(|o| o["provider"] == "vultr" && o["vcpus"] == 2));
        let sao = offers
            .iter()
            .find(|o| {
                o["provider"] == "vultr"
                    && o["size"] == "vc2-4c-8gb"
                    && o["region"].as_str().is_some_and(|r| r.contains("Paulo"))
            })
            .unwrap();
        assert!(sao["details"].as_str().unwrap().contains("4096 GB"));
        println!(
            "{} public offers, regional Vultr compute {}, {:?}",
            offers.len(),
            sao["compute"],
            started.elapsed()
        );
    }
    #[test]
    fn retail_pagination_accepts_canonical_https_port_only() {
        assert!(safe_retail_page(
            "https://prices.azure.com:443/api/retail/prices?$skip=1000"
        ));
        assert!(safe_retail_page(
            "https://prices.azure.com/api/retail/prices?$skip=1000"
        ));
        assert!(!safe_retail_page(
            "https://prices.azure.com.attacker.invalid/api/retail/prices"
        ));
        assert!(!safe_retail_page(
            "https://prices.azure.com:8443/api/retail/prices"
        ));
        assert!(!safe_retail_page(
            "http://prices.azure.com/api/retail/prices"
        ));
    }
    #[test]
    fn azure_sizes_use_exact_cpu_and_memory() {
        assert_eq!(azure_size("Standard_F2s_v2"), Some((2, 4.)));
        assert_eq!(azure_size("Standard_F4s_v2"), Some((4, 8.)));
        assert_eq!(azure_size("Standard_D2s_v5"), Some((2, 8.)));
        assert_eq!(azure_size("Standard_D4s_v5"), Some((4, 16.)));
        assert_eq!(azure_size("Standard_B2s"), None);
        assert_eq!(azure_size("Standard_D4ps_v5"), None);
    }
    #[test]
    fn retail_filter_rejects_commitments_and_spot() {
        let mut p = json!({"productName":"Virtual Machines Dsv5 Series","skuName":"D4s v5","type":"Consumption","unitOfMeasure":"1 Hour","isPrimaryMeterRegion":true,"currencyCode":"USD","retailPrice":0.2});
        assert!(regular_linux(&p));
        p["skuName"] = json!("D4s v5 Spot");
        assert!(!regular_linux(&p));
        p["skuName"] = json!("D4s v5");
        p["type"] = json!("Reservation");
        assert!(!regular_linux(&p));
    }
    #[test]
    fn snapshots_have_fixed_source_dates_and_explicit_ipv4() {
        let offers = snapshots();
        let h = offers.iter().find(|o| o["provider"] == "hetzner").unwrap();
        assert_eq!(h["checked_at"], "2026-09-06");
        assert_eq!(h["live"], false);
        assert_eq!(h["total"], json!(5.99));
        assert_eq!(h["size"], "cx23");
        assert_eq!(h["vcpus"], 2);
        assert_eq!(h["memory_gib"], 4.);
        assert!(h["details"].as_str().unwrap().contains("40 GB disk"));
        let d = offers.iter().find(|o| o["size"] == "s-2vcpu-4gb").unwrap();
        assert_eq!(d["total"], 24.);
        assert_eq!(d["vcpus"], 2);
        assert_eq!(d["memory_gib"], 4.);
        assert!(d["details"].as_str().unwrap().contains("80 GiB disk"));
        assert!(d["details"].as_str().unwrap().contains("4000 GiB transfer"));
    }
}
