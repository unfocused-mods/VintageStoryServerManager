use crate::domain::*;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Db(pub Arc<Mutex<Connection>>);
impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let c = Connection::open(path)?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;
          CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY, body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS servers(id TEXT PRIMARY KEY, body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY, server_id TEXT NOT NULL, action TEXT NOT NULL, status TEXT NOT NULL, message TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, request_key TEXT UNIQUE NOT NULL, request_hash TEXT NOT NULL);
          CREATE UNIQUE INDEX IF NOT EXISTS one_active_operation ON operations(server_id) WHERE status IN ('queued','running');
          CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL, server_id TEXT NOT NULL, kind TEXT NOT NULL, message TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS tokens(hash TEXT PRIMARY KEY, name TEXT NOT NULL, role TEXT NOT NULL, servers TEXT NOT NULL, revoked INTEGER NOT NULL DEFAULT 0);
          CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS operation_logs(seq INTEGER PRIMARY KEY AUTOINCREMENT, operation_id TEXT NOT NULL, at TEXT NOT NULL, level TEXT NOT NULL, stage TEXT NOT NULL, message TEXT NOT NULL); CREATE INDEX IF NOT EXISTS operation_logs_by_operation ON operation_logs(operation_id,seq);")?;
        Ok(Self(Arc::new(Mutex::new(c))))
    }
    pub fn list<T: serde::de::DeserializeOwned>(&self, table: &str) -> Result<Vec<T>> {
        anyhow::ensure!(
            matches!(table, "accounts" | "servers"),
            "Invalid collection"
        );
        let c = self.0.lock().unwrap();
        let mut st = c.prepare(&format!("SELECT body FROM {table} ORDER BY rowid"))?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn put<T: serde::Serialize>(&self, table: &str, id: &str, value: &T) -> Result<()> {
        anyhow::ensure!(
            matches!(table, "accounts" | "servers"),
            "Invalid collection"
        );
        self.0.lock().unwrap().execute(&format!("INSERT INTO {table}(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body"),params![id,serde_json::to_string(value)?])?;
        Ok(())
    }
    pub fn server(&self, id: &str) -> Result<Server> {
        self.list::<Server>("servers")?
            .into_iter()
            .find(|s| s.id == id)
            .context("Server not found")
    }
    pub fn account(&self, id: &str) -> Result<Account> {
        self.list::<Account>("accounts")?
            .into_iter()
            .find(|s| s.id == id)
            .context("Account not found")
    }
    pub fn save_account(&self, account: &Account) -> Result<()> {
        let c = self.0.lock().unwrap();
        let old: Option<String> = c
            .query_row(
                "SELECT body FROM accounts WHERE id=?1",
                [&account.id],
                |r| r.get(0),
            )
            .ok();
        if let Some(old) = old {
            let old: Account = serde_json::from_str(&old)?;
            anyhow::ensure!(
                old.provider == account.provider,
                "The provider cannot be changed. Add another account instead."
            );
            let used: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM servers WHERE json_extract(body,'$.account_id')=?1 AND json_extract(body,'$.state')!='deleted')", [&account.id], |r| r.get(0))?;
            anyhow::ensure!(!used || (old.settings["subscription_id"] == account.settings["subscription_id"] && old.settings["tenant_id"] == account.settings["tenant_id"]), "This account is used by a game server. Add another account to use a different subscription or tenant.");
        }
        c.execute("INSERT INTO accounts(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![account.id, serde_json::to_string(account)?])?;
        Ok(())
    }
    pub fn delete_account(&self, id: &str) -> Result<()> {
        let c = self.0.lock().unwrap();
        let used: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM servers WHERE json_extract(body,'$.account_id')=?1 AND json_extract(body,'$.state')!='deleted')", [id], |r| r.get(0))?;
        anyhow::ensure!(!used, "This account is used by a game server. Delete its deployment before removing the account.");
        let planned: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM meta WHERE key LIKE 'plan:%' AND json_extract(value,'$.spec.account_id')=?1 AND datetime(json_extract(value,'$.created_at')) > datetime('now','-15 minutes'))", [id], |r| r.get(0))?;
        anyhow::ensure!(!planned, "This account has a recent deployment plan. Wait for the plan to expire (up to 15 minutes) before removing it.");
        anyhow::ensure!(
            c.execute("DELETE FROM accounts WHERE id=?1", [id])? == 1,
            "Account not found"
        );
        Ok(())
    }
    pub fn operation(&self, id: &str) -> Result<Operation> {
        self.operations()?
            .into_iter()
            .find(|s| s.id == id)
            .context("Operation not found")
    }
    pub fn operations(&self) -> Result<Vec<Operation>> {
        let c = self.0.lock().unwrap();
        let mut st=c.prepare("SELECT id,server_id,action,status,message,created_at,updated_at FROM operations ORDER BY created_at DESC LIMIT 500")?;
        let rows = st.query_map([], |r| {
            Ok(Operation {
                id: r.get(0)?,
                server_id: r.get(1)?,
                action: r.get(2)?,
                status: r.get(3)?,
                message: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn enqueue(
        &self,
        server_id: &str,
        action: &str,
        key: &str,
        hash: &str,
    ) -> Result<(Operation, bool)> {
        anyhow::ensure!(
            !key.is_empty() && key.len() <= 200,
            "Idempotency-Key is required (max 200 characters)"
        );
        let mut c = self.0.lock().unwrap();
        let tx = c.transaction()?;
        let existing = tx.query_row(
            "SELECT id, request_hash FROM operations WHERE request_key=?1",
            [key],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        );
        if let Ok((id, old_hash)) = existing {
            anyhow::ensure!(
                old_hash == hash,
                "Idempotency key reused with different request"
            );
            drop(tx);
            drop(c);
            return Ok((self.operation(&id)?, false));
        }
        let op = Operation {
            id: uuid::Uuid::new_v4().to_string(),
            server_id: server_id.into(),
            action: action.into(),
            status: "queued".into(),
            message: "Waiting to execute".into(),
            created_at: now(),
            updated_at: now(),
        };
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                op.id,
                op.server_id,
                op.action,
                op.status,
                op.message,
                op.created_at,
                op.updated_at,
                key,
                hash
            ],
        )
        .context("Another operation is active on this server")?;
        tx.commit()?;
        Ok((op, true))
    }
    pub fn progress(&self, id: &str, status: &str, message: &str) -> Result<()> {
        let message = crate::diagnostics::redact(message);
        self.0.lock().unwrap().execute(
            "UPDATE operations SET status=?1,message=?2,updated_at=?3 WHERE id=?4",
            params![status, &message, now(), id],
        )?;
        let op = self.operation(id)?;
        self.operation_log(
            id,
            if status == "failed" { "error" } else { "info" },
            status,
            &message,
        )?;
        self.event(
            &op.server_id,
            "operation",
            &format!("{}: {} — {}", op.action, status, message),
        )?;
        Ok(())
    }
    pub fn recover_interrupted(&self) -> Result<()> {
        self.0.lock().unwrap().execute("UPDATE operations SET status='interrupted',message='Controller restarted. Inspect recorded resources before retrying.',updated_at=?1 WHERE status IN ('running','queued')",[now()])?;
        Ok(())
    }
    pub fn event(&self, server: &str, kind: &str, message: &str) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO events(at,server_id,kind,message) VALUES(?1,?2,?3,?4)",
            params![now(), server, kind, message],
        )?;
        Ok(())
    }
    pub fn events(&self, after: i64) -> Result<Vec<Event>> {
        let c = self.0.lock().unwrap();
        let mut st = c.prepare(
            "SELECT seq,at,server_id,kind,message FROM events WHERE seq>?1 ORDER BY seq LIMIT 500",
        )?;
        let rows = st.query_map([after], |r| {
            Ok(Event {
                seq: r.get(0)?,
                at: r.get(1)?,
                server_id: r.get(2)?,
                kind: r.get(3)?,
                message: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_removal_and_identity_guards() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("db")).unwrap();
        let mut a = Account {
            id: "a".into(),
            name: "Cloud".into(),
            provider: Provider::Azure,
            credential_ref: "".into(),
            settings: serde_json::json!({"subscription_id":"one"}),
        };
        db.save_account(&a).unwrap();
        db.put(
            "servers",
            "s",
            &serde_json::json!({"account_id":"a","state":"running"}),
        )
        .unwrap();
        assert!(db
            .delete_account("a")
            .unwrap_err()
            .to_string()
            .contains("game server"));
        a.settings["subscription_id"] = serde_json::json!("two");
        assert!(db.save_account(&a).is_err());
        a.settings["subscription_id"] = serde_json::json!("one");
        a.name = "Renamed".into();
        db.save_account(&a).unwrap();
        a.provider = Provider::Vultr;
        assert!(db.save_account(&a).is_err());
        db.put(
            "servers",
            "s",
            &serde_json::json!({"account_id":"a","state":"deleted"}),
        )
        .unwrap();
        db.0.lock()
            .unwrap()
            .execute(
                "INSERT INTO meta VALUES('plan:p',?1)",
                [serde_json::json!({"spec":{"account_id":"a"},"created_at":now()}).to_string()],
            )
            .unwrap();
        assert!(db
            .delete_account("a")
            .unwrap_err()
            .to_string()
            .contains("plan"));
        db.0.lock()
            .unwrap()
            .execute("DELETE FROM meta", [])
            .unwrap();
        db.delete_account("a").unwrap();
        assert!(db.account("a").is_err());
    }
    #[test]
    fn idempotency_and_serialization() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("db")).unwrap();
        let (a, new) = db.enqueue("s", "start", "k", "h").unwrap();
        assert!(new);
        assert!(!db.enqueue("s", "start", "k", "h").unwrap().1);
        assert!(db.enqueue("s", "stop", "k", "other").is_err());
        assert!(db.enqueue("s", "stop", "k2", "h2").is_err());
        db.progress(&a.id, "succeeded", "done").unwrap();
        assert!(db.enqueue("s", "stop", "k2", "h2").is_ok());
    }
}
