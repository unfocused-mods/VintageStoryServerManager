// Discovery only reads ARM. Adoption never modifies or installs anything on a host.
fn discovery_resource_id(id: &str, subscription: &str) -> Result<()> {
    anyhow::ensure!(
        id.starts_with('/') && !id.contains(['?', '#', '%', '\\']) && !id.contains(".."),
        "Invalid Azure resource ID"
    );
    anyhow::ensure!(
        id.to_ascii_lowercase().starts_with(
            &format!("/subscriptions/{subscription}/resourcegroups/").to_ascii_lowercase()
        ),
        "Resource belongs to another subscription"
    );
    Ok(())
}
fn discovery_server(vm: &Value, account: &Account) -> Result<Server> {
    let id = vm["tags"]["vsm-owner"]
        .as_str()
        .context("Missing Manager ownership tag")?;
    let _: uuid::Uuid = id.parse().context("Invalid Manager ownership tag")?;
    let vm_id = vm["id"].as_str().context("VM resource ID missing")?;
    discovery_resource_id(
        vm_id,
        account.settings["subscription_id"]
            .as_str()
            .context("Subscription missing")?,
    )?;
    anyhow::ensure!(
        vm_id
            .to_ascii_lowercase()
            .contains("/providers/microsoft.compute/virtualmachines/"),
        "Not a VM resource"
    );
    Ok(Server {
        id: id.into(),
        name: vm["tags"]["vsm-name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("recovered-{}", &id[..8])),
        account_id: Some(account.id.clone()),
        provider: Provider::Azure,
        region: vm["location"].as_str().unwrap_or("").into(),
        size: vm["properties"]["hardwareProfile"]["vmSize"]
            .as_str()
            .unwrap_or("")
            .into(),
        host: String::new(),
        ssh_user: vm["properties"]["osProfile"]["adminUsername"]
            .as_str()
            .unwrap_or("vsm")
            .into(),
        ssh_key: String::new(),
        game_version: String::new(),
        state: "recovered".into(),
        created_at: now(),
        resources: vec![Resource {
            id: vm_id.into(),
            kind: "vm".into(),
            owned: true,
        }],
    })
}
impl Cloud {
    async fn discovery_list(&self, first: String, token: &str) -> Result<Vec<Value>> {
        let mut next = first;
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            anyhow::ensure!(
                seen.insert(next.clone()),
                "Azure discovery pagination repeated a page"
            );
            let page = self
                .request_with_token(Method::GET, &next, None, token)
                .await?;
            let rows = page["value"]
                .as_array()
                .context("Azure discovery returned no resource list")?;
            anyhow::ensure!(
                result.len() + rows.len() <= 10_000,
                "Azure discovery exceeds 10000 resources; narrow the subscription"
            );
            result.extend(rows.iter().cloned());
            match page["nextLink"].as_str().filter(|s| !s.is_empty()) {
                None => return Ok(result),
                Some(link) => {
                    let base = reqwest::Url::parse(self.base())?;
                    let url = base.join(link)?;
                    anyhow::ensure!(
                        url.origin() == base.origin()
                            && url.username().is_empty()
                            && url.password().is_none()
                            && url.fragment().is_none(),
                        "Unsafe Azure discovery continuation"
                    );
                    anyhow::ensure!(
                        url.path().to_ascii_lowercase().starts_with(
                            &format!("/subscriptions/{}/", self.subscription()?)
                                .to_ascii_lowercase()
                        ),
                        "Azure discovery continuation changed subscription"
                    );
                    next = format!(
                        "{}{}",
                        url.path(),
                        url.query().map(|q| format!("?{q}")).unwrap_or_default()
                    );
                }
            }
        }
        anyhow::bail!("Azure discovery exceeded 100 pages")
    }
    pub async fn discover_azure(&self) -> Result<Value> {
        anyhow::ensure!(
            self.account.provider == Provider::Azure,
            "Discovery currently supports Azure accounts"
        );
        let token = self.token().await?;
        let rows = self.discovery_list(format!("/subscriptions/{}/providers/Microsoft.Compute/virtualMachines?api-version=2024-03-01",self.subscription()?), &token).await?;
        let mut deployments = Vec::new();
        let mut warnings = Vec::new();
        for vm in rows {
            if vm["tags"]["vsm-owner"].is_null() {
                continue;
            }
            match discovery_server(&vm, &self.account) {
                Ok(server) => {
                    let vm_id = server.resources[0].id.clone();
                    let offset = vm_id
                        .to_ascii_lowercase()
                        .find("/providers/")
                        .context("Invalid VM resource ID")?;
                    let group = &vm_id[..offset];
                    deployments.push(json!({"server":server,"vm_id":vm_id,"resource_group":group,"already_managed":false,"warnings":["Remote access requires the existing SSH private key and host identity verification. Game version and state are checked when reconnecting."]}));
                }
                Err(_) => {
                    warnings.push("A tagged VM has invalid recovery metadata and was skipped.")
                }
            }
        }
        Ok(json!({"deployments":deployments,"warnings":warnings}))
    }
    pub async fn recover_azure(&self, vm_id: &str) -> Result<Server> {
        anyhow::ensure!(
            self.account.provider == Provider::Azure,
            "Discovery currently supports Azure accounts"
        );
        discovery_resource_id(vm_id, self.subscription()?)?;
        let token = self.token().await?;
        let vm = self
            .request_with_token(
                Method::GET,
                &format!("{vm_id}?api-version=2024-03-01"),
                None,
                &token,
            )
            .await?;
        let mut server = discovery_server(&vm, &self.account)?;
        anyhow::ensure!(
            server.resources[0].id.eq_ignore_ascii_case(vm_id),
            "Azure returned a different VM"
        );
        let lower = vm_id.to_ascii_lowercase();
        let offset = lower
            .find("/providers/")
            .context("Invalid VM resource ID")?;
        let group = &vm_id[..offset];
        let resources = self
            .discovery_list(format!("{group}/resources?api-version=2021-04-01"), &token)
            .await?;
        let mut owned = Vec::new();
        for resource in resources {
            if resource["tags"]["vsm-owner"] != server.id {
                continue;
            }
            let rid = resource["id"].as_str().context("Resource ID missing")?;
            discovery_resource_id(rid, self.subscription()?)?;
            anyhow::ensure!(
                rid.to_ascii_lowercase()
                    .starts_with(&format!("{group}/").to_ascii_lowercase()),
                "Resource escaped deployment group"
            );
            let ty = resource["type"].as_str().unwrap_or("").to_ascii_lowercase();
            let (kind, order) = match ty.as_str() {
                "microsoft.network/networksecuritygroups" => ("network", 0),
                "microsoft.network/virtualnetworks" => ("network", 1),
                "microsoft.network/publicipaddresses" => ("network", 2),
                "microsoft.network/networkinterfaces" => ("network", 3),
                "microsoft.compute/virtualmachines" => {
                    anyhow::ensure!(
                        rid.eq_ignore_ascii_case(vm_id),
                        "Multiple VMs share this ownership tag; automatic recovery refused"
                    );
                    ("vm", 4)
                }
                "microsoft.compute/disks" => ("disk", 5),
                _ => continue,
            };
            owned.push((
                order,
                Resource {
                    id: rid.into(),
                    kind: kind.into(),
                    owned: true,
                },
            ));
        }
        // Recover address only through the VM's actual NIC association, never an arbitrary IP.
        if let Some(nics) = vm["properties"]["networkProfile"]["networkInterfaces"].as_array() {
            for nic in nics.iter().take(8) {
                let Some(nic_id) = nic["id"].as_str() else {
                    continue;
                };
                discovery_resource_id(nic_id, self.subscription()?)?;
                let data = self
                    .request_with_token(
                        Method::GET,
                        &format!("{nic_id}?api-version=2024-05-01"),
                        None,
                        &token,
                    )
                    .await?;
                if let Some(configs) = data["properties"]["ipConfigurations"].as_array() {
                    for config in configs.iter().take(8) {
                        if let Some(ip_id) = config["properties"]["publicIPAddress"]["id"].as_str()
                        {
                            discovery_resource_id(ip_id, self.subscription()?)?;
                            let ip = self
                                .request_with_token(
                                    Method::GET,
                                    &format!("{ip_id}?api-version=2024-05-01"),
                                    None,
                                    &token,
                                )
                                .await?;
                            if let Some(address) = ip["properties"]["ipAddress"].as_str() {
                                if address.parse::<std::net::Ipv4Addr>().is_ok() {
                                    server.host = address.into();
                                    break;
                                }
                            }
                        }
                    }
                }
                if !server.host.is_empty() {
                    break;
                }
            }
        }
        owned.sort_by_key(|(order, _)| *order);
        server.resources = owned.into_iter().map(|(_, r)| r).collect();
        anyhow::ensure!(
            server.resources.iter().any(|r| r.kind == "vm"),
            "VM ownership could not be confirmed in its resource group"
        );
        Ok(server)
    }
}
