#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::Manager;
struct Connection {
    root: PathBuf,
    bundled: PathBuf,
    current: Arc<Mutex<Option<serde_json::Value>>>,
}
#[tauri::command]
async fn store_cloud_credential(secret: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        vsm::auth::store_cloud_credential(&secret)
            .map_err(|_| "Could not save the credential in OS credential storage.".to_string())
    })
    .await
    .map_err(|_| "Credential storage failed.".to_string())?
}

// Tauri synchronous commands run on the window thread. Keep filesystem work,
// credential initialization, SQLite migrations and inventory locks off that thread.
async fn cached_connection<F>(
    current: Arc<Mutex<Option<serde_json::Value>>>,
    initialize: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce() -> Result<serde_json::Value, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let mut cached = current
            .lock()
            .map_err(|_| "Manager startup lock failed.".to_string())?;
        if let Some(value) = cached.as_ref() {
            return Ok(value.clone());
        }
        // Serialize initialization even if the invoking webview reloads while it
        // is in progress. Errors leave the cache empty so Retry can start again.
        let value = initialize()?;
        *cached = Some(value.clone());
        Ok(value)
    })
    .await
    .map_err(|_| "Manager startup failed. Try again.".to_string())?
}

#[tauri::command]
async fn connection(state: tauri::State<'_, Connection>) -> Result<serde_json::Value, String> {
    let root = state.root.clone();
    let bundled = state.bundled.clone();
    let current = state.current.clone();
    let cached = current.clone();
    cached_connection(current, move || {
        if bundled.is_file() {
            std::fs::create_dir_all(root.join("artifacts")).map_err(|e| e.to_string())?;
            std::fs::copy(&bundled, root.join("artifacts/vsm-agent")).map_err(|e| e.to_string())?;
        }
        let (controller, token) =
            vsm::service::Controller::open(root.clone(), false).map_err(|e| e.to_string())?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        vsm::admin::publish_desktop_connection(
            &root,
            &format!("http://127.0.0.1:{port}"),
            &controller.instance_id,
        )
        .map_err(|e| e.to_string())?;
        let value = serde_json::json!({"url":format!("http://127.0.0.1:{port}"),"token":token});
        tauri::async_runtime::spawn(async move {
            if let Ok(listener) = tokio::net::TcpListener::from_std(listener) {
                if let Err(e) =
                    vsm::service::serve(controller, listener, PathBuf::from("unused")).await
                {
                    eprintln!("Server Manager stopped: {e}");
                }
            }
            // Waiting for the short final cache write must also stay off the
            // async runtime: a server may fail immediately during startup.
            let _ = tauri::async_runtime::spawn_blocking(move || {
                if let Ok(mut value) = cached.lock() {
                    *value = None;
                }
            })
            .await;
        });
        Ok(value)
    })
    .await
}
fn command(name: &str) -> std::process::Command {
    vsm::subprocess::std_command(name)
}
#[tauri::command]
fn open_azure_help() -> Result<(), String> {
    let url = "https://learn.microsoft.com/cli/azure/install-azure-cli";
    #[cfg(windows)]
    let mut cmd = {
        let mut c = command("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = command("open");
    #[cfg(target_os = "linux")]
    let mut cmd = command("xdg-open");
    cmd.arg(url).spawn().map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
async fn azure_accounts(login: bool) -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = command("cmd");
        c.args(["/d", "/c", "az"]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = command("az");
    cmd.env("AZURE_CORE_LOGIN_EXPERIENCE_V2", "off");
    if login {
        cmd.args(["login", "--output", "json"]);
    } else {
        cmd.args(["account", "list", "--output", "json"]);
    }
    let mut process = tokio::process::Command::from(cmd);
    process.kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(180), process.output())
        .await
        .map_err(|_| "Azure sign-in timed out. Try again.".to_owned())?
        .map_err(|_| {
            "Azure CLI is required. Install it, then restart Server Manager.".to_owned()
        })?;
    if !output.status.success() {
        return Err(
            "Azure sign-in failed. Check that Azure CLI is installed and try signing in again."
                .to_owned(),
        );
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| "Azure returned an unreadable account list.".to_owned())
}
#[tauri::command]
async fn create_ssh_key(state: tauri::State<'_, Connection>) -> Result<serde_json::Value, String> {
    let directory = state.root.join("keys");
    tauri::async_runtime::spawn_blocking(move || {
        static KEY_LOCK: Mutex<()> = Mutex::new(());
        let _guard = KEY_LOCK.lock().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        vsm::auth::protect_path(&directory, true).map_err(|e| e.to_string())?;
        let key = directory.join("server-manager-ed25519");
        if !key.exists() {
            let result = command("ssh-keygen")
                .args(["-t", "ed25519", "-N", "", "-C", "vsm"])
                .arg("-f")
                .arg(&key)
                .output()
                .map_err(|_| "OpenSSH is required to create a server access key.".to_owned())?;
            if !result.status.success() {
                return Err("Could not create the server access key.".to_owned());
            }
            vsm::auth::protect_path(&key, false).map_err(|e| e.to_string())?;
        }
        let public =
            std::fs::read_to_string(key.with_extension("pub")).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"private_key":key,"public_key":public.trim()}))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn host_key(
    host: String,
    accepted: Option<String>,
    state: tauri::State<'_, Connection>,
) -> Result<serde_json::Value, String> {
    if host.is_empty()
        || host.starts_with('-')
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("Invalid server address.".into());
    }
    let root = state.root.clone();
    let keys = vsm::transport::scan_host_keys(&host, &root)
        .await
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        static SCAN_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temporary = root.join(format!("host-fingerprint-{}-{}.pub", std::process::id(), SCAN_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::write(&temporary, &keys).map_err(|e| e.to_string())?;
        let fingerprint_result = command("ssh-keygen").arg("-lf").arg(&temporary).output();
        let _ = std::fs::remove_file(&temporary);
        let fingerprint = fingerprint_result.map_err(|e| e.to_string())?;
        if !fingerprint.status.success() { return Err("Could not read the server fingerprint.".into()); }
        if let Some(accepted) = accepted {
            if accepted != keys { return Err("The server key changed. Verify the new fingerprint before continuing.".into()); }
            let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map_err(|e| e.to_string())?;
            let directory = PathBuf::from(home).join(".ssh");
            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            let path = directory.join("known_hosts");
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
            file.write_all(format!("\n{keys}").as_bytes()).map_err(|e| e.to_string())?;
        }
        Ok(serde_json::json!({"keys":keys,"fingerprint":String::from_utf8_lossy(&fingerprint.stdout).trim()}))
    }).await.map_err(|e| e.to_string())?
}
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            let bundled = app.path().resource_dir()?.join("agent/vsm-agent");
            app.manage(Connection {
                root,
                bundled,
                current: Arc::new(Mutex::new(None)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connection,
            azure_accounts,
            store_cloud_credential,
            create_ssh_key,
            host_key,
            open_azure_help
        ])
        .run(tauri::generate_context!())
        .expect("Desktop application failed");
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test(flavor = "current_thread")]
    async fn startup_does_not_block_async_work_and_only_initializes_once() {
        let cache = Arc::new(Mutex::new(None));
        let starts = Arc::new(AtomicUsize::new(0));
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let first_cache = cache.clone();
        let first_starts = starts.clone();
        let first = tokio::spawn(async move {
            cached_connection(first_cache, move || {
                first_starts.fetch_add(1, Ordering::SeqCst);
                entered.send(()).unwrap();
                blocked
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                Ok(serde_json::json!({"url":"one"}))
            })
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), ready)
            .await
            .unwrap()
            .unwrap();
        // This runs on the same single-thread executor while initialization is
        // deliberately blocked on another thread.
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            tokio::time::sleep(std::time::Duration::from_millis(10)),
        )
        .await
        .unwrap();
        let second_starts = starts.clone();
        let second = tokio::spawn(async move {
            cached_connection(cache, move || {
                second_starts.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({"url":"duplicate"}))
            })
            .await
        });
        release.send(()).unwrap();
        assert_eq!(
            first.await.unwrap().unwrap(),
            second.await.unwrap().unwrap()
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_startup_and_stopped_service_can_be_retried() {
        let cache = Arc::new(Mutex::new(None));
        assert!(
            cached_connection(cache.clone(), || Err("Inventory locked".into()))
                .await
                .is_err()
        );
        assert!(cache.lock().unwrap().is_none());
        let value = serde_json::json!({"url":"recovered"});
        assert_eq!(
            cached_connection(cache.clone(), || Ok(serde_json::json!({"url":"recovered"})))
                .await
                .unwrap(),
            value
        );
        *cache.lock().unwrap() = None;
        assert_eq!(
            cached_connection(cache, || Ok(serde_json::json!({"url":"restarted"})))
                .await
                .unwrap()["url"],
            "restarted"
        );
    }
}
