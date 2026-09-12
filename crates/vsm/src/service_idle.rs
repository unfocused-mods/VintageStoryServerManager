#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CloudIdlePolicy {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "idle_default_minutes")]
    minutes: u32,
    #[serde(default)]
    external_manager_confirmed: bool,
    #[serde(default)]
    last_error: Option<String>,
}
fn idle_default_minutes() -> u32 {
    15
}
impl Default for CloudIdlePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            minutes: 15,
            external_manager_confirmed: false,
            last_error: None,
        }
    }
}
fn idle_meta(db: &Db, key: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(db
        .0
        .lock()
        .unwrap()
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}
fn set_idle_meta(db: &Db, key: &str, value: &str) -> Result<()> {
    db.0.lock().unwrap().execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![key,value])?;
    Ok(())
}
fn read_cloud_idle_policy(db: &Db, id: &str) -> Result<CloudIdlePolicy> {
    Ok(idle_meta(db, &format!("cloud-idle:{id}"))?
        .map(|v| serde_json::from_str(&v))
        .transpose()?
        .unwrap_or_default())
}
fn cloud_wake_pending(db: &Db, id: &str) -> Result<bool> {
    Ok(idle_meta(db, &format!("cloud-idle-wake:{id}"))?.as_deref() == Some("true"))
}
fn cloud_idle_value(db: &Db, id: &str) -> Result<Value> {
    let mut policy = read_cloud_idle_policy(db, id)?;
    if policy.enabled && policy.last_error.is_none() {
        if let Some(record) = idle_meta(db, &format!("cloud-idle-observation:{id}"))? {
            if let Some(error) = serde_json::from_str::<Option<String>>(&record)? {
                policy.last_error = Some(format!(
                    "Automatic deallocation is paused: {error} VM compute remains billable."
                ));
            }
        }
    }
    let mut value = serde_json::to_value(policy)?;
    value["wake_pending"] = json!(cloud_wake_pending(db, id)?);
    Ok(value)
}
fn record_idle_observation(db: &Db, id: &str, status: &Value) -> Result<()> {
    if let Some(error) = status.get("idle").and_then(|idle| idle.get("error")) {
        if error.is_null() || error.is_string() {
            let serialized = serde_json::to_string(error)?;
            let key = format!("cloud-idle-observation:{id}");
            if idle_meta(db, &key)?.as_deref() != Some(&serialized) {
                set_idle_meta(db, &key, &serialized)?;
            }
        }
    }
    Ok(())
}
fn validate_cloud_idle_version(version: &str, enabled: bool) -> Result<()> {
    anyhow::ensure!(!enabled || crate::game_releases::tested_idle_version(version),"Automatic deallocation requires Vintage Story 1.22.0 or 1.22.7, releases with verified empty-player detection. Update the game before enabling it; other versions remain billable while running.");
    Ok(())
}
async fn get_cloud_idle_policy(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.owner()?;
    c.db.server(&id)?;
    Ok(Json(cloud_idle_value(&c.db, &id)?))
}
async fn save_cloud_idle_policy(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(mut policy): Json<CloudIdlePolicy>,
) -> ApiResult {
    p.owner()?;
    let s = c.db.server(&id)?;
    require!(s.provider==Provider::Azure,"Automatic compute savings currently require Azure deallocation. Other supported providers keep charging for stopped VMs.");
    require!(
        (1..=1440).contains(&policy.minutes),
        "Idle timeout must be 1â€“1440 minutes"
    );
    validate_cloud_idle_version(&s.game_version, policy.enabled)?;
    if policy.enabled {
        require!(
            policy.external_manager_confirmed,
            "Confirm that this Manager runs outside the VM being managed and remains online"
        );
        ensure_outside_vm(&s).await?;
    }
    let hash = auth::hash(&serde_json::to_string(&policy)?);
    let (operation, new) =
        c.db.enqueue(&id, "cloud-idle-policy", request_key(&h)?, &hash)?;
    if !new {
        return Ok(Json(cloud_idle_value(&c.db, &id)?));
    }
    c.db.progress(
        &operation.id,
        "running",
        "Updating automatic deallocation policy",
    )?;
    let result=async {
        policy.last_error=None;
        if !policy.enabled {
            // Disable the controller decision immediately, even if the VM is already deallocated.
            set_idle_meta(&c.db,&format!("cloud-idle:{id}"),&serde_json::to_string(&policy)?)?;
        }
        if policy.enabled || !matches!(s.state.as_str(),"deallocated"|"deleted") {
            let agent=tokio::time::timeout(std::time::Duration::from_secs(25),transport::call(&s,reqwest::Method::PUT,"/v1/idle-policy",Some(json!({"enabled":policy.enabled,"minutes":policy.minutes})))).await;
            if policy.enabled {agent.context("Remote policy update timed out")??;}
            else if !matches!(agent,Ok(Ok(_))) {policy.last_error=Some("Cloud deallocation is disabled. The Remote could not be reached to turn off game sleep.".into());}
        }
        set_idle_meta(&c.db,&format!("cloud-idle:{id}"),&serde_json::to_string(&policy)?)?;
        c.db.event(&id,"audit",&format!("{} {} automatic Azure deallocation",p.name,if policy.enabled{"enabled"}else{"disabled"}))?;
        Ok::<_,anyhow::Error>(())
    }.await;
    c.db.progress(
        &operation.id,
        if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        },
        if result.is_ok() {
            "Automatic deallocation policy saved"
        } else {
            "Could not apply automatic deallocation policy"
        },
    )?;
    result?;
    Ok(Json(cloud_idle_value(&c.db, &id)?))
}
async fn ensure_outside_vm(s: &Server) -> Result<()> {
    // An explicit topology acknowledgement is required because metadata is absent outside Azure.
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    if let Ok(response)=client.get("http://169.254.169.254/metadata/instance/compute/resourceId?api-version=2021-02-01&format=text").header("Metadata","true").send().await {
        if response.status().is_success() {
            let own=response.text().await?.trim().to_ascii_lowercase();
            anyhow::ensure!(!s.resources.iter().any(|r|r.id.to_ascii_lowercase()==own),"This Manager is running on the VM it would deallocate. Move the Manager to another computer first.");
        }
    }
    Ok(())
}
fn consider_cloud_idle(c: &Controller, s: &Server, status: &Value) -> Result<()> {
    if s.provider == Provider::Azure {
        record_idle_observation(&c.db, &s.id, status)?;
    }
    let policy = read_cloud_idle_policy(&c.db, &s.id)?;
    if !policy.enabled
        || !policy.external_manager_confirmed
        || s.provider != Provider::Azure
        || status["state"] != "sleeping"
        || status["idle"]["policy"]["enabled"] != true
    {
        return Ok(());
    }
    let sleep_id = status["idle"]["sleep_id"]
        .as_str()
        .context("Missing sleep generation")?;
    uuid::Uuid::parse_str(sleep_id)?;
    let key = format!("auto-idle:{}:{sleep_id}", s.id);
    let (operation, new) = c.db.enqueue(&s.id, "deallocate", &key, &auth::hash(&key))?;
    if !new {
        return Ok(());
    }
    let c = c.clone();
    let s = s.clone();
    let sleep_id = sleep_id.to_owned();
    tokio::spawn(async move {
        let _ = c.db.progress(
            &operation.id,
            "running",
            "Confirming idle state before Azure deallocation",
        );
        let result = deallocate_idle_vm(&c, &s, &sleep_id).await;
        match result {
            Ok(true) => {
                let _=c.db.progress(&operation.id,"succeeded","Azure VM deallocated. Compute billing stopped; disk and IP charges remain. Start VM from this Manager to play.");
            }
            Ok(false) => {
                let _ = c.db.progress(
                    &operation.id,
                    "cancelled",
                    "Game woke or idle observation changed; Azure VM kept running.",
                );
            }
            Err(e) => {
                let message=format!("Automatic deallocation stopped: {e}. Review infrastructure and use Start VM to recover.");
                let _ = disable_failed_cloud_idle(&c.db, &s.id, &message);
                let _ = c.db.progress(&operation.id, "failed", &message);
            }
        }
    });
    Ok(())
}
fn disable_failed_cloud_idle(db: &Db, id: &str, error: &str) -> Result<()> {
    let mut policy = read_cloud_idle_policy(db, id)?;
    policy.enabled = false;
    policy.last_error = Some(error.into());
    set_idle_meta(
        db,
        &format!("cloud-idle:{id}"),
        &serde_json::to_string(&policy)?,
    )
}
async fn deallocate_idle_vm(c: &Controller, s: &Server, sleep_id: &str) -> Result<bool> {
    anyhow::ensure!(
        read_cloud_idle_policy(&c.db, &s.id)?.enabled,
        "Automatic deallocation was disabled"
    );
    ensure_outside_vm(s).await?;
    // The Remote checks the exact sleep generation and pending wake under its mutation lock.
    set_idle_meta(&c.db, &format!("cloud-idle-wake:{}", s.id), "true")?;
    let reservation = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        transport::call(
            s,
            reqwest::Method::POST,
            "/v1/actions/prepare-cloud-sleep",
            Some(json!({"sleep_id":sleep_id})),
        ),
    )
    .await
    .context("Remote idle reservation timed out")??;
    if !accept_cloud_reservation(&c.db, &s.id, &reservation)? {
        return Ok(false);
    }
    let cloud = Cloud::new(c.db.account(s.account_id.as_deref().context("No cloud account")?)?);
    cloud.power(s, "deallocate").await?;
    cloud.wait_power(s, "deallocate").await?;
    let mut updated = c.db.server(&s.id)?;
    updated.state = "deallocated".into();
    c.db.put("servers", &s.id, &updated)?;
    Ok(true)
}
fn accept_cloud_reservation(db: &Db, id: &str, reservation: &Value) -> Result<bool> {
    if reservation["prepared"] == false
        && matches!(
            reservation["reason"].as_str(),
            Some("stale" | "wake_requested")
        )
    {
        set_idle_meta(db, &format!("cloud-idle-wake:{id}"), "false")?;
        return Ok(false);
    }
    anyhow::ensure!(
        reservation["prepared"] == true,
        "Remote did not confirm cloud shutdown reservation"
    );
    Ok(true)
}
async fn resume_cloud_game(c: &Controller, s: &Server) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
    let mut configured = false;
    let mut start_requested = false;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(status)) = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            transport::call(s, reqwest::Method::GET, "/v1/status", None),
        )
        .await
        {
            if !configured {
                let policy = read_cloud_idle_policy(&c.db, &s.id)?;
                tokio::time::timeout(
                    std::time::Duration::from_secs(35),
                    configure_cloud_wake(
                        &policy,
                        status["state"].as_str().unwrap_or(""),
                        |method, path, body| transport::call(s, method, path, body),
                    ),
                )
                .await
                .context("Game wake request timed out")??;
                start_requested = !matches!(status["state"].as_str(), Some("running" | "starting"));
                configured = true;
                continue; // Require a fresh health observation after policy/start mutation.
            }
            if status["state"] == "running" {
                let mut updated = c.db.server(&s.id)?;
                updated.state = "running".into();
                c.db.put("servers", &s.id, &updated)?;
                set_idle_meta(&c.db, &format!("cloud-idle-wake:{}", s.id), "false")?;
                return Ok(
                    json!({"completed":true,"state":"running","note":"VM and game are running. Compute billing has resumed."}),
                );
            }
            if !start_requested && status["state"] != "starting" {
                tokio::time::timeout(
                    std::time::Duration::from_secs(25),
                    transport::call(
                        s,
                        reqwest::Method::POST,
                        "/v1/actions/resume-cloud-sleep",
                        Some(json!({})),
                    ),
                )
                .await
                .context("Game wake request timed out")??;
                start_requested = true;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    anyhow::bail!("VM started, but game readiness could not be confirmed. Inspect the Remote logs; compute billing has resumed.")
}
async fn configure_cloud_wake<F, Fut>(
    policy: &CloudIdlePolicy,
    state: &str,
    mut request: F,
) -> Result<()>
where
    F: FnMut(reqwest::Method, &'static str, Option<Value>) -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    request(
        reqwest::Method::PUT,
        "/v1/idle-policy",
        Some(json!({"enabled":policy.enabled,"minutes":policy.minutes})),
    )
    .await?;
    if !matches!(state, "running" | "starting") {
        request(
            reqwest::Method::POST,
            "/v1/actions/resume-cloud-sleep",
            Some(json!({})),
        )
        .await?;
    }
    Ok(())
}
#[cfg(test)]
mod cloud_idle_tests {
    use super::*;
    #[test]
    fn unsupported_game_version_cannot_enable_cloud_idle_but_can_disable_it() {
        assert!(validate_cloud_idle_version("1.22.0", true).is_ok());
        for version in ["1.21.6", "1.22.1", "unknown"] {
            assert!(validate_cloud_idle_version(version, true).is_err());
            assert!(validate_cloud_idle_version(version, false).is_ok());
        }
    }
    #[test]
    fn observed_idle_errors_are_visible_and_clear_without_overwriting_operation_errors() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("db")).unwrap();
        let policy = CloudIdlePolicy {
            enabled: true,
            ..Default::default()
        };
        set_idle_meta(
            &db,
            "cloud-idle:s",
            &serde_json::to_string(&policy).unwrap(),
        )
        .unwrap();
        record_idle_observation(
            &db,
            "s",
            &json!({"idle":{"error":"Player list could not be confirmed."}}),
        )
        .unwrap();
        let value = cloud_idle_value(&db, "s").unwrap();
        assert_eq!(value["last_error"],"Automatic deallocation is paused: Player list could not be confirmed. VM compute remains billable.");
        record_idle_observation(&db, "s", &json!({"idle":{"error":null}})).unwrap();
        assert!(cloud_idle_value(&db, "s").unwrap()["last_error"].is_null());
        disable_failed_cloud_idle(&db, "s", "ARM failed").unwrap();
        record_idle_observation(&db, "s", &json!({"idle":{"error":null}})).unwrap();
        assert_eq!(
            cloud_idle_value(&db, "s").unwrap()["last_error"],
            "ARM failed"
        );
    }
    #[test]
    fn definitive_wake_refusal_keeps_policy_but_uncertain_response_keeps_intent() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("db")).unwrap();
        let policy = CloudIdlePolicy {
            enabled: true,
            ..Default::default()
        };
        set_idle_meta(
            &db,
            "cloud-idle:s",
            &serde_json::to_string(&policy).unwrap(),
        )
        .unwrap();
        set_idle_meta(&db, "cloud-idle-wake:s", "true").unwrap();
        assert!(!accept_cloud_reservation(
            &db,
            "s",
            &json!({"prepared":false,"reason":"wake_requested"})
        )
        .unwrap());
        assert!(read_cloud_idle_policy(&db, "s").unwrap().enabled);
        assert!(!cloud_wake_pending(&db, "s").unwrap());
        set_idle_meta(&db, "cloud-idle-wake:s", "true").unwrap();
        assert!(accept_cloud_reservation(&db, "s", &json!({"unknown":true})).is_err());
        assert!(cloud_wake_pending(&db, "s").unwrap());
    }
    #[tokio::test]
    async fn wake_reconciles_disabled_policy_before_starting_game() {
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = calls.clone();
        configure_cloud_wake(
            &CloudIdlePolicy::default(),
            "stopped",
            move |method, path, body| {
                recorded.lock().unwrap().push((method, path, body));
                async { Ok(json!({})) }
            },
        )
        .await
        .unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].1, "/v1/idle-policy");
        assert_eq!(calls[0].2.as_ref().unwrap()["enabled"], false);
        assert_eq!(calls[1].1, "/v1/actions/resume-cloud-sleep");
    }
    #[test]
    fn delayed_agent_observation_cannot_undo_deallocation() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("db")).unwrap();
        db.put("servers", "s", &json!({"id":"s","state":"deallocated"}))
            .unwrap();
        update_observed_state(&db, "s", &json!({"state":"sleeping"})).unwrap();
        assert_eq!(
            db.list::<Value>("servers").unwrap()[0]["state"],
            "deallocated"
        );
    }
    #[test]
    fn failed_cloud_attempt_disables_retry_but_keeps_wake_intent() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("db")).unwrap();
        let policy = CloudIdlePolicy {
            enabled: true,
            external_manager_confirmed: true,
            ..Default::default()
        };
        set_idle_meta(
            &db,
            "cloud-idle:s",
            &serde_json::to_string(&policy).unwrap(),
        )
        .unwrap();
        set_idle_meta(&db, "cloud-idle-wake:s", "true").unwrap();
        disable_failed_cloud_idle(&db, "s", "ARM error").unwrap();
        assert!(!read_cloud_idle_policy(&db, "s").unwrap().enabled);
        assert!(cloud_wake_pending(&db, "s").unwrap());
        assert_eq!(
            read_cloud_idle_policy(&db, "s")
                .unwrap()
                .last_error
                .as_deref(),
            Some("ARM error")
        );
    }
    #[test]
    fn idle_cycles_are_idempotent_and_serialize_with_manual_operations() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("db")).unwrap();
        let (first, new) = db
            .enqueue("s", "deallocate", "auto-idle:s:cycle", "hash")
            .unwrap();
        assert!(new);
        assert!(
            !db.enqueue("s", "deallocate", "auto-idle:s:cycle", "hash")
                .unwrap()
                .1
        );
        assert!(db
            .enqueue("s", "cloud-start", "manual", "other")
            .is_err());
        db.progress(&first.id, "succeeded", "done").unwrap();
        assert!(
            !db.enqueue("s", "deallocate", "auto-idle:s:cycle", "hash")
                .unwrap()
                .1
        );
        assert!(
            db.enqueue("s", "deallocate", "auto-idle:s:new-cycle", "new")
                .unwrap()
                .1
        );
    }
}
