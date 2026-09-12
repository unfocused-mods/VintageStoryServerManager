#[derive(Clone, Debug, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
    pub destructive: bool,
}
struct Spec {
    tool: ToolDefinition,
    method: &'static str,
    route: String,
}
fn schema(fields: &[(&str, &str, bool)]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, kind, needed) in fields {
        let mut item = json!({"type":kind});
        if *kind == "string" {
            item["maxLength"] = json!(4096);
        }
        if *name == "body" {
            item["description"] = json!("API request object; use api_schema to inspect models. Unknown configuration fields must be preserved.");
        }
        properties.insert((*name).into(), item);
        if *needed {
            required.push(*name);
        }
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn specs() -> Vec<Spec> {
    let mut result = Vec::new();
    let mut add = |name: &str,
                   description: &str,
                   method: &'static str,
                   route: &str,
                   fields: &[(&str, &str, bool)],
                   destructive: bool| {
        let mut fields = fields.to_vec();
        if method != "GET" && name != "operation_wait" {
            fields.push(("idempotency_key", "string", false));
        }
        result.push(Spec {
            tool: ToolDefinition {
                name: name.into(),
                description: description.into(),
                input_schema: schema(&fields),
                read_only: method == "GET",
                destructive,
            },
            method,
            route: route.into(),
        });
    };
    for (name, description, path) in [
        ("manager_summary", "Inventory and recent operation summary; recorded state can be stale.", "/summary"),
        ("servers_list", "List servers in this Manager inventory.", "/servers"),
        ("accounts_list", "List configured cloud accounts and credential references, never credential values.", "/accounts"),
        ("operations_list", "List durable activities and their current states.", "/operations"),
        ("api_schema", "Read the versioned Manager OpenAPI schema for complex request bodies and generic API access.", "/openapi.json"),
        ("game_versions", "List available stable game downloads.", "/game/releases"),
    ] { add(name, description, "GET", path, &[], false); }
    for (name, description, path) in [
        ("server_get", "Read recorded server metadata.", ""),
        ("server_status", "Read live Remote game state and host metrics. The VM must be awake.", "/agent/status"),
        ("console_logs", "Read bounded game console history; console command results appear here.", "/agent/logs"),
        ("config_get", "Read game configuration; preserve unknown fields when saving. Password values are redacted.", "/agent/config"),
        ("mods_list", "List installed exact mod versions, hashes, state and available icons.", "/agent/mods"),
        ("worlds_list", "List worlds stored on the Remote.", "/agent/worlds"),
        ("world_options", "Read installed game world presets and configuration options.", "/agent/world-options"),
        ("backups_list", "List recovery points and configured backup policy.", "/agent/backups"),
        ("backup_policy_get", "Read Remote backup policy.", "/agent/backup-policy"),
        ("schedule_get", "Read Remote restart schedule.", "/agent/schedule"),
        ("idle_policy_get", "Read Remote game inactivity policy; stopping the game alone does not stop VM charges.", "/agent/idle-policy"),
        ("cloud_idle_get", "Read Manager cloud cost-saving policy; requires the Manager to remain available.", "/cloud-idle-policy"),
        ("infrastructure_status", "Read actual cloud power, provisioning and Remote installation state.", "/infrastructure/status"),
        ("infrastructure_inspect", "Inspect owned provider resources.", "/infrastructure"),
    ] { add(name, description, "GET", &format!("/servers/{{server_id}}{path}"), &[("server_id","string",true)], false); }
    for (name, description, path) in [
        ("config_set", "Save game configuration while stopped. body={value:{changed fields},etag:from_config_get}; omitted fields are preserved, including redacted passwords.", "/agent/config"),
        ("backup_policy_set", "Configure encrypted backups on the Remote; secret values should be supplied through configured references.", "/agent/backup-policy"),
        ("schedule_set", "Configure the Remote restart schedule.", "/agent/schedule"),
        ("idle_policy_set", "Configure Remote game inactivity behavior.", "/agent/idle-policy"),
        ("cloud_idle_set", "Configure cloud deallocation policy; disk/IP charges can remain.", "/cloud-idle-policy"),
    ] { add(name, description, "PUT", &format!("/servers/{{server_id}}{path}"), &[("server_id","string",true),("body","object",true)], true); }
    for (name, description, action, body, destructive) in [
        ("server_start", "Start the game. Returns an operation; wait and check live status before testing connectivity.", "start", false, false),
        ("server_stop", "Gracefully stop the game, leaving the VM allocated.", "stop", false, true),
        ("server_restart", "Gracefully restart the game.", "restart", false, true),
        ("backup_create", "Create a consistent local recovery copy, then upload off-host if the backup policy is enabled.", "backup", false, false),
        ("backup_restore", "Restore a local recovery point; body contains id. Replaces active game data.", "restore", true, true),
        ("backup_repository_init", "Initialize the configured encrypted backup repository.", "backup-init", false, false),
        ("backup_repository_check", "Run integrity checks against the configured backup repository.", "backup-check", false, false),
        ("backup_snapshots", "Request repository snapshot listing through an operation; inspect its logs/results.", "backup-snapshots", false, false),
        ("backup_repository_restore", "Restore an encrypted repository snapshot; body.snapshot is an explicit restic snapshot ID.", "backup-restore", true, true),
        ("world_create", "Create a world using name, play_style, optional seed/height and world_configuration from world_options. Creates recovery before changing active data.", "world-create", true, true),
        ("world_switch", "Switch active world using body.filename, an existing .vcdbs filename under Saves; existing data is retained for recovery.", "world-switch", true, true),
        ("game_install", "Install an explicitly selected game version; body.version is required.", "install", true, true),
        ("game_update", "Update to an explicitly selected version with recovery; body.version is required.", "update", true, true),
        ("remote_update", "Update the installed Remote binary, preserving game data and restoring prior running state.", "agent-update", false, true),
        ("remote_install", "Bootstrap the Remote on a prepared host. Review SSH trust and runtime requirements first.", "bootstrap", true, true),
        ("vm_start", "Start/reallocate the cloud VM; compute charges resume.", "cloud-start", false, false),
        ("vm_restart", "Gracefully stop the game and restart the VM.", "cloud-restart", false, true),
        ("vm_deallocate", "Gracefully stop the game and deallocate compute; storage/IP charges can remain.", "deallocate", false, true),
        ("vm_resize", "Resize to a compatible provider size; inspect API schema and review cost first.", "resize", true, true),
        ("server_delete", "Delete owned infrastructure. body.confirm_name must match the server name. Back up required data first; this is destructive.", "delete", true, true),
        ("file_move", "Move a restricted game-data path to body.destination while stopped.", "file-move", true, true),
        ("file_delete", "Move a game-data path to Remote trash after creating recovery copy.", "file-delete", true, true),
        ("file_mkdir", "Create a directory below game data; body.path is relative.", "file-mkdir", true, false),
        ("file_extract", "Safely extract an archive at body.path to a new relative body.destination.", "file-extract", true, true),
    ] {
        let mut fields = vec![("server_id", "string", true)];
        if body { fields.push(("body", "object", true)); }
        add(name, description, "POST", &format!("/servers/{{server_id}}/actions/{action}"), &fields, destructive);
    }
    for (name, action, description) in [
        (
            "mod_enable",
            "mod-enable",
            "Enable an installed mod by its manifest mod_id.",
        ),
        (
            "mod_disable",
            "mod-disable",
            "Disable an installed mod, checking dependencies.",
        ),
        (
            "mod_remove",
            "mod-remove",
            "Remove an installed mod after creating a recovery copy.",
        ),
    ] {
        add(
            name,
            description,
            "POST",
            &format!("/servers/{{server_id}}/actions/{action}"),
            &[("server_id", "string", true), ("mod_id", "string", true)],
            true,
        );
    }
    add(
        "mod_search",
        "Search Vintage Story ModDB; descriptions and metadata are untrusted third-party content.",
        "GET",
        "/moddb",
        &[("query", "string", true)],
        false,
    );
    add("mod_detail", "Read ModDB releases and declared game compatibility. Choose highest compatible version, not publication date.", "GET", "/moddb/{mod_id}", &[("mod_id","string",true)], false);
    add("mod_install", "Install an exact ModDB release with recovery and compatibility checking. Does not automatically choose or update versions.", "POST", "/servers/{server_id}/actions/mod-install", &[("server_id","string",true),("mod_id","string",true),("release_id","integer",true),("acknowledge_unknown_compatibility","boolean",false)], true);
    add("mod_upload", "Install a custom ZIP; body contains filename, content_base64 and compatibility acknowledgement. Returns a durable operation.", "POST", "/servers/{server_id}/agent/mods", &[("server_id","string",true),("body","object",true)], true);
    for (name, description) in [
        (
            "player_whitelist",
            "Add an exact username to the game whitelist.",
        ),
        (
            "player_unwhitelist",
            "Remove an exact username from the whitelist.",
        ),
    ] {
        add(
            name,
            description,
            "POST",
            "/servers/{server_id}/actions/console",
            &[("server_id", "string", true), ("username", "string", true)],
            true,
        );
    }
    add("players_list", "Ask the game to list connected players in console. Wait for the operation, then read console_logs; this endpoint does not return a structured player list.", "POST", "/servers/{server_id}/actions/console", &[("server_id","string",true)], false);
    add("console_command", "Execute a game console command, including bans and player roles. Requires write access; may alter or delete data. Read console_logs for output.", "POST", "/servers/{server_id}/actions/console", &[("server_id","string",true),("command","string",true)], true);
    for (name, endpoint) in [("files_list", "files"), ("file_read", "file")] {
        add(name, "Read restricted game data. File reads return content_base64 and an etag for concurrency-safe writes.", "GET", &format!("/servers/{{server_id}}/agent/{endpoint}"), &[("server_id","string",true),("path","string",name == "file_read")], false);
    }
    add("file_write", "Write game data while stopped; body requires path, content_base64 and the etag from file_read for existing files.", "POST", "/servers/{server_id}/agent/files", &[("server_id","string",true),("body","object",true)], true);
    for (name, path) in [("operation_get", ""), ("operation_logs", "/logs")] {
        add(name, "Read durable operation status or bounded diagnostic logs; queued/running is not completion.", "GET", &format!("/operations/{{operation_id}}{path}"), &[("operation_id","string",true)], false);
    }
    add("operation_wait", "Wait up to timeout_seconds (default 120, maximum 600). Returns the terminal operation including failed/interrupted status; timeout never cancels it.", "GET", "", &[("operation_id","string",true),("timeout_seconds","integer",false)], false);
    for (name, method, suffix, body, description) in [
        (
            "account_validate",
            "POST",
            "/validate",
            false,
            "Validate saved cloud account access.",
        ),
        (
            "account_discover",
            "GET",
            "/discover",
            false,
            "Discover tagged Azure deployments without modifying resources.",
        ),
        (
            "account_adopt",
            "POST",
            "/adopt",
            true,
            "Adopt an existing discovered deployment; body.vm_id identifies it. No reinstall.",
        ),
        (
            "account_remove",
            "DELETE",
            "",
            false,
            "Remove this account from inventory; retains cloud resources and stored credentials.",
        ),
        (
            "account_regions",
            "GET",
            "/regions",
            false,
            "List available cloud regions for this account.",
        ),
    ] {
        let mut fields = vec![("account_id", "string", true)];
        if body {
            fields.push(("body", "object", true));
        }
        add(
            name,
            description,
            method,
            &format!("/accounts/{{account_id}}{suffix}"),
            &fields,
            method != "GET",
        );
    }
    add("account_save", "Create or update a cloud account from body; use credential references, not plaintext credentials.", "POST", "/accounts", &[("body","object",true)], true);
    add("deployment_plan", "Create a reviewable deployment plan from a specification. Inspect cost and resource changes before apply.", "POST", "/plans", &[("body","object",true)], false);
    add("deployment_apply", "Apply a reviewed plan; body must contain the API's explicit cost/plan acknowledgements. Creates billable resources.", "POST", "/deployments", &[("body","object",true)], true);
    add("remote_connection_set", "Update an existing Remote SSH key path on the Manager machine; body.ssh_private_key is required.", "PATCH", "/servers/{server_id}/connection", &[("server_id","string",true),("body","object",true)], true);
    add(
        "remote_reconnect",
        "Reconnect an existing Remote after SSH trust/key setup without reinstalling.",
        "POST",
        "/servers/{server_id}/reconnect",
        &[("server_id", "string", true)],
        false,
    );
    add("api_request", "Advanced versioned API escape hatch for workflows not covered by named tools. No direct cloud, database or filesystem access. Mutations may be destructive; inspect api_schema first.", "POST", "", &[("method","string",true),("path","string",true),("body","object",false)], true);
    for spec in &mut result {
        if let Some(body) = body_schema(&spec.tool.name) {
            spec.tool.input_schema["properties"]["body"] = body;
        }
        if let Some(timeout) = spec.tool.input_schema["properties"].get_mut("timeout_seconds") {
            *timeout = json!({"type":"integer","minimum":1,"maximum":600,"default":120});
        }
        if let Some(release) = spec.tool.input_schema["properties"].get_mut("release_id") {
            *release = json!({"type":"integer","minimum":1});
        }
    }
    result
}
pub fn catalog() -> Vec<ToolDefinition> {
    specs().into_iter().map(|s| s.tool).collect()
}

fn body_schema(name: &str) -> Option<Value> {
    let string = json!({"type":"string"});
    let boolean = json!({"type":"boolean"});
    let (properties, required) = match name {
        "config_set" => (
            json!({"value":{"type":"object","description":"Changed configuration fields; omitted fields and redacted passwords are preserved."},"etag":string}),
            vec!["value", "etag"],
        ),
        "file_write" => (
            json!({"path":string,"content_base64":string,"etag":{"type":["string","null"],"description":"Required for existing files; null for new files."}}),
            vec!["path", "content_base64", "etag"],
        ),
        "mod_upload" => (
            json!({"filename":string,"content_base64":string,"acknowledge_unknown_compatibility":{"type":"boolean","const":true}}),
            vec![
                "filename",
                "content_base64",
                "acknowledge_unknown_compatibility",
            ],
        ),
        "world_create" => (
            json!({"name":string,"play_style":{"type":"string","description":"Preset code from world_options, default surviveandbuild."},"seed":{"type":["string","null"]},"height":{"type":"integer"},"world_configuration":{"type":"object","description":"Option codes/values from world_options."}}),
            vec!["name"],
        ),
        "world_switch" => (
            json!({"filename":{"type":"string","pattern":"^[^/\\\\]+\\.vcdbs$"}}),
            vec!["filename"],
        ),
        "backup_restore" => (json!({"id":string}), vec!["id"]),
        "backup_repository_restore" => (
            json!({"snapshot":{"type":"string","pattern":"^[0-9a-fA-F]+$"}}),
            vec!["snapshot"],
        ),
        "game_install" | "game_update" => (json!({"version":string}), vec!["version"]),
        "vm_resize" => (
            json!({"size":string,"accept_cost_change":{"type":"boolean","const":true}}),
            vec!["size", "accept_cost_change"],
        ),
        "server_delete" => (
            json!({"confirm_name":string,"delete_data":{"type":"boolean","description":"For Docker, also remove persistent data. Defaults false."}}),
            vec!["confirm_name"],
        ),
        "file_move" | "file_extract" => (
            json!({"path":string,"destination":string}),
            vec!["path", "destination"],
        ),
        "file_delete" | "file_mkdir" => (json!({"path":string}), vec!["path"]),
        "deployment_apply" => (
            json!({"plan_id":string,"accept_incomplete_estimate":boolean}),
            vec!["plan_id"],
        ),
        "remote_connection_set" => (json!({"ssh_private_key":string}), vec!["ssh_private_key"]),
        "account_adopt" => (
            json!({"vm_id":string,"name":string,"ssh_private_key":string}),
            vec!["vm_id"],
        ),
        _ => return additional_body_schema(name),
    };
    Some(
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    )
}

impl AdminClient {
    pub async fn execute(
        &self,
        name: &str,
        input: Value,
        idempotency_key: Option<&str>,
    ) -> Result<Value> {
        let spec = specs()
            .into_iter()
            .find(|s| s.tool.name == name)
            .ok_or_else(|| AdminError::new("invalid_input", "Unknown administration tool"))?;
        validate_input(&input, &spec.tool.input_schema)?;
        let text = |key: &str| {
            input[key]
                .as_str()
                .ok_or_else(|| AdminError::new("invalid_input", &format!("{key} is required")))
        };
        let key = idempotency_key.or_else(|| input["idempotency_key"].as_str());
        if name == "operation_wait" {
            return self
                .wait_operation(
                    text("operation_id")?,
                    Duration::from_secs(input["timeout_seconds"].as_u64().unwrap_or(120)),
                )
                .await;
        }
        if name == "api_request" {
            return self
                .request(
                    text("method")?,
                    text("path")?,
                    input.get("body").cloned(),
                    key,
                )
                .await;
        }
        let mut route = spec.route;
        for field in ["server_id", "account_id", "operation_id", "mod_id"] {
            let placeholder = format!("{{{field}}}");
            if route.contains(&placeholder) {
                route = route.replace(&placeholder, segment(text(field)?)?);
            }
        }
        let mut body = if spec.method == "GET" {
            None
        } else {
            Some(input.get("body").cloned().unwrap_or_else(|| json!({})))
        };
        if name == "mod_search" || name == "files_list" || name == "file_read" {
            let (key, value) = if name == "mod_search" {
                ("q", text("query")?)
            } else {
                ("path", input["path"].as_str().unwrap_or(""))
            };
            let mut url = reqwest::Url::parse("http://localhost").unwrap();
            url.query_pairs_mut().append_pair(key, value);
            route.push('?');
            route.push_str(url.query().unwrap());
        }
        if name == "mod_install" {
            if input["release_id"].as_u64().is_none_or(|n| n == 0) {
                return Err(AdminError::new(
                    "invalid_input",
                    "release_id must be positive",
                ));
            }
            body = Some(
                json!({"modid":text("mod_id")?,"releaseid":input["release_id"],"acknowledge_unknown_compatibility":input["acknowledge_unknown_compatibility"].as_bool().unwrap_or(false)}),
            );
        } else if ["mod_enable", "mod_disable", "mod_remove"].contains(&name) {
            body = Some(json!({"modid":text("mod_id")?}));
        } else if name == "player_whitelist" || name == "player_unwhitelist" {
            let username = text("username")?;
            if username.is_empty()
                || username.len() > 64
                || !username
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(AdminError::new(
                    "invalid_input",
                    "Username must contain only letters, numbers, underscore or hyphen",
                ));
            }
            body = Some(
                json!({"command":format!("player {username} whitelist {}", if name == "player_whitelist" { "on" } else { "off" })}),
            );
        } else if name == "players_list" {
            body = Some(json!({"command":"list clients"}));
        } else if name == "console_command" {
            let command = text("command")?;
            if command.trim().is_empty() || command.contains(['\r', '\n', '\0']) {
                return Err(AdminError::new(
                    "invalid_input",
                    "Provide one nonempty console command",
                ));
            }
            body = Some(json!({"command":command}));
        }
        self.request(spec.method, &route, body, key).await
    }
}
fn validate_input(input: &Value, schema: &Value) -> Result<()> {
    validate_value(input, schema, "input")
}
fn validate_value(value: &Value, schema: &Value, location: &str) -> Result<()> {
    let matches_type = |kind: &str| match kind {
        "string" => value.is_string(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "integer" => value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    let type_matches = match &schema["type"] {
        Value::String(kind) => matches_type(kind),
        Value::Array(kinds) => kinds
            .iter()
            .any(|kind| kind.as_str().is_some_and(matches_type)),
        Value::Null => true,
        _ => false,
    };
    if !type_matches {
        return Err(AdminError::new(
            "invalid_input",
            &format!("{location} has the wrong value type"),
        ));
    }
    if schema
        .get("const")
        .is_some_and(|expected| expected != value)
    {
        return Err(AdminError::new(
            "invalid_input",
            &format!("{location} requires explicit acknowledgement"),
        ));
    }
    if schema["enum"]
        .as_array()
        .is_some_and(|choices| !choices.contains(value))
    {
        return Err(AdminError::new(
            "invalid_input",
            &format!("{location} is not an allowed option"),
        ));
    }
    if value.as_u64().is_some_and(|n| {
        schema["minimum"].as_u64().is_some_and(|min| n < min)
            || schema["maximum"].as_u64().is_some_and(|max| n > max)
    }) {
        return Err(AdminError::new(
            "invalid_input",
            &format!("{location} is outside the allowed range"),
        ));
    }
    if value.as_str().is_some_and(|s| {
        schema["maxLength"]
            .as_u64()
            .is_some_and(|max| s.len() as u64 > max)
    }) {
        return Err(AdminError::new(
            "invalid_input",
            &format!("{location} is too long"),
        ));
    }
    if value.as_str().is_some_and(|s| {
        schema["minLength"]
            .as_u64()
            .is_some_and(|min| (s.len() as u64) < min)
    }) {
        return Err(AdminError::new(
            "invalid_input",
            &format!("{location} is too short"),
        ));
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for key in required {
                if !object.contains_key(key.as_str().unwrap()) {
                    return Err(AdminError::new(
                        "invalid_input",
                        &format!("{location}.{} is required", key.as_str().unwrap()),
                    ));
                }
            }
        }
        for (key, child) in object {
            if let Some(child_schema) = schema["properties"].get(key) {
                validate_value(child, child_schema, &format!("{location}.{key}"))?;
            } else if schema["additionalProperties"] == false {
                return Err(AdminError::new(
                    "invalid_input",
                    &format!("Unknown input field: {location}.{key}"),
                ));
            }
        }
    }
    Ok(())
}
