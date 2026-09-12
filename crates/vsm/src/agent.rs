macro_rules! ensure { ($cond:expr, $($arg:tt)*) => { if !$cond { return Err(anyhow::anyhow!($($arg)*).into()); } }; }
macro_rules! bail { ($($arg:tt)*) => { return Err(anyhow::anyhow!($($arg)*).into()) }; }
use crate::{auth, domain::now, files};
use anyhow::{Context, Result};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::VecDeque, path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::Mutex,
};

#[derive(Clone)]
pub struct Agent {
    root: PathBuf,
    token: String,
    process: Arc<Mutex<Option<Child>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    logs: Arc<Mutex<VecDeque<Value>>>,
    log_sequence: Arc<std::sync::atomic::AtomicU64>,
    mutation: Arc<Mutex<()>>,
    desired: Arc<Mutex<bool>>,
    ready: Arc<std::sync::atomic::AtomicBool>,
    idle: Arc<Mutex<IdleState>>,
    wake: Arc<Mutex<Option<WakeSockets>>>,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct BackupPolicy {
    pub enabled: bool,
    pub repository: String,
    pub password_env: String,
    pub hour_utc: u8,
    pub daily: u32,
    pub weekly: u32,
    #[serde(default)]
    pub last_success: Option<String>,
}
impl Default for BackupPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            repository: String::new(),
            password_env: "RESTIC_PASSWORD".into(),
            hour_utc: 3,
            daily: 7,
            weekly: 4,
            last_success: None,
        }
    }
}
type ApiResult = std::result::Result<Json<Value>, AgentError>;
pub struct AgentError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for AgentError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
impl axum::response::IntoResponse for AgentError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}
impl Agent {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        auth::protect_path(&root, true)?;
        std::fs::create_dir_all(root.join("data"))?;
        std::fs::create_dir_all(root.join("recovery"))?;
        auth::protect_path(&root.join("recovery"), true)?;
        std::fs::create_dir_all(root.join("releases"))?;
        let path = root.join("agent.token");
        if !path.exists() {
            auth::private_write(&path, auth::token().as_bytes())?;
        }
        let token = std::fs::read_to_string(path)?;
        let identity = root.join("identity");
        if !identity.exists() {
            auth::private_write(&identity, uuid::Uuid::new_v4().to_string().as_bytes())?;
        }
        Ok(Self {
            root: root.clone(),
            token,
            process: Default::default(),
            stdin: Default::default(),
            logs: Default::default(),
            log_sequence: Default::default(),
            mutation: Default::default(),
            desired: Arc::new(Mutex::new(
                std::fs::read_to_string(root.join("desired")).is_ok_and(|v| v == "running"),
            )),
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            idle: Arc::new(Mutex::new(IdleState::default())),
            wake: Default::default(),
        })
    }
    fn authorize(&self, headers: &HeaderMap) -> Result<()> {
        let value = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        ensure!(auth::hash(value) == auth::hash(&self.token), "Unauthorized");
        Ok(())
    }
    async fn running(&self) -> Result<bool> {
        let mut p = self.process.lock().await;
        if let Some(child) = p.as_mut() {
            if child.try_wait()?.is_none() {
                return Ok(true);
            }
            *p = None;
        }
        Ok(false)
    }
    fn active_version(&self) -> Result<String> {
        Ok(std::fs::read_to_string(self.root.join("version"))?
            .trim()
            .into())
    }
    async fn command(&self, text: &str) -> Result<()> {
        ensure!(
            !text.contains(['\n', '\r']) && text.len() <= 4096,
            "Invalid console command"
        );
        let mut stdin = self.stdin.lock().await;
        let stdin = stdin.as_mut().context("Server is not running")?;
        stdin
            .write_all(format!("/{}\n", text.trim().trim_start_matches('/')).as_bytes())
            .await?;
        stdin.flush().await?;
        Ok(())
    }
    async fn start(&self) -> Result<()> {
        ensure!(
            !std::fs::read_to_string(self.root.join("desired")).is_ok_and(|v| v == "cloud_sleep"),
            "VM is reserved for cloud shutdown. Use Start VM from the Manager."
        );
        ensure!(!self.running().await?, "Server is already running");
        self.disarm_idle().await?;
        self.ready
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let version = self.active_version()?;
        let release = files::confined(&self.root.join("releases"), &version)?;
        ensure!(
            release.join("VintagestoryServer.dll").is_file(),
            "Game runtime is not installed"
        );
        let mut command = self.game_command().await?;
        let mut child = command
            .arg(release.join("VintagestoryServer.dll"))
            .args([
                "--dataPath",
                self.root
                    .join("data")
                    .to_str()
                    .context("Invalid data path")?,
            ])
            .current_dir(release)
            .env_clear()
            .env(
                "PATH",
                std::env::var("PATH").unwrap_or("/usr/bin:/bin".into()),
            )
            .env("HOME", self.root.join("data"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        *self.stdin.lock().await = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        for (reader, level) in [
            (
                Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
                "info",
            ),
            (
                Box::new(stderr) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
                "error",
            ),
        ] {
            let logs = self.logs.clone();
            let root = self.root.clone();
            let ready = self.ready.clone();
            let sequence = self.log_sequence.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(reader).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.contains("Dedicated Server now running") {
                        ready.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    let item = json!({"sequence":sequence.fetch_add(1,std::sync::atomic::Ordering::Relaxed)+1,"at":now(),"level":level,"message":redact(&line)});
                    let mut log = logs.lock().await;
                    log.push_back(item.clone());
                    if log.len() > 2000 {
                        log.pop_front();
                    }
                    drop(log);
                    let path = root.join("console.jsonl");
                    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 10 * 1024 * 1024) {
                        let _ = std::fs::rename(&path, root.join("console.previous.jsonl"));
                    }
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        use std::io::Write;
                        let _ = writeln!(file, "{item}");
                    }
                }
            });
        }
        *self.process.lock().await = Some(child);
        *self.desired.lock().await = true;
        std::fs::write(self.root.join("desired"), "running")?;
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        if std::fs::read_to_string(self.root.join("desired")).is_ok_and(|v| v == "cloud_sleep") {
            return Ok(());
        }
        self.disarm_idle().await?;
        self.stop_game().await
    }
    async fn stop_game(&self) -> Result<()> {
        *self.desired.lock().await = false;
        std::fs::write(self.root.join("desired"), "stopped")?;
        if !self.running().await? {
            return Ok(());
        }
        self.command("stop").await?;
        for _ in 0..120 {
            if !self.running().await? {
                *self.stdin.lock().await = None;
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        bail!("Graceful stop timed out; no files changed and process was not force-killed")
    }
    async fn recovery(&self) -> Result<PathBuf> {
        let size = tree_bytes(&self.root.join("data"))?;
        let limit = std::env::var("VSM_RECOVERY_LIMIT_GIB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(20)
            * 1024
            * 1024
            * 1024;
        ensure!(size <= limit, "Recovery copy exceeds VSM_RECOVERY_LIMIT_GIB (default 20 GiB); increase the limit or free data before changing the server");
        ensure!(
            fs2::available_space(&self.root)? > size + 512 * 1024 * 1024,
            "Insufficient free disk space for a safe recovery copy"
        );
        ensure!(
            !self.running().await?,
            "Server must be stopped before recovery copy"
        );
        let id = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
        let stage = self.root.join("recovery").join(format!(".{id}.partial"));
        let root = self.root.clone();
        let dest = stage.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            files::copy_tree(&root.join("data"), &dest.join("data"))?;
            if root.join("version").exists() {
                std::fs::copy(root.join("version"), dest.join("version"))?;
            }
            if root.join("mods.json").exists() {
                std::fs::copy(root.join("mods.json"), dest.join("mods.json"))?;
            }
            Ok(())
        })
        .await??;
        let final_path = self.root.join("recovery").join(&id);
        std::fs::rename(stage, &final_path)?;
        let mut entries = std::fs::read_dir(self.root.join("recovery"))?
            .filter_map(|e| e.ok())
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .collect::<Vec<_>>();
        entries.sort_by_key(|e| e.file_name());
        while entries.len() > 3 {
            let entry = entries.remove(0);
            let target = files::confined(
                &self.root.join("recovery"),
                entry
                    .file_name()
                    .to_str()
                    .context("Invalid recovery path")?,
            )?;
            std::fs::remove_dir_all(target)?;
        }
        Ok(final_path)
    }
    async fn backup(&self) -> Result<Value> {
        let was_running = self.running().await?;
        let was_sleeping = self.is_sleeping();
        self.stop_game().await?;
        let copy = self.recovery().await;
        if was_running {
            self.start().await?;
        }
        if was_sleeping {
            std::fs::write(self.root.join("desired"), "sleeping")?;
        }
        let copy = copy?;
        let mut policy = self.policy()?;
        if !policy.enabled {
            return Ok(json!({"recovery":copy.file_name(),"off_host":false}));
        }
        ensure!(
            !policy.repository.is_empty(),
            "Backup repository is required"
        );
        let password = std::env::var(&policy.password_env)
            .context("Backup password environment variable missing on host")?;
        let backup_tag = format!(
            "vsm:{}",
            std::fs::read_to_string(self.root.join("identity"))?.trim()
        );
        let output = crate::subprocess::command("restic")
            .args(["-r", &policy.repository, "backup", "--json"])
            .args(["--tag", &backup_tag])
            .arg(&copy)
            .env("RESTIC_PASSWORD", &password)
            .output()
            .await?;
        ensure!(output.status.success(),"Restic backup failed. Ensure the repository is initialized and host credentials are configured.");
        let retain = crate::subprocess::command("restic")
            .args([
                "-r",
                &policy.repository,
                "forget",
                "--tag",
                &backup_tag,
                "--group-by",
                "",
                "--keep-daily",
                &policy.daily.to_string(),
                "--keep-weekly",
                &policy.weekly.to_string(),
            ])
            .env("RESTIC_PASSWORD", &password)
            .output()
            .await?;
        ensure!(
            retain.status.success(),
            "Backup uploaded, but retention failed"
        );
        policy.last_success = Some(now());
        std::fs::write(
            self.root.join("backup.json"),
            serde_json::to_vec_pretty(&policy)?,
        )?;
        Ok(json!({"recovery":copy.file_name(),"off_host":true,"last_success":policy.last_success}))
    }
    fn policy(&self) -> Result<BackupPolicy> {
        let p = self.root.join("backup.json");
        if p.exists() {
            Ok(serde_json::from_slice(&std::fs::read(p)?)?)
        } else {
            Ok(BackupPolicy::default())
        }
    }
    async fn install(&self, version: &str) -> Result<Value> {
        ensure!(
            crate::game_releases::stable_version(version).is_some(),
            "Only explicit stable numeric releases are supported"
        );
        // Download and verify before interrupting a running world.
        self.download_release(version).await?;
        let was_running = self.running().await?;
        self.stop().await?;
        let old = self.active_version().ok();
        if old.is_some() {
            if let Err(e) = self.recovery().await {
                if was_running {
                    self.start().await?;
                }
                return Err(e);
            }
        }
        std::fs::write(self.root.join("version"), version)?;
        if was_running {
            self.start().await?;
        }
        Ok(
            json!({"installed":version,"previous_version":old,"state":if was_running{"starting"}else{"stopped"}}),
        )
    }
    async fn download_release(&self, version: &str) -> Result<Value> {
        ensure!(
            crate::game_releases::stable_version(version).is_some(),
            "An exact stable game version is required"
        );
        let dest = self.root.join("releases").join(version);
        if dest.join("VintagestoryServer.dll").exists() {
            return Ok(json!({"cached":true}));
        }
        let release = crate::game_releases::resolve(version).await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut res = client
            .get(&release.download_url)
            .send()
            .await?
            .error_for_status()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = res.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 1024 * 1024 * 1024,
                "Release exceeds 1 GiB download limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        crate::game_releases::verify_archive(&bytes, &release)?;
        let stage = self
            .root
            .join("releases")
            .join(format!(".{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&stage)?;
        let work = stage.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut archive =
                tar::Archive::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)));
            let mut total = 0u64;
            for entry in archive.entries()? {
                let mut e = entry?;
                ensure!(
                    e.header().entry_type().is_file() || e.header().entry_type().is_dir(),
                    "Links are not accepted in release archives"
                );
                total += e.size();
                ensure!(
                    total <= 4 * 1024 * 1024 * 1024,
                    "Extracted release exceeds 4 GiB"
                );
                let p = e.path()?.to_string_lossy().to_string();
                let target = files::confined(&work, &p)?;
                if e.header().entry_type().is_dir() {
                    std::fs::create_dir_all(target)?;
                } else {
                    if let Some(p) = target.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    e.unpack(target)?;
                }
            }
            Ok(())
        })
        .await??;
        let actual = if stage.join("VintagestoryServer.dll").exists() {
            stage.clone()
        } else {
            std::fs::read_dir(&stage)?
                .filter_map(|e| e.ok())
                .find(|e| e.path().join("VintagestoryServer.dll").exists())
                .context("Downloaded release has no server executable")?
                .path()
        };
        std::fs::rename(actual, &dest)?;
        Ok(json!({"downloaded":true,"checksum_verified":true}))
    }
    async fn restore(&self, id: &str) -> Result<Value> {
        let source = files::confined(&self.root.join("recovery"), id)?;
        ensure!(source.join("data").is_dir(), "Recovery not found");
        // Stage before taking a new recovery copy, which may rotate the selected source.
        let stage = self.root.join(format!("restore-{}", uuid::Uuid::new_v4()));
        files::copy_tree(&source, &stage)?;
        let version = std::fs::read_to_string(stage.join("version"))?;
        self.download_release(version.trim()).await?;
        let was_running = self.running().await?;
        self.stop().await?;
        self.recovery().await?;
        let previous = self.root.join(format!("previous-{}", uuid::Uuid::new_v4()));
        std::fs::rename(self.root.join("data"), &previous)?;
        if let Err(e) = std::fs::rename(stage.join("data"), self.root.join("data")) {
            std::fs::rename(&previous, self.root.join("data"))?;
            return Err(e.into());
        }
        std::fs::write(self.root.join("version"), version)?;
        if stage.join("mods.json").exists() {
            std::fs::copy(stage.join("mods.json"), self.root.join("mods.json"))?;
        }
        if was_running {
            self.start().await?;
        }
        Ok(json!({"restored":id,"previous_data_retained":previous.file_name()}))
    }
}
fn redact(line: &str) -> String {
    let lower = line.to_lowercase();
    if [
        "password",
        "authorization",
        "access_token",
        "secret",
        "token=",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        "[redacted sensitive log entry]".into()
    } else {
        line.chars().take(16000).collect()
    }
}
pub async fn run(root: PathBuf) -> Result<()> {
    let agent = Agent::new(root)?;
    agent.spawn_wake_loop();
    let scheduler = agent.clone();
    tokio::spawn(async move {
        let mut failures = 0;
        let mut backup_attempt = std::time::Instant::now() - std::time::Duration::from_secs(3600);
        let mut last_restart = std::time::Instant::now();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let Ok(_lock) = scheduler.mutation.try_lock() else {
                continue;
            };
            if let Err(e) = scheduler.idle_tick().await {
                let message = e.to_string();
                let mut state = scheduler.idle.lock().await;
                if state.error.as_deref() != Some(&message) {
                    scheduler.agent_event("error", &format!("Automatic sleep paused: {message}"));
                }
                state.error = Some(message);
                state.since = None;
            }
            if let Ok(p) = scheduler.policy() {
                use chrono::Timelike;
                let today = chrono::Utc::now().date_naive().to_string();
                if p.enabled
                    && backup_attempt.elapsed().as_secs() >= 900
                    && chrono::Utc::now().hour() == p.hour_utc as u32
                    && !p
                        .last_success
                        .as_deref()
                        .is_some_and(|s| s.starts_with(&today))
                {
                    backup_attempt = std::time::Instant::now();
                    if let Err(e) = scheduler.backup().await {
                        scheduler.agent_event("error", &format!("Scheduled backup failed: {e}"));
                    }
                }
            }
            if let Err(e) = scheduler.scheduled_restart().await {
                scheduler.agent_event("error", &format!("Scheduled restart failed: {e}"));
            }
            let should_run = *scheduler.desired.lock().await;
            if should_run && !scheduler.running().await.unwrap_or(false) {
                if last_restart.elapsed().as_secs() > 600 {
                    failures = 0;
                }
                if failures < 3 {
                    failures += 1;
                    last_restart = std::time::Instant::now();
                    let _ = scheduler.start().await;
                } else {
                    *scheduler.desired.lock().await = false;
                    let _ = std::fs::write(scheduler.root.join("desired"), "stopped");
                    scheduler.agent_event(
                        "error",
                        "Automatic restart limit reached. Inspect logs and start manually.",
                    );
                }
            }
        }
    });
    let app = Router::new()
        .route("/v1/status", get(status))
        .route("/v1/actions/{action}", post(action))
        .route("/v1/logs", get(logs))
        .route("/v1/config", get(config).put(save_config))
        .route("/v1/files", get(file_list).post(file_write))
        .route("/v1/file", get(file_read))
        .route("/v1/backups", get(backups))
        .route("/v1/backup-policy", get(policy).put(save_policy))
        .route("/v1/mods", get(mods).post(install_mod))
        .route("/v1/operation-logs/{id}", get(agent_operation_logs))
        .route("/v1/schedule", get(schedule).put(save_schedule))
        .route("/v1/idle-policy", get(idle_policy).put(save_idle_policy))
        .route("/v1/worlds", get(worlds))
        .route("/v1/world-options", get(world_options))
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
        .with_state(agent);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4318").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
