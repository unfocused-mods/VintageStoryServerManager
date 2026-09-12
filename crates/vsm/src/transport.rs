use crate::domain::Server;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::process::{Child, Command};

pub(crate) fn ssh(s: &Server) -> Result<Command> {
    anyhow::ensure!(
        !s.host.is_empty()
            && s.host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
            && !s.host.starts_with('-'),
        "Invalid host"
    );
    anyhow::ensure!(
        !s.ssh_user.is_empty()
            && s.ssh_user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            && !s.ssh_user.starts_with('-'),
        "Invalid SSH user"
    );
    let mut cmd = crate::subprocess::command("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ConnectTimeout=10",
        "-i",
        &s.ssh_key,
    ]);
    cmd.kill_on_drop(true);
    Ok(cmd)
}
pub async fn check(s: &Server) -> Result<Value> {
    let out = ssh(s)?
        .arg(format!("{}@{}", s.ssh_user, s.host))
        .arg("cat /etc/os-release")
        .output()
        .await?;
    anyhow::ensure!(
        out.status.success(),
        "SSH failed. Verify the host fingerprint and add it to known_hosts before connecting."
    );
    let os = String::from_utf8_lossy(&out.stdout);
    anyhow::ensure!(
        os.contains("ID=ubuntu") && os.contains("24.04"),
        "Host must run Ubuntu 24.04 LTS"
    );
    Ok(json!({"reachable":true,"os":"Ubuntu 24.04"}))
}
pub async fn call(
    s: &Server,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    anyhow::ensure!(path.starts_with("/v1/"), "Invalid agent endpoint");
    if crate::docker::is_docker(&s.provider) {
        return crate::docker::call(s, method, path, body).await;
    }
    let token = ssh(s)?
        .arg(format!("{}@{}", s.ssh_user, s.host))
        .arg("sudo -n cat /var/lib/vsm/agent.token")
        .output()
        .await
        .context("Could not run SSH to contact the Remote. Check that OpenSSH is available on this Manager machine.")?;
    if !token.status.success() {
        anyhow::bail!(remote_credential_failure(
            token.status.code(),
            &token.stderr
        ));
    }
    let token = String::from_utf8(token.stdout)
        .context("The host returned an invalid Remote credential")?
        .trim()
        .to_string();
    anyhow::ensure!(token.len() == 64, "Invalid Remote credential");
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let mut child = ssh(s)?
        .args([
            "-o",
            "ExitOnForwardFailure=yes",
            "-N",
            "-L",
            &format!("127.0.0.1:{port}:127.0.0.1:4318"),
        ])
        .arg(format!("{}@{}", s.ssh_user, s.host))
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let result = call_tunnel(&mut child, port, &token, method, path, body).await;
    let _ = child.kill().await;
    result
}

