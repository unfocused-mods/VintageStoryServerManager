async fn update_remote(c:&Controller,server:Server,operation:&str)->Result<Value> {
    let mut recovery_server=server.clone();
    match tokio::time::timeout(std::time::Duration::from_secs(600),update_remote_inner(c,server,operation)).await {
        Ok(result)=>result,
        Err(_)=> {
            recovery_server.state="attention".into();c.db.put("servers",&recovery_server.id,&recovery_server)?;
            anyhow::bail!("Remote update exceeded its time limit. Check Remote health before retrying; saved game restart intent is retained")
        }
    }
}
async fn update_remote_inner(c:&Controller,mut server:Server,operation:&str)->Result<Value> {
    anyhow::ensure!(remote_installation_ready(&c.db,&server)?,"Install the Remote before updating it");
    let binary=std::env::var("VSM_AGENT_BINARY").map(PathBuf::from).unwrap_or_else(|_|c.root.join("artifacts/vsm-agent"));
    let bytes=tokio::fs::read(&binary).await.context("Bundled Remote binary not found")?;
    anyhow::ensure!(bytes.starts_with(b"\x7fELF") && bytes.get(18..20)==Some(&[0x3e,0]),"Remote update requires a Linux x64 agent");
    let hash=crate::files::etag(&bytes);
    let status=update_remote_call(&server,reqwest::Method::GET,"/v1/status",None).await?;
    anyhow::ensure!(matches!(status["state"].as_str(),Some("running"|"starting"|"stopped"|"sleeping")),"Remote game state is unknown; refresh before updating");
    let previous_hash=status["agent_binary_sha256"].as_str().map(str::to_owned);
    let key=format!("agent-update-resume:{}",server.id);
    let was_running={
        let db=c.db.0.lock().unwrap();
        let previous=db.query_row("SELECT value FROM meta WHERE key=?1",[&key],|r|r.get::<_,String>(0)).ok();
        let desired=previous.map_or(matches!(status["state"].as_str(),Some("running"|"starting")),|value|value=="true");
        db.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",rusqlite::params![key,desired.to_string()])?;desired
    };
    c.db.operation_log(operation,"info","stop","Saving and stopping the game before binary-only Remote update")?;
    update_remote_call(&server,reqwest::Method::POST,"/v1/actions/stop",Some(json!({}))).await?;
    server.state="stopped".into();c.db.put("servers",&server.id,&server)?;
    let result=async {
        let _=c.db.operation_log(operation,"info","upload",&format!("Uploading Remote SHA256={hash}; preserving previous binary and all game data"));
        tokio::time::timeout(std::time::Duration::from_secs(180),async {
            if crate::docker::is_docker(&server.provider) {crate::docker::update_agent(&server,&binary,operation).await}
            else {transport::update_agent(&server,&binary,operation).await}
        }).await.context("Remote binary update timed out")??;
        let _=c.db.operation_log(operation,"info","verify","Verifying running Remote binary identity and health");
        wait_updated_remote(&server,Some(&hash)).await?;
        if was_running {
            let _=c.db.operation_log(operation,"info","restart","Restarting the previously running game");
            update_remote_call(&server,reqwest::Method::POST,"/v1/actions/start",Some(json!({}))).await?;
        }
        Ok::<(),anyhow::Error>(())
    }.await;
    if let Err(error)=result {
        let _=c.db.operation_log(operation,"error","update-failed",&format!("{error:#}"));
        let _=c.db.operation_log(operation,"warning","rollback","Attempting to restore the previous Remote binary");
        let rollback=tokio::time::timeout(std::time::Duration::from_secs(120),async {
            let current=update_remote_call(&server,reqwest::Method::GET,"/v1/status",None).await.ok();
            let unchanged=previous_hash.as_ref().is_some_and(|hash|current.as_ref().is_some_and(|v|v["agent_binary_sha256"]==*hash));
            if let Some(current)=current {
                if matches!(current["state"].as_str(),Some("running"|"starting")) {update_remote_call(&server,reqwest::Method::POST,"/v1/actions/stop",Some(json!({}))).await?;}
            }
            if !unchanged {
                if crate::docker::is_docker(&server.provider) {crate::docker::rollback_agent(&server,operation).await?;}
                else {transport::rollback_agent(&server,operation).await?;}
            }
            wait_updated_remote(&server,previous_hash.as_deref()).await?;
            if was_running {update_remote_call(&server,reqwest::Method::POST,"/v1/actions/start",Some(json!({}))).await?;}
            Ok::<(),anyhow::Error>(())
        }).await;
        let restored=matches!(rollback,Ok(Ok(())));
        if restored { c.db.0.lock().unwrap().execute("DELETE FROM meta WHERE key=?1",[&key])?; }
        server.state=if restored {if was_running{"starting"}else{"stopped"}}else{"attention"}.into();
        c.db.put("servers",&server.id,&server)?;
        let _=c.db.operation_log(operation,if restored{"info"}else{"error"},"rollback",if restored{"Previous Remote restored; original game state restored or restart submitted"}else{"Rollback could not be verified. Game restart intent is retained; check the Remote before retrying"});
        anyhow::bail!("Remote update failed: {error}. {}",if restored{"Previous Remote restored"}else{"Rollback needs attention; the game may remain stopped"});
    }
    server.state=if was_running{"starting"}else{"stopped"}.into();c.db.put("servers",&server.id,&server)?;
    let cleanup=tokio::time::timeout(std::time::Duration::from_secs(20),async {
        if crate::docker::is_docker(&server.provider){crate::docker::finish_agent_update(&server,operation).await}
        else{transport::finish_agent_update(&server,operation).await}
    }).await;
    if !matches!(cleanup,Ok(Ok(()))) {let _=c.db.operation_log(operation,"warning","cleanup","Remote updated; previous binary retained because backup cleanup did not complete");}
    c.db.0.lock().unwrap().execute("DELETE FROM meta WHERE key=?1",[key])?;
    Ok(json!({"updated":true,"state":server.state,"agent_binary_sha256":hash,"game_files_preserved":true}))
}
async fn wait_updated_remote(server:&Server,hash:Option<&str>)->Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(60),async {
        loop {
            if let Ok(status)=update_remote_call(server,reqwest::Method::GET,"/v1/status",None).await {
                if hash.is_none_or(|hash|status["agent_binary_sha256"]==hash) {return;}
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }).await.context("Remote health or binary identity could not be verified")
}

async fn update_remote_call(server:&Server,method:reqwest::Method,path:&str,body:Option<Value>)->Result<Value> {
    let seconds=if path.ends_with("/stop"){150}else{20};
    tokio::time::timeout(std::time::Duration::from_secs(seconds),transport::call(server,method,path,body)).await.context("Remote update communication timed out")?
}
