#[derive(Clone, Serialize, Deserialize)]
struct IdlePolicy {
    enabled: bool,
    minutes: u32,
}
impl Default for IdlePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            minutes: 15,
        }
    }
}
#[derive(Default)]
struct IdleState {
    since: Option<std::time::Instant>,
    error: Option<String>,
    wake_requested: bool,
}
struct WakeSockets {
    tcp: tokio::net::TcpListener,
    udp: tokio::net::UdpSocket,
}

impl Agent {
    fn idle_policy(&self) -> Result<IdlePolicy> {
        let path = self.root.join("idle-policy.json");
        if path.exists() {
            Ok(serde_json::from_slice(&std::fs::read(path)?)?)
        } else {
            Ok(IdlePolicy::default())
        }
    }
    fn is_sleeping(&self) -> bool {
        std::fs::read_to_string(self.root.join("desired")).is_ok_and(|s| s == "sleeping")
    }
    async fn idle_status(&self) -> Result<Value> {
        let s = self.idle.lock().await;
        Ok(
            json!({"policy":self.idle_policy()?,"sleeping":self.is_sleeping(),"sleep_id":std::fs::read_to_string(self.root.join("sleep-id")).ok(),"idle_seconds":s.since.map(|t|t.elapsed().as_secs()),"error":s.error}),
        )
    }
    async fn disarm_idle(&self) -> Result<()> {
        self.wake.lock().await.take();
        if self.is_sleeping() {
            std::fs::write(self.root.join("desired"), "stopped")?;
        }
        let _ = std::fs::remove_file(self.root.join("sleep-id"));
        let mut state = self.idle.lock().await;
        state.since = None;
        state.wake_requested = false;
        Ok(())
    }
    fn game_address(&self) -> Result<std::net::SocketAddr> {
        let bytes = files::read(&self.root.join("data"), "serverconfig.json", 1024 * 1024)?;
        let config: Value = serde_json::from_slice(&bytes)?;
        let port = config["Port"].as_u64().context("Game port is unknown")?;
        ensure!(
            (1024..=65535).contains(&port),
            "Automatic sleep requires an unprivileged game port"
        );
        let ip: std::net::IpAddr = config["Ip"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("0.0.0.0")
            .parse()
            .context("Game bind address is not an IP address")?;
        Ok(std::net::SocketAddr::new(ip, port as u16))
    }
    async fn arm_wake(&self) -> Result<()> {
        let address = self.game_address()?;
        let tcp = tokio::net::TcpListener::bind(address)
            .await
            .context("Cannot reserve TCP wake port")?;
        let udp = tokio::net::UdpSocket::bind(address)
            .await
            .context("Cannot reserve UDP wake port")?;
        *self.wake.lock().await = Some(WakeSockets { tcp, udp });
        if !self.is_sleeping() || !self.root.join("sleep-id").exists() {
            std::fs::write(self.root.join("sleep-id"), uuid::Uuid::new_v4().to_string())?;
        }
        std::fs::write(self.root.join("desired"), "sleeping")?;
        Ok(())
    }
    async fn idle_tick(&self) -> Result<()> {
        let policy = self.idle_policy()?;
        if !policy.enabled {
            self.disarm_idle().await?;
            return Ok(());
        }
        if self.is_sleeping() {
            if self.wake.lock().await.is_none() {
                self.arm_wake().await?;
            }
            return Ok(());
        }
        if !self.running().await? || !self.ready.load(std::sync::atomic::Ordering::Relaxed) {
            self.idle.lock().await.since = None;
            return Ok(());
        }
        let address = self.game_address()?;
        let (listening, connections) = game_tcp_connections(address.port())?;
        ensure!(
            listening,
            "Game TCP listener could not be confirmed; keeping the game running"
        );
        // TCP is only a conservative guard. The game itself must positively confirm no players.
        let empty = connections == 0 && self.confirm_empty_players().await?;
        let due = {
            let mut state = self.idle.lock().await;
            state.error = None;
            if !empty {
                state.since = None;
                false
            } else {
                state
                    .since
                    .get_or_insert_with(std::time::Instant::now)
                    .elapsed()
                    .as_secs()
                    >= u64::from(policy.minutes) * 60
            }
        };
        if due {
            // Recheck immediately before the graceful stop, under the same mutation lock.
            let (listening, connections) = game_tcp_connections(address.port())?;
            if !listening || connections > 0 {
                self.idle.lock().await.since = None;
                return Ok(());
            }
            self.stop_game().await?;
            if let Err(e) = self.arm_wake().await {
                self.disarm_idle().await?;
                let restart = self.start().await;
                bail!(
                    "Cannot arm wake listener: {e}. Game restart: {}",
                    match restart {
                        Ok(_) => "started".to_string(),
                        Err(error) => format!("failed: {error}"),
                    }
                );
            }
            self.idle.lock().await.since = None;
            self.agent_event("info","Game saved and sleeping after inactivity. A connection to the game port wakes it; players may need to retry joining.");
        }
        Ok(())
    }
    async fn confirm_empty_players(&self) -> Result<bool> {
        ensure!(
            crate::game_releases::tested_idle_version(&self.active_version()?),
            "Automatic sleep player detection is tested with Vintage Story 1.22.0 and 1.22.7"
        );
        let baseline = self.log_sequence.load(std::sync::atomic::Ordering::Relaxed);
        self.command("list clients").await?;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let logs = self.logs.lock().await;
            let messages: Vec<&str> = logs
                .iter()
                .filter(|v| v["sequence"].as_u64().is_some_and(|s| s > baseline))
                .filter_map(|v| v["message"].as_str())
                .collect();
            if let Some(empty) = empty_player_response(&messages) {
                return Ok(empty);
            }
        }
        bail!("The game did not confirm an empty player list; keeping it running")
    }
    fn spawn_wake_loop(&self) {
        let a = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                let awakened = {
                    let guard = a.wake.lock().await;
                    if let Some(sockets) = guard.as_ref() {
                        let mut byte = [0u8; 1];
                        let accepted=tokio::time::timeout(std::time::Duration::from_millis(100),async {
                            tokio::select! { result=sockets.tcp.accept()=>result.map(|_|()), result=sockets.udp.recv_from(&mut byte)=>result.map(|_|()) }
                        }).await.is_ok_and(|r|r.is_ok());
                        if accepted {
                            a.idle.lock().await.wake_requested = true;
                        }
                        accepted
                    } else {
                        false
                    }
                };
                if !awakened {
                    continue;
                }
                let _guard = a.mutation.lock().await;
                if !a.is_sleeping() || !a.idle_policy().is_ok_and(|p| p.enabled) {
                    continue;
                }
                // Release both ports before launching; never proxy or alter player addresses.
                a.wake.lock().await.take();
                if let Err(e) = a.start().await {
                    let _ = a.disarm_idle().await;
                    *a.desired.lock().await = false;
                    let _ = std::fs::write(a.root.join("desired"), "stopped");
                    a.idle.lock().await.error = Some(format!(
                        "Wake failed: {e}. Start the game manually to retry."
                    ));
                    a.agent_event(
                        "error",
                        "Automatic wake failed and has been disarmed. Start manually to retry.",
                    );
                } else {
                    a.agent_event(
                        "info",
                        "Game is starting after a connection to its wake port.",
                    );
                }
            }
        });
    }
    async fn prepare_cloud_sleep(&self, sleep_id: &str) -> Result<Value> {
        if !self.is_sleeping() || self.running().await? {
            return Ok(json!({"prepared":false,"reason":"stale"}));
        }
        if sleep_id.is_empty()
            || std::fs::read_to_string(self.root.join("sleep-id"))
                .ok()
                .as_deref()
                != Some(sleep_id)
        {
            return Ok(json!({"prepared":false,"reason":"stale"}));
        }
        // Freeze the listener before checking pending wake requests. Caller holds mutation.
        let mut wake = self.wake.lock().await;
        if self.idle.lock().await.wake_requested {
            return Ok(json!({"prepared":false,"reason":"wake_requested"}));
        }
        wake.take();
        drop(wake);
        std::fs::write(self.root.join("desired"), "cloud_sleep")?;
        self.agent_event(
            "info",
            "Game reserved for cloud deallocation. Start the VM from the Manager to play again.",
        );
        Ok(json!({"prepared":true,"sleep_id":sleep_id}))
    }
}
// Connection counts are a conservative activity guard, never reported as player counts.
fn parse_game_tcp(text: &str, port: u16) -> Result<(bool, usize)> {
    let mut listening = false;
    let mut connections = 0;
    ensure!(
        text.lines()
            .next()
            .is_some_and(|h| h.contains("local_address")),
        "Invalid TCP socket table"
    );
    for line in text.lines().skip(1) {
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns.is_empty() {
            continue;
        }
        ensure!(columns.len() >= 4, "Incomplete TCP socket table");
        let local = columns[1]
            .rsplit_once(':')
            .context("Invalid socket address")?;
        if u16::from_str_radix(local.1, 16)? != port {
            continue;
        }
        match columns[3] {
            "0A" => listening = true,
            "06" | "07" => {}
            _ => connections += 1,
        }
    }
    Ok((listening, connections))
}
fn game_tcp_connections(port: u16) -> Result<(bool, usize)> {
    ensure!(
        cfg!(target_os = "linux"),
        "Automatic sleep requires Linux socket activity monitoring"
    );
    let a = parse_game_tcp(
        &std::fs::read_to_string("/proc/net/tcp").context("Cannot read TCP activity")?,
        port,
    )?;
    let b = parse_game_tcp(
        &std::fs::read_to_string("/proc/net/tcp6").context("Cannot read IPv6 TCP activity")?,
        port,
    )?;
    Ok((a.0 || b.0, a.1 + b.1))
}
async fn idle_policy(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    Ok(Json(a.idle_status().await?))
}
async fn save_idle_policy(
    State(a): State<Agent>,
    h: HeaderMap,
    Json(policy): Json<IdlePolicy>,
) -> ApiResult {
    a.authorize(&h)?;
    ensure!(
        (1..=1440).contains(&policy.minutes),
        "Idle timeout must be 1â€“1440 minutes"
    );
    let _lock = a.mutation.lock().await;
    if policy.enabled {
        a.game_address()?;
        game_tcp_connections(a.game_address()?.port())?;
    }
    let temp = a
        .root
        .join(format!("idle-policy-{}.tmp", uuid::Uuid::new_v4()));
    auth::private_write(&temp, &serde_json::to_vec_pretty(&policy)?)?;
    let saved = std::fs::rename(&temp, a.root.join("idle-policy.json"));
    if saved.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    saved?;
    if !policy.enabled {
        a.disarm_idle().await?;
    }
    let mut state = a.idle.lock().await;
    state.since = None;
    state.error = None;
    drop(state);
    Ok(Json(a.idle_status().await?))
}
// Verified against the unmodified 1.22.0 and 1.22.7 English server console. Unknown formats fail closed.
fn empty_player_response(messages: &[&str]) -> Option<bool> {
    let notification = |line: &str, expected: &str| {
        line.split_once(" [Server Notification] ")
            .is_some_and(|(timestamp, message)| {
                message == expected
                    && chrono::NaiveDateTime::parse_from_str(timestamp, "%d.%m.%Y %H:%M:%S").is_ok()
            })
    };
    let command = messages
        .iter()
        .position(|s| notification(s, "Handling Console Command /list clients"))?;
    let header = messages.get(command + 1)?;
    if !notification(header, "List of online Players") {
        return None;
    }
    Some(messages.get(command + 2)?.is_empty())
}
#[cfg(test)]
mod idle_tests {
    use super::*;
    #[test]
    fn empty_players_requires_fresh_complete_game_response() {
        let command =
            "6.9.2026 02:21:06 [Server Notification] Handling Console Command /list clients";
        let header = "6.9.2026 02:21:06 [Server Notification] List of online Players";
        assert_eq!(empty_player_response(&[command, header, ""]), Some(true));
        assert_eq!(
            empty_player_response(&[command, header, "PlayerOne"]),
            Some(false)
        );
        assert_eq!(empty_player_response(&[command, header]), None);
        assert_eq!(empty_player_response(&[header, ""]), None);
        assert_eq!(
            empty_player_response(&[command, "unknown response", ""]),
            None
        );
        assert_eq!(
            empty_player_response(&[&format!("player chat {command}"), header, ""]),
            None
        );
    }
    #[test]
    fn active_pending_and_closing_connections_prevent_sleep() {
        for state in ["01", "02", "03", "04", "05", "08", "09", "0B"] {
            let data=format!("sl local_address rem_address st\n0: 00000000:A5B4 00000000:0000 0A\n1: 00000000:A5B4 00000000:1234 {state}\n");
            assert_eq!(parse_game_tcp(&data, 42420).unwrap(), (true, 1));
        }
        assert!(parse_game_tcp("bad", 42420).is_err());
        assert_eq!(
            parse_game_tcp(
                "sl local_address rem_address st\n0: 00000000:1234 00000000:0000 0A",
                42420
            )
            .unwrap(),
            (false, 0)
        );
    }
    #[tokio::test]
    async fn manual_stop_disarms_wake_and_frees_both_ports() {
        let dir = tempfile::tempdir().unwrap();
        let a = Agent::new(dir.path().into()).unwrap();
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let udp = tokio::net::UdpSocket::bind(addr).await.unwrap();
        *a.wake.lock().await = Some(WakeSockets { tcp, udp });
        std::fs::write(dir.path().join("desired"), "sleeping").unwrap();
        a.stop().await.unwrap();
        assert!(!a.is_sleeping());
        assert!(a.wake.lock().await.is_none());
        assert!(tokio::net::TcpListener::bind(addr).await.is_ok());
        assert!(tokio::net::UdpSocket::bind(addr).await.is_ok());
    }
    #[tokio::test]
    async fn policy_can_be_updated_and_disabled_more_than_once() {
        let dir = tempfile::tempdir().unwrap();
        let a = Agent::new(dir.path().into()).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", a.token).parse().unwrap(),
        );
        for minutes in [15, 30] {
            assert!(save_idle_policy(
                State(a.clone()),
                headers.clone(),
                Json(IdlePolicy {
                    enabled: false,
                    minutes
                })
            )
            .await
            .is_ok());
            assert_eq!(a.idle_policy().unwrap().minutes, minutes);
        }
    }
    #[tokio::test]
    async fn cloud_reservation_rejects_stale_or_pending_wake_and_blocks_manual_start() {
        let dir = tempfile::tempdir().unwrap();
        let a = Agent::new(dir.path().into()).unwrap();
        std::fs::write(dir.path().join("desired"), "sleeping").unwrap();
        std::fs::write(dir.path().join("sleep-id"), "cycle").unwrap();
        assert_eq!(
            a.prepare_cloud_sleep("old").await.unwrap()["prepared"],
            false
        );
        a.idle.lock().await.wake_requested = true;
        assert_eq!(
            a.prepare_cloud_sleep("cycle").await.unwrap()["reason"],
            "wake_requested"
        );
        assert!(a.is_sleeping());
        a.idle.lock().await.wake_requested = false;
        assert_eq!(
            a.prepare_cloud_sleep("cycle").await.unwrap()["prepared"],
            true
        );
        assert!(a
            .start()
            .await
            .unwrap_err()
            .to_string()
            .contains("reserved for cloud shutdown"));
        a.stop().await.unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("desired")).unwrap(),
            "cloud_sleep"
        );
    }
}