/// Classify SSH diagnostics without forwarding untrusted stderr, paths or secrets.
/// OpenSSH reserves exit 255 for connection/authentication failures; other exit
/// codes normally come from the remote sudo/cat command.
fn remote_credential_failure(code: Option<i32>, stderr: &[u8]) -> &'static str {
    let diagnostic = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if diagnostic.contains("identity file") && diagnostic.contains("not accessible")
        || diagnostic.contains("load key") && diagnostic.contains("no such file")
    {
        return "The SSH private key file is not accessible on this Manager machine. Select an existing private key in Infrastructure.";
    }
    if diagnostic.contains("host key verification failed")
        || diagnostic.contains("remote host identification has changed")
    {
        return "SSH could not verify the host identity. Check the Remote identity in Infrastructure before reconnecting.";
    }
    if diagnostic.contains("unprotected private key file")
        || diagnostic.contains("bad permissions")
        || diagnostic.contains("load key")
            && (diagnostic.contains("invalid format")
                || diagnostic.contains("permission denied")
                || diagnostic.contains("error in libcrypto"))
    {
        return "SSH could not use the selected private key. Check its format and file permissions on this Manager machine.";
    }
    if code == Some(255) {
        if diagnostic.contains("permission denied")
            || diagnostic.contains("authentication failed")
            || diagnostic.contains("no supported authentication methods")
        {
            return "SSH authentication failed. Check the Remote's SSH username and selected private key in Infrastructure.";
        }
        if diagnostic.contains("could not resolve hostname") {
            return "SSH could not resolve the Remote host address. Check the address and this Manager machine's network connection.";
        }
        if diagnostic.contains("timed out")
            || diagnostic.contains("connection refused")
            || diagnostic.contains("no route to host")
            || diagnostic.contains("network is unreachable")
        {
            return "The Remote host is not reachable over SSH. Check the VM status in Infrastructure, start it if stopped or deallocated, and check allowed SSH access.";
        }
        return "SSH could not connect to the Remote. Check the VM status, network access, host identity and SSH credentials in Infrastructure.";
    }
    if code.is_none() {
        return "SSH ended before the Remote credentials could be read. Check the host connection and retry.";
    }
    if diagnostic.contains("agent.token") && diagnostic.contains("no such file or directory") {
        return "Connected to the host, but its Remote credential file was not found. Check the existing Remote installation in Infrastructure.";
    }
    if diagnostic.contains("sudo:")
        || diagnostic.contains("permission denied")
        || diagnostic.contains("not in the sudoers")
    {
        return "Connected to the host, but the SSH account cannot read the Remote credentials. Check that the account has the required passwordless sudo access.";
    }
    "Connected to the host, but could not read the Remote credentials. Check the Remote installation and the SSH account's access."
}
async fn call_tunnel(
    child: &mut Child,
    port: u16,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let mut ready = false;
    for _ in 0..50 {
        anyhow::ensure!(child.try_wait()?.is_none(), "SSH tunnel exited");
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    anyhow::ensure!(ready, "SSH tunnel timed out");
    let mut req = client
        .request(method, format!("http://127.0.0.1:{port}{path}"))
        .bearer_auth(token);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let res = req.send().await?;
    let status = res.status();
    if status == reqwest::StatusCode::NOT_FOUND
        && path.split('?').next() == Some("/v1/world-options")
    {
        anyhow::bail!(
            "World options require a newer Remote. Update Remote in Infrastructure, then retry."
        );
    }
    let data: Value = res.json().await.context("Agent response was not JSON")?;
    anyhow::ensure!(
        status.is_success(),
        "Agent: {}",
        data["error"].as_str().unwrap_or("request failed")
    );
    Ok(data)
}
pub async fn bootstrap(s: &Server, binary: &std::path::Path) -> Result<()> {
    check(s).await?;
    let bytes = tokio::fs::read(binary).await?;
    anyhow::ensure!(
        bytes.starts_with(b"\x7fELF"),
        "Bootstrap requires a Linux x64 vsm-agent executable, not a Windows binary"
    );
    anyhow::ensure!(
        bytes.get(18..20) == Some(&[0x3e, 0x00]),
        "Agent must be Linux x86-64"
    );
    let remote = format!("/tmp/vsm-agent-{}", uuid::Uuid::new_v4());
    let status = crate::subprocess::command("scp")
        .kill_on_drop(true)
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-i",
            &s.ssh_key,
        ])
        .arg(binary)
        .arg(format!("{}@{}:{remote}", s.ssh_user, s.host))
        .status()
        .await?;
    anyhow::ensure!(status.success(), "Agent upload failed");
    let script = format!(
        r#"set -euo pipefail
test "$(uname -m)" = x86_64
. /etc/os-release
test "$ID" = ubuntu && test "$VERSION_ID" = 24.04
id vintagestory >/dev/null 2>&1 || useradd --system --home /var/lib/vsm --shell /usr/sbin/nologin vintagestory
install -d -m 711 -o root -g root /var/lib/vsm
chown -R root:root /var/lib/vsm
install -m 755 -o root -g root {remote} /usr/local/bin/vsm-agent
rm -- {remote}
apt-get update -qq
apt-get install -y ca-certificates curl restic util-linux
curl --fail --location https://packages.microsoft.com/config/ubuntu/24.04/packages-microsoft-prod.deb -o /tmp/vsm-microsoft.deb
dpkg -i /tmp/vsm-microsoft.deb
apt-get update -qq
apt-get install -y dotnet-runtime-8.0 dotnet-runtime-10.0
cat > /etc/systemd/system/vsm-agent.service <<'UNIT'
[Unit]
Description=Vintage Story management agent
After=network-online.target
[Service]
User=root
Group=root
Environment=VSM_GAME_USER=vintagestory
CapabilityBoundingSet=CAP_CHOWN CAP_DAC_OVERRIDE CAP_FOWNER CAP_SETUID CAP_SETGID CAP_SETPCAP
ExecStart=/usr/local/bin/vsm-agent --root /var/lib/vsm
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/vsm
UMask=0077
[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable vsm-agent
systemctl restart vsm-agent
"#
    );
    use tokio::io::AsyncWriteExt;
    let mut child = ssh(s)?
        .arg(format!("{}@{}", s.ssh_user, s.host))
        .arg("sudo -n bash -s")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .await?;
    anyhow::ensure!(
        child.wait().await?.success(),
        "Agent bootstrap failed; inspect the host system journal"
    );
    Ok(())
}
/// Replace only the executable. Preserve one previous binary for verified rollback.
pub async fn update_agent(s: &Server, binary: &std::path::Path, backup: &str) -> Result<()> {
    let _: uuid::Uuid = backup.parse()?;
    let temporary = format!("/tmp/vsm-agent-update-{}", uuid::Uuid::new_v4());
    let status = crate::subprocess::command("scp")
        .kill_on_drop(true)
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=10",
            "-i",
            &s.ssh_key,
        ])
        .arg(binary)
        .arg(format!("{}@{}:{temporary}", s.ssh_user, s.host))
        .status()
        .await?;
    anyhow::ensure!(status.success(), "Remote binary upload failed");
    let script=format!("set -eu; install -m 0755 {temporary} /usr/local/bin/vsm-agent.next; cp -p /usr/local/bin/vsm-agent /usr/local/bin/vsm-agent.previous-{backup}; mv -f /usr/local/bin/vsm-agent.next /usr/local/bin/vsm-agent; rm -f {temporary}; systemctl restart vsm-agent");
    remote_update_script(s, &script).await
}
pub async fn rollback_agent(s: &Server, backup: &str) -> Result<()> {
    let _: uuid::Uuid = backup.parse()?;
    remote_update_script(s,&format!("set -eu; test -f /usr/local/bin/vsm-agent.previous-{backup}; mv -f /usr/local/bin/vsm-agent.previous-{backup} /usr/local/bin/vsm-agent; systemctl restart vsm-agent")).await
}
pub async fn finish_agent_update(s: &Server, backup: &str) -> Result<()> {
    let _: uuid::Uuid = backup.parse()?;
    remote_update_script(
        s,
        &format!("rm -f /usr/local/bin/vsm-agent.previous-{backup}"),
    )
    .await
}
async fn remote_update_script(s: &Server, script: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut child = ssh(s)?
        .arg(format!("{}@{}", s.ssh_user, s.host))
        .arg("sudo -n sh -s")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .context("Remote update stdin unavailable")?
        .write_all(script.as_bytes())
        .await?;
    let output = child.wait_with_output().await?;
    anyhow::ensure!(
        output.status.success(),
        "Remote service update failed: {}",
        crate::diagnostics::redact(&String::from_utf8_lossy(&output.stderr))
    );
    Ok(())
}

