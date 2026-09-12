//! Provider HTTP adapters. Credentials remain in the controller process.
use crate::{auth, db::Db, domain::*};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{Client, Method};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Cloud {
    pub account: Account,
    client: Client,
    #[cfg(test)]
    endpoint: Option<String>,
}
impl Cloud {
    pub fn new(account: Account) -> Self {
        Self {
            account,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(90))
                .build()
                .unwrap(),
            #[cfg(test)]
            endpoint: None,
        }
    }
    async fn token(&self) -> Result<String> {
        #[cfg(test)]
        if self.endpoint.is_some() {
            return Ok("fixture-token".into());
        }
        if self.account.provider != Provider::Azure {
            return auth::credential(&self.account.credential_ref);
        }
        match self.account.settings["auth"]
            .as_str()
            .unwrap_or("azure_cli")
        {
            "azure_cli" => {
                #[cfg(windows)]
                let mut cmd = {
                    let mut c = crate::subprocess::command("cmd.exe");
                    // Token refresh runs inside the desktop Manager as well as the CLI.
                    // Hide cmd/az's console; this does not suppress browser sign-in.
                    c.args(["/D", "/C", "az.cmd"]);
                    c
                };
                #[cfg(not(windows))]
                let mut cmd = crate::subprocess::command("az");
                let output = cmd
                    .kill_on_drop(true)
                    .args([
                        "account",
                        "get-access-token",
                        "--resource",
                        "https://management.azure.com/",
                        "--subscription",
                        self.subscription()?,
                        "--output",
                        "json",
                    ])
                    .output()
                    .await
                    .context("Azure CLI not found. Install it and run az login first")?;
                anyhow::ensure!(
                    output.status.success(),
                    "Azure login expired or subscription inaccessible; run az login"
                );
                Ok(
                    serde_json::from_slice::<Value>(&output.stdout)?["accessToken"]
                        .as_str()
                        .context("Azure CLI returned no token")?
                        .into(),
                )
            }
            "managed_identity" => {
                let mut url="http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https%3A%2F%2Fmanagement.azure.com%2F".to_string();
                if let Some(id) = self.account.settings["client_id"].as_str() {
                    let _: uuid::Uuid = id.parse()?;
                    url.push_str(&format!("&client_id={id}"));
                }
                let data: Value = self
                    .client
                    .get(url)
                    .header("Metadata", "true")
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                Ok(data["access_token"]
                    .as_str()
                    .context("Managed identity token unavailable")?
                    .into())
            }
            "service_principal" => {
                let tenant = self.account.settings["tenant_id"]
                    .as_str()
                    .context("tenant_id required")?;
                let _: uuid::Uuid = tenant.parse()?;
                let client = self.account.settings["client_id"]
                    .as_str()
                    .context("client_id required")?;
                let secret = auth::credential(&self.account.credential_ref)?;
                let data: Value = self
                    .client
                    .post(format!(
                        "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"
                    ))
                    .form(&[
                        ("client_id", client),
                        ("client_secret", secret.as_str()),
                        ("grant_type", "client_credentials"),
                        ("scope", "https://management.azure.com/.default"),
                    ])
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                Ok(data["access_token"]
                    .as_str()
                    .context("Service principal token unavailable")?
                    .into())
            }
            _ => anyhow::bail!("Unknown Azure authentication mode"),
        }
    }
    fn subscription(&self) -> Result<&str> {
        let s = self.account.settings["subscription_id"]
            .as_str()
            .context("Azure subscription_id is required")?;
        let _: uuid::Uuid = s.parse()?;
        Ok(s)
    }
    fn base(&self) -> &str {
        #[cfg(test)]
        if let Some(endpoint) = &self.endpoint {
            return endpoint;
        }
        match self.account.provider {
            Provider::Azure => "https://management.azure.com",
            Provider::Hetzner => "https://api.hetzner.cloud/v1",
            Provider::DigitalOcean => "https://api.digitalocean.com/v2",
            Provider::Vultr => "https://api.vultr.com/v2",
            Provider::Ssh | Provider::Docker | Provider::SshDocker => "",
        }
    }
    async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let token = self.token().await?;
        self.request_with_token(method, path, body, &token).await
    }
    async fn request_with_token(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        token: &str,
    ) -> Result<Value> {
        anyhow::ensure!(
            path.starts_with('/') && !path.contains(".."),
            "Invalid provider resource path"
        );
        let empty_post = method == Method::POST && body.is_none();
        let mut req = self
            .client
            .request(method, format!("{}{path}", self.base()))
            .bearer_auth(token);
        if let Some(body) = body {
            req = req.json(&body);
        } else if empty_post {
            req = req.header(reqwest::header::CONTENT_LENGTH, 0);
        }
        let response = req.send().await?;
        let status = response.status();
        // Do not include upstream bodies; they can echo credentials or bootstrap data.
        anyhow::ensure!(
            status.is_success(),
            "{} API returned {} for {}",
            self.account.provider.key(),
            status,
            path
        );
        let text = response.text().await?;
        if text.is_empty() {
            Ok(json!({}))
        } else {
            Ok(serde_json::from_str(&text)?)
        }
    }
    pub async fn validate(&self) -> Result<Value> {
        if self.account.provider == Provider::Docker {
            return crate::docker::validate_local().await;
        }
        if matches!(self.account.provider, Provider::Ssh | Provider::SshDocker) {
            return Ok(
                json!({"requires_host_verification":true,"capabilities":[self.account.provider.key()]}),
            );
        }
        let path = match self.account.provider {
            Provider::Azure => format!(
                "/subscriptions/{}?api-version=2022-12-01",
                self.subscription()?
            ),
            Provider::Hetzner => "/servers?per_page=1".into(),
            Provider::DigitalOcean => "/account".into(),
            Provider::Vultr => "/account".into(),
            Provider::Ssh | Provider::Docker | Provider::SshDocker => {
                return Ok(json!({"valid":true,"capabilities":["ssh"]}))
            }
        };
        self.request(Method::GET, &path, None).await?;
        Ok(json!({"valid":true}))
    }
    pub async fn catalog(&self) -> Result<Value> {
        match self.account.provider {
            Provider::Azure => {
                let sub = self.subscription()?;
                let regions = self
                    .request(
                        Method::GET,
                        &format!("/subscriptions/{sub}/locations?api-version=2022-12-01"),
                        None,
                    )
                    .await?;
                let sizes=self.azure_pages(&format!("/subscriptions/{sub}/providers/Microsoft.Compute/skus?api-version=2021-07-01")).await?;
                Ok(
                    json!({"regions":regions["value"],"sizes":sizes,"pricing":"retail","fetched_at":now()}),
                )
            }
            Provider::Hetzner => Ok(
                json!({"regions":self.request(Method::GET,"/locations",None).await?["locations"],"sizes":self.request(Method::GET,"/server_types?per_page=50",None).await?["server_types"],"fetched_at":now()}),
            ),
            Provider::DigitalOcean => Ok(
                json!({"regions":self.request(Method::GET,"/regions?per_page=200",None).await?["regions"],"sizes":self.request(Method::GET,"/sizes?per_page=200",None).await?["sizes"],"fetched_at":now()}),
            ),
            Provider::Vultr => Ok(
                json!({"regions":self.request(Method::GET,"/regions?per_page=500",None).await?["regions"],"sizes":self.request(Method::GET,"/plans?per_page=500",None).await?["plans"],"fetched_at":now()}),
            ),
            Provider::Ssh | Provider::Docker | Provider::SshDocker => {
                Ok(json!({"regions":[],"sizes":[],"pricing":"unavailable"}))
            }
        }
    }
    async fn azure_pages(&self, path: &str) -> Result<Vec<Value>> {
        let token = self.token().await?;
        let mut path = path.to_owned();
        let mut rows = vec![];
        for _ in 0..100 {
            let data = self
                .request_with_token(Method::GET, &path, None, &token)
                .await?;
            rows.extend(
                data["value"]
                    .as_array()
                    .context("Malformed Azure catalog page")?
                    .iter()
                    .cloned(),
            );
            anyhow::ensure!(rows.len() <= 50000, "Azure catalog exceeds row limit");
            anyhow::ensure!(
                data.get("nextLink")
                    .is_none_or(|next| next.is_null() || next.is_string()),
                "Malformed Azure pagination link"
            );
            match data["nextLink"].as_str() {
                Some(next) if !next.is_empty() => {
                    let next = reqwest::Url::parse(next)?;
                    anyhow::ensure!(
                        next.scheme() == "https"
                            && next.host_str() == Some("management.azure.com")
                            && next.port().is_none()
                            && next.username().is_empty()
                            && next.password().is_none()
                            && next.fragment().is_none(),
                        "Unexpected Azure pagination host"
                    );
                    path = next.path().to_string();
                    if let Some(query) = next.query() {
                        path.push('?');
                        path.push_str(query);
                    }
                }
                _ => return Ok(rows),
            }
        }
        anyhow::bail!("Azure catalog pagination limit reached")
    }
    pub async fn azure_preflight(&self, spec: &DeploySpec) -> Result<Value> {
        let sub = self.subscription()?;
        self.validate().await?;
        for provider in ["Microsoft.Compute", "Microsoft.Network"] {
            let data = self
                .request(
                    Method::GET,
                    &format!("/subscriptions/{sub}/providers/{provider}?api-version=2021-04-01"),
                    None,
                )
                .await?;
            anyhow::ensure!(
                data["registrationState"] == "Registered",
                "Register {provider} in the Azure subscription before deploying"
            );
        }
        let skus = self
            .azure_pages(&regional_skus_path(sub, segment(&spec.region)?))
            .await?;
        let sku = skus
            .iter()
            .find(|s| {
                s["name"] == spec.size
                    && s["resourceType"] == "virtualMachines"
                    && sku_in_region(s, &spec.region)
            })
            .context("VM size unavailable in selected region")?;
        let blocked = sku_region_restricted(sku, &spec.region);
        anyhow::ensure!(
            !blocked,
            "Subscription is restricted from this VM size in the selected region"
        );
        let caps = sku["capabilities"]
            .as_array()
            .context("Missing SKU capabilities")?;
        let cap = |name: &str| {
            caps.iter()
                .find(|c| c["name"] == name)
                .and_then(|c| c["value"].as_str())
                .unwrap_or("")
        };
        anyhow::ensure!(
            cap("CpuArchitectureType") != "Arm64",
            "ARM hosts are not supported"
        );
        let cpu: f64 = cap("vCPUs").parse().context("Missing CPU count")?;
        let usage=self.request(Method::GET,&format!("/subscriptions/{sub}/providers/Microsoft.Compute/locations/{}/usages?api-version=2024-03-01",segment(&spec.region)?),None).await?;
        let family = sku["family"].as_str().unwrap_or("");
        let usages = usage["value"]
            .as_array()
            .context("Missing quota information")?;
        for name in ["cores", family] {
            let u = usages
                .iter()
                .find(|u| {
                    u["name"]["value"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case(name))
                })
                .context("Unable to verify regional/family quota")?;
            anyhow::ensure!(
                u["limit"].as_f64().unwrap_or(0.0) - u["currentValue"].as_f64().unwrap_or(0.0)
                    >= cpu,
                "Insufficient Azure vCPU quota for {name}"
            );
        }
        Ok(
            json!({"valid":true,"sku":sku,"quota_checked":true,"permission_note":"Resource write permissions are enforced by ARM; policy can still reject a deployment."}),
        )
    }
    /// Fast retail estimate for size selection. Capacity remains a plan/apply check.
    pub async fn price(&self, region: &str, size: &str) -> Result<Quote> {
        if self.account.provider != Provider::Azure {
            return self.quote(region, size).await;
        }
        segment(region)?;
        segment(size)?;
        let price = crate::azure_prices::lookup(region, size, false).await?;
        Ok(Quote {
            id: format!("azure:{region}:{size}"),
            provider: Provider::Azure,
            region: region.into(),
            size: size.into(),
            vcpus: 0,
            memory_gib: 0.0,
            currency: "USD".into(),
            monthly_compute: price.hourly * 730.0,
            monthly_storage: None,
            monthly_ip: None,
            monthly_transfer: None,
            monthly_backup: None,
            quoted_at: price.quoted_at,
            available: false,
            shared_cpu: size.starts_with("Standard_B"),
            assumptions: vec![
                "Compute estimate for 730 hours. Disk, IP, traffic and backups are additional."
                    .into(),
                "Capacity and subscription access are checked when reviewing deployment.".into(),
                "Retail USD estimate excludes taxes, credits and negotiated discounts.".into(),
            ],
        })
    }
    pub async fn quote(&self, region: &str, size: &str) -> Result<Quote> {
        segment(region)?;
        segment(size)?;
        let mut q=Quote{id:format!("{}:{region}:{size}",self.account.provider.key()),provider:self.account.provider.clone(),region:region.into(),size:size.into(),vcpus:0,memory_gib:0.0,currency:"USD".into(),monthly_compute:0.0,monthly_storage:Some(0.0),monthly_ip:Some(0.0),monthly_transfer:None,monthly_backup:Some(0.0),quoted_at:now(),available:false,shared_cpu:true,assumptions:vec!["Retail estimate; excludes taxes, credits, negotiated discounts and optional backup storage.".into(),"Transfer usage is unknown; total remains incomplete until budgeted.".into()]};
        match self.account.provider {
            Provider::Azure => {
                let price = crate::azure_prices::lookup(region, size, true).await?;
                q.monthly_compute = price.hourly * 730.0;
                q.quoted_at = price.quoted_at;
                q.monthly_storage = None;
                q.monthly_ip = None;
                q.shared_cpu = size.starts_with("Standard_B");
                let skus = self
                    .azure_pages(&regional_skus_path(self.subscription()?, region))
                    .await?;
                if let Some(s) = skus
                    .iter()
                    .find(|s| s["name"] == size && sku_in_region(s, region))
                {
                    q.available = !sku_region_restricted(s, region);
                    for cap in s["capabilities"].as_array().into_iter().flatten() {
                        match cap["name"].as_str().unwrap_or("") {
                            "vCPUs" => {
                                q.vcpus = cap["value"].as_str().unwrap_or("0").parse().unwrap_or(0)
                            }
                            "MemoryGB" => {
                                q.memory_gib =
                                    cap["value"].as_str().unwrap_or("0").parse().unwrap_or(0.0)
                            }
                            "CpuArchitectureType" if cap["value"] != "x64" => q.available = false,
                            _ => {}
                        }
                    }
                }
                q.assumptions.push("730 compute hours. Disk and Standard IPv4 meters must be supplied before a complete total can be ranked. Regular Linux pricing only.".into());
            }
            Provider::Hetzner => {
                let data = self
                    .request(Method::GET, "/server_types?per_page=50", None)
                    .await?;
                let s = data["server_types"]
                    .as_array()
                    .context("No sizes")?
                    .iter()
                    .find(|s| s["name"] == size)
                    .context("Unknown size")?;
                let p = s["prices"]
                    .as_array()
                    .context("No pricing")?
                    .iter()
                    .find(|p| p["location"] == region)
                    .context("No regional pricing")?;
                q.monthly_compute = p["price_monthly"]["net"]
                    .as_str()
                    .context("Missing price")?
                    .parse()?;
                q.currency = "EUR".into();
                q.vcpus = s["cores"].as_u64().unwrap_or(0) as u32;
                q.memory_gib = s["memory"].as_f64().unwrap_or(0.0);
                q.monthly_ip = None;
                q.available = !s["deprecated"].as_bool().unwrap_or(false);
                q.shared_cpu = s["cpu_type"] != "dedicated";
            }
            Provider::DigitalOcean => {
                let data = self
                    .request(Method::GET, "/sizes?per_page=200", None)
                    .await?;
                let s = data["sizes"]
                    .as_array()
                    .context("No sizes")?
                    .iter()
                    .find(|s| s["slug"] == size)
                    .context("Unknown size")?;
                q.monthly_compute = s["price_monthly"].as_f64().context("Missing price")?;
                q.vcpus = s["vcpus"].as_u64().unwrap_or(0) as u32;
                q.memory_gib = s["memory"].as_f64().unwrap_or(0.0) / 1024.0;
                q.available = s["available"] == true
                    && s["regions"]
                        .as_array()
                        .is_some_and(|r| r.contains(&json!(region)));
            }
            Provider::Vultr => {
                let data = self
                    .request(Method::GET, "/plans?per_page=500", None)
                    .await?;
                let s = data["plans"]
                    .as_array()
                    .context("No plans")?
                    .iter()
                    .find(|s| s["id"] == size)
                    .context("Unknown size")?;
                q.monthly_compute = s["monthly_cost"].as_f64().context("Missing price")?;
                q.vcpus = s["vcpu_count"].as_u64().unwrap_or(0) as u32;
                q.memory_gib = s["ram"].as_f64().unwrap_or(0.0) / 1024.0;
                let avail = self
                    .request(
                        Method::GET,
                        &format!("/regions/{region}/availability"),
                        None,
                    )
                    .await?;
                q.available = avail["available_plans"]
                    .as_array()
                    .is_some_and(|r| r.contains(&json!(size)));
            }
            Provider::Ssh | Provider::Docker | Provider::SshDocker => {
                anyhow::bail!("Existing hosts do not have discoverable cloud pricing")
            }
        }
        Ok(q)
    }
    pub async fn provision(&self, spec: &DeploySpec, server: &mut Server, db: &Db) -> Result<()> {
        spec.validate_for(&self.account.provider)?;
        if self.account.provider == Provider::Azure {
            return self.azure_provision(spec, server, db).await;
        }
        if matches!(
            self.account.provider,
            Provider::Ssh | Provider::Docker | Provider::SshDocker
        ) {
            server.host = if self.account.provider == Provider::Docker {
                "127.0.0.1".into()
            } else {
                spec.existing_host.clone()
            };
            server.ssh_user = spec.ssh_user.clone();
            anyhow::ensure!(!server.host.is_empty(), "Existing host is required");
            server.state = "awaiting_bootstrap".into();
            db.put("servers", &server.id, server)?;
            return Ok(());
        }
        self.validate().await?;
        let user_data = cloud_init(spec);
        let (path, body, field) = match self.account.provider {
            Provider::Hetzner => (
                "/servers",
                json!({"name":spec.name,"server_type":spec.size,"location":spec.region,"image":"ubuntu-24.04","user_data":user_data,"labels":{"vsm-owner":server.id},"public_net":{"enable_ipv4":true,"enable_ipv6":true}}),
                "server",
            ),
            Provider::DigitalOcean => (
                "/droplets",
                json!({"name":spec.name,"size":spec.size,"region":spec.region,"image":"ubuntu-24-04-x64","user_data":user_data,"tags":[format!("vsm-{}",server.id)]}),
                "droplet",
            ),
            Provider::Vultr => {
                let os = self.request(Method::GET, "/os?per_page=500", None).await?;
                let os_id = os["os"]
                    .as_array()
                    .context("No OS catalog")?
                    .iter()
                    .find(|o| {
                        o["name"]
                            .as_str()
                            .is_some_and(|s| s.contains("Ubuntu 24.04") && s.contains("x64"))
                    })
                    .context("Ubuntu 24.04 x64 unavailable")?["id"]
                    .clone();
                (
                    "/instances",
                    json!({"label":spec.name,"hostname":spec.name,"plan":spec.size,"region":spec.region,"os_id":os_id,"user_data":STANDARD.encode(user_data),"tags":[format!("vsm-{}",server.id)]}),
                    "instance",
                )
            }
            _ => unreachable!(),
        };
        let value = self.request(Method::POST, path, Some(body)).await?;
        let id = value[field]["id"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| value[field]["id"].to_string());
        anyhow::ensure!(
            id != "null",
            "Provider returned no resource ID; inspect provider account before retrying"
        );
        server.resources.push(Resource {
            id: format!("{path}/{id}"),
            kind: "vm".into(),
            owned: true,
        });
        db.put("servers", &server.id, server)?;
        for _ in 0..90 {
            let data = self
                .request(Method::GET, &format!("{path}/{id}"), None)
                .await?;
            let vm = &data[field];
            let ip = match self.account.provider {
                Provider::Hetzner => vm["public_net"]["ipv4"]["ip"].as_str(),
                Provider::Vultr => vm["main_ip"].as_str(),
                Provider::DigitalOcean => vm["networks"]["v4"]
                    .as_array()
                    .and_then(|a| a.iter().find(|a| a["type"] == "public"))
                    .and_then(|a| a["ip_address"].as_str()),
                _ => None,
            };
            if let Some(ip) = ip.filter(|s| !s.is_empty() && *s != "0.0.0.0") {
                server.host = ip.into();
                server.state = "awaiting_bootstrap".into();
                db.put("servers", &server.id, server)?;
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        anyhow::bail!("Timed out awaiting public IP; resources retained for inspection")
    }
    async fn azure_put(
        &self,
        path: &str,
        version: &str,
        body: Value,
        kind: &str,
        server: &mut Server,
        db: &Db,
    ) -> Result<Value> {
        // Record intent first: deterministic ARM IDs make interrupted PUTs inspectable.
        server.resources.push(Resource {
            id: path.into(),
            kind: kind.into(),
            owned: true,
        });
        db.put("servers", &server.id, server)?;
        let data = self
            .request(
                Method::PUT,
                &format!("{path}?api-version={version}"),
                Some(body),
            )
            .await?;
        for _ in 0..120 {
            let current = self
                .request(Method::GET, &format!("{path}?api-version={version}"), None)
                .await?;
            match current["properties"]["provisioningState"].as_str() {
                Some("Succeeded") | None => return Ok(current),
                Some("Failed") | Some("Canceled") => {
                    anyhow::bail!("Azure resource provisioning failed: {kind}")
                }
                _ => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
            }
        }
        let _ = data;
        anyhow::bail!("Azure provisioning timed out; resources retained")
    }
    async fn azure_provision(&self, spec: &DeploySpec, s: &mut Server, db: &Db) -> Result<()> {
        self.azure_preflight(spec).await?;
        let group = format!(
            "/subscriptions/{}/resourceGroups/vsm-{}",
            self.subscription()?,
            s.id
        );
        let tags = json!({"vsm-owner":s.id,"vsm-name":s.name});
        let loc = &spec.region;
        // A fresh random deployment ID is used; an existing group must never be adopted implicitly.
        let exists = self
            .client
            .get(format!("{}{group}?api-version=2022-09-01", self.base()))
            .bearer_auth(self.token().await?)
            .send()
            .await?;
        anyhow::ensure!(
            exists.status() == reqwest::StatusCode::NOT_FOUND,
            "Azure resource group already exists or cannot be checked; refusing to overwrite"
        );
        self.azure_put(
            &group,
            "2022-09-01",
            json!({"location":loc,"tags":tags}),
            "resource_group",
            s,
            db,
        )
        .await?;
        let network = format!("{group}/providers/Microsoft.Network");
        let nsg = format!("{network}/networkSecurityGroups/vsm");
        self.azure_put(&nsg,"2024-05-01",json!({"location":loc,"tags":tags,"properties":{"securityRules":[{"name":"ssh","properties":{"priority":100,"protocol":"Tcp","access":"Allow","direction":"Inbound","sourceAddressPrefix":spec.admin_cidr,"sourcePortRange":"*","destinationAddressPrefix":"*","destinationPortRange":"22"}},{"name":"game","properties":{"priority":110,"protocol":"*","access":"Allow","direction":"Inbound","sourceAddressPrefix":"*","sourcePortRange":"*","destinationAddressPrefix":"*","destinationPortRange":"42420"}}]}}),"network",s,db).await?;
        let vnet = format!("{network}/virtualNetworks/vsm");
        self.azure_put(&vnet,"2024-05-01",json!({"location":loc,"tags":tags,"properties":{"addressSpace":{"addressPrefixes":["10.42.0.0/16"]},"subnets":[{"name":"default","properties":{"addressPrefix":"10.42.0.0/24","networkSecurityGroup":{"id":nsg}}}]}}),"network",s,db).await?;
        let pip = format!("{network}/publicIPAddresses/vsm");
        let ip=self.azure_put(&pip,"2024-05-01",json!({"location":loc,"tags":tags,"sku":{"name":"Standard"},"properties":{"publicIPAllocationMethod":"Static","publicIPAddressVersion":"IPv4"}}),"network",s,db).await?;
        let nic = format!("{network}/networkInterfaces/vsm");
        self.azure_put(&nic,"2024-05-01",json!({"location":loc,"tags":tags,"properties":{"ipConfigurations":[{"name":"primary","properties":{"subnet":{"id":format!("{vnet}/subnets/default")},"privateIPAllocationMethod":"Dynamic","publicIPAddress":{"id":pip}}}]}}),"network",s,db).await?;
        let vm = format!("{group}/providers/Microsoft.Compute/virtualMachines/vsm");
        let disk_name = format!("vsm-{}", s.id);
        self.azure_put(&vm,"2024-03-01",json!({"location":loc,"tags":tags,"properties":{"hardwareProfile":{"vmSize":spec.size},"storageProfile":{"imageReference":{"publisher":"Canonical","offer":"ubuntu-24_04-lts","sku":"server","version":"latest"},"osDisk":{"name":disk_name,"createOption":"FromImage","deleteOption":"Detach","diskSizeGB":64,"managedDisk":{"storageAccountType":"StandardSSD_LRS"}}},"osProfile":{"computerName":"vsm","adminUsername":"vsm","customData":STANDARD.encode(cloud_init(spec)),"linuxConfiguration":{"disablePasswordAuthentication":true,"ssh":{"publicKeys":[{"path":"/home/vsm/.ssh/authorized_keys","keyData":spec.ssh_public_key}]}}},"networkProfile":{"networkInterfaces":[{"id":nic}]}}}),"vm",s,db).await?;
        // Tag the implicitly created OS disk before permitting app-managed deletion.
        let disk = format!("{group}/providers/Microsoft.Compute/disks/{disk_name}");
        self.request(
            Method::PATCH,
            &format!("{disk}?api-version=2024-03-02"),
            Some(json!({"tags":tags})),
        )
        .await?;
        s.resources.push(Resource {
            id: disk,
            kind: "disk".into(),
            owned: true,
        });
        s.host = ip["properties"]["ipAddress"]
            .as_str()
            .context("Azure returned no public IP")?
            .into();
        s.state = "awaiting_bootstrap".into();
        db.put("servers", &s.id, s)?;
        Ok(())
    }
    pub async fn power(&self, s: &Server, action: &str) -> Result<Value> {
        if crate::docker::is_docker(&s.provider) {
            return crate::docker::power(s, action).await;
        }
        let r = s
            .resources
            .iter()
            .find(|r| r.kind == "vm" && r.owned)
            .context("No owned VM resource")?;
        self.verify_owned(s, r).await?;
        let (path, body) = match self.account.provider {
            Provider::Azure => {
                let a = match action {
                    "start" => "start",
                    "restart" => "restart",
                    "deallocate" => "deallocate",
                    _ => anyhow::bail!("Unsupported Azure power action"),
                };
                (format!("{}/{a}?api-version=2024-03-01", r.id), None)
            }
            Provider::Hetzner => {
                let a = match action {
                    "start" => "poweron",
                    "restart" => "reboot",
                    "deallocate" => "shutdown",
                    _ => anyhow::bail!("Unsupported action"),
                };
                (format!("{}/actions/{a}", r.id), Some(json!({})))
            }
            Provider::DigitalOcean => {
                let a = match action {
                    "start" => "power_on",
                    "restart" => "reboot",
                    "deallocate" => "shutdown",
                    _ => anyhow::bail!("Unsupported action"),
                };
                (format!("{}/actions", r.id), Some(json!({"type":a})))
            }
            Provider::Vultr => {
                let a = match action {
                    "start" => "start",
                    "restart" => "reboot",
                    "deallocate" => "halt",
                    _ => anyhow::bail!("Unsupported action"),
                };
                (format!("{}/{a}", r.id), Some(json!({})))
            }
            Provider::Ssh | Provider::Docker | Provider::SshDocker => {
                anyhow::bail!("Power management unavailable for existing hosts")
            }
        };
        self.request(Method::POST, &path, body).await
    }
    fn version(kind: &str) -> &'static str {
        match kind {
            "vm" => "2024-03-01",
            "disk" => "2024-03-02",
            "resource_group" => "2022-09-01",
            _ => "2024-05-01",
        }
    }
    async fn verify_owned(&self, s: &Server, r: &Resource) -> Result<Value> {
        anyhow::ensure!(r.owned, "Resource not owned");
        let path = if self.account.provider == Provider::Azure {
            format!("{}?api-version={}", r.id, Self::version(&r.kind))
        } else {
            r.id.clone()
        };
        let data = self.request(Method::GET, &path, None).await?;
        let owned = match self.account.provider {
            Provider::Azure => data["tags"]["vsm-owner"] == s.id,
            Provider::Hetzner => data["server"]["labels"]["vsm-owner"] == s.id,
            Provider::DigitalOcean => data["droplet"]["tags"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(format!("vsm-{}", s.id)))),
            Provider::Vultr => data["instance"]["tags"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(format!("vsm-{}", s.id)))),
            Provider::Ssh | Provider::Docker | Provider::SshDocker => false,
        };
        anyhow::ensure!(owned, "Ownership tag missing or changed; refusing mutation");
        Ok(data)
    }
    pub async fn infrastructure_status(&self, s: &Server) -> Result<Value> {
        let resource = s
            .resources
            .iter()
            .find(|r| r.kind == "vm")
            .context("No VM resource has been recorded yet")?;
        let (power, provisioning) = match self.account.provider {
            Provider::Azure => {
                let data = self
                    .request(
                        Method::GET,
                        &format!("{}/instanceView?api-version=2024-03-01", resource.id),
                        None,
                    )
                    .await?;
                let codes: Vec<&str> = data["statuses"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v["code"].as_str())
                    .collect();
                (
                    codes
                        .iter()
                        .find_map(|code| code.strip_prefix("PowerState/"))
                        .map(str::to_owned),
                    codes
                        .iter()
                        .find_map(|code| code.strip_prefix("ProvisioningState/"))
                        .map(str::to_owned),
                )
            }
            Provider::Hetzner => {
                let data = self.request(Method::GET, &resource.id, None).await?;
                (data["server"]["status"].as_str().map(str::to_owned), None)
            }
            Provider::DigitalOcean => {
                let data = self.request(Method::GET, &resource.id, None).await?;
                (data["droplet"]["status"].as_str().map(str::to_owned), None)
            }
            Provider::Vultr => {
                let data = self.request(Method::GET, &resource.id, None).await?;
                (
                    data["instance"]["power_status"].as_str().map(str::to_owned),
                    data["instance"]["server_status"]
                        .as_str()
                        .map(str::to_owned),
                )
            }
            _ => anyhow::bail!("Provider VM status is unavailable for this host"),
        };
        Ok(
            json!({"provider":s.provider,"power_state":power,"provisioning_state":provisioning,"checked_at":now(),"remote_installed":null,"error":if power.is_none(){Some("Provider did not report a power state")}else{None}}),
        )
    }
    pub async fn inspect(&self, s: &Server) -> Result<Value> {
        if crate::docker::is_docker(&s.provider) {
            return crate::docker::inspect(s).await;
        }
        let mut result = vec![];
        for r in &s.resources {
            result.push(match self.verify_owned(s, r).await {
                Ok(data) => json!({"resource":r,"observed":data}),
                Err(error) => json!({"resource":r,"inspection_error":error.to_string()}),
            });
        }
        Ok(json!(result))
    }
    pub async fn resize(&self, s: &Server, size: &str) -> Result<Value> {
        segment(size)?;
        let resource = s
            .resources
            .iter()
            .find(|r| r.kind == "vm")
            .context("No VM resource")?;
        self.verify_owned(s, resource).await?;
        let quote = self.quote(&s.region, size).await?;
        anyhow::ensure!(quote.available, "Requested size is not available");
        match self.account.provider {
            Provider::Azure => {
                let options = self
                    .request(
                        Method::GET,
                        &format!("{}/vmSizes?api-version=2024-03-01", resource.id),
                        None,
                    )
                    .await?;
                anyhow::ensure!(options["value"].as_array().is_some_and(|v|v.iter().any(|v|v["name"]==size)),"Size is not compatible with the allocated VM; deallocate and inspect options first");
                self.request(
                    Method::PATCH,
                    &format!("{}?api-version=2024-03-01", resource.id),
                    Some(json!({"properties":{"hardwareProfile":{"vmSize":size}}})),
                )
                .await
            }
            Provider::Hetzner => {
                self.request(
                    Method::POST,
                    &format!("{}/actions/change_type", resource.id),
                    Some(json!({"server_type":size,"upgrade_disk":false})),
                )
                .await
            }
            Provider::DigitalOcean => {
                self.request(
                    Method::POST,
                    &format!("{}/actions", resource.id),
                    Some(json!({"type":"resize","size":size,"disk":false})),
                )
                .await
            }
            Provider::Vultr => {
                self.request(Method::PATCH, &resource.id, Some(json!({"plan":size})))
                    .await
            }
            Provider::Ssh | Provider::Docker | Provider::SshDocker => {
                anyhow::bail!("Resize unavailable for existing SSH hosts")
            }
        }
    }
    pub async fn wait_power(&self, s: &Server, action: &str) -> Result<()> {
        if crate::docker::is_docker(&s.provider) {
            let v = crate::docker::inspect(s).await?;
            anyhow::ensure!(
                v["State"]["Running"] == json!(action != "deallocate"),
                "Container power state not verified"
            );
            return Ok(());
        }
        let r = s
            .resources
            .iter()
            .find(|r| r.kind == "vm")
            .context("No VM resource")?;
        for _ in 0..120 {
            let stopped = action == "deallocate";
            let ready = match self.account.provider {
                Provider::Azure => {
                    let data = self
                        .request(
                            Method::GET,
                            &format!("{}/instanceView?api-version=2024-03-01", r.id),
                            None,
                        )
                        .await?;
                    data["statuses"].as_array().is_some_and(|v| {
                        v.iter().any(|v| {
                            v["code"]
                                == if stopped {
                                    "PowerState/deallocated"
                                } else {
                                    "PowerState/running"
                                }
                        })
                    })
                }
                Provider::Hetzner => {
                    let data = self.request(Method::GET, &r.id, None).await?;
                    data["server"]["status"] == if stopped { "off" } else { "running" }
                }
                Provider::DigitalOcean => {
                    let data = self.request(Method::GET, &r.id, None).await?;
                    data["droplet"]["status"] == if stopped { "off" } else { "active" }
                }
                Provider::Vultr => {
                    let data = self.request(Method::GET, &r.id, None).await?;
                    data["instance"]["power_status"] == if stopped { "stopped" } else { "running" }
                }
                Provider::Ssh | Provider::Docker | Provider::SshDocker => false,
            };
            if ready {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        anyhow::bail!(
            "Timed out verifying cloud power state; inspect infrastructure before retrying"
        )
    }
    pub async fn wait_resize(&self, s: &Server, size: &str) -> Result<()> {
        let r = s
            .resources
            .iter()
            .find(|r| r.kind == "vm")
            .context("No VM resource")?;
        for _ in 0..120 {
            let ready = match self.account.provider {
                Provider::Azure => {
                    let d = self
                        .request(
                            Method::GET,
                            &format!("{}?api-version=2024-03-01", r.id),
                            None,
                        )
                        .await?;
                    d["properties"]["hardwareProfile"]["vmSize"] == size
                        && d["properties"]["provisioningState"] == "Succeeded"
                }
                Provider::Hetzner => {
                    self.request(Method::GET, &r.id, None).await?["server"]["server_type"]["name"]
                        == size
                }
                Provider::DigitalOcean => {
                    self.request(Method::GET, &r.id, None).await?["droplet"]["size_slug"] == size
                }
                Provider::Vultr => {
                    self.request(Method::GET, &r.id, None).await?["instance"]["plan"] == size
                }
                Provider::Ssh | Provider::Docker | Provider::SshDocker => false,
            };
            if ready {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        anyhow::bail!("Timed out verifying resize; inspect infrastructure before retrying")
    }
    pub async fn delete(&self, s: &Server) -> Result<()> {
        if crate::docker::is_docker(&s.provider) {
            return crate::docker::delete(s).await;
        }
        // Individual resources only. Never recursively delete a resource group.
        let mut resources = s
            .resources
            .iter()
            .filter(|r| r.kind != "resource_group")
            .collect::<Vec<_>>();
        resources.sort_by_key(|r| match r.kind.as_str() {
            "vm" => 0,
            "disk" => 1,
            _ => 2,
        });
        // Network resources reverse creation order: NIC -> IP -> VNet -> NSG.
        let mut networks = resources
            .iter()
            .filter(|r| r.kind == "network")
            .copied()
            .collect::<Vec<_>>();
        networks.reverse();
        resources.retain(|r| r.kind != "network");
        resources.extend(networks);
        for r in resources {
            let path = if self.account.provider == Provider::Azure {
                format!("{}?api-version={}", r.id, Self::version(&r.kind))
            } else {
                r.id.clone()
            };
            anyhow::ensure!(
                path.starts_with('/') && !path.contains(".."),
                "Invalid resource path"
            );
            let observed = self
                .client
                .get(format!("{}{path}", self.base()))
                .bearer_auth(self.token().await?)
                .send()
                .await?;
            if observed.status() == reqwest::StatusCode::NOT_FOUND {
                continue;
            }
            anyhow::ensure!(
                observed.status().is_success(),
                "Cannot inspect resource before deletion"
            );
            self.verify_owned(s, r).await?;
            self.request(Method::DELETE, &path, None).await?;
            if self.account.provider == Provider::Azure {
                let mut gone = false;
                for _ in 0..120 {
                    let resp = self
                        .client
                        .get(format!("{}{path}", self.base()))
                        .bearer_auth(self.token().await?)
                        .send()
                        .await?;
                    if resp.status() == reqwest::StatusCode::NOT_FOUND {
                        gone = true;
                        break;
                    }
                    anyhow::ensure!(resp.status().is_success(), "Cannot verify deletion");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                anyhow::ensure!(gone, "Deletion timed out; inspect remaining resources");
            }
        }
        Ok(())
    }
}
include!("provider_catalog.rs");
include!("provider_discovery.rs");
fn segment(value: &str) -> Result<&str> {
    anyhow::ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Invalid region or size identifier"
    );
    Ok(value)
}
pub fn cloud_init(spec: &DeploySpec) -> String {
    // serde_json strings are valid YAML scalars; no untrusted shell interpolation.
    let key = serde_json::to_string(&spec.ssh_public_key).unwrap();
    format!("#cloud-config\nusers:\n  - default\n  - name: vsm\n    groups: [sudo]\n    shell: /bin/bash\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    ssh_authorized_keys:\n      - {key}\nssh_pwauth: false\npackage_update: true\npackages: [ufw, curl, ca-certificates, restic]\nruncmd:\n  - [ufw, default, deny, incoming]\n  - [ufw, allow, from, {}, to, any, port, '22', proto, tcp]\n  - [ufw, allow, '42420/tcp']\n  - [ufw, allow, '42420/udp']\n  - [ufw, --force, enable]\n",spec.admin_cidr)
}
#[cfg(test)]
mod tests {
    include!("provider_tests.rs");
}
