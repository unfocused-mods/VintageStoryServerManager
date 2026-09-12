// Request shapes for the remaining administration settings and provisioning tools.
// Dynamic game fields still come from config_get/world_options; these schemas
// describe the stable Manager/Remote envelopes, not a guessed game version.
fn additional_body_schema(name: &str) -> Option<Value> {
    let boolean = json!({"type":"boolean"});
    let hour =
        json!({"type":"integer","minimum":0,"maximum":23,"description":"Hour of day in UTC."});
    let minutes = json!({"type":"integer","minimum":1,"maximum":1440,"default":15,"description":"Minutes with no connected players before stopping."});
    let (properties, required) = match name {
        "schedule_set" => (
            json!({
                "enabled":boolean, "hour_utc":hour
            }),
            vec!["enabled", "hour_utc"],
        ),
        "idle_policy_set" => (
            json!({
                "enabled":boolean, "minutes":minutes
            }),
            vec!["enabled", "minutes"],
        ),
        "cloud_idle_set" => (
            json!({
                "enabled":boolean, "minutes":minutes,
                "external_manager_confirmed":{"type":"boolean","description":"Set true only when this Manager is outside the managed VM and will remain running. Required to enable Azure deallocation; does not enable wake-on-join."}
            }),
            vec!["enabled", "minutes", "external_manager_confirmed"],
        ),
        "backup_policy_set" => (
            json!({
                "enabled":boolean,
                "repository":{"type":"string","description":"Restic local path or S3-compatible repository URL. Credentials must already be configured on the Remote; do not embed credentials in the URL."},
                "password_env":{"type":"string","description":"Name of an existing environment variable on the Remote containing the restic password, not the password itself.","default":"RESTIC_PASSWORD"},
                "hour_utc":hour,
                "daily":{"type":"integer","minimum":1,"maximum":4294967295u64,"default":7,"description":"Number of daily snapshots to retain."},
                "weekly":{"type":"integer","minimum":1,"maximum":4294967295u64,"default":4,"description":"Number of weekly snapshots to retain."}
            }),
            vec![
                "enabled",
                "repository",
                "password_env",
                "hour_utc",
                "daily",
                "weekly",
            ],
        ),
        "account_save" => (
            json!({
                "id":{"type":"string","maxLength":99,"description":"Existing account ID to update, or an empty string to create a new account."},
                "name":{"type":"string","minLength":1},
                "provider":{"type":"string","enum":["azure","hetzner","digital_ocean","vultr","ssh","docker","ssh_docker"]},
                "credential_ref":{"type":"string","description":"Existing env:, keyring: or vault: credential reference. Empty string is permitted for Azure CLI/managed identity and SSH/Docker accounts. Never a credential value."},
                "settings":{"type":"object","additionalProperties":true,"properties":{
                    "auth":{"type":"string","enum":["azure_cli","managed_identity","service_principal"],"description":"Azure authentication mode; defaults to azure_cli."},
                    "subscription_id":{"type":"string","description":"Azure subscription ID."},
                    "tenant_id":{"type":"string","description":"Azure tenant ID; required for a service principal."},
                    "client_id":{"type":"string","description":"Service principal application ID or optional user-assigned managed identity client ID."},
                    "game_port":{"type":"integer","description":"Docker published game port; defaults to 42420."}
                }}
            }),
            vec!["id", "name", "provider", "credential_ref"],
        ),
        "deployment_plan" => (
            json!({
                "name":{"type":"string","minLength":3,"maxLength":40,"pattern":"^[a-z0-9-]+$","description":"Deployment name."},
                "account_id":{"type":"string","description":"Account ID from accounts_list."},
                "region":{"type":"string","description":"Provider region code from account_regions; empty for existing SSH or Docker hosts."},
                "size":{"type":"string","description":"Available provider VM size code; empty for existing SSH or Docker hosts."},
                "game_version":{"type":"string","description":"An explicit version from game_versions."},
                "ssh_public_key":{"type":"string","description":"OpenSSH public key text for cloud provisioning; empty when unused. Never private key contents."},
                "ssh_private_key":{"type":"string","description":"Private key file path on the Manager machine for SSH access; empty for local Docker. Never private key contents."},
                "admin_cidr":{"type":"string","description":"Restricted IPv4 source CIDR allowed to access SSH, for example 203.0.113.10/32; empty when unused by the provider."},
                "existing_host":{"type":"string","default":"","description":"Existing host address for SSH or SSH Docker."},
                "ssh_user":{"type":"string","default":"vsm","description":"SSH username on an existing host."}
            }),
            vec![
                "name",
                "account_id",
                "region",
                "size",
                "game_version",
                "ssh_public_key",
                "ssh_private_key",
                "admin_cidr",
            ],
        ),
        "remote_install" => (json!({}), vec![]),
        _ => return None,
    };
    Some(
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    )
}
