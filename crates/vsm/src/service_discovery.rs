#[derive(Deserialize)]
struct AdoptionRequest {
    vm_id: String,
    #[serde(default)]
    ssh_private_key: String,
    name: Option<String>,
}
#[derive(Deserialize)]
struct RecoveryConnection {
    ssh_private_key: String,
}
async fn discover_account(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.owner()?;
    let mut data = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        Cloud::new(c.db.account(&id)?).discover_azure(),
    )
    .await
    .context("Azure discovery timed out; retry")??;
    let existing = c.db.list::<Server>("servers")?;
    for item in data["deployments"]
        .as_array_mut()
        .context("Invalid discovery response")?
    {
        item["already_managed"] = json!(existing.iter().any(|s| s.id
            == item["server"]["id"].as_str().unwrap_or("")
            || s.resources.iter().any(|r| r
                .id
                .eq_ignore_ascii_case(item["vm_id"].as_str().unwrap_or("")))));
    }
    Ok(Json(data))
}
fn validate_recovery_key(key: &str) -> Result<()> {
    anyhow::ensure!(
        key.len() <= 4096 && !key.contains(['\0', '\n', '\r']),
        "Invalid SSH private key path"
    );
    if !key.is_empty() {
        anyhow::ensure!(
            std::path::Path::new(key).is_file(),
            "SSH private key file was not found on this Manager machine"
        );
    }
    Ok(())
}
fn save_recovered(db: &Db, server: &Server) -> Result<()> {
    let mut conn = db.0.lock().unwrap();
    let tx = conn.transaction()?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
        [server.account_id.as_deref().unwrap_or("")],
        |r| r.get(0),
    )?;
    anyhow::ensure!(
        exists,
        "Cloud account was removed during discovery; add it again and retry"
    );
    let bodies = {
        let mut q = tx.prepare("SELECT body FROM servers")?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for body in bodies {
        let old: Server = serde_json::from_str(&body)?;
        anyhow::ensure!(old.id!=server.id && !old.resources.iter().any(|r|server.resources.iter().any(|n|n.id.eq_ignore_ascii_case(&r.id))),"This deployment is already in the Manager or its ownership ID conflicts with an existing server");
    }
    tx.execute(
        "INSERT INTO servers(id,body) VALUES(?1,?2)",
        rusqlite::params![server.id, serde_json::to_string(server)?],
    )?;
    tx.commit()?;
    Ok(())
}
async fn adopt_server(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(body): Json<AdoptionRequest>,
) -> ApiResult {
    p.owner()?;
    validate_recovery_key(&body.ssh_private_key)?;
    let mut server = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        Cloud::new(c.db.account(&id)?).recover_azure(&body.vm_id),
    )
    .await
    .context("Azure recovery timed out; nothing was added")??;
    server.ssh_key = body.ssh_private_key;
    if let Some(name) = body.name {
        let name = name.trim();
        require!(
            !name.is_empty() && name.len() <= 100,
            "Server name must contain 1–100 characters"
        );
        server.name = name.into();
    }
    save_recovered(&c.db, &server)?;
    c.db.event(
        &server.id,
        "audit",
        &format!(
            "{} recovered Azure deployment inventory; Remote access remains unverified",
            p.name
        ),
    )?;
    let mut result = json!(server);
    result["recovery_warnings"]=json!(["Only resources with the deployment ownership tag were recovered. Untagged resources remain unmanaged and may continue to incur charges.","Remote access and existing schedules must be reviewed after reconnecting. Cloud cost-saving policies are not restored from Azure."]);
    if server.host.is_empty() {
        result["recovery_warnings"].as_array_mut().unwrap().push(json!("No public IPv4 address was found. Restore network access in Azure before reconnecting."));
    }
    Ok(Json(result))
}
fn recovery_idle(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    let active:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM operations WHERE server_id=?1 AND status IN ('queued','running'))",[id],|r|r.get(0))?;
    anyhow::ensure!(!active, "Wait for the active server operation to finish");
    Ok(())
}
async fn recovery_connection(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(body): Json<RecoveryConnection>,
) -> ApiResult {
    p.owner()?;
    validate_recovery_key(&body.ssh_private_key)?;
    let conn = c.db.0.lock().unwrap();
    recovery_idle(&conn, &id)?;
    let raw: String =
        conn.query_row("SELECT body FROM servers WHERE id=?1", [&id], |r| r.get(0))?;
    let mut server: Server = serde_json::from_str(&raw)?;
    require!(
        server.provider == Provider::Azure,
        "Recovery connection is for Azure deployments"
    );
    server.ssh_key = body.ssh_private_key;
    conn.execute(
        "UPDATE servers SET body=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(&server)?, id],
    )?;
    drop(conn);
    c.db.event(
        &id,
        "audit",
        &format!(
            "{} changed the recovered Remote connection key reference",
            p.name
        ),
    )?;
    Ok(Json(json!(server)))
}
async fn reconnect_server(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.owner()?;
    let server = c.db.server(&id)?;
    require!(
        server.provider == Provider::Azure,
        "Recovery connection is for Azure deployments"
    );
    require!(
        !server.ssh_key.is_empty(),
        "Select the existing SSH private key before reconnecting"
    );
    {
        recovery_idle(&c.db.0.lock().unwrap(), &id)?;
    }
    let data = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        transport::call(&server, reqwest::Method::GET, "/v1/status", None),
    )
    .await
    .context("Remote did not respond; check VM power, SSH access and allowed access address")??;
    let state = data["state"]
        .as_str()
        .filter(|s| ["running", "starting", "stopped", "sleeping"].contains(s))
        .context("Remote returned an invalid game state")?;
    let mut conn = c.db.0.lock().unwrap();
    let tx = conn.transaction()?;
    recovery_idle(&tx, &id)?;
    let current: String =
        tx.query_row("SELECT body FROM servers WHERE id=?1", [&id], |r| r.get(0))?;
    let mut current: Server = serde_json::from_str(&current)?;
    require!(
        current.ssh_key == server.ssh_key && current.host == server.host,
        "Connection changed during verification; reconnect again"
    );
    current.state = state.into();
    current.game_version = data["game_version"].as_str().unwrap_or("").into();
    tx.execute(
        "UPDATE servers SET body=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(&current)?, id],
    )?;
    tx.execute(
        "INSERT INTO meta(key,value) VALUES(?1,'true') ON CONFLICT(key) DO UPDATE SET value='true'",
        [format!("remote-installed:{id}")],
    )?;
    tx.commit()?;
    drop(conn);
    c.db.event(
        &id,
        "audit",
        &format!("{} verified and reconnected the existing Remote", p.name),
    )?;
    Ok(Json(json!(current)))
}
#[cfg(test)]
mod discovery_storage_tests {
    use super::*;
    #[test]
    fn adoption_is_atomic_and_rejects_duplicates_and_removed_accounts() {
        let temp = tempfile::tempdir().unwrap();
        let db = Db::open(&temp.path().join("db")).unwrap();
        let account = Account {
            id: "account".into(),
            name: "Azure".into(),
            provider: Provider::Azure,
            credential_ref: String::new(),
            settings: json!({}),
        };
        db.save_account(&account).unwrap();
        let server = Server {
            id: uuid::Uuid::new_v4().to_string(),
            name: "recovered".into(),
            account_id: Some(account.id.clone()),
            provider: Provider::Azure,
            region: String::new(),
            size: String::new(),
            host: String::new(),
            ssh_user: "vsm".into(),
            ssh_key: String::new(),
            game_version: String::new(),
            state: "recovered".into(),
            created_at: now(),
            resources: vec![Resource {
                id: "/vm/ONE".into(),
                kind: "vm".into(),
                owned: true,
            }],
        };
        save_recovered(&db, &server).unwrap();
        assert!(save_recovered(&db, &server).is_err());
        let mut duplicate = server.clone();
        duplicate.id = uuid::Uuid::new_v4().to_string();
        duplicate.resources[0].id = "/vm/one".into();
        assert!(save_recovered(&db, &duplicate).is_err());
        duplicate.account_id = Some("removed".into());
        duplicate.resources[0].id = "/other".into();
        assert!(save_recovered(&db, &duplicate).is_err());
        assert_eq!(db.list::<Server>("servers").unwrap().len(), 1);
        update_observed_state(&db, &server.id, &json!({"state":"stopped"})).unwrap();
        assert_eq!(db.server(&server.id).unwrap().state, "recovered");
    }
}
