//! Test the shipped CLI entry point with an ordinary newline-delimited MCP client.
use axum::{
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn exchange(
    input: &mut tokio::process::ChildStdin,
    output: &mut BufReader<tokio::process::ChildStdout>,
    request: Value,
) -> Value {
    input
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    input.flush().await.unwrap();
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), output.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).expect("stdout must contain JSON-RPC, never banners or diagnostics")
}

#[tokio::test]
async fn cli_mcp_initializes_lists_and_calls_over_stdio() {
    async fn servers(headers: HeaderMap) -> Json<Value> {
        assert_eq!(
            headers.get("authorization").unwrap(),
            "Bearer test-only-mcp-token"
        );
        Json(json!([{"id":"stdio-fixture","name":"Fixture"}]))
    }
    async fn start(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(
            headers.get("authorization").unwrap(),
            "Bearer test-only-mcp-token"
        );
        assert_eq!(headers.get("idempotency-key").unwrap(), "mcp-stdio-retry");
        assert_eq!(body, json!({}));
        Json(
            json!({"id":"fixture-operation", "server_id":"stdio-fixture", "action":"start", "status":"queued"}),
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let manager = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/api/v1/servers", get(servers))
                .route("/api/v1/servers/stdio-fixture/actions/start", post(start)),
        )
        .await
        .unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let token_path = directory.path().join("test.token");
    std::fs::write(&token_path, "test-only-mcp-token").unwrap();
    let mut process = vsm::subprocess::command(env!("CARGO_BIN_EXE_vsm"));
    process
        .args(["--url", &format!("http://{address}"), "--token-file"])
        .arg(&token_path)
        .arg("mcp")
        .env_remove("VSM_URL")
        .env_remove("VSM_TOKEN")
        .env_remove("VSM_TOKEN_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = process.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let initialized = exchange(
        &mut input,
        &mut output,
        json!({
            "jsonrpc":"2.0", "id":1, "method":"initialize", "params":{
                "protocolVersion":"2025-11-25", "capabilities":{},
                "clientInfo":{"name":"stdio-test", "version":"1"}
            }
        }),
    )
    .await;
    assert_eq!(initialized["result"]["serverInfo"]["name"], "vsm");
    input
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let tools = exchange(
        &mut input,
        &mut output,
        json!({
            "jsonrpc":"2.0", "id":2, "method":"tools/list", "params":{}
        }),
    )
    .await;
    assert!(tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "mod_install"));
    let result = exchange(
        &mut input,
        &mut output,
        json!({
            "jsonrpc":"2.0", "id":3, "method":"tools/call", "params":{
                "name":"servers_list", "arguments":{}
            }
        }),
    )
    .await;
    assert_eq!(
        result["result"]["structuredContent"]["data"][0]["id"],
        "stdio-fixture"
    );
    assert_eq!(result["result"]["isError"], false);
    assert!(!result.to_string().contains("test-only-mcp-token"));
    for id in [4, 5] {
        let accepted = exchange(
            &mut input,
            &mut output,
            json!({
                "jsonrpc":"2.0", "id":id, "method":"tools/call", "params":{
                    "name":"server_start", "arguments":{
                        "server_id":"stdio-fixture", "idempotency_key":"mcp-stdio-retry"
                    }
                }
            }),
        )
        .await;
        assert_eq!(accepted["result"]["isError"], false);
        assert_eq!(
            accepted["result"]["structuredContent"]["data"]["id"],
            "fixture-operation"
        );
        assert_eq!(
            accepted["result"]["structuredContent"]["data"]["status"],
            "queued"
        );
    }
    drop(input);
    let completed = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        completed.status.success(),
        "MCP exited unsuccessfully: {}",
        String::from_utf8_lossy(&completed.stderr)
    );
    manager.abort();
}
