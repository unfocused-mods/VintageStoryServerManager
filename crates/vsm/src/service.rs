use crate::{
    auth::{self, Principal},
    db::Db,
    domain::*,
    providers::Cloud,
    transport,
};
use anyhow::{Context, Result};
use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use utoipa::OpenApi;

#[derive(Clone)]
pub struct Controller {
    pub instance_id: String,
    pub db: Db,
    pub root: PathBuf,
    pub public: bool,
    /// Rust-only catalog injection for isolated integration tests.
    pub release_catalog_override: Option<Arc<crate::game_releases::ReleaseCatalog>>,
    _inventory_lock: Arc<std::fs::File>,
    login_attempts: Arc<tokio::sync::Mutex<HashMap<String, (u32, std::time::Instant)>>>,
}
pub struct ApiError(pub StatusCode, pub String);
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(StatusCode::BAD_REQUEST, e.into().to_string())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;
macro_rules! require{($c:expr,$($arg:tt)*)=>{if !$c{return Err(anyhow::anyhow!($($arg)*).into());}}}

impl Controller {
    pub fn open(root: PathBuf, public: bool) -> Result<(Self, String)> {
        std::fs::create_dir_all(&root)?;
        auth::protect_path(&root, true)?;
        let inventory_lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("controller.lock"))?;
        fs2::FileExt::try_lock_exclusive(&inventory_lock)
            .context("This inventory already has a running controller")?;
        let db = Db::open(&root.join("controller.db"))?;
        let token_path = root.join("owner.token");
        if !token_path.exists() {
            auth::private_write(&token_path, auth::token().as_bytes())?;
        }
        let token = std::fs::read_to_string(token_path)?;
        auth::initialize(&db, &token)?;
        db.recover_interrupted()?;
        Ok((
            Self {
                instance_id: uuid::Uuid::new_v4().to_string(),
                db,
                root,
                public,
                release_catalog_override: None,
                _inventory_lock: Arc::new(inventory_lock),
                login_attempts: Default::default(),
            },
            token,
        ))
    }
}
pub fn router(c: Controller, ui: PathBuf) -> Router {
    let instance_id = c.instance_id.clone();
    let api = Router::new()
        .route("/summary", get(summary))
        .route("/accounts", get(accounts).post(save_account))
        .route("/accounts/{id}", axum::routing::delete(delete_account))
        .route("/accounts/{id}/validate", post(validate_account))
        .route("/accounts/{id}/discover", get(discover_account))
        .route("/accounts/{id}/adopt", post(adopt_server))
        .route(
            "/servers/{id}/connection",
            axum::routing::patch(recovery_connection),
        )
        .route("/servers/{id}/reconnect", post(reconnect_server))
        .route("/accounts/{id}/catalog", get(catalog))
        .route("/accounts/{id}/regions", get(regions))
        .route("/accounts/{id}/sizes", get(sizes))
        .route("/accounts/{id}/quote", get(quote))
        .route("/accounts/{id}/price", get(price))
        .route("/recommendations", post(recommendations))
        .route("/moddb", get(mod_search))
        .route("/moddb/{id}", get(mod_detail))
        .route("/plans", post(plan))
        .route("/deployments", post(deploy))
        .route("/servers", get(servers))
        .route("/servers/{id}", get(server))
        .route(
            "/servers/{id}/cloud-idle-policy",
            get(get_cloud_idle_policy).put(save_cloud_idle_policy),
        )
        .route("/servers/{id}/actions/{action}", post(action))
        .route(
            "/servers/{id}/agent/{*endpoint}",
            get(agent_get).post(agent_post).put(agent_put),
        )
        .route("/servers/{id}/infrastructure", get(infrastructure))
        .route(
            "/servers/{id}/infrastructure/status",
            get(infrastructure_status),
        )
        .route("/operations", get(operations))
        .route("/operations/{id}", get(operation))
        .route("/operations/{id}/logs", get(operation_logs))
        .route("/events", get(events))
        .route("/users", post(create_user))
        .route("/tokens/revoke", post(revoke))
        .route("/recovery/export", post(export))
        .route("/recovery/import", post(import))
        .route("/openapi.json", get(openapi))
        .layer(middleware::from_fn_with_state(c.clone(), authorize));
    let allowed = [
        "http://localhost:1420",
        "http://127.0.0.1:1420",
        "tauri://localhost",
        "http://tauri.localhost",
    ]
    .map(|s| s.parse().unwrap());
    Router::new()
        .nest("/api/v1", api)
        .route("/api/v1/login", post(login))
        .route("/api/v1/hosting/catalog", get(hosting_catalog))
        .route("/api/v1/game/releases", get(game_releases))
        .route(
            "/health",
            get(move || { let instance_id = instance_id.clone(); async move { Json(json!({"status":"ok","version":env!("CARGO_PKG_VERSION"),"instance_id":instance_id})) } }),
        )
        .fallback_service(
            tower_http::services::ServeDir::new(&ui)
                .not_found_service(tower_http::services::ServeFile::new(ui.join("index.html"))),
        )
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(allowed)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::PATCH,
                    axum::http::Method::DELETE,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderName::from_static("idempotency-key"),
                    axum::http::HeaderName::from_static("last-event-id"),
                ]),
        )
        .with_state(c)
}
pub async fn serve(c: Controller, listener: tokio::net::TcpListener, ui: PathBuf) -> Result<()> {
    let monitor = c.clone();
    let task = tokio::spawn(async move {
        loop {
            if let Ok(servers) = monitor.db.list::<Server>("servers") {
                for s in servers.into_iter().filter(|s| {
                    matches!(
                        s.state.as_str(),
                        "starting" | "running" | "stopped" | "sleeping"
                    )
                }) {
                    if let Ok(Ok(status)) = tokio::time::timeout(
                        std::time::Duration::from_secs(15),
                        transport::call(&s, reqwest::Method::GET, "/v1/status", None),
                    )
                    .await
                    {
                        let _ = update_observed_state(&monitor.db, &s.id, &status);
                        let _ = consider_cloud_idle(&monitor, &s, &status);
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
    axum::serve(listener, router(c, ui)).await?;
    task.abort();
    Ok(())
}
async fn authorize(State(c): State<Controller>, mut request: Request, next: Next) -> Response {
    let value = request
        .headers()
        .get("authorization")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("");
    match auth::authenticate(&c.db, value, c.public) {
        Ok(p) => {
            request.extensions_mut().insert(p);
            next.run(request).await
        }
        Err(_) => {
            ApiError(StatusCode::UNAUTHORIZED, "Authentication required".into()).into_response()
        }
    }
}
async fn login(State(c): State<Controller>, Json(body): Json<Value>) -> ApiResult {
    let name = body["name"].as_str().unwrap_or("");
    let mut attempts = c.login_attempts.lock().await;
    attempts.retain(|_, entry| entry.1.elapsed().as_secs() <= 300);
    require!(
        attempts.len() < 10000 || attempts.contains_key(name),
        "Login capacity exceeded; try later"
    );
    let entry = attempts
        .entry(name.chars().take(60).collect())
        .or_insert((0, std::time::Instant::now()));
    if entry.1.elapsed().as_secs() > 300 {
        *entry = (0, std::time::Instant::now());
    }
    require!(entry.0 < 10, "Too many login attempts; wait five minutes");
    entry.0 += 1;
    drop(attempts);
    let token = auth::login(
        &c.db,
        name,
        body["password"].as_str().unwrap_or(""),
        body["code"].as_str().unwrap_or(""),
        c.public,
    )
    .map_err(|_| {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "Invalid credentials or TOTP enrollment required".into(),
        )
    })?;
    Ok(Json(json!({"token":token})))
}
async fn summary(State(c): State<Controller>, Extension(p): Extension<Principal>) -> ApiResult {
    let servers =
        c.db.list::<Server>("servers")?
            .into_iter()
            .filter(|s| p.access(&s.id, false).is_ok())
            .collect::<Vec<_>>();
    let operations =
        c.db.operations()?
            .into_iter()
            .filter(|o| p.access(&o.server_id, false).is_ok())
            .collect::<Vec<_>>();
    Ok(Json(
        json!({"servers":servers,"operations":operations,"user":p,"version":env!("CARGO_PKG_VERSION"),"providers":["azure","hetzner","digital_ocean","vultr","ssh","docker","ssh_docker"]}),
    ))
}
async fn accounts(State(c): State<Controller>, Extension(p): Extension<Principal>) -> ApiResult {
    p.owner()?;
    Ok(Json(json!(c.db.list::<Account>("accounts")?)))
}
async fn save_account(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Json(mut account): Json<Account>,
) -> ApiResult {
    p.owner()?;
    if account.id.is_empty() {
        account.id = uuid::Uuid::new_v4().to_string();
    }
    require!(
        account.id.len() < 100 && !account.name.trim().is_empty(),
        "Invalid account"
    );
    if !(matches!(
        account.provider,
        Provider::Ssh | Provider::Docker | Provider::SshDocker
    ) || account.provider == Provider::Azure
        && account.settings["auth"] != "service_principal")
    {
        require!(
            account.credential_ref.starts_with("env:")
                || account.credential_ref.starts_with("keyring:")
                || account.credential_ref.starts_with("vault:"),
            "Use a credential reference, not a plaintext secret"
        );
    }
    c.db.save_account(&account)?;
    crate::providers::invalidate_catalog(&account.id).await;
    c.db.event(
        "",
        "audit",
        &format!("{} saved provider account {}", p.name, account.name),
    )?;
    Ok(Json(json!(account)))
}
async fn delete_account(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.owner()?;
    let account = c.db.account(&id)?;
    c.db.delete_account(&id)?;
    crate::providers::invalidate_catalog(&id).await;
    c.db.event(
        "",
        "audit",
        &format!("{} removed provider account {}", p.name, account.name),
    )?;
    Ok(Json(json!({"deleted": true})))
}
async fn validate_account(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.owner()?;
    Ok(Json(Cloud::new(c.db.account(&id)?).validate().await?))
}
async fn game_releases() -> ApiResult {
    Ok(Json(json!(crate::game_releases::catalog().await)))
}
async fn resolve_game_release(
    c: &Controller,
    version: &str,
) -> Result<crate::game_releases::GameRelease> {
    if let Some(catalog) = &c.release_catalog_override {
        crate::game_releases::select(catalog, version)
    } else {
        crate::game_releases::resolve(version).await
    }
}
async fn hosting_catalog() -> ApiResult {
    Ok(Json(crate::hosting::catalog().await?))
}
async fn catalog(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.owner()?;
    Ok(Json(Cloud::new(c.db.account(&id)?).catalog().await?))
}
#[derive(Deserialize)]
struct QuoteQuery {
    region: String,
    size: String,
}
#[derive(Deserialize)]
struct RegionQuery {
    region: String,
}
async fn regions(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.owner()?;
    Ok(Json(Cloud::new(c.db.account(&id)?).regions().await?))
}
async fn sizes(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Query(q): Query<RegionQuery>,
) -> ApiResult {
    p.owner()?;
    Ok(Json(Cloud::new(c.db.account(&id)?).sizes(&q.region).await?))
}
async fn quote(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Query(q): Query<QuoteQuery>,
) -> ApiResult {
    p.owner()?;
    Ok(Json(json!(
        Cloud::new(c.db.account(&id)?)
            .quote(&q.region, &q.size)
            .await?
    )))
}
async fn price(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Query(q): Query<QuoteQuery>,
) -> ApiResult {
    p.owner()?;
    Ok(Json(json!(
        Cloud::new(c.db.account(&id)?)
            .price(&q.region, &q.size)
            .await?
    )))
}
async fn recommendations(
    Extension(_p): Extension<Principal>,
    Json(input): Json<RecommendationInput>,
) -> ApiResult {
    Ok(Json(json!(crate::recommend::recommend(input))))
}
async fn plan(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Json(spec): Json<DeploySpec>,
) -> ApiResult {
    p.owner()?;
    let account = c.db.account(&spec.account_id)?;
    spec.validate_for(&account.provider)?;
    resolve_game_release(&c, &spec.game_version).await?;
    if account.provider == Provider::Docker {
        crate::docker::validate_local().await?;
    }
    let cloud = Cloud::new(account.clone());
    let quote = if matches!(
        account.provider,
        Provider::Ssh | Provider::Docker | Provider::SshDocker
    ) {
        None
    } else {
        Some(cloud.quote(&spec.region, &spec.size).await?)
    };
    if account.provider == Provider::Azure {
        cloud.azure_preflight(&spec).await?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let value = json!({"id":id,"spec":spec,"quote":quote,"created_at":now(),"expires_in_seconds":900,"resources":if account.provider==Provider::Azure{vec!["resource group (retained after teardown)","virtual machine","64 GiB managed OS disk","virtual network and subnet","network security group","network interface","Standard static IPv4"]}else if crate::docker::is_docker(&account.provider){vec!["Linux x64 container","persistent game data volume","game TCP/UDP port"]}else{vec!["virtual machine (existing for SSH)"]},"warnings":if crate::docker::is_docker(&account.provider){vec!["Docker must be running with Linux x64 containers.","Game port is reachable on the host network; management stays private.","Deleting the container retains the data volume by default."]}else{vec!["Cloud costs are charged by your provider.","Verify the SSH host fingerprint before bootstrap.","A Linux x64 agent artifact is required to install the game."]}});
    c.db.0.lock().unwrap().execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2)",
        rusqlite::params![format!("plan:{id}"), value.to_string()],
    )?;
    Ok(Json(value))
}
fn request_key(h: &HeaderMap) -> Result<&str> {
    h.get("idempotency-key")
        .and_then(|h| h.to_str().ok())
        .context("Idempotency-Key header is required")
}
async fn deploy(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    p.owner()?;
    let id = body["plan_id"].as_str().context("plan_id is required")?;
    let _: uuid::Uuid = id.parse()?;
    let stored: String = c.db.0.lock().unwrap().query_row(
        "SELECT value FROM meta WHERE key=?1",
        [format!("plan:{id}")],
        |r| r.get(0),
    )?;
    let value: Value = serde_json::from_str(&stored)?;
    let spec: DeploySpec = serde_json::from_value(value["spec"].clone())?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(
        value["created_at"].as_str().context("Invalid plan")?,
    )?;
    require!(
        chrono::Utc::now()
            .signed_duration_since(timestamp)
            .num_seconds()
            < 900,
        "Plan expired; create a fresh plan"
    );
    resolve_game_release(&c, &spec.game_version).await?;
    let account = c.db.account(&spec.account_id)?;
    let cloud = Cloud::new(account.clone());
    if !matches!(
        account.provider,
        Provider::Ssh | Provider::Docker | Provider::SshDocker
    ) {
        let quote = cloud.quote(&spec.region, &spec.size).await?;
        require!(quote.available, "Selected size is not available");
        let old: Quote = serde_json::from_value(value["quote"].clone())?;
        require!(
            (quote.monthly_compute - old.monthly_compute).abs() < 0.01
                && quote.currency == old.currency,
            "Price changed; create a fresh plan"
        );
        require!(
            quote.total().is_some() || body["accept_incomplete_estimate"] == true,
            "Incomplete price estimate requires explicit acknowledgment"
        );
    }
    // Plan ID is also the deployment ID, so alternate request keys cannot create duplicate VMs.
    let hash = auth::hash(&format!("deploy:{id}"));
    let (op, new) = c.db.enqueue(id, "deploy", request_key(&h)?, &hash)?;
    if new {
        if c.db.server(id).is_ok() {
            c.db.progress(
                &op.id,
                "failed",
                "Deployment already exists; inspect it before retrying",
            )?;
            return Err(anyhow::anyhow!("Deployment already exists").into());
        }
        let mut server = Server {
            id: id.into(),
            name: spec.name.clone(),
            account_id: Some(account.id.clone()),
            provider: account.provider,
            region: spec.region.clone(),
            size: spec.size.clone(),
            host: String::new(),
            ssh_user: spec.ssh_user.clone(),
            ssh_key: spec.ssh_private_key.clone(),
            game_version: spec.game_version.clone(),
            state: "provisioning".into(),
            created_at: now(),
            resources: vec![],
        };
        c.db.put("servers", &server.id, &server)?;
        let state = c.clone();
        let opid = op.id.clone();
        tokio::spawn(async move {
            let _ = state.db.progress(
                &opid,
                "running",
                if crate::docker::is_docker(&server.provider) {
                    "Preparing Docker deployment"
                } else if server.provider == Provider::Ssh {
                    "Preparing remote host"
                } else {
                    "Provisioning cloud resources"
                },
            );
            let result = async {
                cloud.provision(&spec, &mut server, &state.db).await?;
                if server.provider == Provider::Docker {
                    let binary = std::env::var("VSM_AGENT_BINARY")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| state.root.join("artifacts/vsm-agent"));
                    let account = state.db.account(&spec.account_id)?;
                    crate::docker::bootstrap(&mut server, &account, &binary, &state.db).await?;
                    transport::call(
                        &server,
                        reqwest::Method::POST,
                        "/v1/actions/install",
                        Some(json!({"version":server.game_version})),
                    )
                    .await?;
                    record_remote_installation(&state.db, &server.id)?;
                    server.state = "stopped".into();
                    state.db.put("servers", &server.id, &server)?;
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;
            match result {
                Ok(()) => {
                    let _ = state.db.progress(
                        &opid,
                        "succeeded",
                        if server.provider == Provider::Docker { "Docker server installed. Ready to start." } else { "Infrastructure prepared. Verify SSH host key, then install the remote agent." },
                    );
                }
                Err(e) => {
                    server.state = "attention".into();
                    let _ = state.db.put("servers", &server.id, &server);
                    let _ = state.db.progress(&opid, "failed", &e.to_string());
                }
            }
        });
    }
    Ok(Json(json!(op)))
}
async fn servers(State(c): State<Controller>, Extension(p): Extension<Principal>) -> ApiResult {
    Ok(Json(json!(c
        .db
        .list::<Server>("servers")?
        .into_iter()
        .filter(|s| p.access(&s.id, false).is_ok())
        .collect::<Vec<_>>())))
}
async fn server(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.access(&id, false)?;
    Ok(Json(json!(c.db.server(&id)?)))
}
fn remote_installation_ready(db: &Db, s: &Server) -> Result<bool> {
    Ok(matches!(s.state.as_str(), "running" | "sleeping" | "starting") || db.0.lock().unwrap().query_row(
        "SELECT EXISTS(SELECT 1 FROM meta WHERE key=?1 AND value='true') OR EXISTS(SELECT 1 FROM operations WHERE server_id=?2 AND action='bootstrap' AND status='succeeded')",
        rusqlite::params![format!("remote-installed:{}",s.id),s.id], |r| r.get::<_,bool>(0))?)
}
fn record_remote_installation(db: &Db, id: &str) -> Result<()> {
    db.0.lock().unwrap().execute(
        "INSERT INTO meta(key,value) VALUES(?1,'true') ON CONFLICT(key) DO UPDATE SET value='true'",
        [format!("remote-installed:{id}")],
    )?;
    Ok(())
}
async fn infrastructure_status(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.access(&id, false)?;
    let s = c.db.server(&id)?;
    let account =
        c.db.account(s.account_id.as_deref().context("No provider account")?)?;
    let cloud = Cloud::new(account);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        cloud.infrastructure_status(&s),
    )
    .await;
    let mut value = match result {
        Ok(Ok(value)) => value,
        other => {
            json!({"provider":s.provider,"power_state":null,"provisioning_state":null,"checked_at":now(),"remote_installed":null,"error":match other { Ok(Err(error)) => error.to_string(), _ => "Provider status request timed out. Retry shortly.".into() }})
        }
    };
    value["remote_installed"] = if s.state == "recovered" {
        Value::Null
    } else {
        json!(remote_installation_ready(&c.db, &s)?)
    };
    Ok(Json(value))
}
async fn infrastructure(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.owner()?;
    let s = c.db.server(&id)?;
    let a =
        c.db.account(s.account_id.as_deref().context("No provider account")?)?;
    Ok(Json(Cloud::new(a).inspect(&s).await?))
}
async fn action(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path((id, action)): Path<(String, String)>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    p.access(&id, true)?;
    require!(
        [
            "start",
            "stop",
            "restart",
            "console",
            "install",
            "update",
            "backup",
            "backup-init",
            "backup-check",
            "backup-snapshots",
            "backup-restore",
            "file-move",
            "file-delete",
            "file-mkdir",
            "file-extract",
            "mod-enable",
            "mod-disable",
            "mod-remove",
            "mod-install",
            "mod-upload",
            "world-switch",
            "world-create",
            "restore",
            "bootstrap",
            "agent-update",
            "cloud-start",
            "cloud-restart",
            "resize",
            "deallocate",
            "delete"
        ]
        .contains(&action.as_str()),
        "Unknown action"
    );
    if [
        "bootstrap",
        "agent-update",
        "cloud-start",
        "cloud-restart",
        "resize",
        "deallocate",
        "delete",
    ]
    .contains(&action.as_str())
    {
        p.owner()?;
    }
    let s = c.db.server(&id)?;
    if action == "delete" {
        require!(
            body["confirm_name"] == s.name,
            "Confirm the server name before deleting infrastructure"
        );
    }
    require!(
        s.state != "recovered" || action == "cloud-start",
        "Reconnect to the existing Remote before changing this recovered server"
    );
    if action == "resize" {
        require!(
            body["accept_cost_change"] == true,
            "Acknowledge the changed recurring cost before resizing"
        );
    }
    let hash = auth::hash(&format!("{id}:{action}:{body}"));
    let (op, new) = c.db.enqueue(&id, &action, request_key(&h)?, &hash)?;
    if new {
        let state = c.clone();
        let opid = op.id.clone();
        let action = action.clone();
        tokio::spawn(async move {
            let _ = state.db.progress(&opid, "running", "Executing operation");
            let result = execute_action(&state, s, &action, body, &opid).await;
            match result {
                Ok(v) => {
                    let _ = state.db.progress(&opid, "succeeded", &v.to_string());
                }
                Err(e) => {
                    let _ = state.db.progress(&opid, "failed", &e.to_string());
                }
            }
        });
    }
    Ok(Json(json!(op)))
}
async fn execute_action(
    c: &Controller,
    mut s: Server,
    action: &str,
    body: Value,
    operation_id: &str,
) -> Result<Value> {
    c.db.operation_log(
        operation_id,
        "info",
        "dispatch",
        &format!("Dispatching {action} to {}", s.provider.key()),
    )?;
    match action {
        "agent-update" => update_remote(c, s, operation_id).await,
        "mod-upload" => {
            let mut body = body;
            c.db.operation_log(
                operation_id,
                "info",
                "archive",
                &format!("Uploaded custom mod filename={}", body["filename"]),
            )?;
            body["operation_id"] = json!(operation_id);
            let result = transport::call(&s, reqwest::Method::POST, "/v1/mods", Some(body)).await;
            if let Ok(Ok(logs)) = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                transport::call(
                    &s,
                    reqwest::Method::GET,
                    &format!("/v1/operation-logs/{operation_id}"),
                    None,
                ),
            )
            .await
            {
                for entry in logs.as_array().into_iter().flatten() {
                    let _ = c.db.operation_log(
                        operation_id,
                        entry["level"].as_str().unwrap_or("info"),
                        entry["stage"].as_str().unwrap_or("remote"),
                        entry["message"].as_str().unwrap_or("Remote step"),
                    );
                }
            } else {
                let _ = c.db.operation_log(
                    operation_id,
                    "info",
                    "diagnostics",
                    "Detailed Remote stages were unavailable; older Remotes may need an update",
                );
            }
            result
        }
        "mod-install" => {
            use base64::Engine;
            let id = body["modid"].as_str().context("modid required")?;
            let release = body["releaseid"].as_u64().context("releaseid required")?;
            c.db.operation_log(
                operation_id,
                "info",
                "download",
                &format!("Fetching ModDB mod {id}, release {release}"),
            )?;
            let (metadata, bytes) = crate::moddb::archive(id, release).await?;
            c.db.operation_log(
                operation_id,
                "info",
                "archive",
                &format!(
                    "Downloaded filename={} version={} bytes={} SHA256={}",
                    metadata["filename"],
                    metadata["modversion"],
                    bytes.len(),
                    crate::files::etag(&bytes)
                ),
            )?;
            let declared = metadata["tags"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(s.game_version)));
            anyhow::ensure!(
                declared || body["acknowledge_unknown_compatibility"] == true,
                "This release does not declare compatibility with the installed game version"
            );
            c.db.operation_log(
                operation_id,
                "info",
                "compatibility",
                if declared {
                    "Game compatibility declared by publisher"
                } else {
                    "Unknown game compatibility explicitly accepted"
                },
            )?;
            c.db.operation_log(
                operation_id,
                "info",
                "remote",
                "Sending archive for Remote validation, recovery copy and installation",
            )?;
            let result=transport::call(&s,reqwest::Method::POST,"/v1/mods",Some(json!({"operation_id":operation_id,"filename":metadata["filename"],"content_base64":base64::engine::general_purpose::STANDARD.encode(bytes),"acknowledge_unknown_compatibility":true,"source":format!("moddb:{id}/{release}"),"compatibility":if declared{"declared_compatible"}else{"unknown"}}))).await;
            if let Ok(Ok(logs)) = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                transport::call(
                    &s,
                    reqwest::Method::GET,
                    &format!("/v1/operation-logs/{operation_id}"),
                    None,
                ),
            )
            .await
            {
                for entry in logs.as_array().into_iter().flatten() {
                    c.db.operation_log(
                        operation_id,
                        entry["level"].as_str().unwrap_or("info"),
                        entry["stage"].as_str().unwrap_or("remote"),
                        entry["message"].as_str().unwrap_or("Remote step"),
                    )?;
                }
            } else {
                c.db.operation_log(
                    operation_id,
                    "info",
                    "diagnostics",
                    "Detailed Remote stages were unavailable; older Remotes may need an update",
                )?;
            }
            result
        }
        "bootstrap" => {
            let binary = std::env::var("VSM_AGENT_BINARY")
                .map(PathBuf::from)
                .unwrap_or_else(|_| c.root.join("artifacts/vsm-agent"));
            anyhow::ensure!(binary.is_file(),"Set VSM_AGENT_BINARY or place a Linux x64 agent at controller-data/artifacts/vsm-agent");
            if crate::docker::is_docker(&s.provider) {
                let account =
                    c.db.account(s.account_id.as_deref().context("No deployment account")?)?;
                crate::docker::bootstrap(&mut s, &account, &binary, &c.db).await?;
            } else {
                transport::bootstrap(&s, &binary).await?;
            }
            let result = transport::call(
                &s,
                reqwest::Method::POST,
                "/v1/actions/install",
                Some(json!({"version":s.game_version})),
            )
            .await?;
            record_remote_installation(&c.db, &s.id)?;
            s.state = "stopped".into();
            c.db.put("servers", &s.id, &s)?;
            Ok(result)
        }

        "cloud-start" | "cloud-restart" | "deallocate" | "delete" | "resize" => {
            let a =
                c.db.account(s.account_id.as_deref().context("No provider account")?)?;
            let cloud = Cloud::new(a);
            let installed = remote_installation_ready(&c.db, &s)?;
            if installed
                && action != "cloud-start"
                && !matches!(
                    s.state.as_str(),
                    "awaiting_bootstrap" | "provisioning" | "attention" | "deallocated" | "deleted"
                )
            {
                transport::call(
                    &s,
                    reqwest::Method::POST,
                    "/v1/actions/stop",
                    Some(json!({})),
                )
                .await?;
            }
            if action == "delete" {
                if crate::docker::is_docker(&s.provider) {
                    crate::docker::delete_with_data(&s, body["delete_data"] == true).await?;
                } else {
                    cloud.delete(&s).await?;
                }
                s.state = "deleted".into();
                c.db.put("servers", &s.id, &s)?;
                Ok(
                    json!({"deleted":true,"note":if crate::docker::is_docker(&s.provider) { "Container removed. Persistent data is retained unless delete_data was explicitly selected." } else { "Azure resource group is retained; inspect the provider for remaining resources." }}),
                )
            } else if action == "resize" {
                let size = body["size"].as_str().context("size required")?;
                cloud.resize(&s, size).await?;
                cloud.wait_resize(&s, size).await?;
                s.size = size.into();
                s.state = if installed {
                    "stopped"
                } else {
                    "awaiting_bootstrap"
                }
                .into();
                c.db.put("servers", &s.id, &s)?;
                Ok(
                    json!({"resized":size,"note":"Game remains stopped; start it after reviewing the host."}),
                )
            } else {
                let command = match action {
                    "cloud-start" => "start",
                    "cloud-restart" => "restart",
                    _ => "deallocate",
                };
                let data = cloud.power(&s, command).await?;
                cloud.wait_power(&s, command).await?;
                s.state = if command == "deallocate" {
                    "deallocated"
                } else if s.state == "recovered" {
                    "recovered"
                } else if installed {
                    "stopped"
                } else {
                    "awaiting_bootstrap"
                }
                .into();
                c.db.put("servers", &s.id, &s)?;
                if command == "start" && cloud_wake_pending(&c.db, &s.id)? {
                    return resume_cloud_game(c, &s).await;
                }
                Ok(
                    json!({"completed":true,"provider_response":data,"note":"Provider power state verified. Storage and IP billing may continue. Start the game when ready."}),
                )
            }
        }
        _ => {
            let result = transport::call(
                &s,
                reqwest::Method::POST,
                &format!("/v1/actions/{action}"),
                Some(body),
            )
            .await?;
            if let Some(state) = result["state"].as_str() {
                s.state = state.into();
            }
            if let Some(version) = result["installed"].as_str() {
                s.game_version = version.into();
            }
            c.db.put("servers", &s.id, &s)?;
            Ok(result)
        }
    }
}
#[derive(Deserialize)]
struct ProxyQuery {
    #[serde(default)]
    path: String,
}
async fn agent_get(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path((id, endpoint)): Path<(String, String)>,
    Query(q): Query<ProxyQuery>,
) -> ApiResult {
    p.access(&id, false)?;
    require!(
        [
            "status",
            "logs",
            "config",
            "files",
            "file",
            "mods",
            "backups",
            "backup-policy",
            "schedule",
            "idle-policy",
            "worlds",
            "world-options"
        ]
        .contains(&endpoint.as_str()),
        "Unknown agent endpoint"
    );
    if [
        "config",
        "files",
        "file",
        "backup-policy",
        "schedule",
        "worlds",
        "mods",
        "backups",
    ]
    .contains(&endpoint.as_str())
    {
        p.access(&id, true)?;
    }
    let mut url = reqwest::Url::parse(&format!("http://localhost/v1/{endpoint}"))?;
    url.query_pairs_mut().append_pair("path", &q.path);
    let path = format!("{}?{}", url.path(), url.query().unwrap_or(""));
    let data = transport::call(&c.db.server(&id)?, reqwest::Method::GET, &path, None).await?;
    if endpoint == "status" {
        update_observed_state(&c.db, &id, &data)?;
    }
    Ok(Json(data))
}
fn update_observed_state(db: &Db, id: &str, data: &Value) -> Result<()> {
    if let Some(state @ ("running" | "starting" | "stopped" | "sleeping")) = data["state"].as_str()
    {
        db.0.lock().unwrap().execute("UPDATE servers SET body=json_set(body,'$.state',?1) WHERE id=?2 AND json_extract(body,'$.state') NOT IN ('deallocated','deleted','recovered') AND NOT EXISTS(SELECT 1 FROM operations WHERE server_id=?2 AND status IN ('queued','running'))", rusqlite::params![state,id])?;
    }
    Ok(())
}
async fn proxy_write(
    c: Controller,
    p: Principal,
    id: String,
    endpoint: String,
    body: Value,
    mut headers: HeaderMap,
    method: reqwest::Method,
) -> ApiResult {
    p.access(&id, true)?;
    require!(
        c.db.server(&id)?.state != "recovered",
        "Reconnect to the existing Remote before changing this recovered server"
    );
    require!(
        [
            "config",
            "files",
            "mods",
            "backup-policy",
            "schedule",
            "idle-policy"
        ]
        .contains(&endpoint.as_str()),
        "Unknown agent endpoint"
    );
    if endpoint == "mods" {
        if !headers.contains_key("Idempotency-Key") {
            headers.insert("Idempotency-Key", uuid::Uuid::new_v4().to_string().parse()?);
        }
        return action(
            State(c),
            Extension(p),
            Path((id, "mod-upload".into())),
            headers,
            Json(body),
        )
        .await;
    }
    let server = c.db.server(&id)?;
    let data = transport::call(&server, method, &format!("/v1/{endpoint}"), Some(body)).await?;
    c.db.event(&id, "audit", &format!("{} changed {}", p.name, endpoint))?;
    Ok(Json(data))
}
async fn agent_post(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path((id, endpoint)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    proxy_write(c, p, id, endpoint, body, headers, reqwest::Method::POST).await
}
async fn agent_put(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path((id, endpoint)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    proxy_write(c, p, id, endpoint, body, headers, reqwest::Method::PUT).await
}
async fn operations(State(c): State<Controller>, Extension(p): Extension<Principal>) -> ApiResult {
    Ok(Json(json!(c
        .db
        .operations()?
        .into_iter()
        .filter(|o| p.access(&o.server_id, false).is_ok())
        .collect::<Vec<_>>())))
}
async fn operation(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    let o = c.db.operation(&id)?;
    p.access(&o.server_id, false)?;
    Ok(Json(json!(o)))
}
async fn operation_logs(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    let operation = c.db.operation(&id)?;
    p.access(&operation.server_id, false)?;
    Ok(Json(c.db.operation_logs(&id)?))
}
#[derive(Deserialize)]
struct EventQuery {
    #[serde(default)]
    after: i64,
}
async fn events(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    h: HeaderMap,
    Query(q): Query<EventQuery>,
) -> Sse<
    impl tokio_stream_compat::Stream<Item = std::result::Result<SseEvent, std::convert::Infallible>>,
> {
    let mut after = h
        .get("last-event-id")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(q.after);
    let token = h
        .get("authorization")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_string();
    Sse::new(async_stream::stream!{loop{if auth::authenticate(&c.db, &token, c.public).is_err() { break; }
if let Ok(events)=c.db.events(after){for e in events{after=e.seq;if p.role=="owner" || p.access(&e.server_id,false).is_ok(){yield Ok(SseEvent::default().id(e.seq.to_string()).event("activity").data(serde_json::to_string(&e).unwrap()));}}}tokio::time::sleep(std::time::Duration::from_secs(1)).await;}}).keep_alive(KeepAlive::default())
}
async fn create_user(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    p.owner()?;
    let name = body["name"].as_str().context("name required")?;
    let servers: Vec<String> = serde_json::from_value(body["servers"].clone()).unwrap_or_default();
    auth::create_user(
        &c.db,
        name,
        body["password"].as_str().context("password required")?,
        body["role"].as_str().context("role required")?,
        &servers,
        body["totp_secret"].as_str(),
    )?;
    c.db.event("", "audit", &format!("{} created user {name}", p.name))?;
    Ok(Json(json!({"created":true})))
}
async fn revoke(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    p.owner()?;
    let name = body["name"].as_str().context("name required")?;
    require!(
        name != "bootstrap",
        "Bootstrap token is local-only; rotate it from the filesystem"
    );
    c.db.0
        .lock()
        .unwrap()
        .execute("UPDATE tokens SET revoked=1 WHERE name=?1", [name])?;
    Ok(Json(json!({"revoked":true})))
}
async fn export(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    use base64::Engine;
    p.owner()?;
    let value = json!({"format":1,"accounts":c.db.list::<Account>("accounts")?,"servers":c.db.list::<Server>("servers")?,"created_at":now()});
    let data = auth::seal(
        &serde_json::to_vec(&value)?,
        body["passphrase"].as_str().context("passphrase required")?,
    )?;
    Ok(Json(
        json!({"content_base64":base64::engine::general_purpose::STANDARD.encode(data),"note":"Inventory export excludes credentials, users and backup keys. Transfer these separately; stop the source controller before importing."}),
    ))
}
async fn import(
    State(c): State<Controller>,
    Extension(p): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    use base64::Engine;
    p.owner()?;
    require!(
        body["source_stopped"] == true,
        "Stop the source controller and acknowledge ownership transfer"
    );
    require!(
        c.db.list::<Server>("servers")?.is_empty() && c.db.list::<Account>("accounts")?.is_empty(),
        "Import requires an empty controller"
    );
    let data = base64::engine::general_purpose::STANDARD.decode(
        body["content_base64"]
            .as_str()
            .context("content required")?,
    )?;
    let value: Value = serde_json::from_slice(&auth::unseal(
        &data,
        body["passphrase"].as_str().context("passphrase required")?,
    )?)?;
    require!(value["format"] == 1, "Unsupported export format");
    let accounts: Vec<Account> = serde_json::from_value(value["accounts"].clone())?;
    let servers: Vec<Server> = serde_json::from_value(value["servers"].clone())?;
    let mut conn = c.db.0.lock().unwrap();
    let tx = conn.transaction()?;
    for a in accounts {
        tx.execute(
            "INSERT INTO accounts VALUES(?1,?2)",
            rusqlite::params![a.id, serde_json::to_string(&a)?],
        )?;
    }
    for s in servers {
        tx.execute(
            "INSERT INTO servers VALUES(?1,?2)",
            rusqlite::params![s.id, serde_json::to_string(&s)?],
        )?;
    }
    tx.commit()?;
    Ok(Json(json!({"imported":true})))
}
#[derive(OpenApi)]
#[openapi(components(schemas(
    Account,
    Server,
    Resource,
    Operation,
    Event,
    DeploySpec,
    Quote,
    RecommendationInput,
    Recommendations,
    LatencySample,
    RankedQuote,
    Provider
)))]
pub struct ApiDoc;
pub fn api_document() -> Value {
    let mut doc = serde_json::to_value(ApiDoc::openapi()).unwrap();
    doc["info"] = json!({"title":"Vintage Story Server Manager","version":"0.1.0"});
    let routes = [
        ("/summary", "get"),
        ("/accounts", "get"),
        ("/accounts", "post"),
        ("/accounts/{id}", "delete"),
        ("/accounts/{id}/validate", "post"),
        ("/accounts/{id}/discover", "get"),
        ("/accounts/{id}/adopt", "post"),
        ("/servers/{id}/connection", "patch"),
        ("/servers/{id}/reconnect", "post"),
        ("/accounts/{id}/catalog", "get"),
        ("/accounts/{id}/regions", "get"),
        ("/accounts/{id}/sizes", "get"),
        ("/accounts/{id}/quote", "get"),
        ("/accounts/{id}/price", "get"),
        ("/recommendations", "post"),
        ("/hosting/catalog", "get"),
        ("/game/releases", "get"),
        ("/plans", "post"),
        ("/deployments", "post"),
        ("/servers", "get"),
        ("/servers/{id}", "get"),
        ("/servers/{id}/cloud-idle-policy", "get"),
        ("/servers/{id}/cloud-idle-policy", "put"),
        ("/servers/{id}/actions/{action}", "post"),
        ("/servers/{id}/agent/mods", "post"),
        ("/servers/{id}/agent/{endpoint}", "get"),
        ("/servers/{id}/agent/{endpoint}", "post"),
        ("/servers/{id}/agent/{endpoint}", "put"),
        ("/servers/{id}/infrastructure", "get"),
        ("/servers/{id}/infrastructure/status", "get"),
        ("/operations", "get"),
        ("/operations/{id}", "get"),
        ("/operations/{id}/logs", "get"),
        ("/events", "get"),
        ("/users", "post"),
        ("/tokens/revoke", "post"),
        ("/recovery/export", "post"),
        ("/recovery/import", "post"),
        ("/login", "post"),
        ("/moddb", "get"),
        ("/moddb/{id}", "get"),
    ];
    for (path, method) in routes {
        doc["paths"][format!("/api/v1{path}")][method] = json!({"responses":{"200":{"description":"Successful response"},"400":{"description":"Invalid request"},"401":{"description":"Authentication required"}},"security":[{"bearerAuth":[]}]});
        let operation = &mut doc["paths"][format!("/api/v1{path}")][method];
        if path == "/servers/{id}/agent/mods" {
            operation["description"] = json!("Enqueue a custom ZIP install. Returns a durable mod-upload operation immediately; poll its status and logs for the result.");
            operation["responses"]["200"]["content"]["application/json"]["schema"] =
                json!({"$ref":"#/components/schemas/Operation"});
        }
        if path == "/operations/{id}/logs" {
            operation["responses"]["200"]["content"]["application/json"]["schema"] = json!({"type":"object","required":["available","entries","summary","retention"],"properties":{"available":{"type":"boolean"},"summary":{"type":"string"},"retention":{"type":"object","properties":{"days":{"type":"integer"},"max_entries":{"type":"integer"}}},"entries":{"type":"array","items":{"type":"object","required":["seq","at","level","stage","message"],"properties":{"seq":{"type":"integer"},"at":{"type":"string"},"level":{"type":"string"},"stage":{"type":"string"},"message":{"type":"string"}}}}}});
        }
        if path == "/hosting/catalog" || path == "/game/releases" {
            operation["security"] = json!([]);
        }
        let mut params = Vec::new();
        for name in ["id", "action", "endpoint"] {
            if path.contains(&format!("{{{name}}}")) {
                params.push(
                    json!({"name":name,"in":"path","required":true,"schema":{"type":"string"}}),
                );
            }
        }
        if path.ends_with("/quote") || path.ends_with("/price") {
            for name in ["region", "size"] {
                params.push(
                    json!({"name":name,"in":"query","required":true,"schema":{"type":"string"}}),
                );
            }
        }
        if path.ends_with("/sizes") {
            params.push(
                json!({"name":"region","in":"query","required":true,"schema":{"type":"string"}}),
            );
        }
        if path == "/deployments"
            || path.contains("/actions/")
            || (path.ends_with("/cloud-idle-policy") && method == "put")
        {
            params.push(json!({"name":"Idempotency-Key","in":"header","required":true,"schema":{"type":"string"}}));
        }
        operation["parameters"] = json!(params);
        if method == "post" || method == "put" {
            let schema = match path {
                "/accounts" => json!({"$ref":"#/components/schemas/Account"}),
                "/plans" => json!({"$ref":"#/components/schemas/DeploySpec"}),
                "/recommendations" => json!({"$ref":"#/components/schemas/RecommendationInput"}),
                "/servers/{id}/cloud-idle-policy" => {
                    json!({"type":"object","required":["enabled","minutes","external_manager_confirmed"],"properties":{"enabled":{"type":"boolean","default":false},"minutes":{"type":"integer","minimum":1,"maximum":1440,"default":15},"external_manager_confirmed":{"type":"boolean","default":false}}})
                }
                _ => json!({"type":"object","additionalProperties":true}),
            };
            operation["requestBody"] =
                json!({"required":true,"content":{"application/json":{"schema":schema}}});
        }
        if path == "/login" {
            operation["security"] = json!([]);
        }
    }
    doc["components"]["securitySchemes"] = json!({"bearerAuth":{"type":"http","scheme":"bearer"}});
    doc
}
async fn openapi() -> Json<Value> {
    Json(api_document())
}
#[derive(Deserialize)]
struct ModQuery {
    #[serde(default)]
    q: String,
}
async fn mod_search(Extension(_p): Extension<Principal>, Query(q): Query<ModQuery>) -> ApiResult {
    Ok(Json(crate::moddb::search(&q.q).await?))
}
async fn mod_detail(Extension(_p): Extension<Principal>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(crate::moddb::detail(&id).await?))
}
// Use the trait directly without exposing a concrete stream type.
mod tokio_stream_compat {
    pub use futures_core::Stream;
}
include!("service_idle.rs");
include!("service_discovery.rs");

#[cfg(test)]
mod remote_installation_tests {
    use super::*;
    #[test]
    fn provider_power_states_do_not_establish_installation() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("db")).unwrap();
        let mut server:Server=serde_json::from_value(json!({"id":"test","name":"test","account_id":null,"provider":"azure","region":"test","size":"test","host":"example.test","ssh_user":"vsm","ssh_key":"unused","game_version":"1.22.7","state":"awaiting_bootstrap","created_at":now(),"resources":[]})).unwrap();
        for state in ["awaiting_bootstrap", "deallocated", "stopped", "attention"] {
            server.state = state.into();
            assert!(!remote_installation_ready(&db, &server).unwrap());
        }
        record_remote_installation(&db, &server.id).unwrap();
        for state in ["deallocated", "stopped", "attention"] {
            server.state = state.into();
            assert!(remote_installation_ready(&db, &server).unwrap());
        }
    }
}

include!("service_agent_update.rs");
