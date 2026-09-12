impl Agent {
    fn binary_identity(&self)->Option<String> {
        static HASH:std::sync::OnceLock<Option<String>>=std::sync::OnceLock::new();
        HASH.get_or_init(||std::env::current_exe().ok().and_then(|path|std::fs::read(path).ok()).map(|bytes|crate::files::etag(&bytes))).clone()
    }
    fn operation_step(&self,id:Option<&str>,stage:&str,message:&str) {
        let Some(id)=id.filter(|id|uuid::Uuid::parse_str(id).is_ok()) else {return;};
        let work=||->Result<()> {
            let directory=self.root.join("operation-logs");std::fs::create_dir_all(&directory)?;
            let path=directory.join(format!("{id}.json"));
            let mut entries:Vec<Value>=serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|_|b"[]".to_vec()))?;
            entries.push(json!({"at":now(),"level":if stage=="failed"{"error"}else{"info"},"stage":stage,"message":crate::diagnostics::redact(message)}));
            if entries.len()>200 { entries.drain(..entries.len()-200); }
            std::fs::write(path,serde_json::to_vec(&entries)?)?;
            let mut files=std::fs::read_dir(&directory)?.filter_map(|entry|entry.ok()).collect::<Vec<_>>();
            files.sort_by_key(|entry|entry.metadata().and_then(|m|m.modified()).ok());
            let excess=files.len().saturating_sub(100);
            for (index,entry) in files.into_iter().enumerate() {
                let old=entry.metadata()?.modified()?.elapsed().unwrap_or_default().as_secs()>30*86400;
                if index<excess || old {std::fs::remove_file(entry.path())?;}
            }
            Ok(())
        };
        if work().is_err() {self.agent_event("error","Operation diagnostic history could not be saved");}
    }
}
async fn agent_operation_logs(State(a):State<Agent>,h:HeaderMap,axum::extract::Path(id):axum::extract::Path<String>)->ApiResult {
    a.authorize(&h)?;
    let id:uuid::Uuid=id.parse()?;
    let path=a.root.join("operation-logs").join(format!("{id}.json"));
    Ok(Json(serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|_|b"[]".to_vec()))?))
}
