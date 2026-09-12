//! Bounded operational history. Request bodies, credentials and archive contents are never logged.
use crate::{db::Db, domain::now};
use anyhow::Result;
use rusqlite::params;
use serde_json::{json, Value};

pub fn redact(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if [
        "password",
        "authorization",
        "access_token",
        "accesstoken",
        "refresh_token",
        "secret",
        "token=",
        "token\":",
        "sig=",
        "private key",
        "private_key",
        "content_base64",
        "accountkey=",
        "apikey",
    ]
    .iter()
    .any(|word| lower.contains(word))
    {
        return "[Sensitive diagnostic content withheld]".into();
    }
    message
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(4096)
        .collect()
}
impl Db {
    pub fn operation_log(&self, id: &str, level: &str, stage: &str, message: &str) -> Result<()> {
        let c = self.0.lock().unwrap();
        let level = if ["info", "warning", "error"].contains(&level) {
            level
        } else {
            "info"
        };
        let stage = redact(stage).chars().take(80).collect::<String>();
        c.execute("INSERT INTO operation_logs(operation_id,at,level,stage,message) VALUES(?1,?2,?3,?4,?5)",params![id,now(),level,stage,redact(message)])?;
        c.execute("DELETE FROM operation_logs WHERE operation_id=?1 AND seq NOT IN (SELECT seq FROM operation_logs WHERE operation_id=?1 ORDER BY seq DESC LIMIT 200)",[id])?;
        c.execute("DELETE FROM operation_logs WHERE at < strftime('%Y-%m-%dT%H:%M:%fZ','now','-30 days') OR seq NOT IN (SELECT seq FROM operation_logs ORDER BY seq DESC LIMIT 20000)",[])?;
        Ok(())
    }
    pub fn operation_logs(&self, id: &str) -> Result<Value> {
        let operation = self.operation(id)?;
        let c = self.0.lock().unwrap();
        let mut query=c.prepare("SELECT seq,at,level,stage,message FROM operation_logs WHERE operation_id=?1 AND at >= strftime('%Y-%m-%dT%H:%M:%fZ','now','-30 days') ORDER BY seq")?;
        let entries=query.query_map([id],|row|Ok(json!({"seq":row.get::<_,i64>(0)?,"at":row.get::<_,String>(1)?,"level":row.get::<_,String>(2)?,"stage":row.get::<_,String>(3)?,"message":redact(&row.get::<_,String>(4)?)})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(
            json!({"available":!entries.is_empty(),"entries":entries,"summary":redact(&operation.message),"retention":{"days":30,"max_entries":200}}),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logs_are_bounded_redacted_and_legacy_is_truthful() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).unwrap();
        let (op, _) = db.enqueue("server", "test", "unique", "hash").unwrap();
        assert_eq!(db.operation_logs(&op.id).unwrap()["available"], false);
        db.operation_log(
            &op.id,
            "error",
            "request",
            "Authorization: Bearer very-private",
        )
        .unwrap();
        assert!(!db
            .operation_logs(&op.id)
            .unwrap()
            .to_string()
            .contains("very-private"));
        for i in 0..205 {
            db.operation_log(&op.id, "info", "progress", &format!("step{i}"))
                .unwrap();
        }
        let value = db.operation_logs(&op.id).unwrap();
        assert_eq!(value["entries"].as_array().unwrap().len(), 200);
        assert_eq!(value["entries"][0]["message"], "step5");
        assert_eq!(redact(&"x".repeat(5000)).len(), 4096);
        db.0.lock()
            .unwrap()
            .execute("UPDATE operation_logs SET at='2000-01-01T00:00:00Z'", [])
            .unwrap();
        assert!(db.operation_logs(&op.id).unwrap()["entries"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