/// Collect an untrusted host identity without supplying credentials or opening a session.
/// The caller must display its fingerprint and obtain explicit trust before installation.
pub async fn scan_host_keys(host: &str, root: &std::path::Path) -> Result<String> {
    anyhow::ensure!(
        !host.is_empty()
            && !host.starts_with('-')
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b)),
        "Invalid server address"
    );
    let mut scan = crate::subprocess::command("ssh-keyscan");
    scan.args(["-T", "5", "-t", "ed25519", host])
        .kill_on_drop(true);
    let initial = tokio::time::timeout(std::time::Duration::from_secs(8), scan.output()).await;
    if let Ok(Ok(output)) = initial {
        if let Some(keys) = scanned_key(&String::from_utf8_lossy(&output.stdout), host) {
            return Ok(keys);
        }
    }
    // Some Windows keyscan builds advertise a KEX algorithm they cannot execute.
    // A normal SSH handshake can constrain KEX. Its temporary trust file is never
    // reused for management, and every credential and session mechanism is disabled.
    let temporary = tempfile::Builder::new()
        .prefix("host-scan-")
        .tempdir_in(root)?;
    let known = temporary.path().join("known_hosts");
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut probe = crate::subprocess::command("ssh");
    probe
        .args([
            "-F",
            null,
            "-N",
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            "-o",
            "ConnectionAttempts=1",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "HashKnownHosts=no",
            "-o",
            "UpdateHostKeys=no",
            "-o",
            "HostKeyAlgorithms=ssh-ed25519",
            "-o",
            "KexAlgorithms=curve25519-sha256,curve25519-sha256@libssh.org",
            "-o",
            "PreferredAuthentications=none",
            "-o",
            "PubkeyAuthentication=no",
            "-o",
            "PasswordAuthentication=no",
            "-o",
            "KbdInteractiveAuthentication=no",
            "-o",
            "HostbasedAuthentication=no",
            "-o",
            "GSSAPIAuthentication=no",
            "-o",
            "IdentityAgent=none",
            "-o",
            "IdentityFile=none",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ClearAllForwardings=yes",
            "-o",
            "PermitLocalCommand=no",
            "-o",
            "ProxyCommand=none",
            "-o",
            "ProxyJump=none",
            "-o",
        ])
        .arg(format!("GlobalKnownHostsFile={null}"))
        .arg("-o")
        .arg("UserKnownHostsFile=known_hosts")
        .current_dir(temporary.path())
        .args(["-l", "vsm-host-key-probe", host])
        .kill_on_drop(true);
    let result = tokio::time::timeout(std::time::Duration::from_secs(12), probe.output()).await;
    if let Some(keys) = std::fs::read_to_string(&known)
        .ok()
        .and_then(|text| scanned_key(&text, host))
    {
        return Ok(keys);
    }
    match result {
        Ok(Err(_)) => anyhow::bail!("OpenSSH is required to verify the server identity. Install or update OpenSSH and retry."),
        _ => anyhow::bail!("Could not read the server SSH identity. Check SSH access on port 22 and update OpenSSH if key exchange is unsupported. The VM may still be running."),
    }
}
fn scanned_key(text: &str, host: &str) -> Option<String> {
    use base64::Engine;
    text.lines().find_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != host || fields[1] != "ssh-ed25519" {
            return None;
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(fields[2])
            .ok()?;
        if decoded.len() != 51 || !decoded.starts_with(b"\0\0\0\x0bssh-ed25519\0\0\0\x20") {
            return None;
        }
        Some(format!("{} ssh-ed25519 {}\n", host, fields[2]))
    })
}
#[cfg(test)]
mod remote_credential_tests {
    use super::remote_credential_failure;

