use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Azure,
    Hetzner,
    DigitalOcean,
    Vultr,
    Ssh,
    Docker,
    SshDocker,
}
impl Provider {
    pub fn key(&self) -> &'static str {
        match self {
            Self::Azure => "azure",
            Self::Hetzner => "hetzner",
            Self::DigitalOcean => "digital_ocean",
            Self::Vultr => "vultr",
            Self::Ssh => "ssh",
            Self::Docker => "docker",
            Self::SshDocker => "ssh_docker",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    /// An environment variable or OS keyring entry name; never a plaintext credential.
    pub credential_ref: String,
    #[serde(default)]
    pub settings: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub account_id: Option<String>,
    pub provider: Provider,
    pub region: String,
    pub size: String,
    pub host: String,
    pub ssh_user: String,
    pub ssh_key: String,
    pub game_version: String,
    pub state: String,
    pub created_at: String,
    #[serde(default)]
    pub resources: Vec<Resource>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Resource {
    pub id: String,
    pub kind: String,
    pub owned: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Operation {
    pub id: String,
    pub server_id: String,
    pub action: String,
    pub status: String,
    pub message: String,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Event {
    pub seq: i64,
    pub at: String,
    pub server_id: String,
    pub kind: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeploySpec {
    pub name: String,
    pub account_id: String,
    pub region: String,
    pub size: String,
    pub game_version: String,
    pub ssh_public_key: String,
    pub ssh_private_key: String,
    /// Explicit SSH ingress CIDR. Game port is public; management is not.
    pub admin_cidr: String,
    #[serde(default)]
    pub existing_host: String,
    #[serde(default = "default_user")]
    pub ssh_user: String,
}
fn default_user() -> String {
    "vsm".into()
}
impl DeploySpec {
    pub fn validate(&self) -> anyhow::Result<()> {
        self.validate_for(&Provider::Azure)
    }
    pub fn validate_for(&self, provider: &Provider) -> anyhow::Result<()> {
        anyhow::ensure!(
            (3..=40).contains(&self.name.len())
                && self
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "Name must be 3–40 lowercase letters, numbers or hyphens"
        );
        anyhow::ensure!(
            self.game_version.split('.').count() >= 3
                && self
                    .game_version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b)),
            "Invalid game version"
        );
        if *provider == Provider::Docker {
            return Ok(());
        }
        if matches!(provider, Provider::Ssh | Provider::SshDocker) {
            anyhow::ensure!(
                !self.existing_host.is_empty() && !self.ssh_private_key.is_empty(),
                "SSH host and private key required"
            );
            return Ok(());
        }
        anyhow::ensure!(
            self.ssh_public_key.starts_with("ssh-ed25519 ")
                || self.ssh_public_key.starts_with("ssh-rsa "),
            "Provide an OpenSSH public key"
        );
        anyhow::ensure!(
            !self.ssh_public_key.contains(['\n', '\r']),
            "Invalid SSH key"
        );
        let (ip, prefix) = self
            .admin_cidr
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("An explicit SSH source CIDR is required"))?;
        let _: std::net::Ipv4Addr = ip.parse()?;
        let prefix: u8 = prefix.parse()?;
        anyhow::ensure!(
            (1..=32).contains(&prefix),
            "SSH ingress must be a restricted IPv4 CIDR"
        );
        anyhow::ensure!(
            !self.ssh_private_key.is_empty(),
            "SSH private key path is required"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Quote {
    pub id: String,
    pub provider: Provider,
    pub region: String,
    pub size: String,
    pub vcpus: u32,
    pub memory_gib: f64,
    pub currency: String,
    pub monthly_compute: f64,
    pub monthly_storage: Option<f64>,
    pub monthly_ip: Option<f64>,
    pub monthly_transfer: Option<f64>,
    pub monthly_backup: Option<f64>,
    pub quoted_at: String,
    pub available: bool,
    pub shared_cpu: bool,
    pub assumptions: Vec<String>,
}
impl Quote {
    pub fn total(&self) -> Option<f64> {
        if [
            Some(self.monthly_compute),
            self.monthly_storage,
            self.monthly_ip,
            self.monthly_transfer,
            self.monthly_backup,
        ]
        .iter()
        .any(|v| v.is_none_or(|v| !v.is_finite() || v < 0.0))
        {
            return None;
        }
        Some(
            self.monthly_compute
                + self.monthly_storage?
                + self.monthly_ip?
                + self.monthly_transfer?
                + self.monthly_backup?,
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LatencySample {
    pub quote_id: String,
    pub location: String,
    pub players: u32,
    pub milliseconds: f64,
    pub source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecommendationInput {
    pub quotes: Vec<Quote>,
    pub samples: Vec<LatencySample>,
    pub min_memory_gib: f64,
    pub min_vcpus: u32,
    pub budget: Option<f64>,
    pub currency: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Recommendations {
    pub balanced: Option<String>,
    pub cheapest: Option<String>,
    pub lowest_latency: Option<String>,
    pub candidates: Vec<RankedQuote>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RankedQuote {
    pub quote: Quote,
    pub total: Option<f64>,
    pub weighted_ms: Option<f64>,
    pub worst_ms: Option<f64>,
    pub eligible: bool,
    pub reasons: Vec<String>,
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
