//! Shared, HTTP-only administration surface for the CLI and MCP clients.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Serialize)]
pub struct AdminError {
    pub code: String,
    pub message: String,
    pub status: Option<u16>,
}
impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AdminError {}
impl AdminError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status: None,
        }
    }
    pub fn exit_code(&self) -> i32 {
        match self.code.as_str() {
            "invalid_input" | "configuration" => 2,
            "unauthorized" | "forbidden" => 3,
            "not_found" => 4,
            "conflict" => 5,
            "timeout" => 6,
            "transport" => 7,
            "operation_failed" => 8,
            _ => 1,
        }
    }
}
type Result<T> = std::result::Result<T, AdminError>;

#[derive(Clone)]
pub struct AdminClient {
    http: reqwest::Client,
    base: reqwest::Url,
    token: String,
    instance: Option<String>,
    desktop_root: Option<PathBuf>,
}
#[derive(Serialize, Deserialize)]
struct DesktopConnection {
    version: u32,
    url: String,
    instance_id: String,
}

pub fn publish_desktop_connection(root: &Path, url: &str, instance_id: &str) -> anyhow::Result<()> {
    let data = serde_json::to_vec(&DesktopConnection {
        version: 1,
        url: url.into(),
        instance_id: instance_id.into(),
    })?;
    // NamedTempFile replaces atomically on Windows as well as Unix. Protect before publication.
    use std::io::Write;
    let mut temporary = tempfile::NamedTempFile::new_in(root)?;
    temporary.write_all(&data)?;
    temporary.as_file().sync_all()?;
    crate::auth::protect_path(temporary.path(), false)?;
    temporary.persist(root.join("connection.json"))?;
    Ok(())
}
pub fn desktop_data_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root =
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
    root.map(|p| p.join("org.vsm.community")).ok_or_else(|| {
        AdminError::new(
            "configuration",
            "Desktop data directory is unavailable; provide its path explicitly",
        )
    })
}
impl AdminClient {
    pub fn new(base_url: &str, token: &str) -> Result<Self> {
        let token = token.trim();
        let base = reqwest::Url::parse(base_url)
            .map_err(|_| AdminError::new("configuration", "Manager URL is invalid"))?;
        let loopback = base.host_str().is_some_and(|h| {
            h == "localhost"
                || h.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
            || !(base.scheme() == "https" || base.scheme() == "http" && loopback)
        {
            return Err(AdminError::new("configuration", "Use a Manager origin with HTTPS, or HTTP on loopback; credentials, paths and query strings are not allowed"));
        }
        if token.trim().is_empty() || token.contains(['\r', '\n']) {
            return Err(AdminError::new(
                "configuration",
                "A valid Manager token is required",
            ));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| AdminError::new("configuration", "Could not initialize HTTP client"))?;
        Ok(Self {
            http,
            base,
            token: token.trim().into(),
            instance: None,
            desktop_root: None,
        })
    }
    pub fn from_desktop(root: Option<PathBuf>) -> Result<Self> {
        let root = match root {
            Some(p) => p,
            None => desktop_data_dir()?,
        };
        let bytes = std::fs::read(root.join("connection.json")).map_err(|_| {
            AdminError::new(
                "configuration",
                "Open an updated desktop app first, or specify a Manager URL and token file",
            )
        })?;
        let descriptor: DesktopConnection = serde_json::from_slice(&bytes).map_err(|_| {
            AdminError::new("configuration", "Invalid desktop connection descriptor")
        })?;
        let token = std::fs::read_to_string(root.join("owner.token"))
            .map_err(|_| AdminError::new("configuration", "Desktop token could not be read"))?;
        let mut client = Self::new(&descriptor.url, &token)?;
        if descriptor.version != 1
            || uuid::Uuid::parse_str(&descriptor.instance_id).is_err()
            || client.base.scheme() != "http"
            || client.base.host_str() != Some("127.0.0.1")
        {
            return Err(AdminError::new(
                "configuration",
                "Invalid desktop connection descriptor",
            ));
        }
        client.instance = Some(descriptor.instance_id);
        client.desktop_root = Some(root);
        Ok(client)
    }
    async fn verify_instance(&self) -> Result<()> {
        if let Some(instance) = &self.instance {
            let response = self
                .http
                .get(self.base.join("health").unwrap())
                .timeout(Duration::from_secs(5))
                .send()
                .await
                .map_err(|_| {
                    AdminError::new(
                        "transport",
                        "Desktop Manager is not reachable; reopen the desktop app",
                    )
                })?;
            if !response.status().is_success() {
                return Err(AdminError::new(
                    "transport",
                    "Desktop Manager identity check failed",
                ));
            }
            let value: Value = response.json().await.map_err(|_| {
                AdminError::new("transport", "Desktop Manager identity check failed")
            })?;
            if value["instance_id"].as_str() != Some(instance.as_str()) {
                return Err(AdminError::new(
                    "transport",
                    "Desktop connection is stale; reconnect to the running desktop app",
                ));
            }
        }
        Ok(())
    }
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        idempotency_key: Option<&str>,
    ) -> Result<Value> {
        if let Some(root) = &self.desktop_root {
            // Desktop restarts use a new loopback port and runtime identity. Resolve only
            // the explicitly selected inventory; never search for another signed-in app.
            return Self::from_desktop(Some(root.clone()))?
                .request_current(method, path, body, idempotency_key)
                .await;
        }
        self.request_current(method, path, body, idempotency_key)
            .await
    }
    async fn request_current(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        idempotency_key: Option<&str>,
    ) -> Result<Value> {
        let method = match method.to_ascii_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "PATCH" => reqwest::Method::PATCH,
            "DELETE" => reqwest::Method::DELETE,
            _ => {
                return Err(AdminError::new(
                    "invalid_input",
                    "Supported methods: GET, POST, PUT, PATCH, DELETE",
                ))
            }
        };
        // No URL joining with user-controlled paths: reject traversal, encoded separators and fragments.
        let lower = path.to_ascii_lowercase();
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['\\', '#', '\r', '\n'])
            || lower
                .split('?')
                .next()
                .unwrap_or("")
                .split('/')
                .any(|s| s == "." || s == ".." || s.contains('%'))
        {
            return Err(AdminError::new("invalid_input", "Provide an API-relative path beginning with a single slash, without traversal or encoded path segments"));
        }
        self.verify_instance().await?;
        let url = self
            .base
            .join(&format!("api/v1{path}"))
            .map_err(|_| AdminError::new("invalid_input", "Invalid API path"))?;
        let key = idempotency_key
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if key.len() > 200 || key.is_empty() || !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(AdminError::new(
                "invalid_input",
                "Idempotency key must contain 1–200 printable ASCII characters",
            ));
        }
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(&self.token)
            .header("Idempotency-Key", &key);
        if let Some(mut body) = body {
            // A read/edit/write round trip must never replace an existing secret with our sentinel.
            if path
                .split('?')
                .next()
                .is_some_and(|p| p.ends_with("/agent/config"))
            {
                preserve_redacted_secrets(&mut body);
            }
            request = request.json(&body);
        }
        let mut response = request.send().await.map_err(|e| AdminError::new(if e.is_timeout() { "timeout" } else { "transport" }, &format!("Manager request did not complete; inspect state before retrying. Supported durable operations can be retried with idempotency key: {}", key.replace(&self.token, "[redacted]"))))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AdminError::new("transport", "Manager response was interrupted"))?
        {
            if bytes.len() + chunk.len() > 96 * 1024 * 1024 {
                return Err(AdminError::new(
                    "response_too_large",
                    "Manager response exceeds the client limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| AdminError {
            code: "invalid_response".into(),
            message: "Manager did not return JSON".into(),
            status: Some(status.as_u16()),
        })?;
        if !status.is_success() {
            let code = match status.as_u16() {
                401 => "unauthorized",
                403 => "forbidden",
                404 => "not_found",
                409 => "conflict",
                408 | 504 => "timeout",
                _ => "api_error",
            };
            let message = crate::diagnostics::redact(
                &value["error"]
                    .as_str()
                    .unwrap_or("Manager request failed")
                    .replace(&self.token, "[redacted]"),
            );
            return Err(AdminError {
                code: code.into(),
                message,
                status: Some(status.as_u16()),
            });
        }
        Ok(redact_output(value, &self.token))
    }
    pub async fn wait_operation(&self, id: &str, timeout: Duration) -> Result<Value> {
        segment(id)?;
        if timeout.is_zero() || timeout > Duration::from_secs(600) {
            return Err(AdminError::new(
                "invalid_input",
                "Wait timeout must be 1–600 seconds",
            ));
        }
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let result = tokio::time::timeout_at(
                deadline,
                self.request("GET", &format!("/operations/{id}"), None, None),
            )
            .await
            .map_err(|_| {
                AdminError::new(
                    "timeout",
                    "Operation is still pending or unreachable; waiting does not cancel it",
                )
            })??;
            match result["status"].as_str() {
                Some("succeeded" | "failed" | "cancelled" | "interrupted") => return Ok(result),
                Some("queued" | "running") => {}
                _ => {
                    return Err(AdminError::new(
                        "invalid_response",
                        "Operation has an unknown status",
                    ))
                }
            }
            if tokio::time::Instant::now() + Duration::from_secs(1) >= deadline {
                return Err(AdminError::new(
                    "timeout",
                    "Operation is still pending; waiting does not cancel it",
                ));
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
fn redact_output(value: Value, token: &str) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let secret = sensitive_key(&key);
                    (
                        key,
                        if secret {
                            json!("[redacted]")
                        } else {
                            redact_output(value, token)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(|v| redact_output(v, token)).collect()),
        Value::String(s) => Value::String(s.replace(token, "[redacted]")),
        other => other,
    }
}
fn sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [
        "token",
        "password",
        "secret",
        "authorization",
        "totp",
        "access_token",
        "refresh_token",
        "passphrase",
    ]
    .contains(&lower.as_str())
        || lower.ends_with("password")
        || lower.ends_with("secret")
        || lower.ends_with("_token")
}
fn preserve_redacted_secrets(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, value| !(sensitive_key(key) && value.as_str() == Some("[redacted]")));
            for value in map.values_mut() {
                preserve_redacted_secrets(value);
            }
        }
        Value::Array(array) => {
            for value in array {
                preserve_redacted_secrets(value);
            }
        }
        _ => {}
    }
}
fn segment(value: &str) -> Result<&str> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        || value == "."
        || value == ".."
    {
        return Err(AdminError::new(
            "invalid_input",
            "Identifier contains unsupported characters",
        ));
    }
    Ok(value)
}