    #[test]
    fn unreachable_vm_is_not_reported_as_a_missing_remote() {
        for diagnostic in [
            "ssh: connect to host example port 22: Connection timed out",
            "ssh: connect to host example port 22: Connection refused",
            "ssh: connect to host example port 22: No route to host",
            "ssh: connect to host example port 22: Network is unreachable",
        ] {
            let message = remote_credential_failure(Some(255), diagnostic.as_bytes());
            assert!(message.contains("not reachable over SSH"));
            assert!(message.contains("stopped or deallocated"));
            assert!(!message.contains("installation"));
            assert!(!message.contains("bootstrap"));
        }
    }

    #[test]
    fn authentication_and_remote_sudo_permissions_are_distinct() {
        let authentication =
            remote_credential_failure(Some(255), b"user@example: Permission denied (publickey).");
        let sudo = remote_credential_failure(Some(1), b"sudo: a password is required");
        let read =
            remote_credential_failure(Some(1), b"cat: /var/lib/vsm/agent.token: Permission denied");
        assert!(authentication.contains("SSH authentication failed"));
        assert!(!authentication.contains("Connected to the host"));
        assert!(sudo.contains("Connected to the host"));
        assert!(sudo.contains("passwordless sudo"));
        assert_eq!(sudo, read);
    }

