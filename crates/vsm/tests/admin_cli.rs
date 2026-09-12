use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::time::Duration;

async fn fixture(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, task)
}
async fn cli(url: &str, arguments: &[&str]) -> std::process::Output {
    let token = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(token.path(), "test-scoped-token").unwrap();
    let mut command = vsm::subprocess::command(env!("CARGO_BIN_EXE_vsm"));
    command
        .env_remove("VSM_URL")
        .env_remove("VSM_TOKEN")
        .env_remove("VSM_TOKEN_FILE")
        .args(["--url", url, "--token-file"])
        .arg(token.path())
        .arg("--json")
        .args(arguments)
        .kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn waited_failure_is_not_success_and_keeps_operation_details() {
    let app = Router::new()
        .route("/api/v1/servers/test-server/actions/start", post(|headers: axum::http::HeaderMap| async move {
            assert_eq!(headers["authorization"], "Bearer test-scoped-token");
            assert_eq!(headers["idempotency-key"], "repeatable-start");
            Json(json!({"id":"operation-1", "status":"queued"}))
        }))
        .route("/api/v1/operations/operation-1", get(|| async { Json(json!({"id":"operation-1", "status":"failed", "message":"Disk is full", "action":"start"})) }));
    let (url, task) = fixture(app).await;
    let output = cli(
        &url,
        &[
            "--idempotency-key",
            "repeatable-start",
            "start",
            "test-server",
            "--wait",
        ],
    )
    .await;
    task.abort();
    assert_eq!(output.status.code(), Some(8));
    assert!(output.stdout.is_empty());
    let result: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(result["error"]["code"], "operation_failed");
    assert_eq!(result["operation"]["id"], "operation-1");
    assert_eq!(result["operation"]["message"], "Disk is full");
}

#[tokio::test]
async fn permission_failures_have_stable_json_exit_codes_and_redact_tokens() {
    let (url, task) = fixture(Router::new().route(
        "/api/v1/servers",
        get(|| async {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Rejected test-scoped-token"})),
            )
        }),
    ))
    .await;
    let output = cli(&url, &["servers"]).await;
    task.abort();
    assert_eq!(output.status.code(), Some(3));
    let result: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(result["error"]["code"], "forbidden");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("test-scoped-token"));
}

#[tokio::test]
async fn bounded_wait_times_out_without_cancelling_the_operation() {
    let app = Router::new().route(
        "/api/v1/operations/operation-2",
        get(|| async { Json(json!({"id":"operation-2", "status":"running"})) }),
    );
    let (url, task) = fixture(app).await;
    let output = cli(
        &url,
        &["--wait-timeout", "1", "activity", "operation-2", "--wait"],
    )
    .await;
    task.abort();
    assert_eq!(output.status.code(), Some(6));
    let result: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(result["error"]["code"], "timeout");
}
