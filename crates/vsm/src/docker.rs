use crate::{
    db::Db,
    domain::{Account, Provider, Resource, Server},
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{path::Path, process::Stdio};
use tokio::io::AsyncWriteExt;

pub fn is_docker(p: &Provider) -> bool {
    matches!(p, Provider::Docker | Provider::SshDocker)
}
fn name(s: &Server) -> Result<String> {
    let id: uuid::Uuid = s.id.parse()?;
    Ok(format!("vsm-{id}"))
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
async fn execute(s: &Server, args: &[String], input: Option<Vec<u8>>) -> Result<Vec<u8>> {
    tokio::time::timeout(
        std::time::Duration::from_secs(1200),
        execute_inner(s, args, input),
    )
    .await
    .context("Docker command timed out; inspect resources before retrying")?
}
async fn execute_inner(s: &Server, args: &[String], input: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let mut cmd = if s.provider == Provider::Docker {
        crate::subprocess::command("docker")
    } else {
        let mut c = crate::transport::ssh(s)?;
        c.arg(format!("{}@{}", s.ssh_user, s.host));
        c.arg(format!(
            "docker {}",
            args.iter().map(|v| quote(v)).collect::<Vec<_>>().join(" ")
        ));
        c
    };
    if s.provider == Provider::Docker {
        cmd.args(args);
    }
    cmd.kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().context("Docker could not start. Install and start Docker Desktop locally, or Docker Engine on the remote host.")?;
    let mut stdin = child.stdin.take().context("Docker stdin unavailable")?;
    let writer = async move {
        if let Some(bytes) = input {
            stdin.write_all(&bytes).await?;
        }
        drop(stdin);
        Ok::<(), std::io::Error>(())
    };
    let (_, result) = tokio::try_join!(writer, child.wait_with_output())?;
    anyhow::ensure!(
        result.status.success(),
        "Docker operation failed: {}",
        String::from_utf8_lossy(&result.stderr)
            .chars()
            .take(1200)
            .collect::<String>()
    );
    Ok(result.stdout)
}
async fn command(s: &Server, args: &[&str]) -> Result<Vec<u8>> {
    execute(
        s,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        None,
    )
    .await
}
pub async fn validate_local() -> Result<Value> {
    let s = Server {
        id: uuid::Uuid::nil().to_string(),
        name: String::new(),
        account_id: None,
        provider: Provider::Docker,
        region: String::new(),
        size: String::new(),
        host: "127.0.0.1".into(),
        ssh_user: String::new(),
        ssh_key: String::new(),
        game_version: String::new(),
        state: String::new(),
        created_at: String::new(),
        resources: vec![],
    };
    validate(&s).await
}
pub async fn validate(s: &Server) -> Result<Value> {
    if s.provider == Provider::Docker {
        let endpoint = if let Ok(value) = std::env::var("DOCKER_HOST") {
            value
        } else {
            let out = command(
                s,
                &[
                    "context",
                    "inspect",
                    "--format",
                    "{{.Endpoints.docker.Host}}",
                ],
            )
            .await?;
            String::from_utf8(out)?.trim().to_string()
        };
        anyhow::ensure!(endpoint.starts_with("npipe://") || endpoint.starts_with("unix://"), "Local Docker requires a local Docker context. Select Docker Desktop's local context, or use Docker on Remote for a network host.");
    }
    let out = command(s, &["info", "--format", "{{json .}}"]).await?;
    let info: Value = serde_json::from_slice(&out)?;
    anyhow::ensure!(
        info["OSType"] == "linux",
        "Switch Docker Desktop to Linux containers"
    );
    anyhow::ensure!(
        matches!(info["Architecture"].as_str(), Some("x86_64" | "amd64")),
        "A Linux x64 Docker host is required"
    );
    Ok(json!({"valid":true,"engine":info["ServerVersion"],"architecture":info["Architecture"]}))
}
async fn exists(s: &Server, kind: &str, target: &str) -> Result<bool> {
    if kind == "container" {
        let out = command(s, &["container", "ls", "-a", "--format", "{{.Names}}"]).await?;
        return Ok(String::from_utf8(out)?.lines().any(|n| n == target));
    }
    let out = command(s, &["volume", "ls", "--format", "{{.Name}}"]).await?;
    Ok(String::from_utf8(out)?.lines().any(|n| n == target))
}
async fn owned(s: &Server, kind: &str, target: &str) -> Result<Value> {
    let bytes = command(s, &[kind, "inspect", target]).await?;
    let v: Value = serde_json::from_slice(&bytes)?;
    let labels = if kind == "container" {
        &v[0]["Config"]["Labels"]
    } else {
        &v[0]["Labels"]
    };
    verify_owner(labels, &s.id, kind)?;
    Ok(v[0].clone())
}
fn verify_owner(labels: &Value, id: &str, kind: &str) -> Result<()> {
    anyhow::ensure!(
        labels["org.vsm.owner"] == id,
        "Refusing to modify an unowned Docker {kind}"
    );
    Ok(())
}
const DOCKERFILE: &str = r#"FROM mcr.microsoft.com/dotnet/runtime:8.0-noble AS runtime8
FROM mcr.microsoft.com/dotnet/runtime:10.0-noble
COPY --from=runtime8 /usr/share/dotnet/shared/Microsoft.NETCore.App /usr/share/dotnet/shared/Microsoft.NETCore.App
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl restic util-linux libdbus-1-3 && rm -rf /var/lib/apt/lists/* && useradd --system --home /var/lib/vsm --shell /usr/sbin/nologin vintagestory
COPY --chmod=755 vsm-agent /usr/local/bin/vsm-agent
ENV VSM_GAME_USER=vintagestory
ENTRYPOINT ["/usr/local/bin/vsm-agent"]
"#;
fn context(binary: &[u8]) -> Result<Vec<u8>> {
    anyhow::ensure!(
        binary.starts_with(b"\x7fELF") && binary.get(18..20) == Some(&[0x3e, 0]),
        "Docker requires the bundled Linux x64 agent"
    );
    let mut tar = tar::Builder::new(Vec::new());
    for (path, data, mode) in [
        ("Dockerfile", DOCKERFILE.as_bytes(), 0o644),
        ("vsm-agent", binary, 0o755),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        tar.append_data(&mut header, path, data)?;
    }
    Ok(tar.into_inner()?)
}
fn verify_config(v: &Value, image: &str, volume: &str, port: u64) -> Result<()> {
    anyhow::ensure!(
        v["Config"]["Image"] == image,
        "Existing container uses a different agent image; inspect before replacing it"
    );
    anyhow::ensure!(
        v["Mounts"]
            .as_array()
            .is_some_and(|mounts| mounts.iter().any(|m| m["Type"] == "volume"
                && m["Name"] == volume
                && m["Destination"] == "/var/lib/vsm")
                && mounts
                    .iter()
                    .all(|m| m["Destination"] != "/var/run/docker.sock")),
        "Existing container data mount does not match the deployment"
    );
    let bindings = &v["HostConfig"]["PortBindings"];
    anyhow::ensure!(
        bindings.as_object().is_some_and(|p| p.len() == 2)
            && ["42420/tcp", "42420/udp"]
                .iter()
                .all(|key| bindings[key][0]["HostPort"]
                    .as_str()
                    .and_then(|p| p.parse::<u64>().ok())
                    == Some(port)),
        "Existing container ports do not match the deployment"
    );
    Ok(())
}
pub async fn bootstrap(s: &mut Server, account: &Account, binary: &Path, db: &Db) -> Result<()> {
    validate(s).await?;
    let n = name(s)?;
    let volume = format!("{n}-data");
    let bytes = tokio::fs::read(binary)
        .await
        .context("Bundled Linux agent not found")?;
    let image = format!(
        "vsm-agent:{}",
        &crate::auth::hash(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &bytes
        ))[..16]
    );
    execute(
        s,
        &[
            "build".into(),
            "--platform=linux/amd64".into(),
            "--label".into(),
            "org.vsm.component=agent".into(),
            "-t".into(),
            image.clone(),
            "-".into(),
        ],
        Some(context(&bytes)?),
    )
    .await?;
    let port = account.settings["game_port"].as_u64().unwrap_or(42420);
    anyhow::ensure!(
        (1024..=65535).contains(&port),
        "Game port must be between 1024 and 65535"
    );
    // Persist intended names before creating anything. Retries inspect exact ownership labels.
    s.resources = vec![
        Resource {
            id: port.to_string(),
            kind: "docker_game_port".into(),
            owned: false,
        },
        Resource {
            id: volume.clone(),
            kind: "docker_volume".into(),
            owned: true,
        },
        Resource {
            id: n.clone(),
            kind: "docker_container".into(),
            owned: true,
        },
    ];
    db.put("servers", &s.id, s)?;
    if exists(s, "volume", &volume).await? {
        owned(s, "volume", &volume).await?;
    } else {
        command(
            s,
            &[
                "volume",
                "create",
                "--label",
                &format!("org.vsm.owner={}", s.id),
                &volume,
            ],
        )
        .await?;
    }
    if exists(s, "container", &n).await? {
        let existing = owned(s, "container", &n).await?;
        verify_config(&existing, &image, &volume, port)?;
        command(s, &["start", &n]).await?;
    } else {
        command(
            s,
            &[
                "run",
                "-d",
                "--name",
                &n,
                "--label",
                &format!("org.vsm.owner={}", s.id),
                "--restart",
                "unless-stopped",
                "--security-opt",
                "no-new-privileges",
                "--cap-drop",
                "ALL",
                "--cap-add",
                "CHOWN",
                "--cap-add",
                "DAC_OVERRIDE",
                "--cap-add",
                "FOWNER",
                "--cap-add",
                "SETUID",
                "--cap-add",
                "SETGID",
                "--cap-add",
                "SETPCAP",
                "--pids-limit",
                "512",
                "--log-opt",
                "max-size=10m",
                "--log-opt",
                "max-file=2",
                "-v",
                &format!("{volume}:/var/lib/vsm"),
                "-p",
                &format!("{port}:42420/tcp"),
                "-p",
                &format!("{port}:42420/udp"),
                &image,
            ],
        )
        .await?;
    }
    for _ in 0..30 {
        if call(s, reqwest::Method::GET, "/v1/status", None)
            .await
            .is_ok()
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    anyhow::bail!("Container started but agent health check failed; inspect Docker logs")
}
pub async fn update_agent(s: &Server, binary: &Path, backup: &str) -> Result<()> {
    let _: uuid::Uuid = backup.parse()?;
    let n = name(s)?;
    owned(s, "container", &n).await?;
    let bytes = tokio::fs::read(binary).await?;
    anyhow::ensure!(
        bytes.starts_with(b"\x7fELF") && bytes.get(18..20) == Some(&[0x3e, 0]),
        "Remote update requires a Linux x64 agent"
    );
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, "vsm-agent.next", bytes.as_slice())?;
    execute(
        s,
        &["cp".into(), "-".into(), format!("{n}:/usr/local/bin/")],
        Some(tar.into_inner()?),
    )
    .await?;
    command(
        s,
        &[
            "exec",
            "-u",
            "0",
            &n,
            "cp",
            "-p",
            "/usr/local/bin/vsm-agent",
            &format!("/usr/local/bin/vsm-agent.previous-{backup}"),
        ],
    )
    .await?;
    command(
        s,
        &[
            "exec",
            "-u",
            "0",
            &n,
            "mv",
            "/usr/local/bin/vsm-agent.next",
            "/usr/local/bin/vsm-agent",
        ],
    )
    .await?;
    command(s, &["restart", "--time", "30", &n]).await?;
    Ok(())
}
pub async fn rollback_agent(s: &Server, backup: &str) -> Result<()> {
    use std::io::Read;
    let _: uuid::Uuid = backup.parse()?;
    let n = name(s)?;
    owned(s, "container", &n).await?;
    // docker cp works for stopped/crashing containers, unlike docker exec.
    let bytes = execute(
        s,
        &[
            "cp".into(),
            format!("{n}:/usr/local/bin/vsm-agent.previous-{backup}"),
            "-".into(),
        ],
        None,
    )
    .await?;
    let mut archive = tar::Archive::new(std::io::Cursor::new(bytes));
    let mut old = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        anyhow::ensure!(
            entry.header().entry_type().is_file() && entry.size() <= 64 * 1024 * 1024,
            "Invalid previous Remote archive"
        );
        entry.read_to_end(&mut old)?;
    }
    anyhow::ensure!(
        old.starts_with(b"\x7fELF") && old.get(18..20) == Some(&[0x3e, 0]),
        "Previous Remote executable is invalid"
    );
    command(s, &["stop", "--time", "30", &n]).await?;
    let mut archive = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(old.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    archive.append_data(&mut header, "vsm-agent", old.as_slice())?;
    execute(
        s,
        &["cp".into(), "-".into(), format!("{n}:/usr/local/bin/")],
        Some(archive.into_inner()?),
    )
    .await?;
    command(s, &["start", &n]).await?;
    Ok(())
}
pub async fn finish_agent_update(s: &Server, backup: &str) -> Result<()> {
    let _: uuid::Uuid = backup.parse()?;
    let n = name(s)?;
    owned(s, "container", &n).await?;
    command(
        s,
        &[
            "exec",
            "-u",
            "0",
            &n,
            "rm",
            "-f",
            &format!("/usr/local/bin/vsm-agent.previous-{backup}"),
        ],
    )
    .await?;
    Ok(())
}
pub async fn call(
    s: &Server,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    anyhow::ensure!(
        path.starts_with("/v1/") && !path.contains(['\r', '\n']),
        "Invalid agent endpoint"
    );
    let n = name(s)?;
    owned(s, "container", &n).await?;
    let token = command(s, &["exec", &n, "cat", "/var/lib/vsm/agent.token"]).await?;
    let token = String::from_utf8(token)?.trim().to_string();
    anyhow::ensure!(
        token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid agent credential"
    );
    // Configuration travels over stdin: the game cannot read the agent credential in argv.
    let mut args = vec![
        "exec".into(),
        "-i".into(),
        n,
        "curl".into(),
        "--silent".into(),
        "--show-error".into(),
        "--fail-with-body".into(),
        "--max-time".into(),
        "600".into(),
        "--config".into(),
        "-".into(),
    ];
    let world_options = path.split('?').next() == Some("/v1/world-options");
    if world_options {
        args.retain(|arg| arg != "--fail-with-body");
        args.extend(["--write-out".into(), "\n%{http_code}".into()]);
    }
    let mut config = format!(
        "request = {}\nurl = {}\nheader = {}\nheader = \"Content-Type: application/json\"\n",
        serde_json::to_string(method.as_str())?,
        serde_json::to_string(&format!("http://127.0.0.1:4318{path}"))?,
        serde_json::to_string(&format!("Authorization: Bearer {token}"))?
    );
    if let Some(body) = body {
        config.push_str(&format!(
            "data = {}\n",
            serde_json::to_string(&body.to_string())?
        ));
    }
    let out = execute(s, &args, Some(config.into_bytes())).await?;
    if world_options {
        let split = out
            .iter()
            .rposition(|byte| *byte == b'\n')
            .context("Missing Remote response status")?;
        let status = std::str::from_utf8(&out[split + 1..])?.trim();
        anyhow::ensure!(
            status != "404",
            "World options require a newer Remote. Update Remote in Infrastructure, then retry."
        );
        anyhow::ensure!(
            status.starts_with('2'),
            "Remote world options returned HTTP {status}"
        );
        return Ok(serde_json::from_slice(&out[..split])?);
    }
    Ok(serde_json::from_slice(&out)?)
}
pub async fn delete(s: &Server) -> Result<()> {
    delete_with_data(s, false).await
}
pub async fn delete_with_data(s: &Server, delete_data: bool) -> Result<()> {
    let n = name(s)?;
    let volume = format!("{n}-data");
    for (kind, target) in [("container", n), ("volume", volume)] {
        if kind == "volume" && !delete_data {
            continue;
        }
        if exists(s, kind, &target).await? {
            owned(s, kind, &target).await?;
            if kind == "container" {
                command(s, &["container", "rm", "-f", &target]).await?;
            } else {
                command(s, &["volume", "rm", &target]).await?;
            }
        }
    }
    Ok(())
}
pub async fn inspect(s: &Server) -> Result<Value> {
    owned(s, "container", &name(s)?).await
}
pub async fn power(s: &Server, action: &str) -> Result<Value> {
    let command_name = match action {
        "start" => "start",
        "restart" => "restart",
        "deallocate" => "stop",
        _ => anyhow::bail!("Unsupported container action"),
    };
    let n = name(s)?;
    owned(s, "container", &n).await?;
    command(s, &[command_name, &n]).await?;
    let observed = owned(s, "container", &n).await?;
    anyhow::ensure!(
        observed["State"]["Running"] == json!(action != "deallocate"),
        "Container state could not be verified"
    );
    Ok(observed)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_different_ownership_blocks_mutation() {
        assert!(verify_owner(&json!({}), "owner", "volume").is_err());
        assert!(verify_owner(&json!({"org.vsm.owner":"other"}), "owner", "container").is_err());
        assert!(verify_owner(&json!({"org.vsm.owner":"owner"}), "owner", "container").is_ok());
    }
    #[test]
    fn shell_values_are_quoted() {
        assert_eq!(
            quote("x'; touch /tmp/nope; '"),
            "'x'\\''; touch /tmp/nope; '\\'''"
        );
    }
    #[test]
    fn context_rejects_non_linux() {
        assert!(context(b"MZ executable").is_err());
    }
    #[test]
    fn rejects_mismatched_container_configuration() {
        let mut v = json!({"Config":{"Image":"agent:1"},"Mounts":[{"Type":"volume","Name":"data","Destination":"/var/lib/vsm"}],"HostConfig":{"PortBindings":{"42420/tcp":[{"HostPort":"42429"}],"42420/udp":[{"HostPort":"42429"}]}}});
        assert!(verify_config(&v, "agent:1", "data", 42429).is_ok());
        assert!(verify_config(&v, "agent:1", "other", 42429).is_err());
        v["HostConfig"]["PortBindings"]["4318/tcp"] = json!([{"HostPort":"4318"}]);
        assert!(verify_config(&v, "agent:1", "data", 42429).is_err());
    }
    #[test]
    fn no_management_port_or_docker_socket() {
        assert!(!DOCKERFILE.contains("docker.sock"));
        assert!(!DOCKERFILE.contains("EXPOSE 4318"));
    }
}