    #[test]
    fn missing_local_key_is_not_confused_with_a_missing_remote_credential() {
        let local = remote_credential_failure(
            Some(255),
            b"Warning: Identity file C:/private/key not accessible: No such file or directory.",
        );
        let remote = remote_credential_failure(
            Some(1),
            b"cat: /var/lib/vsm/agent.token: No such file or directory",
        );
        assert!(local.contains("on this Manager machine"));
        assert!(remote.contains("Connected to the host"));
        assert!(remote.contains("credential file was not found"));
        assert!(!remote.contains("bootstrap"));
    }

    #[test]
    fn identity_key_and_unknown_errors_never_forward_stderr() {
        for (code, diagnostic, expected) in [
            (
                Some(255),
                "Host key verification failed. PRIVATE-DIAGNOSTIC",
                "host identity",
            ),
            (
                Some(255),
                "Load key C:/private/key: invalid format PRIVATE-DIAGNOSTIC",
                "format and file permissions",
            ),
            (
                Some(255),
                "Could not resolve hostname PRIVATE-DIAGNOSTIC",
                "host address",
            ),
            (Some(255), "PRIVATE-DIAGNOSTIC", "could not connect"),
            (Some(1), "PRIVATE-DIAGNOSTIC", "could not read"),
            (None, "PRIVATE-DIAGNOSTIC", "SSH ended"),
        ] {
            let message = remote_credential_failure(code, diagnostic.as_bytes());
            assert!(message.contains(expected), "{message}");
            assert!(!message.contains("PRIVATE-DIAGNOSTIC"));
            assert!(!message.contains("C:/private/key"));
        }
    }
}

#[cfg(test)]
mod host_scan_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Read-only SSH identity scan of an explicitly supplied VSM_SCAN_HOST"]
    async fn live_identity_scan() {
        let host = std::env::var("VSM_SCAN_HOST").expect("Set VSM_SCAN_HOST explicitly");
        let root = tempfile::Builder::new()
            .prefix("vsm scan with spaces ")
            .tempdir()
            .unwrap();
        let keys = scan_host_keys(&host, root.path()).await.unwrap();
        assert!(scanned_key(&keys, &host).is_some());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        println!("Read-only identity scan succeeded; temporary trust removed");
    }
    #[test]
    fn only_requested_valid_ed25519_identity_is_returned() {
        use base64::Engine;
        let mut blob = b"\0\0\0\x0bssh-ed25519\0\0\0\x20".to_vec();
        blob.extend([7u8; 32]);
        let key = base64::engine::general_purpose::STANDARD.encode(blob);
        let line = format!("example.test ssh-ed25519 {key}");
        assert_eq!(
            scanned_key(&format!("# banner\n{line}\n"), "example.test"),
            Some(format!("{line}\n"))
        );
        assert!(scanned_key(&line, "different.test").is_none());
        assert!(scanned_key("example.test ssh-ed25519 ZmFrZQ==", "example.test").is_none());
    }
}
