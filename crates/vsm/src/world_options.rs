//! Read the installed game's world-creation metadata without loading executable code.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

fn embedded(bytes: &[u8]) -> Result<Option<Value>> {
    let marker = b"T\x0e\x0bWorldConfig";
    let Some(at) = bytes.windows(marker.len()).position(|v| v == marker) else {
        return Ok(None);
    };
    let tail = &bytes[at + marker.len()..];
    let first = *tail.first().context("Missing metadata length")?;
    let (width, mask) = if first < 128 {
        (1, 127)
    } else if first < 192 {
        (2, 16383)
    } else if first < 224 {
        (4, 536870911)
    } else {
        anyhow::bail!("Invalid metadata length")
    };
    let mut len = 0usize;
    for byte in tail.get(..width).context("Truncated metadata length")? {
        len = (len << 8) | *byte as usize;
    }
    len &= mask;
    anyhow::ensure!(len <= 1024 * 1024, "World metadata exceeds 1 MiB");
    Ok(Some(serde_json::from_slice(
        tail.get(width..width + len)
            .context("Truncated world metadata")?,
    )?))
}
fn read_json(path: &Path) -> Result<Value> {
    anyhow::ensure!(
        std::fs::metadata(path)?.len() <= 16 * 1024 * 1024,
        "Game metadata is too large"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
fn translated(lang: &Value, key: &str, fallback: &str) -> String {
    lang[key].as_str().unwrap_or(fallback).to_owned()
}
pub fn catalog(release: &Path, version: &str) -> Result<Value> {
    let lang = read_json(&release.join("assets/game/lang/en.json"))?;
    let mut presets = Vec::new();
    let mut fields = Vec::new();
    for file in ["VSSurvivalMod.dll", "VSCreativeMod.dll", "VSEssentials.dll"] {
        let path = release.join("Mods").join(file);
        anyhow::ensure!(
            std::fs::metadata(&path)?.len() <= 64 * 1024 * 1024,
            "Game assembly is too large"
        );
        let Some(metadata) = embedded(&std::fs::read(path)?)? else {
            continue;
        };
        for preset in metadata["playstyles"].as_array().into_iter().flatten() {
            let code = preset["code"].as_str().context("Missing play style code")?;
            let mut preset = preset.clone();
            let langcode = preset["langCode"].as_str().unwrap_or(code);
            preset["label"] = json!(translated(&lang, &format!("playstyle-{langcode}"), code));
            presets.push(preset);
        }
        for field in metadata["worldConfigAttributes"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let code = field["code"]
                .as_str()
                .context("Missing world option code")?;
            anyhow::ensure!(
                matches!(field["dataType"].as_str(), Some("dropdown" | "bool")),
                "Unsupported world option type for {code}"
            );
            let mut field = field.clone();
            field["label"] = json!(translated(&lang, &format!("worldattribute-{code}"), code));
            let cat = field["category"].as_str().unwrap_or("general");
            field["categoryLabel"] = json!(translated(
                &lang,
                &format!("worldconfig-category-{cat}"),
                cat
            ));
            field["description"] = json!(translated(
                &lang,
                &format!("worldattribute-{code}-desc"),
                ""
            ));
            if let Some(names) = field["names"].as_array() {
                field["names"] = json!(names
                    .iter()
                    .map(|name| {
                        let name = name.as_str().unwrap_or("");
                        translated(&lang, &format!("worldconfig-{code}-{name}"), name)
                    })
                    .collect::<Vec<_>>());
            }
            fields.push(field);
        }
    }
    anyhow::ensure!(
        !presets.is_empty() && !fields.is_empty(),
        "Installed game has no supported world-creation metadata"
    );
    Ok(
        json!({"version":version,"presets":presets,"fields":fields,"source":"Installed game metadata","mod_options_included":false}),
    )
}
pub fn validate_values(schema: &Value, values: &Value) -> Result<()> {
    let values = values
        .as_object()
        .context("World settings must be an object")?;
    for (code, value) in values {
        let field = schema["fields"]
            .as_array()
            .context("Missing world schema")?
            .iter()
            .find(|f| f["code"] == *code)
            .with_context(|| format!("Unknown world option: {code}"))?;
        if field["dataType"] == "bool" {
            anyhow::ensure!(value.is_boolean(), "{code} must be a boolean");
        } else {
            anyhow::ensure!(
                field["values"]
                    .as_array()
                    .is_some_and(|allowed| allowed.contains(value)),
                "Unsupported value for {code}"
            );
        }
    }
    Ok(())
}

/// Prepare new-world settings without replacing unrelated or mod-specific configuration.
pub fn prepare_world(config: &Value, schema: &Value, body: &Value, save: &Path) -> Result<Value> {
    let name = body["name"].as_str().context("World name required")?;
    anyhow::ensure!(
        !name.trim().is_empty() && name.chars().count() < 100,
        "World name must be 1–99 characters"
    );
    let code = body["play_style"].as_str().unwrap_or("surviveandbuild");
    let preset = schema["presets"]
        .as_array()
        .context("Missing presets")?
        .iter()
        .find(|p| p["code"] == code)
        .context("Select a play style available in the installed game")?;
    let empty = json!({});
    let overrides = body.get("world_configuration").unwrap_or(&empty);
    validate_values(schema, overrides)?;
    anyhow::ensure!(
        body["seed"].is_null() || body["seed"].as_str().is_some_and(|v| v.len() <= 100),
        "Seed must be text up to 100 characters"
    );
    let mut next = config.clone();
    anyhow::ensure!(next.is_object(), "Server configuration is invalid");
    if !next["WorldConfig"].is_object() {
        next["WorldConfig"] = json!({});
    }
    let mut values = next["WorldConfig"]["WorldConfiguration"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    // Reset known settings to the selected preset; retain unrelated mod keys.
    for f in schema["fields"].as_array().context("Missing fields")? {
        if let Some(code) = f["code"].as_str() {
            values.remove(code);
            if let Some(default) = f.get("default") {
                let default = if f["dataType"] == "bool" {
                    match default {
                        Value::Bool(value) => json!(value),
                        Value::String(value) if value == "true" || value == "false" => {
                            json!(value == "true")
                        }
                        _ => anyhow::bail!("Invalid boolean default for {code}"),
                    }
                } else {
                    default.clone()
                };
                values.insert(code.into(), default);
            }
        }
    }
    for (k, v) in preset["worldConfig"]
        .as_object()
        .context("Invalid preset settings")?
    {
        values.insert(k.clone(), v.clone());
    }
    for (k, v) in overrides.as_object().unwrap() {
        values.insert(k.clone(), v.clone());
    }
    // Shipped preset dictionaries encode some boolean values as strings.
    for field in schema["fields"].as_array().context("Missing fields")? {
        if field["dataType"] == "bool" {
            if let Some(code) = field["code"].as_str() {
                if let Some(value) = values.get_mut(code) {
                    if let Some(text) = value.as_str() {
                        anyhow::ensure!(
                            text == "true" || text == "false",
                            "Invalid boolean preset value for {code}"
                        );
                        *value = json!(text == "true");
                    }
                }
            }
        }
    }
    for (key, axis) in [("worldWidth", "MapSizeX"), ("worldLength", "MapSizeZ")] {
        if let Some(value) = values.get(key) {
            let n = value
                .as_str()
                .context("Map size must be numeric text")?
                .parse::<u32>()?;
            anyhow::ensure!(
                n >= 32 && n % 32 == 0,
                "Map dimensions must be multiples of 32"
            );
            next[axis] = json!(n);
        }
    }
    for (key, top) in [
        ("allowFireSpread", "AllowFireSpread"),
        ("allowFallingBlocks", "AllowFallingBlocks"),
    ] {
        if let Some(value) = values.get(key) {
            next[top] = value.clone();
        }
    }
    if let Some(height) = body.get("height") {
        let h = height.as_u64().context("Height must be an integer")?;
        anyhow::ensure!(
            (128..=2048).contains(&h) && h % 32 == 0,
            "Height must be 128–2048 blocks in multiples of 32"
        );
        next["MapSizeY"] = json!(h);
        next["WorldConfig"]["MapSizeY"] = json!(h);
    }
    let world = &mut next["WorldConfig"];
    world["WorldName"] = json!(name.trim());
    world["SaveFileLocation"] = json!(save);
    world["Seed"] = body["seed"].clone();
    world["PlayStyle"] = json!(code);
    world["PlayStyleLangCode"] = preset["langCode"].clone();
    world["WorldType"] = preset["worldType"].clone();
    world["WorldConfiguration"] = json!(values);
    world["RepairMode"] = json!(false);
    Ok(next)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_metadata_parser_and_values_fail_closed() {
        let payload = br#"{"playstyles":[],"worldConfigAttributes":[]}"#;
        let mut bytes = b"T\x0e\x0bWorldConfig".to_vec();
        bytes.push(payload.len() as u8);
        bytes.extend(payload);
        assert!(embedded(&bytes).unwrap().is_some());
        bytes.pop();
        assert!(embedded(&bytes).is_err());
        let schema = json!({"fields":[{"code":"hostility","dataType":"dropdown","values":["passive","aggressive"]},{"code":"map","dataType":"bool"}]});
        assert!(validate_values(&schema, &json!({"hostility":"passive","map":false})).is_ok());
        assert!(validate_values(&schema, &json!({"map":"false"})).is_err());
        assert!(validate_values(&schema, &json!({"hostility":"invented"})).is_err());
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    #[test]
    fn new_world_preserves_unknowns_and_applies_preset_and_overrides() {
        let schema = json!({"presets":[{"code":"exploration","langCode":"preset-exploration","worldType":"standard","worldConfig":{"worldWidth":"1024","gameMode":"survival","allowFireSpread":true}}],"fields":[{"code":"worldWidth","dataType":"dropdown","values":["1024","2048"]},{"code":"gameMode","dataType":"dropdown","values":["survival","creative"]},{"code":"allowFireSpread","dataType":"bool"}]});
        let config = json!({"ModPaths":["Mods"],"ModSpecific":42,"WorldConfig":{"Unknown":"kept","WorldConfiguration":{"modKey":"kept","gameMode":"creative"}}});
        let next=prepare_world(&config,&schema,&json!({"name":"New world","play_style":"exploration","seed":"example","world_configuration":{"worldWidth":"2048","allowFireSpread":false}}),Path::new("/safe/new.vcdbs")).unwrap();
        assert_eq!(next["ModSpecific"], 42);
        assert_eq!(next["WorldConfig"]["Unknown"], "kept");
        assert_eq!(next["WorldConfig"]["WorldConfiguration"]["modKey"], "kept");
        assert_eq!(
            next["WorldConfig"]["WorldConfiguration"]["gameMode"],
            "survival"
        );
        assert_eq!(next["MapSizeX"], 2048);
        assert_eq!(next["AllowFireSpread"], false);
        assert_eq!(next["WorldConfig"]["PlayStyle"], "exploration");
    }
    #[test]
    fn preset_and_default_boolean_strings_become_typed_settings() {
        let schema = json!({"presets":[{"code":"surviveandbuild","worldType":"standard","worldConfig":{"allowFireSpread":"false"}}],"fields":[{"code":"allowFireSpread","dataType":"bool","default":"true"},{"code":"allowFallingBlocks","dataType":"bool","default":"true"}]});
        let next = prepare_world(
            &json!({}),
            &schema,
            &json!({"name":"Typed","world_configuration":{}}),
            Path::new("/safe/world.vcdbs"),
        )
        .unwrap();
        assert_eq!(next["AllowFireSpread"], false);
        assert_eq!(next["AllowFallingBlocks"], true);
        assert_eq!(
            next["WorldConfig"]["WorldConfiguration"]["allowFireSpread"],
            false
        );
    }
    #[test]
    #[ignore = "Reads an explicitly supplied installed game VSM_GAME_RELEASE"]
    fn live_installed_world_metadata() {
        let path = std::env::var("VSM_GAME_RELEASE").unwrap();
        let schema = catalog(Path::new(&path), "1.22.7").unwrap();
        assert_eq!(schema["presets"].as_array().unwrap().len(), 5);
        assert_eq!(schema["fields"].as_array().unwrap().len(), 64);
        let hunger = schema["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["code"] == "playerHungerSpeed")
            .unwrap();
        assert_eq!(hunger["label"], "Hunger rate");
        println!("Loaded 5 presets and 64 localized world settings from actual release");
    }
}
