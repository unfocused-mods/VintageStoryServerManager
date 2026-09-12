use crate::db::Db;
use aes_gcm::{
    aead::{rand_core::RngCore, Aead, OsRng},
    Aes256Gcm, KeyInit,
};
use anyhow::{Context, Result};
use argon2::{
    password_hash::{PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
pub fn token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Principal {
    pub name: String,
    pub role: String,
    pub servers: Vec<String>,
}
impl Principal {
    pub fn owner(&self) -> Result<()> {
        anyhow::ensure!(self.role == "owner", "Owner access required");
        Ok(())
    }
    pub fn access(&self, id: &str, write: bool) -> Result<()> {
        anyhow::ensure!(
            self.role == "owner" || self.servers.iter().any(|s| s == id),
            "No access to this server"
        );
        anyhow::ensure!(!write || self.role != "viewer", "Read-only account");
        Ok(())
    }
}
pub fn initialize(db: &Db, bootstrap: &str) -> Result<()> {
    let c = db.0.lock().unwrap();
    c.execute_batch("CREATE TABLE IF NOT EXISTS users(name TEXT PRIMARY KEY, password TEXT NOT NULL, role TEXT NOT NULL, servers TEXT NOT NULL, totp TEXT, disabled INTEGER NOT NULL DEFAULT 0);")?;
    c.execute(
        "INSERT OR IGNORE INTO tokens(hash,name,role,servers) VALUES(?1,'bootstrap','owner','[]')",
        [hash(bootstrap)],
    )?;
    Ok(())
}
pub fn authenticate(db: &Db, value: &str, public: bool) -> Result<Principal> {
    let c = db.0.lock().unwrap();
    let p = c
        .query_row(
            "SELECT name,role,servers FROM tokens WHERE hash=?1 AND revoked=0",
            [hash(value)],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .context("Invalid or revoked token")?;
    anyhow::ensure!(!public || p.0!="bootstrap","Bootstrap token is restricted to local management; create a TOTP user before public hosting");
    Ok(Principal {
        name: p.0,
        role: p.1,
        servers: serde_json::from_str(&p.2)?,
    })
}
pub fn create_user(
    db: &Db,
    name: &str,
    password: &str,
    role: &str,
    servers: &[String],
    totp: Option<&str>,
) -> Result<()> {
    anyhow::ensure!(
        name.len() >= 3 && name.len() <= 60 && name != "bootstrap",
        "Invalid username"
    );
    anyhow::ensure!(
        password.len() >= 12,
        "Password must contain at least 12 characters"
    );
    anyhow::ensure!(
        matches!(role, "owner" | "administrator" | "viewer"),
        "Invalid role"
    );
    if let Some(s) = totp {
        totp_instance(s)?;
    }
    let password = Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .to_string();
    db.0.lock().unwrap().execute(
        "INSERT INTO users(name,password,role,servers,totp) VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![name, password, role, serde_json::to_string(servers)?, totp],
    )?;
    Ok(())
}
fn totp_instance(secret: &str) -> Result<totp_rs::TOTP> {
    Ok(totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(secret.into())
            .to_bytes()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        Some("VSM".into()),
        "owner".into(),
    )?)
}
pub fn login(db: &Db, name: &str, password: &str, code: &str, public: bool) -> Result<String> {
    let c = db.0.lock().unwrap();
    let row = c
        .query_row(
            "SELECT password,role,servers,totp FROM users WHERE name=?1 AND disabled=0",
            [name],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .context("Invalid credentials")?;
    Argon2::default()
        .verify_password(
            password.as_bytes(),
            &PasswordHash::new(&row.0).map_err(|_| anyhow::anyhow!("Invalid credentials"))?,
        )
        .map_err(|_| anyhow::anyhow!("Invalid credentials"))?;
    anyhow::ensure!(
        !public || row.1 == "viewer" || row.3.is_some(),
        "TOTP is required for public write access"
    );
    if let Some(s) = row.3 {
        anyhow::ensure!(
            totp_instance(&s)?.check_current(code)?,
            "Invalid credentials"
        );
    }
    let value = token();
    c.execute(
        "INSERT INTO tokens(hash,name,role,servers) VALUES(?1,?2,?3,?4)",
        rusqlite::params![hash(&value), name, row.1, row.2],
    )?;
    Ok(value)
}
pub fn credential(reference: &str) -> Result<String> {
    if let Some(name) = reference.strip_prefix("vault:") {
        let path = std::env::var("VSM_VAULT_FILE")
            .context("VSM_VAULT_FILE is required for vault credentials")?;
        let key = std::env::var("VSM_VAULT_KEY")
            .context("VSM_VAULT_KEY is required for vault credentials")?;
        let secrets: std::collections::HashMap<String, String> =
            serde_json::from_slice(&unseal(&std::fs::read(path)?, &key)?)?;
        return secrets
            .get(name)
            .cloned()
            .context("Credential not found in encrypted vault");
    }
    if let Some(name) = reference.strip_prefix("env:") {
        return std::env::var(name).context("Credential environment variable is missing");
    }
    if let Some(name) = reference.strip_prefix("keyring:") {
        return keyring::Entry::new("vsm", name)?
            .get_password()
            .context("Credential not found in OS keyring");
    }
    anyhow::bail!("Credentials must use env:NAME, keyring:NAME or vault:NAME references")
}
pub fn vault_set(name: &str, value: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty() && name.len() < 100,
        "Invalid credential name"
    );
    let path = std::path::PathBuf::from(
        std::env::var("VSM_VAULT_FILE")
            .context("Set VSM_VAULT_FILE to the encrypted vault path")?,
    );
    let key =
        std::env::var("VSM_VAULT_KEY").context("Set VSM_VAULT_KEY separately from the vault")?;
    let mut secrets: std::collections::HashMap<String, String> = if path.exists() {
        serde_json::from_slice(&unseal(&std::fs::read(&path)?, &key)?)?
    } else {
        Default::default()
    };
    secrets.insert(name.into(), value.into());
    let encrypted = seal(&serde_json::to_vec(&secrets)?, &key)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(uuid::Uuid::new_v4().to_string());
    private_write(&temporary, &encrypted)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}
pub fn seal(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    anyhow::ensure!(
        passphrase.len() >= 16,
        "Recovery passphrase must have at least 16 characters"
    );
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let encrypted = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .encrypt((&nonce).into(), data)
        .map_err(|_| anyhow::anyhow!("Encryption failed"))?;
    let mut out = b"VSM1".to_vec();
    out.extend(salt);
    out.extend(nonce);
    out.extend(encrypted);
    Ok(out)
}
pub fn unseal(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    anyhow::ensure!(
        data.len() > 32 && &data[..4] == b"VSM1",
        "Invalid recovery file"
    );
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), &data[4..20], &mut key)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .decrypt((&data[20..32]).into(), &data[32..])
        .map_err(|_| anyhow::anyhow!("Invalid recovery passphrase or damaged file"))
}
pub fn private_write(path: &std::path::Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(data)?;
    drop(file);
    #[cfg(windows)]
    protect_path(path, false)?;
    Ok(())
}
pub fn protect_path(path: &std::path::Path, directory: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
        )?;
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME")
            .context("Cannot determine current user for private file permissions")?;
        let domain = std::env::var("USERDOMAIN").unwrap_or_default();
        let identity = if domain.is_empty() {
            user
        } else {
            format!("{domain}\\{user}")
        };
        let grant = format!("{identity}:{}F", if directory { "(OI)(CI)" } else { "" });
        let result = crate::subprocess::std_command("icacls.exe")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &grant])
            .output()?;
        anyhow::ensure!(
            result.status.success(),
            "Could not restrict management data permissions"
        );
    }
    Ok(())
}
/// Store a new cloud credential without exposing it through account records.
pub fn store_cloud_credential(secret: &str) -> Result<String> {
    anyhow::ensure!(
        !secret.trim().is_empty() && secret.len() <= 16384,
        "Enter a valid credential"
    );
    let name = format!("cloud-{}", uuid::Uuid::new_v4());
    keyring::Entry::new("vsm", &name)?.set_password(secret.trim())?;
    Ok(format!("keyring:{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_recovery_rejects_tampering() {
        let data = seal(b"private", "sixteen character secret").unwrap();
        assert_eq!(
            unseal(&data, "sixteen character secret").unwrap(),
            b"private"
        );
        let mut bad = data.clone();
        bad[35] ^= 1;
        assert!(unseal(&bad, "sixteen character secret").is_err());
    }
    #[test]
    fn scopes_are_not_global() {
        let p = Principal {
            name: "reader".into(),
            role: "viewer".into(),
            servers: vec!["one".into()],
        };
        assert!(p.access("one", false).is_ok());
        assert!(p.access("one", true).is_err());
        assert!(p.access("two", false).is_err());
    }
}