async fn status(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    let running = a.running().await?;
    Ok(Json(
        json!({"state":if running {if a.ready.load(std::sync::atomic::Ordering::Relaxed){"running"}else{"starting"}}else if a.is_sleeping(){"sleeping"}else{"stopped"},"game_version":a.active_version().ok(),"agent_binary_sha256":a.binary_identity(),"metrics":{"load":std::fs::read_to_string("/proc/loadavg").ok(),"memory":std::fs::read_to_string("/proc/meminfo").ok(),"disk_free_bytes":fs2::available_space(&a.root).ok(),"players":null},"backup_policy":a.policy()?,"idle":a.idle_status().await?}),
    ))
}
async fn action(
    State(a): State<Agent>,
    h: HeaderMap,
    Path(action): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    a.authorize(&h)?;
    let _lock = a
        .mutation
        .try_lock()
        .map_err(|_| anyhow::anyhow!("Another server operation is running"))?;
    let data = match action.as_str() {
        "prepare-cloud-sleep" => {
            a.prepare_cloud_sleep(body["sleep_id"].as_str().context("sleep_id required")?)
                .await?
        }
        "resume-cloud-sleep" => {
            if !a.running().await? {
                std::fs::write(a.root.join("desired"), "stopped")?;
                a.start().await?;
            }
            json!({"state":"starting"})
        }
        "start" => {
            a.start().await?;
            json!({"state":"starting"})
        }
        "stop" => {
            a.stop().await?;
            json!({"state":"stopped"})
        }
        "restart" => {
            a.stop().await?;
            a.start().await?;
            json!({"state":"starting"})
        }
        "console" => {
            let command = body["command"].as_str().context("command is required")?;
            if command.trim().trim_start_matches('/') == "stop" {
                a.stop().await?;
            } else {
                a.command(command).await?;
            }
            json!({"accepted":true})
        }
        "install" | "update" => {
            a.install(body["version"].as_str().context("version is required")?)
                .await?
        }
        "backup" => a.backup().await?,
        "file-move" | "file-delete" | "file-mkdir" | "file-extract" => {
            a.file_operation(&action, &body).await?
        }
        "mod-enable" | "mod-disable" | "mod-remove" => a.mod_operation(&action, &body).await?,
        "world-switch" => a.world_switch(&body).await?,
        "world-create" => a.world_create(&body).await?,
        "backup-init" | "backup-check" | "backup-snapshots" | "backup-restore" => {
            a.repository_operation(&action, &body).await?
        }
        "restore" => {
            a.restore(body["id"].as_str().context("id is required")?)
                .await?
        }
        _ => bail!("Unknown action"),
    };
    Ok(Json(data))
}
async fn logs(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    let memory = a.logs.lock().await;
    if memory.is_empty() {
        let data = std::fs::read_to_string(a.root.join("console.jsonl")).unwrap_or_default();
        Ok(Json(json!(data
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .collect::<Vec<_>>())))
    } else {
        Ok(Json(json!(*memory)))
    }
}
async fn world_options(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    let version = a.active_version()?;
    Ok(Json(crate::world_options::catalog(
        &a.root.join("releases").join(&version),
        &version,
    )?))
}
async fn config(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    let bytes = files::read(&a.root.join("data"), "serverconfig.json", 4 * 1024 * 1024)?;
    Ok(Json(
        json!({"value":serde_json::from_slice::<Value>(&bytes)?,"etag":files::etag(&bytes)}),
    ))
}
async fn save_config(State(a): State<Agent>, h: HeaderMap, Json(body): Json<Value>) -> ApiResult {
    a.authorize(&h)?;
    let _lock = a.mutation.lock().await;
    ensure!(
        !a.running().await?,
        "Stop the server before editing configuration"
    );
    ensure!(
        body["value"].is_object(),
        "Configuration must be a JSON object"
    );
    let original: Value = serde_json::from_slice(&files::read(
        &a.root.join("data"),
        "serverconfig.json",
        4 * 1024 * 1024,
    )?)?;
    let validated = crate::config_validation::merge_and_validate(&original, &body["value"])?;
    let bytes = serde_json::to_vec_pretty(&validated)?;
    a.disarm_idle().await?;
    a.recovery().await?;
    let etag = files::write(
        &a.root.join("data"),
        "serverconfig.json",
        &bytes,
        body["etag"].as_str(),
        false,
    )?;
    Ok(Json(json!({"etag":etag})))
}
#[derive(Deserialize)]
struct FileQuery {
    #[serde(default)]
    path: String,
}
async fn file_list(State(a): State<Agent>, h: HeaderMap, Query(q): Query<FileQuery>) -> ApiResult {
    a.authorize(&h)?;
    Ok(Json(json!(files::list(&a.root.join("data"), &q.path)?)))
}
async fn file_read(State(a): State<Agent>, h: HeaderMap, Query(q): Query<FileQuery>) -> ApiResult {
    a.authorize(&h)?;
    ensure!(
        !a.running().await? || !q.path.to_lowercase().starts_with("saves/"),
        "Stop the server before exporting live world database files"
    );
    let bytes = files::read(&a.root.join("data"), &q.path, 64 * 1024 * 1024)?;
    Ok(Json(
        json!({"content_base64":STANDARD.encode(&bytes),"etag":files::etag(&bytes)}),
    ))
}
async fn file_write(State(a): State<Agent>, h: HeaderMap, Json(body): Json<Value>) -> ApiResult {
    a.authorize(&h)?;
    let _lock = a.mutation.lock().await;
    ensure!(
        !a.running().await?,
        "Stop the server before editing or uploading files"
    );
    a.recovery().await?;
    a.disarm_idle().await?;
    let path = body["path"].as_str().context("path is required")?;
    let content = STANDARD.decode(
        body["content_base64"]
            .as_str()
            .context("content_base64 is required")?,
    )?;
    let etag = files::write(
        &a.root.join("data"),
        path,
        &content,
        body["etag"].as_str(),
        a.running().await?,
    )?;
    Ok(Json(json!({"etag":etag})))
}
async fn backups(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    Ok(Json(
        json!({"recovery_points":files::list(&a.root.join("recovery"),"")?,"policy":a.policy()?}),
    ))
}
async fn policy(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    Ok(Json(json!(a.policy()?)))
}
async fn save_policy(
    State(a): State<Agent>,
    h: HeaderMap,
    Json(p): Json<BackupPolicy>,
) -> ApiResult {
    a.authorize(&h)?;
    ensure!(
        p.hour_utc < 24 && p.daily > 0 && p.weekly > 0,
        "Invalid retention or schedule"
    );
    ensure!(
        !p.enabled || (!p.repository.is_empty() && std::env::var(&p.password_env).is_ok()),
        "Configure repository and password environment variable on the agent first"
    );
    std::fs::write(a.root.join("backup.json"), serde_json::to_vec_pretty(&p)?)?;
    Ok(Json(json!(p)))
}
async fn mods(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    let mut manifest = serde_json::from_slice(
        &std::fs::read(a.root.join("mods.json")).unwrap_or_else(|_| b"[]".to_vec()),
    )?;
    crate::mod_icons::enrich(&a.root, &mut manifest);
    Ok(Json(manifest))
}
async fn install_mod(State(a): State<Agent>, h: HeaderMap, Json(body): Json<Value>) -> ApiResult {
    a.authorize(&h)?;
    let operation = body["operation_id"].as_str().map(str::to_owned);
    a.operation_step(
        operation.as_deref(),
        "validation",
        "Validating mod archive and manifest before changing game files",
    );
    let result = install_mod_work(State(a.clone()), h, Json(body)).await;
    match &result {
        Ok(_) => a.operation_step(
            operation.as_deref(),
            "completed",
            "Mod installed; requested game restart submitted",
        ),
        Err(error) => a.operation_step(operation.as_deref(), "failed", &format!("{:#}", error.0)),
    }
    result
}
async fn install_mod_work(
    State(a): State<Agent>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    a.authorize(&h)?;
    let _lock = a.mutation.lock().await;
    let filename = body["filename"].as_str().context("filename required")?;
    ensure!(
        filename.ends_with(".zip") && !filename.contains(['/', '\\']) && filename.len() < 200,
        "A ZIP filename is required"
    );
    let bytes = STANDARD.decode(
        body["content_base64"]
            .as_str()
            .context("Mod ZIP content required")?,
    )?;
    let stage = tempfile::Builder::new()
        .prefix("mod-")
        .tempdir_in(&a.root)
        .context("Could not create temporary mod validation directory")?;
    a.operation_step(
        body["operation_id"].as_str(),
        "archive",
        &format!(
            "Validating {filename}, {} bytes, SHA256={}",
            bytes.len(),
            files::etag(&bytes)
        ),
    );
    files::extract_zip(stage.path(), &bytes, 512 * 1024 * 1024)?;
    a.operation_step(
        body["operation_id"].as_str(),
        "archive",
        "Archive paths and extraction limits passed",
    );
    a.operation_step(
        body["operation_id"].as_str(),
        "manifest",
        "Reading modinfo.json as UTF-8 JSON (optional BOM supported)",
    );
    let info = crate::mod_manifest::parse(
        &std::fs::read(stage.path().join("modinfo.json"))
            .context("Could not read modinfo.json from the extracted ZIP")?,
    )?;
    ensure!(
        info["modid"].is_string() && info["version"].is_string(),
        "Mod ID and version are required"
    );
    let existing = match std::fs::read(a.root.join("mods.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => b"[]".to_vec(),
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context("Could not read installed mod manifest mods.json")
                .into())
        }
    };
    let mut manifest: Vec<Value> = serde_json::from_slice(&existing).map_err(|error| {
        anyhow::anyhow!("Could not parse installed mod manifest mods.json: {error}")
    })?;
    if let Some(deps) = info["dependencies"].as_object() {
        for (id, version) in deps {
            if ["game", "survival", "creative"].contains(&id.as_str()) {
                continue;
            }
            ensure!(
                manifest.iter().any(|m| m["modid"] == *id
                    && m["enabled"] != false
                    && (version == "" || m["version"] == *version)),
                "Missing dependency {id}; install the declared version first"
            );
        }
    }
    ensure!(
        body["acknowledge_unknown_compatibility"] == true,
        "Compatibility must be reviewed and explicitly acknowledged"
    );
    let was_running = a.running().await?;
    let dir = a.root.join("data/Mods");
    std::fs::create_dir_all(&dir)?;
    let target = files::confined(&dir, filename)?;
    ensure!(
        !target.exists(),
        "Mod filename already exists; disable or remove the previous release first"
    );
    a.operation_step(
        body["operation_id"].as_str(),
        "stop",
        "Stopping game before applying mod changes",
    );
    a.stop().await?;
    a.operation_step(
        body["operation_id"].as_str(),
        "recovery",
        "Creating full local recovery copy",
    );
    a.recovery().await?;
    a.operation_step(
        body["operation_id"].as_str(),
        "apply",
        "Recovery copy succeeded; applying mod files and manifest",
    );
    if let Some(previous) = manifest.iter().find(|m| m["modid"] == info["modid"]) {
        let previous_dir = if previous["enabled"].as_bool().unwrap_or(true) {
            "Mods"
        } else {
            "ModsDisabled"
        };
        let path = files::confined(
            &a.root.join("data"),
            &format!(
                "{previous_dir}/{}",
                previous["filename"]
                    .as_str()
                    .context("Invalid previous mod filename")?
            ),
        )?;
        if path.exists() {
            let trash = a.root.join("trash");
            std::fs::create_dir_all(&trash)?;
            std::fs::rename(path, trash.join(uuid::Uuid::new_v4().to_string()))?;
        }
    }
    std::fs::write(&target, &bytes)?;
    manifest.retain(|m| m["modid"] != info["modid"]);
    manifest.push(json!({"modid":info["modid"],"version":info["version"],"filename":filename,"dependencies":info["dependencies"],"sha256":files::etag(&bytes),"source":body["source"].as_str().unwrap_or("upload"),"compatibility":body["compatibility"].as_str().unwrap_or("unknown")}));
    std::fs::write(
        a.root.join("mods.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    if was_running {
        a.start().await?;
    }
    Ok(Json(json!(manifest)))
}
include!("agent_actions.rs");
include!("agent_idle.rs");
include!("agent_diagnostics.rs");

#[cfg(test)]
mod mod_manifest_tests {
    use super::*;
    #[tokio::test]
    async fn malformed_manifest_cleans_staging_before_touching_game() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let agent = Agent::new(root.path().to_path_buf()).unwrap();
        std::fs::write(root.path().join("data/unchanged.txt"), b"original").unwrap();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("modinfo.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"{ invalid }").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", agent.token).parse().unwrap(),
        );
        let result=install_mod_work(State(agent),headers,Json(json!({"filename":"test.zip","content_base64":STANDARD.encode(bytes),"acknowledge_unknown_compatibility":true}))).await;
        let error = result.expect_err("invalid JSON must fail");
        assert!(error.0.to_string().contains("modinfo.json"));
        assert!(error.0.to_string().contains("line 1 column"));
        assert!(!std::fs::read_dir(root.path()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("mod-")));
        assert!(
            !root.path().join("desired").exists(),
            "validation must not stop the game"
        );
        assert_eq!(
            std::fs::read(root.path().join("data/unchanged.txt")).unwrap(),
            b"original"
        );
    }
}