include!("admin_catalog.rs");
include!("admin_bodies.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::HeaderMap,
        routing::{get, post},
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    async fn start(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (base, task)
    }
    #[test]
    fn origin_and_argument_validation() {
        for url in [
            "http://example.com",
            "https://user:secret@example.com",
            "https://example.com/path",
            "https://example.com/?token=x",
            "file:///tmp",
        ] {
            assert!(AdminClient::new(url, "test-token").is_err());
        }
        assert!(AdminClient::new("https://example.com", " test-token\n").is_ok());
        assert!(AdminClient::new("http://127.0.0.1:4317", "x\ny").is_err());
        let tools = catalog();
        let names: std::collections::HashSet<_> = tools.iter().map(|t| &t.name).collect();
        assert_eq!(names.len(), tools.len());
        let delete = tools.iter().find(|t| t.name == "server_delete").unwrap();
        assert!(delete.destructive && !delete.read_only);
        assert!(validate_input(&json!({"server_id":"x","body":{}}), &delete.input_schema).is_err());
        let resize = tools.iter().find(|t| t.name == "vm_resize").unwrap();
        assert!(validate_input(
            &json!({"server_id":"x","body":{"size":"x","accept_cost_change":false}}),
            &resize.input_schema
        )
        .is_err());
    }
    #[tokio::test]
    async fn paths_and_console_injection_rejected_before_network() {
        let client = AdminClient::new("http://127.0.0.1:1", "test-token").unwrap();
        for path in [
            "//example.com",
            "/../health",
            "/%2e%2e/health",
            "/servers\\evil",
            "/servers#ignored",
        ] {
            assert_eq!(
                client
                    .request("GET", path, None, None)
                    .await
                    .unwrap_err()
                    .code,
                "invalid_input"
            );
        }
        for username in ["name\nstop", "name whitelist off", "x;stop", ""] {
            assert_eq!(
                client
                    .execute(
                        "player_whitelist",
                        json!({"server_id":"server","username":username}),
                        None
                    )
                    .await
                    .unwrap_err()
                    .code,
                "invalid_input"
            );
        }
    }
    #[tokio::test]
    async fn exact_mod_release_and_whitelist_map_to_api_with_stable_key() {
        type Requests = Arc<Mutex<Vec<(String, Value)>>>;
        async fn capture(
            State(requests): State<Requests>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            assert_eq!(headers["authorization"], "Bearer test-token");
            requests
                .lock()
                .unwrap()
                .push((headers["idempotency-key"].to_str().unwrap().into(), body));
            Json(json!({"id":"operation","status":"queued"}))
        }
        let requests: Requests = Default::default();
        let router = Router::new()
            .route("/api/v1/servers/server/actions/mod-install", post(capture))
            .route("/api/v1/servers/server/actions/console", post(capture))
            .with_state(requests.clone());
        let (url, task) = start(router).await;
        let client = AdminClient::new(&url, "test-token").unwrap();
        let op = client.execute("mod_install",json!({"server_id":"server","mod_id":"4425","release_id":55847,"idempotency_key":"retry-1"}),None).await.unwrap();
        assert_eq!(op["status"], "queued");
        client
            .execute(
                "player_whitelist",
                json!({"server_id":"server","username":"Chris_1"}),
                Some("retry-2"),
            )
            .await
            .unwrap();
        let captured = requests.lock().unwrap();
        assert_eq!(
            captured[0],
            (
                "retry-1".into(),
                json!({"modid":"4425","releaseid":55847,"acknowledge_unknown_compatibility":false})
            )
        );
        assert_eq!(
            captured[1].1,
            json!({"command":"player Chris_1 whitelist on"})
        );
        task.abort();
    }
    #[tokio::test]
    async fn config_round_trip_preserves_secret_and_error_redacts_bearer() {
        async fn save(Json(body): Json<Value>) -> Json<Value> {
            assert!(body["value"].get("ServerPassword").is_none());
            assert_eq!(body["value"]["ServerName"], "New name");
            Json(json!({"etag":"new"}))
        }
        let router = Router::new().route("/api/v1/servers/s/agent/config", get(|| async { Json(json!({"value":{"ServerPassword":"secret","ServerName":"Old name","unknown":true},"etag":"old"})) }).put(save)).route("/api/v1/fail",get(|| async { (axum::http::StatusCode::FORBIDDEN,Json(json!({"error":"rejected test-token"}))) }));
        let (url, task) = start(router).await;
        let client = AdminClient::new(&url, "test-token").unwrap();
        let mut config = client
            .execute("config_get", json!({"server_id":"s"}), None)
            .await
            .unwrap();
        assert_eq!(config["value"]["ServerPassword"], "[redacted]");
        config["value"]["ServerName"] = json!("New name");
        client
            .execute("config_set", json!({"server_id":"s","body":config}), None)
            .await
            .unwrap();
        let error = client
            .request("GET", "/fail", None, None)
            .await
            .unwrap_err();
        assert_eq!(error.code, "forbidden");
        assert!(!error.message.contains("test-token"));
        task.abort();
    }
    #[tokio::test]
    async fn redirect_is_never_followed_and_desktop_instance_is_verified_without_auth() {
        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = requests.clone();
        let router = Router::new()
            .route(
                "/api/v1/servers",
                get(|| async { axum::response::Redirect::temporary("/destination") }),
            )
            .route(
                "/destination",
                get(move || {
                    let count = count.clone();
                    async move {
                        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        Json(json!({}))
                    }
                }),
            )
            .route(
                "/health",
                get(|headers: HeaderMap| async move {
                    assert!(!headers.contains_key("authorization"));
                    Json(json!({"instance_id":"different-instance"}))
                }),
            );
        let (url, task) = start(router).await;
        let client = AdminClient::new(&url, "test-token").unwrap();
        assert!(client.request("GET", "/servers", None, None).await.is_err());
        assert_eq!(requests.load(std::sync::atomic::Ordering::Relaxed), 0);
        let root = tempfile::tempdir().unwrap();
        publish_desktop_connection(root.path(), &url, &uuid::Uuid::new_v4().to_string()).unwrap();
        std::fs::write(root.path().join("owner.token"), "test-token\n").unwrap();
        let discovered = AdminClient::from_desktop(Some(root.path().into())).unwrap();
        assert_eq!(
            discovered
                .request("GET", "/servers", None, None)
                .await
                .unwrap_err()
                .code,
            "transport"
        );
        task.abort();
    }
    #[tokio::test]
    async fn waits_preserve_failure_and_do_not_report_timeout_as_success() {
        let router = Router::new()
            .route(
                "/api/v1/operations/failed",
                get(|| async {
                    Json(json!({"id":"failed","status":"failed","message":"Archive invalid"}))
                }),
            )
            .route(
                "/api/v1/operations/pending",
                get(|| async { Json(json!({"id":"pending","status":"running"})) }),
            );
        let (url, task) = start(router).await;
        let client = AdminClient::new(&url, "test-token").unwrap();
        assert_eq!(
            client
                .wait_operation("failed", Duration::from_secs(1))
                .await
                .unwrap()["status"],
            "failed"
        );
        assert_eq!(
            client
                .wait_operation("pending", Duration::from_secs(1))
                .await
                .unwrap_err()
                .code,
            "timeout"
        );
        task.abort();
    }
    #[tokio::test]
    async fn desktop_restart_refreshes_same_inventory_connection_and_token() {
        let root = tempfile::tempdir().unwrap();
        let mut client = None;
        for token in ["first-token", "second-token"] {
            let instance = uuid::Uuid::new_v4().to_string();
            let health_instance = instance.clone();
            let router = Router::new()
                .route(
                    "/health",
                    get(move |headers: HeaderMap| {
                        let instance = health_instance.clone();
                        async move {
                            assert!(!headers.contains_key("authorization"));
                            Json(json!({"instance_id":instance}))
                        }
                    }),
                )
                .route(
                    "/api/v1/servers",
                    get(move |headers: HeaderMap| async move {
                        assert_eq!(headers["authorization"], format!("Bearer {token}"));
                        Json(json!([]))
                    }),
                );
            let (url, task) = start(router).await;
            publish_desktop_connection(root.path(), &url, &instance).unwrap();
            std::fs::write(root.path().join("owner.token"), token).unwrap();
            if client.is_none() {
                client = Some(AdminClient::from_desktop(Some(root.path().into())).unwrap());
            }
            assert_eq!(
                client
                    .as_ref()
                    .unwrap()
                    .execute("servers_list", json!({}), None)
                    .await
                    .unwrap(),
                json!([])
            );
            task.abort();
        }
    }
}
