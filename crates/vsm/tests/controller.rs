use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;
use vsm::{
    auth,
    domain::*,
    service::{router, Controller},
};

async fn request(
    app: axum::Router,
    token: &str,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let r = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1{path}"))
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .header("Idempotency-Key", "test-key")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = r.status();
    let bytes = to_bytes(r.into_body(), 10 * 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn account() -> Value {
    json!({"id":"ssh-test","name":"Test host","provider":"ssh","credential_ref":"","settings":{}})
}
#[tokio::test]
async fn recovery_endpoints_require_owner_before_accessing_cloud_or_host() {
    let temp = tempfile::tempdir().unwrap();
    let (c, _) = Controller::open(temp.path().join("controller"), false).unwrap();
    let app = router(c.clone(), temp.path().join("ui"));
    // Nonexistent account/host ensures authorization must happen before lookup/network access.
    for role in ["viewer", "administrator"] {
        auth::create_user(&c.db, role, "long-test-password", role, &[], None).unwrap();
        let token = auth::login(&c.db, role, "long-test-password", "", false).unwrap();
        for (method, path, body) in [
            ("GET", "/accounts/missing/discover", json!({})),
            (
                "POST",
                "/accounts/missing/adopt",
                json!({"vm_id":"/missing"}),
            ),
            (
                "PATCH",
                "/servers/missing/connection",
                json!({"ssh_private_key":""}),
            ),
            ("POST", "/servers/missing/reconnect", json!({})),
        ] {
            let (status, body) = request(app.clone(), &token, method, path, body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{role}: {path}");
            assert!(
                body["error"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("owner"),
                "{body}"
            );
        }
    }
    assert!(c.db.operations().unwrap().is_empty());
}
#[tokio::test]
async fn recovered_server_cannot_install_or_mutate_before_reconnect() {
    let temp = tempfile::tempdir().unwrap();
    let (c, token) = Controller::open(temp.path().join("controller"), false).unwrap();
    let server = json!({"id":"recovered-test","name":"Recovered","account_id":null,"provider":"azure","region":"","size":"","host":"","ssh_user":"vsm","ssh_key":"","game_version":"","state":"recovered","created_at":"2026-09-06","resources":[]});
    c.db.put("servers", "recovered-test", &server).unwrap();
    let app = router(c.clone(), temp.path().join("ui"));
    for action in [
        "bootstrap",
        "agent-update",
        "cloud-restart",
        "deallocate",
        "resize",
        "delete",
        "start",
        "stop",
    ] {
        let (status, error) = request(
            app.clone(),
            &token,
            "POST",
            &format!("/servers/recovered-test/actions/{action}"),
            json!({"confirm_name":"Recovered","accept_cost_change":true}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{action}");
        assert!(
            error["error"].as_str().unwrap().contains("Reconnect"),
            "{action}: {error}"
        );
        assert!(
            c.db.operations().unwrap().is_empty(),
            "{action} must not enqueue work"
        );
    }
    let (status, error) = request(
        app.clone(),
        &token,
        "PUT",
        "/servers/recovered-test/agent/config",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("Reconnect"));
    assert!(c.db.operations().unwrap().is_empty());
    assert_eq!(c.db.server("recovered-test").unwrap().state, "recovered");
}
#[tokio::test]
async fn custom_mod_upload_returns_queued_operation_and_records_failure() {
    let temp = tempfile::tempdir().unwrap();
    let (controller, token) = Controller::open(temp.path().join("controller"), false).unwrap();
    // Invalid host prevents network or game access; validation fails in the background.
    let server = json!({"id":"mods-test","name":"Mods test","account_id":null,"provider":"ssh","region":"","size":"","host":"","ssh_user":"test","ssh_key":"","game_version":"1.22.7","state":"stopped","created_at":"2026-09-06","resources":[]});
    controller.db.put("servers", "mods-test", &server).unwrap();
    let app = router(controller.clone(), temp.path().join("ui"));
    let body = json!({"filename":"example.zip","content_base64":"not-logged","acknowledge_unknown_compatibility":true});
    let (status, operation) = request(
        app.clone(),
        &token,
        "POST",
        "/servers/mods-test/agent/mods",
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(operation["action"], "mod-upload");
    assert_eq!(operation["status"], "queued");
    let id = operation["id"].as_str().unwrap();
    let (_, duplicate) = request(app, &token, "POST", "/servers/mods-test/agent/mods", body).await;
    assert_eq!(duplicate["id"], id);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if controller.db.operation(id).unwrap().status == "failed" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let logs = controller.db.operation_logs(id).unwrap().to_string();
    assert!(logs.contains("Invalid host"));
    assert!(!logs.contains("not-logged"));
}
#[tokio::test]
async fn authentication_and_owner_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let (c, token) = Controller::open(temp.path().join("controller"), false).unwrap();
    let app = router(c.clone(), temp.path().join("ui"));
    assert_eq!(
        request(app.clone(), "bad", "GET", "/summary", json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(app.clone(), &token, "POST", "/accounts", account())
            .await
            .0,
        StatusCode::OK
    );
    auth::create_user(&c.db, "viewer", "long-test-password", "viewer", &[], None).unwrap();
    let viewer = auth::login(&c.db, "viewer", "long-test-password", "", false).unwrap();
    assert_eq!(
        request(app.clone(), &viewer, "GET", "/accounts", json!({}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(app.clone(), &viewer, "POST", "/accounts", account())
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    request(
        app.clone(),
        &token,
        "POST",
        "/tokens/revoke",
        json!({"name":"viewer"}),
    )
    .await;
    assert_eq!(
        request(app, &viewer, "GET", "/summary", json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn recovery_import_is_encrypted_and_transactional() {
    let t = tempfile::tempdir().unwrap();
    let (c, token) = Controller::open(t.path().join("source"), false).unwrap();
    let app = router(c, t.path().join("ui"));
    request(app.clone(), &token, "POST", "/accounts", account()).await;
    let (_, export) = request(
        app,
        &token,
        "POST",
        "/recovery/export",
        json!({"passphrase":"correct long test passphrase"}),
    )
    .await;
    assert!(export["content_base64"].is_string());
    let (c, token) = Controller::open(t.path().join("target"), false).unwrap();
    let app = router(c.clone(), t.path().join("ui"));
    let mut body = json!({"passphrase":"wrong passphrase","content_base64":export["content_base64"],"source_stopped":true});
    assert_eq!(
        request(
            app.clone(),
            &token,
            "POST",
            "/recovery/import",
            body.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(c.db.list::<Account>("accounts").unwrap().is_empty());
    body["passphrase"] = json!("correct long test passphrase");
    assert_eq!(
        request(
            app.clone(),
            &token,
            "POST",
            "/recovery/import",
            body.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(c.db.list::<Account>("accounts").unwrap().len(), 1);
    assert_eq!(
        request(app, &token, "POST", "/recovery/import", body)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn public_controller_rejects_bootstrap_and_unenrolled_writers() {
    let t = tempfile::tempdir().unwrap();
    let (c, token) = Controller::open(t.path().join("controller"), true).unwrap();
    auth::create_user(&c.db, "writer", "long-test-password", "owner", &[], None).unwrap();
    let app = router(c, t.path().join("ui"));
    assert_eq!(
        request(app.clone(), &token, "GET", "/summary", json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            app,
            "",
            "POST",
            "/login",
            json!({"name":"writer","password":"long-test-password"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn ssh_deployment_is_durable_and_does_not_spawn_a_vm() {
    let t = tempfile::tempdir().unwrap();
    let (mut c, token) = Controller::open(t.path().join("controller"), false).unwrap();
    c.release_catalog_override = Some(std::sync::Arc::new(serde_json::from_value(json!({
        "versions":[{"version":"1.21.6","filename":"vs_server_linux-x64_1.21.6.tar.gz","download_url":"https://cdn.vintagestory.at/gamefiles/stable/vs_server_linux-x64_1.21.6.tar.gz","filesize":"43 MB","md5":"0c2841f5a7638bc26719993416cf44d9","latest":true,"runtime":".NET 8","tested":false,"automatic_idle":false}],
        "latest":"1.21.6","tested_version":"1.22.7","source":"https://api.vintagestory.at/stable.json","fetched_at":vsm::domain::now(),"status":"current","error":null,"excluded":[],"login_required":false
    })).unwrap()));
    let app = router(c.clone(), t.path().join("ui"));
    request(app.clone(), &token, "POST", "/accounts", account()).await;
    let spec = json!({"name":"test-world","account_id":"ssh-test","region":"local","size":"existing","game_version":"1.21.6","ssh_public_key":"ssh-ed25519 AAAATEST","ssh_private_key":"test-key","admin_cidr":"192.0.2.1/32","existing_host":"192.0.2.2","ssh_user":"tester"});
    let mut unavailable = spec.clone();
    unavailable["game_version"] = json!("1.22.999");
    assert_eq!(
        request(app.clone(), &token, "POST", "/plans", unavailable)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, plan) = request(app.clone(), &token, "POST", "/plans", spec).await;
    assert_eq!(status, StatusCode::OK);
    let (status, op) = request(
        app.clone(),
        &token,
        "POST",
        "/deployments",
        json!({"plan_id":plan["id"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for _ in 0..100 {
        if c.db.operation(op["id"].as_str().unwrap()).unwrap().status == "succeeded" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let server = c.db.server(plan["id"].as_str().unwrap()).unwrap();
    assert_eq!(server.state, "awaiting_bootstrap");
    assert!(server.resources.is_empty());
    let (_, retry) = request(
        app,
        &token,
        "POST",
        "/deployments",
        json!({"plan_id":plan["id"]}),
    )
    .await;
    assert_eq!(retry["id"], op["id"]);
    assert_eq!(c.db.list::<Server>("servers").unwrap().len(), 1);
}

#[test]
fn controller_inventory_has_one_owner_process() {
    let root = tempfile::tempdir().unwrap();
    let (first, _) = Controller::open(root.path().to_path_buf(), false).unwrap();
    assert!(Controller::open(root.path().to_path_buf(), false).is_err());
    drop(first);
    assert!(Controller::open(root.path().to_path_buf(), false).is_ok());
}

#[tokio::test]
async fn account_edit_and_delete_are_owner_only() {
    let temp = tempfile::tempdir().unwrap();
    let (c, token) = Controller::open(temp.path().join("controller"), false).unwrap();
    let app = router(c.clone(), temp.path().join("ui"));
    request(app.clone(), &token, "POST", "/accounts", account()).await;
    auth::create_user(&c.db, "viewer", "long-test-password", "viewer", &[], None).unwrap();
    let viewer = auth::login(&c.db, "viewer", "long-test-password", "", false).unwrap();
    assert_eq!(
        request(
            app.clone(),
            &viewer,
            "DELETE",
            "/accounts/ssh-test",
            json!({})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(c.db.account("ssh-test").is_ok());
    let mut edited = account();
    edited["name"] = json!("Renamed");
    assert_eq!(
        request(app.clone(), &token, "POST", "/accounts", edited)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(c.db.list::<Account>("accounts").unwrap().len(), 1);
    assert_eq!(c.db.account("ssh-test").unwrap().name, "Renamed");
    assert_eq!(
        request(
            app.clone(),
            &token,
            "DELETE",
            "/accounts/ssh-test",
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(c.db.list::<Account>("accounts").unwrap().is_empty());
}

#[tokio::test]
async fn desktop_can_preflight_and_remove_account_without_allowing_arbitrary_origins() {
    let temp = tempfile::tempdir().unwrap();
    let (c, token) = Controller::open(temp.path().join("controller"), false).unwrap();
    let app = router(c.clone(), temp.path().join("ui"));
    request(app.clone(), &token, "POST", "/accounts", account()).await;
    for origin in [
        "http://tauri.localhost",
        "tauri://localhost",
        "http://localhost:1420",
    ] {
        let preflight = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/accounts/ssh-test")
                    .header("Origin", origin)
                    .header("Access-Control-Request-Method", "DELETE")
                    .header("Access-Control-Request-Headers", "authorization")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(preflight.status(), StatusCode::OK);
        assert_eq!(preflight.headers()["access-control-allow-origin"], origin);
        assert!(preflight.headers()["access-control-allow-methods"]
            .to_str()
            .unwrap()
            .split(',')
            .any(|method| method.trim() == "DELETE"));
        assert!(preflight.headers()["access-control-allow-methods"]
            .to_str()
            .unwrap()
            .split(',')
            .any(|method| method.trim() == "PATCH"));
        assert!(preflight.headers()["access-control-allow-headers"]
            .to_str()
            .unwrap()
            .contains("authorization"));
    }
    let denied = app
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/v1/accounts/ssh-test")
                .header("Origin", "https://untrusted.invalid")
                .header("Access-Control-Request-Method", "DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(!denied.headers().contains_key("access-control-allow-origin"));
    let removed = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/accounts/ssh-test")
                .header("Origin", "http://tauri.localhost")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::OK);
    assert_eq!(
        removed.headers()["access-control-allow-origin"],
        "http://tauri.localhost"
    );
    assert!(c.db.account("ssh-test").is_err());
}
