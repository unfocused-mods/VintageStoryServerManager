use anyhow::{Context, Result};
use serde_json::Value;
pub fn merge_and_validate(original: &Value, proposed: &Value) -> Result<Value> {
    anyhow::ensure!(proposed.is_object(), "Configuration must be an object");
    let mut next = original.clone();
    fn merge(old: &mut Value, new: &Value) {
        if let (Some(old), Some(new)) = (old.as_object_mut(), new.as_object()) {
            for (k, v) in new {
                if let Some(existing) = old.get_mut(k) {
                    if existing.is_object() && v.is_object() {
                        merge(existing, v);
                        continue;
                    }
                }
                old.insert(k.clone(), v.clone());
            }
        }
    }
    merge(&mut next, proposed);
    for key in ["Ip", "Port", "ModPaths"] {
        anyhow::ensure!(
            next[key] == original[key],
            "{key} is managed by deployment and cannot be changed here"
        );
    }
    anyhow::ensure!(
        next["WorldConfig"]["SaveFileLocation"] == original["WorldConfig"]["SaveFileLocation"],
        "Use world selection to change the world file"
    );
    for key in [
        "ServerName",
        "ServerDescription",
        "ServerUrl",
        "WelcomeMessage",
        "ServerLanguage",
        "Password",
        "DefaultRoleCode",
    ] {
        if let Some(value) = next.get(key) {
            anyhow::ensure!(
                value.is_null() || value.as_str().is_some_and(|s| s.len() <= 65536),
                "{key} must be text or null"
            );
        }
    }
    for key in [
        "VerifyPlayerAuth",
        "AdvertiseServer",
        "AllowPvP",
        "AllowFireSpread",
        "AllowFallingBlocks",
        "PassTimeWhenEmpty",
        "CompressPackets",
        "LogBlockBreakPlace",
        "LoginFloodProtection",
        "CorruptionProtection",
    ] {
        if let Some(value) = next.get(key) {
            anyhow::ensure!(value.is_boolean(), "{key} must be a boolean");
        }
    }
    for key in [
        "MaxClients",
        "MaxChunkRadius",
        "ClientConnectionTimeout",
        "BlockTickInterval",
        "DieAboveMemoryUsageMb",
    ] {
        if let Some(value) = next.get(key) {
            anyhow::ensure!(
                value
                    .as_u64()
                    .is_some_and(|n| n > 0 && n <= i32::MAX as u64),
                "{key} must be a positive integer"
            );
        }
    }
    for key in [
        "MaxClientsInQueue",
        "WarnClientsAfterAfkSeconds",
        "KickClientsAfterAfkSeconds",
        "ChatRateLimitMs",
        "MaxOwnedGroupChannelsPerUser",
        "BlockTickChunkRange",
        "MaxMainThreadBlockTicks",
        "RandomBlockTicksPerChunk",
        "DieBelowDiskSpaceMb",
    ] {
        if let Some(value) = next.get(key) {
            anyhow::ensure!(
                value.as_u64().is_some_and(|n| n <= i32::MAX as u64),
                "{key} must be a nonnegative integer"
            );
        }
    }
    for key in ["LogFileSplitAfterLine", "DieAboveErrorCount"] {
        if let Some(value) = next.get(key) {
            anyhow::ensure!(
                value.is_null()
                    || value
                        .as_u64()
                        .is_some_and(|n| n > 0 && n <= i32::MAX as u64),
                "{key} must be a positive integer or null"
            );
        }
    }
    for (key, min) in [
        ("TickTime", f64::MIN_POSITIVE),
        ("SpawnCapPlayerScaling", 0.),
    ] {
        if let Some(value) = next.get(key) {
            anyhow::ensure!(
                value.as_f64().is_some_and(|n| n.is_finite() && n >= min),
                "{key} is outside its numeric range"
            );
        }
    }
    if let Some(value) = next.get("WhitelistMode") {
        anyhow::ensure!(
            value.as_u64().is_some_and(|n| n <= 2),
            "WhitelistMode must be 0, 1 or 2"
        );
    }
    if let Some(role) = next.get("DefaultRoleCode").and_then(Value::as_str) {
        let roles = next["Roles"].as_array().context("Roles must be an array")?;
        anyhow::ensure!(
            roles.iter().any(|r| r["Code"] == role),
            "Select an existing default role"
        );
    }
    Ok(next)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_unknowns_and_rejects_invalid_or_managed_changes() {
        let old = json!({"Port":42420,"ModPaths":["Mods"],"custom":{"nested":9},"WorldConfig":{"SaveFileLocation":"safe"}});
        let next = merge_and_validate(&old, &json!({"MaxClients":12})).unwrap();
        assert_eq!(next["custom"]["nested"], 9);
        for input in [
            json!({"MaxClients":0}),
            json!({"TickTime":0}),
            json!({"AllowPvP":"true"}),
            json!({"Port":22}),
            json!({"WorldConfig":{"SaveFileLocation":"outside"}}),
        ] {
            assert!(merge_and_validate(&old, &input).is_err());
        }
    }
}
