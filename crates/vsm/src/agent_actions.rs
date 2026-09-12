// Agent workflows kept separate from routing and process supervision.
fn tree_bytes(path: &std::path::Path) -> Result<u64> {
    let mut size = 0u64;
    for entry in std::fs::read_dir(path)? {
        let e = entry?;
        let m = e.file_type()?;
        ensure!(!m.is_symlink(), "Recovery source contains a symlink");
        if m.is_dir() {
            size = size
                .checked_add(tree_bytes(&e.path())?)
                .context("Directory size overflow")?;
        } else {
            size = size
                .checked_add(e.metadata()?.len())
                .context("Directory size overflow")?;
        }
    }
    Ok(size)
}
impl Agent {
    async fn game_command(&self) -> Result<Command> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let user = std::env::var("VSM_GAME_USER").unwrap_or_else(|_| "vintagestory".into());
            ensure!(
                !user.is_empty()
                    && user != "root"
                    && user
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                "Invalid game user"
            );
            let uid = crate::subprocess::command("id")
                .args(["-u", &user])
                .output()
                .await?;
            ensure!(
                uid.status.success() && String::from_utf8_lossy(&uid.stdout).trim() != "0",
                "Game account must be unprivileged"
            );
            ensure!(
                std::fs::metadata(&self.root)?.uid() == 0,
                "Agent data must be owned by root; use the supplied systemd service"
            );
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o711))?;
            let data = self.root.join("data");
            ensure!(
                crate::subprocess::command("chown")
                    .args(["-hR", &format!("{user}:{user}")])
                    .arg(&data)
                    .status()
                    .await?
                    .success(),
                "Cannot prepare game data ownership"
            );
            let releases = self.root.join("releases");
            ensure!(
                crate::subprocess::command("chmod")
                    .args(["-R", "a+rX"])
                    .arg(releases)
                    .status()
                    .await?
                    .success(),
                "Cannot prepare read-only game runtime"
            );
            let mut command = crate::subprocess::command("setpriv");
            command.args([
                "--reuid",
                &user,
                "--regid",
                &user,
                "--clear-groups",
                "--bounding-set=-all",
                "--no-new-privs",
                "--",
                "dotnet",
            ]);
            Ok(command)
        }
        #[cfg(not(target_os = "linux"))]
        {
            bail!("Game hosts must run Linux x64; use SSH bootstrap")
        }
    }
    fn agent_event(&self, level: &str, message: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("console.jsonl"))
        {
            let _ = writeln!(
                f,
                "{}",
                json!({"at":now(),"level":level,"message":redact(message)})
            );
        }
    }
    async fn file_operation(&self, action: &str, body: &Value) -> Result<Value> {
        ensure!(
            !self.running().await?,
            "Stop the server before modifying files"
        );
        let path = body["path"].as_str().context("path required")?;
        ensure!(!path.is_empty() && path != ".", "Cannot mutate data root");
        let root = self.root.join("data");
        let source = files::confined(&root, path)?;
        let dest = if action == "file-move" {
            let d = body["destination"]
                .as_str()
                .context("destination required")?;
            let p = files::confined(&root, d)?;
            ensure!(!p.exists(), "Destination already exists");
            Some(p)
        } else {
            None
        };
        self.recovery().await?;
        match action {
            "file-move" => {
                let dest = dest.unwrap();
                if let Some(p) = dest.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::rename(source, dest)?;
            }
            "file-delete" => {
                ensure!(source.exists(), "File or directory not found");
                let trash = self.root.join("trash");
                std::fs::create_dir_all(&trash)?;
                std::fs::rename(source, trash.join(uuid::Uuid::new_v4().to_string()))?;
            }
            "file-mkdir" => std::fs::create_dir(&source)?,
            "file-extract" => {
                let bytes = std::fs::read(&source)?;
                let stage = self.root.join(format!("extract-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&stage)?;
                files::extract_zip(&stage, &bytes, 2 * 1024 * 1024 * 1024)?;
                let destination = files::confined(
                    &root,
                    body["destination"]
                        .as_str()
                        .context("destination required")?,
                )?;
                ensure!(!destination.exists(), "Extract into a new directory");
                std::fs::rename(stage, destination)?;
            }
            _ => bail!("Unsupported file action"),
        }
        Ok(json!({"completed":true}))
    }
    async fn mod_operation(&self, action: &str, body: &Value) -> Result<Value> {
        let id = body["modid"].as_str().context("modid required")?;
        let mut manifest: Vec<Value> =
            serde_json::from_slice(&std::fs::read(self.root.join("mods.json"))?)?;
        let m = manifest
            .iter()
            .find(|m| m["modid"] == id)
            .context("Mod not found")?
            .clone();
        let enabled = m["enabled"].as_bool().unwrap_or(true);
        if action != "mod-enable" {
            for dependent in &manifest {
                ensure!(
                    dependent["enabled"] == false
                        || dependent["modid"] == id
                        || dependent["dependencies"].get(id).is_none(),
                    "An enabled mod depends on this mod; disable dependents first"
                );
            }
        } else if let Some(deps) = m["dependencies"].as_object() {
            for (id, version) in deps {
                if ["game", "survival", "creative"].contains(&id.as_str()) {
                    continue;
                }
                ensure!(
                    manifest.iter().any(|m| m["modid"] == *id
                        && m["enabled"] != false
                        && (version == "" || m["version"] == *version)),
                    "Enable the required dependency first: {id}"
                );
            }
        }
        ensure!(
            action == "mod-remove" || enabled == (action == "mod-disable"),
            "Mod already has requested state"
        );
        let source_dir = if enabled { "Mods" } else { "ModsDisabled" };
        let source = files::confined(
            &self.root.join("data"),
            &format!(
                "{source_dir}/{}",
                m["filename"].as_str().context("Missing filename")?
            ),
        )?;
        let was_running = self.running().await?;
        self.stop().await?;
        self.recovery().await?;
        if action == "mod-remove" {
            let trash = self.root.join("trash");
            std::fs::create_dir_all(&trash)?;
            std::fs::rename(source, trash.join(uuid::Uuid::new_v4().to_string()))?;
            manifest.retain(|m| m["modid"] != id);
        } else {
            let target_dir = if enabled { "ModsDisabled" } else { "Mods" };
            let dest = files::confined(
                &self.root.join("data"),
                &format!("{target_dir}/{}", m["filename"].as_str().unwrap()),
            )?;
            std::fs::create_dir_all(dest.parent().unwrap())?;
            ensure!(!dest.exists(), "Destination mod already exists");
            std::fs::rename(source, dest)?;
            manifest.iter_mut().find(|m| m["modid"] == id).unwrap()["enabled"] = json!(!enabled);
        }
        std::fs::write(
            self.root.join("mods.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        if was_running {
            self.start().await?;
        }
        Ok(json!(manifest))
    }
    async fn world_switch(&self, body: &Value) -> Result<Value> {
        let name = body["filename"].as_str().context("filename required")?;
        ensure!(
            name.ends_with(".vcdbs") && !name.contains(['/', '\\']),
            "Provide a .vcdbs filename"
        );
        let root = self.root.join("data");
        let world = files::confined(&root, &format!("Saves/{name}"))?;
        ensure!(world.is_file(), "Import the world under Saves first");
        let was_running = self.running().await?;
        self.stop().await?;
        self.recovery().await?;
        let path = root.join("serverconfig.json");
        let mut config: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        config["WorldConfig"]["SaveFileLocation"] = json!(world.to_string_lossy());
        std::fs::write(path, serde_json::to_vec_pretty(&config)?)?;
        if was_running {
            self.start().await?;
        }
        Ok(json!({"world":name}))
    }
    async fn world_create(&self, body: &Value) -> Result<Value> {
        let version = self.active_version()?;
        let schema =
            crate::world_options::catalog(&self.root.join("releases").join(&version), &version)?;
        let path = self.root.join("data/serverconfig.json");
        let original =
            std::fs::read(&path).context("Start the server once to generate its configuration")?;
        let config: Value = serde_json::from_slice(&original)?;
        let filename = format!("{}.vcdbs", uuid::Uuid::new_v4());
        let world = self.root.join("data/Saves").join(&filename);
        crate::world_options::prepare_world(&config, &schema, body, &world)?;
        let was_running = self.running().await?;
        self.stop().await?;
        if let Err(error) = self.recovery().await {
            if was_running {
                self.start().await?;
            }
            return Err(error);
        }
        let original = std::fs::read(&path)?;
        let latest: Value = serde_json::from_slice(&original)?;
        let next = crate::world_options::prepare_world(&latest, &schema, body, &world)?;
        if let Err(error) = files::write(
            &self.root.join("data"),
            "serverconfig.json",
            &serde_json::to_vec_pretty(&next)?,
            Some(&files::etag(&original)),
            false,
        ) {
            if was_running {
                self.start().await?;
            }
            return Err(error);
        }
        if was_running {
            self.start().await?;
        }
        Ok(
            json!({"world":body["name"],"filename":filename,"note":"World selected; generates on startup. Previous worlds remain in Saves."}),
        )
    }
    async fn repository_operation(&self, action: &str, body: &Value) -> Result<Value> {
        let p = self.policy()?;
        ensure!(
            !p.repository.is_empty(),
            "Configure a backup repository first"
        );
        let password = std::env::var(&p.password_env)
            .context("Backup password environment variable missing")?;
        let mut cmd = crate::subprocess::command("restic");
        cmd.args(["-r", &p.repository])
            .env("RESTIC_PASSWORD", password);
        match action {
            "backup-init" => {
                cmd.arg("init");
            }
            "backup-check" => {
                cmd.args(["check", "--read-data-subset=5%"]);
            }
            "backup-snapshots" => {
                cmd.args(["snapshots", "--json"]);
            }
            "backup-restore" => {
                let snapshot = body["snapshot"].as_str().context("snapshot required")?;
                ensure!(
                    !snapshot.is_empty() && snapshot.bytes().all(|b| b.is_ascii_hexdigit()),
                    "Explicit snapshot ID required"
                );
                let stage = self
                    .root
                    .join(format!("remote-restore-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&stage)?;
                cmd.args(["restore", snapshot, "--target"]).arg(&stage);
                let output = cmd.output().await?;
                ensure!(
                    output.status.success(),
                    "Remote restore failed; original server unchanged"
                );
                let copy = find_backup_root(&stage, 0)?
                    .context("Snapshot contains no complete VSM recovery point")?;
                let id = format!("remote-{}", uuid::Uuid::new_v4());
                let dest = self.root.join("recovery").join(&id);
                std::fs::rename(copy, dest)?;
                return self.restore(&id).await;
            }
            _ => bail!("Unknown backup operation"),
        };
        let output = cmd.output().await?;
        ensure!(
            output.status.success(),
            "Restic operation failed; check repository credentials and agent logs"
        );
        if action == "backup-snapshots" {
            Ok(serde_json::from_slice(&output.stdout)?)
        } else {
            Ok(json!({"completed":true,"action":action}))
        }
    }
    async fn scheduled_restart(&self) -> Result<()> {
        use chrono::Timelike;
        let path = self.root.join("schedule.json");
        if !path.exists() {
            return Ok(());
        }
        let mut schedule: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        let today = chrono::Utc::now().date_naive().to_string();
        if schedule["enabled"] == true
            && schedule["hour_utc"].as_u64() == Some(chrono::Utc::now().hour() as u64)
            && schedule["last_date"] != today
            && self.running().await?
        {
            schedule["last_date"] = json!(today);
            std::fs::write(&path, serde_json::to_vec_pretty(&schedule)?)?;
            self.command("announce Scheduled server restart in 30 seconds.")
                .await?;
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            self.stop().await?;
            self.start().await?;
            self.agent_event("info", "Scheduled restart completed");
        }
        Ok(())
    }
}
fn find_backup_root(path: &std::path::Path, depth: usize) -> Result<Option<PathBuf>> {
    ensure!(depth < 20, "Unexpected backup directory depth");
    if path.join("data").is_dir() && path.join("version").is_file() {
        return Ok(Some(path.into()));
    }
    for entry in std::fs::read_dir(path)? {
        let e = entry?;
        if e.file_type()?.is_dir() {
            if let Some(found) = find_backup_root(&e.path(), depth + 1)? {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
}
async fn schedule(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    Ok(Json(serde_json::from_slice(
        &std::fs::read(a.root.join("schedule.json"))
            .unwrap_or_else(|_| br#"{"enabled":false,"hour_utc":4}"#.to_vec()),
    )?))
}
async fn save_schedule(State(a): State<Agent>, h: HeaderMap, Json(body): Json<Value>) -> ApiResult {
    a.authorize(&h)?;
    ensure!(
        body["hour_utc"].as_u64().is_some_and(|h| h < 24),
        "Hour must be 0–23 UTC"
    );
    ensure!(body["enabled"].is_boolean(), "enabled must be boolean");
    std::fs::write(
        a.root.join("schedule.json"),
        serde_json::to_vec_pretty(&body)?,
    )?;
    Ok(Json(body))
}
async fn worlds(State(a): State<Agent>, h: HeaderMap) -> ApiResult {
    a.authorize(&h)?;
    let root = a.root.join("data/Saves");
    if !root.exists() {
        return Ok(Json(json!([])));
    }
    Ok(Json(json!(files::list(&root, "")?
        .into_iter()
        .filter(|f| f.name.ends_with(".vcdbs"))
        .collect::<Vec<_>>())))
}

#[cfg(test)]
mod workflow_tests {
    use super::*;
    #[tokio::test]
    async fn restore_keeps_current_data_and_versions() {
        let t = tempfile::tempdir().unwrap();
        let a = Agent::new(t.path().into()).unwrap();
        std::fs::write(a.root.join("version"), "1.21.6").unwrap();
        std::fs::create_dir(a.root.join("releases/1.21.6")).unwrap();
        std::fs::write(
            a.root.join("releases/1.21.6/VintagestoryServer.dll"),
            b"fixture",
        )
        .unwrap();
        std::fs::write(a.root.join("data/world.txt"), b"old").unwrap();
        let point = a.recovery().await.unwrap();
        std::fs::write(a.root.join("data/world.txt"), b"new").unwrap();
        a.restore(point.file_name().unwrap().to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(a.root.join("data/world.txt")).unwrap(),
            b"old"
        );
    }
    #[test]
    fn rejects_archive_traversal_before_writing() {
        use std::io::Write;
        let t = tempfile::tempdir().unwrap();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"bad").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(files::extract_zip(t.path(), &bytes, 1000).is_err());
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 0);
    }
    #[cfg(unix)]
    #[test]
    fn rejects_dangling_symlink() {
        let t = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/does-not-exist", t.path().join("link")).unwrap();
        assert!(files::confined(t.path(), "link/file").is_err());
    }
}
