//! Public stable Linux server releases published by Anego Studios. No game login is needed.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

pub const SOURCE: &str = "https://api.vintagestory.at/stable.json";
pub const TESTED_IDLE_VERSIONS: &[&str] = &["1.22.0", "1.22.7"];
pub fn tested_idle_version(version: &str) -> bool {
    TESTED_IDLE_VERSIONS.contains(&version)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GameRelease {
    pub version: String,
    pub filename: String,
    pub download_url: String,
    pub filesize: String,
    pub md5: String,
    pub latest: bool,
    pub runtime: String,
    pub tested: bool,
    pub automatic_idle: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ExcludedRelease {
    pub version: String,
    pub reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReleaseCatalog {
    pub versions: Vec<GameRelease>,
    pub latest: Option<String>,
    pub tested_version: String,
    pub source: String,
    pub fetched_at: String,
    pub status: String,
    pub error: Option<String>,
    pub excluded: Vec<ExcludedRelease>,
    pub login_required: bool,
}
type Cache = tokio::sync::Mutex<Option<(Instant, ReleaseCatalog)>>;
static CACHE: OnceLock<Cache> = OnceLock::new();

pub async fn catalog() -> ReleaseCatalog {
    let mut cache = CACHE.get_or_init(Default::default).lock().await;
    if let Some((at, value)) = cache.as_ref() {
        let ttl = if value.status == "current" { 900 } else { 60 };
        if at.elapsed() < Duration::from_secs(ttl) {
            return value.clone();
        }
    }
    let value = match fetch().await {
        Ok(value) => value,
        Err(_) => {
            let mut value = cache
                .as_ref()
                .map(|(_, value)| value.clone())
                .unwrap_or_else(empty_catalog);
            value.status = if value.versions.is_empty() {
                "unavailable"
            } else {
                "stale"
            }
            .into();
            value.error = Some("Available game downloads could not be refreshed. Check the internet connection and retry; no game account sign-in is required.".into());
            value
        }
    };
    *cache = Some((Instant::now(), value.clone()));
    value
}
fn empty_catalog() -> ReleaseCatalog {
    ReleaseCatalog {
        versions: vec![],
        latest: None,
        tested_version: "1.22.7".into(),
        source: SOURCE.into(),
        fetched_at: crate::domain::now(),
        status: "unavailable".into(),
        error: None,
        excluded: vec![],
        login_required: false,
    }
}
async fn fetch() -> Result<ReleaseCatalog> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut response = client.get(SOURCE).send().await?.error_for_status()?;
    anyhow::ensure!(
        response.status().is_success(),
        "Unexpected download catalog redirect"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= 2 * 1024 * 1024,
            "Download catalog exceeds size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    parse_catalog(&serde_json::from_slice(&bytes)?)
}
pub fn stable_version(version: &str) -> Option<[u32; 3]> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}
fn parse_catalog(value: &Value) -> Result<ReleaseCatalog> {
    let entries = value.as_object().context("Invalid game download catalog")?;
    anyhow::ensure!(entries.len() <= 1000, "Too many game releases");
    let mut catalog = empty_catalog();
    for (version, entry) in entries {
        let Some(numbers) = stable_version(version) else {
            continue;
        };
        let Some(server) = entry.get("linuxserver") else {
            continue;
        };
        let runtime = match numbers {
            [1, 21, _] => ".NET 8",
            [1, 22, _] => ".NET 10",
            _ => {
                catalog.excluded.push(ExcludedRelease{version:version.clone(),reason:"Requires a runtime outside the supported .NET 8 / .NET 10 server releases (1.21 and 1.22).".into()});
                continue;
            }
        };
        let filename = format!("vs_server_linux-x64_{version}.tar.gz");
        anyhow::ensure!(
            server["filename"] == filename,
            "Unexpected server archive filename for {version}"
        );
        let url = server["urls"]["cdn"]
            .as_str()
            .context("Missing public server download URL")?;
        // Exact canonical URL also rejects credentials, ports, query strings, fragments and traversal.
        anyhow::ensure!(
            url == format!("https://cdn.vintagestory.at/gamefiles/stable/{filename}"),
            "Untrusted server download URL"
        );
        let md5 = server["md5"].as_str().context("Missing archive checksum")?;
        anyhow::ensure!(
            md5.len() == 32 && md5.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid archive checksum"
        );
        catalog.versions.push(GameRelease {
            version: version.clone(),
            filename,
            download_url: url.into(),
            filesize: server["filesize"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(64)
                .collect(),
            md5: md5.to_ascii_lowercase(),
            latest: false,
            runtime: runtime.into(),
            tested: tested_idle_version(version),
            automatic_idle: tested_idle_version(version),
        });
    }
    catalog
        .versions
        .sort_by_key(|entry| std::cmp::Reverse(stable_version(&entry.version)));
    anyhow::ensure!(
        !catalog.versions.is_empty(),
        "No supported Linux server downloads are available"
    );
    catalog.versions[0].latest = true;
    catalog.latest = Some(catalog.versions[0].version.clone());
    catalog.status = "current".into();
    Ok(catalog)
}
pub fn select(catalog: &ReleaseCatalog, version: &str) -> Result<GameRelease> {
    anyhow::ensure!(
        catalog.status == "current",
        "Refresh available game downloads before installing or deploying"
    );
    catalog.versions.iter().find(|release|release.version==version).cloned().context("Selected game version is not an available supported Linux x64 stable download. Choose a version from the download list.")
}
pub async fn resolve(version: &str) -> Result<GameRelease> {
    select(&catalog().await, version)
}
pub fn verify_archive(bytes: &[u8], release: &GameRelease) -> Result<()> {
    anyhow::ensure!(format!("{:x}",md5::compute(bytes))==release.md5,"Downloaded game archive failed the publisher's integrity checksum; no game files were changed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(version: &str) -> Value {
        serde_json::json!({"linuxserver":{"filename":format!("vs_server_linux-x64_{version}.tar.gz"),"urls":{"cdn":format!("https://cdn.vintagestory.at/gamefiles/stable/vs_server_linux-x64_{version}.tar.gz")},"md5":"900150983cd24fb0d6963f7d28e17f72","filesize":"51 MB"}})
    }
    #[test]
    fn sorts_numeric_stable_versions_and_excludes_other_runtimes_and_platforms() {
        let value = serde_json::json!({"1.22.9":entry("1.22.9"),"1.22.10":entry("1.22.10"),"1.21.6":entry("1.21.6"),"1.20.0":entry("1.20.0"),"1.23.0":entry("1.23.0"),"1.22.11-rc.1":entry("1.22.11-rc.1"),"1.22.12":{"linuxclient":{}}});
        let result = parse_catalog(&value).unwrap();
        assert_eq!(result.latest.as_deref(), Some("1.22.10"));
        assert_eq!(result.versions.len(), 3);
        assert_eq!(result.excluded.len(), 2);
        assert!(!result.login_required);
    }
    #[test]
    fn rejects_redirect_origins_traversal_queries_credentials_and_invalid_checksums() {
        let mut value = serde_json::json!({"1.22.7":entry("1.22.7")});
        for url in ["http://cdn.vintagestory.at/gamefiles/stable/vs_server_linux-x64_1.22.7.tar.gz","https://cdn.vintagestory.at.evil.test/file","https://cdn.vintagestory.at/gamefiles/stable/../vs_server_linux-x64_1.22.7.tar.gz","https://user@cdn.vintagestory.at/gamefiles/stable/vs_server_linux-x64_1.22.7.tar.gz","https://cdn.vintagestory.at/gamefiles/stable/vs_server_linux-x64_1.22.7.tar.gz?token=a"] {
            value["1.22.7"]["linuxserver"]["urls"]["cdn"]=serde_json::json!(url);assert!(parse_catalog(&value).is_err());
        }
        value = serde_json::json!({"1.22.7":entry("1.22.7")});
        value["1.22.7"]["linuxserver"]["md5"] = serde_json::json!("invalid");
        assert!(parse_catalog(&value).is_err());
    }
    #[test]
    fn stale_or_missing_selection_and_corrupt_archive_fail_closed() {
        let mut catalog = parse_catalog(&serde_json::json!({"1.22.7":entry("1.22.7")})).unwrap();
        let release = select(&catalog, "1.22.7").unwrap();
        verify_archive(b"abc", &release).unwrap();
        assert!(verify_archive(b"corrupt", &release).is_err());
        assert!(select(&catalog, "1.22.999").is_err());
        catalog.status = "stale".into();
        assert!(select(&catalog, "1.22.7").is_err());
        for version in ["1..0", "1.22.0.1", "1.022.0", "latest", "../1.22.0"] {
            assert!(stable_version(version).is_none());
        }
    }
    #[test]
    fn idle_compatibility_is_explicitly_tested_not_inferred() {
        assert!(tested_idle_version("1.22.0"));
        assert!(tested_idle_version("1.22.7"));
        assert!(!tested_idle_version("1.22.8"));
    }
    #[tokio::test]
    #[ignore = "Fetches the official public game release feed"]
    async fn live_release_catalog() {
        let value = fetch().await.unwrap();
        println!(
            "Latest supported: {:?}; {} Linux stable downloads",
            value.latest,
            value.versions.len()
        );
        assert!(!value.login_required);
        assert!(value.versions.iter().any(|r| r.version == "1.22.7"));
    }
}
