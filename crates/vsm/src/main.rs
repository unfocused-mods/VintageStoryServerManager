use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "vsm", version, about = "Vintage Story server manager")]
struct Args {
    /// Manager URL. Defaults to http://127.0.0.1:4317 outside desktop mode.
    #[arg(long, env = "VSM_URL", global = true, conflicts_with = "desktop")]
    url: Option<String>,
    #[arg(long, env = "VSM_TOKEN", hide_env_values = true, global = true, conflicts_with_all = ["desktop", "token_file"])]
    token: Option<String>,
    /// Read a scoped Manager token from a file instead of a command-line secret.
    #[arg(
        long,
        env = "VSM_TOKEN_FILE",
        global = true,
        conflicts_with = "desktop"
    )]
    token_file: Option<PathBuf>,
    /// Connect to the running desktop Manager using its local connection metadata.
    #[arg(long, global = true)]
    desktop: bool,
    /// Desktop data directory override; requires --desktop.
    #[arg(long, global = true, requires = "desktop")]
    desktop_data: Option<PathBuf>,
    /// Reuse this key when retrying the same mutation after an uncertain response.
    #[arg(long, global = true)]
    idempotency_key: Option<String>,
    /// Maximum seconds to wait for an operation; timing out does not cancel it.
    #[arg(long, global = true, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=600))]
    wait_timeout: u64,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Serve the shared administration tools over MCP stdio (stdout is protocol only).
    Mcp,
    /// Discover schemas or invoke any named administration tool, shared with MCP.
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    /// Start a game server, optionally waiting for completion.
    Start {
        id: String,
        #[arg(long)]
        wait: bool,
    },
    /// Gracefully stop a game server, optionally waiting for completion.
    Stop {
        id: String,
        #[arg(long)]
        wait: bool,
    },
    /// Gracefully restart a game server, optionally waiting for completion.
    Restart {
        id: String,
        #[arg(long)]
        wait: bool,
    },
    /// Browse installed mods and install an exact reviewed ModDB release.
    Mods {
        #[command(subcommand)]
        command: ModsCommand,
    },
    /// Add an exact Vintage Story username to the whitelist, or remove it.
    Whitelist {
        id: String,
        username: String,
        #[arg(long)]
        remove: bool,
        #[arg(long)]
        wait: bool,
    },
    /// Read configuration, or update it from a JSON file after inspecting the current configuration.
    Config {
        id: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        wait: bool,
    },
    /// Browse, read or edit files within the Remote's restricted file root.
    Files {
        #[command(subcommand)]
        command: FilesCommand,
    },
    /// List backups or create a consistent server backup.
    Backups {
        #[command(subcommand)]
        command: BackupsCommand,
    },
    /// List activities, inspect one, retrieve its detailed logs or wait for completion.
    Activity {
        id: Option<String>,
        #[arg(long, requires = "id", conflicts_with = "wait")]
        logs: bool,
        #[arg(long, requires = "id")]
        wait: bool,
    },
    /// List available stable Linux server downloads; no sign-in required.
    Versions,
    /// Run the local or self-hosted controller.
    Serve {
        #[arg(long, default_value = ".vsm")]
        data: PathBuf,
        #[arg(long, default_value = "127.0.0.1:4317")]
        bind: String,
        #[arg(long, default_value = "apps/ui/dist")]
        ui: PathBuf,
        #[arg(long)]
        public: bool,
    },
    /// Show controller inventory and current operations.
    Status,
    Servers,
    Accounts,
    /// Add a provider account from JSON. Store credential references, never secrets.
    AccountAdd {
        file: PathBuf,
    },
    AccountValidate {
        id: String,
    },
    /// Discover existing Manager deployments in an Azure subscription without changing them.
    AccountDiscover {
        id: String,
    },
    /// Restore a discovered Azure deployment to this Manager without reinstalling its Remote.
    AccountAdopt {
        id: String,
        #[arg(long)]
        vm_id: String,
        /// Existing private key on the Manager's machine; omit to restore cloud controls only.
        #[arg(long)]
        ssh_private_key: Option<String>,
        #[arg(long)]
        name: Option<String>,
    },
    /// Set the SSH private key path after recovering access to an existing Remote.
    ServerConnection {
        id: String,
        /// Path on the Manager's machine, not necessarily the CLI machine.
        #[arg(long)]
        ssh_private_key: String,
    },
    /// Reconnect to an existing Remote after configuring its SSH key and verifying host identity.
    ServerReconnect {
        id: String,
    },
    Catalog {
        id: String,
    },
    Quote {
        id: String,
        #[arg(long)]
        region: String,
        #[arg(long)]
        size: String,
    },
    /// Build a reviewable deployment plan from a JSON specification.
    Plan {
        file: PathBuf,
    },
    /// Apply a reviewed Docker, SSH, or cloud deployment plan.
    Apply {
        plan_id: String,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        accept_incomplete_estimate: bool,
    },
    /// Execute a game or infrastructure action.
    Action {
        id: String,
        action: String,
        #[arg(long)]
        body: Option<PathBuf>,
        #[arg(long)]
        wait: bool,
    },
    Operations,
    Logs {
        id: String,
        #[arg(long)]
        follow: bool,
    },
    /// Read or mutate any documented endpoint (full API access for automation).
    Request {
        method: String,
        path: String,
        #[arg(long)]
        body: Option<PathBuf>,
    },
    /// Store a provider secret in the OS credential store; reads from stdin.
    SecretSet {
        name: String,
        #[arg(long)]
        vault: bool,
    },
    /// Print OpenAPI without starting a controller.
    Openapi,
    /// Create a local controller user from JSON on stdin (for first hosted setup).
    UserAdd {
        #[arg(long, default_value = ".vsm")]
        data: PathBuf,
    },
    Completions {
        shell: clap_complete::Shell,
    },
    /// Measure TCP connection latency from this computer; exports portable measurements.
    Probe {
        host: String,
        #[arg(long, default_value_t = 443)]
        port: u16,
        #[arg(long)]
        quote_id: String,
        #[arg(long)]
        location: String,
        #[arg(long, default_value_t = 1)]
        players: u32,
    },
}
#[derive(Subcommand)]
enum AdminCommand {
    List,
    Schema {
        name: String,
    },
    Run {
        name: String,
        /// JSON object as text; use --file for large input or shell portability.
        #[arg(long, conflicts_with = "file")]
        input: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        wait: bool,
    },
}
#[derive(Subcommand)]
enum ModsCommand {
    List {
        id: String,
    },
    Search {
        query: String,
    },
    Detail {
        mod_id: String,
    },
    Install {
        id: String,
        mod_id: String,
        release_id: u64,
        #[arg(long)]
        acknowledge_unknown_compatibility: bool,
        #[arg(long)]
        wait: bool,
    },
}
#[derive(Subcommand)]
enum FilesCommand {
    List {
        id: String,
        #[arg(default_value = "")]
        path: String,
    },
    Read {
        id: String,
        path: String,
    },
    /// Apply the documented file-write JSON body, including its concurrency precondition.
    Write {
        id: String,
        file: PathBuf,
        #[arg(long)]
        wait: bool,
    },
}
#[derive(Subcommand)]
enum BackupsCommand {
    List {
        id: String,
    },
    Create {
        id: String,
        #[arg(long)]
        wait: bool,
    },
}
fn admin_client(args: &Args) -> Result<vsm::admin::AdminClient> {
    if args.desktop {
        return Ok(vsm::admin::AdminClient::from_desktop(
            args.desktop_data.clone(),
        )?);
    }
    let token = if let Some(token) = &args.token {
        token.clone()
    } else if let Some(path) = &args.token_file {
        std::fs::read_to_string(path).map_err(|_| {
            vsm::admin::AdminError::new("configuration", "Could not read Manager token file")
        })?
    } else if args.url.is_none() {
        std::fs::read_to_string(".vsm/owner.token").map_err(|_| {
            vsm::admin::AdminError::new(
                "configuration",
                "Use --desktop for the desktop app, or set VSM_URL and VSM_TOKEN_FILE",
            )
        })?
    } else {
        return Err(vsm::admin::AdminError::new(
            "configuration",
            "Set VSM_TOKEN_FILE or VSM_TOKEN for the selected Manager URL",
        )
        .into());
    };
    Ok(vsm::admin::AdminClient::new(
        args.url.as_deref().unwrap_or("http://127.0.0.1:4317"),
        token.trim(),
    )?)
}
async fn tool(args: &Args, name: &str, input: Value, wait: bool) -> Result<Value> {
    let client = admin_client(args)?;
    let value = client
        .execute(name, input, args.idempotency_key.as_deref())
        .await?;
    if wait {
        wait_for_operation(args, &client, value).await
    } else {
        Ok(value)
    }
}
async fn wait_for_operation(
    args: &Args,
    client: &vsm::admin::AdminClient,
    value: Value,
) -> Result<Value> {
    if value.get("status").is_none() || value.get("id").is_none() {
        return Ok(value);
    }
    ensure_operation_succeeded(
        client
            .wait_operation(
                value["id"].as_str().context("Invalid operation ID")?,
                std::time::Duration::from_secs(args.wait_timeout),
            )
            .await?,
    )
}
#[derive(Debug)]
struct OperationFailed(Value);
impl std::fmt::Display for OperationFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Operation {} {}: {}. Inspect it with vsm activity ID --logs",
            self.0["id"].as_str().unwrap_or("unknown"),
            self.0["status"].as_str().unwrap_or("unknown"),
            self.0["message"].as_str().unwrap_or("No details")
        )
    }
}
impl std::error::Error for OperationFailed {}
fn ensure_operation_succeeded(value: Value) -> Result<Value> {
    if value["status"] != "succeeded" {
        return Err(OperationFailed(value).into());
    }
    Ok(value)
}
fn read(path: PathBuf) -> Result<Value> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
async fn api(
    args: &Args,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    Ok(admin_client(args)?
        .request(method.as_str(), path, body, args.idempotency_key.as_deref())
        .await?)
}
#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(e) = run(&args).await {
        let code = if let Some(error) = e.downcast_ref::<vsm::admin::AdminError>() {
            error.exit_code()
        } else if e.downcast_ref::<OperationFailed>().is_some() {
            8
        } else {
            1
        };
        if args.json {
            let error = e
                .downcast_ref::<vsm::admin::AdminError>()
                .map(|error| json!(error))
                .unwrap_or_else(|| json!({"code":"cli_error", "message": e.to_string()}));
            let mut output = json!({"error":error,"exit_code":code});
            if let Some(failure) = e.downcast_ref::<OperationFailed>() {
                output["operation"] = failure.0.clone();
                output["error"]["code"] = json!("operation_failed");
            }
            eprintln!("{}", output);
        } else {
            eprintln!("vsm: {e:#}");
        }
        std::process::exit(code);
    }
}
async fn run(args: &Args) -> Result<()> {
    let value = match &args.command {
        Commands::Mcp => { vsm::mcp::serve(admin_client(args)?).await?; return Ok(()); }
        Commands::Admin { command } => match command {
            AdminCommand::List => json!(vsm::admin::catalog()),
            AdminCommand::Schema { name } => json!(vsm::admin::catalog().into_iter().find(|item| &item.name == name).context("Unknown administration tool; use vsm admin list")?),
            AdminCommand::Run { name, input, file, wait } => {
                let input = if let Some(path) = file { read(path.clone())? }
                    else if let Some(input) = input { serde_json::from_str(input).context("--input must be a JSON object")? }
                    else { json!({}) };
                tool(args, name, input, *wait).await?
            }
        },
        Commands::Start { id, wait } => tool(args, "server_start", json!({"server_id":id}), *wait).await?,
        Commands::Stop { id, wait } => tool(args, "server_stop", json!({"server_id":id}), *wait).await?,
        Commands::Restart { id, wait } => tool(args, "server_restart", json!({"server_id":id}), *wait).await?,
        Commands::Mods { command } => match command {
            ModsCommand::List { id } => tool(args, "mods_list", json!({"server_id":id}), false).await?,
            ModsCommand::Search { query } => tool(args, "mod_search", json!({"query":query}), false).await?,
            ModsCommand::Detail { mod_id } => tool(args, "mod_detail", json!({"mod_id":mod_id}), false).await?,
            ModsCommand::Install { id, mod_id, release_id, acknowledge_unknown_compatibility, wait } => tool(args, "mod_install", json!({"server_id":id,"mod_id":mod_id,"release_id":release_id,"acknowledge_unknown_compatibility":acknowledge_unknown_compatibility}), *wait).await?,
        },
        Commands::Whitelist { id, username, remove, wait } => tool(args, if *remove { "player_unwhitelist" } else { "player_whitelist" }, json!({"server_id":id,"username":username}), *wait).await?,
        Commands::Config { id, file, wait } => if let Some(file) = file {
            tool(args, "config_set", json!({"server_id":id,"body":read(file.clone())?}), *wait).await?
        } else { tool(args, "config_get", json!({"server_id":id}), false).await? },
        Commands::Files { command } => match command {
            FilesCommand::List { id, path } => tool(args, "files_list", json!({"server_id":id,"path":path}), false).await?,
            FilesCommand::Read { id, path } => tool(args, "file_read", json!({"server_id":id,"path":path}), false).await?,
            FilesCommand::Write { id, file, wait } => tool(args, "file_write", json!({"server_id":id,"body":read(file.clone())?}), *wait).await?,
        },
        Commands::Backups { command } => match command {
            BackupsCommand::List { id } => tool(args, "backups_list", json!({"server_id":id}), false).await?,
            BackupsCommand::Create { id, wait } => tool(args, "backup_create", json!({"server_id":id}), *wait).await?,
        },
        Commands::Activity { id, logs, wait } => if let Some(id) = id {
            if *wait { ensure_operation_succeeded(admin_client(args)?.wait_operation(id, std::time::Duration::from_secs(args.wait_timeout)).await?)? }
            else { tool(args, if *logs { "operation_logs" } else { "operation_get" }, json!({"operation_id":id}), false).await? }
        } else { tool(args, "operations_list", json!({}), false).await? },
        Commands::Serve {
            data,
            bind,
            ui,
            public,
        } => {
            let address: std::net::SocketAddr = bind.parse()?;
            anyhow::ensure!(
                address.ip().is_loopback() || *public,
                "Use --public with a TLS reverse proxy to bind outside loopback"
            );
            let (c, _) = vsm::service::Controller::open(data.clone(), *public)?;
            let listener = tokio::net::TcpListener::bind(address).await?;
            eprintln!(
                "VSM controller: http://{address}\nLocal owner token file: {}",
                data.join("owner.token").display()
            );
            vsm::service::serve(c, listener, ui.clone()).await?;
            return Ok(());
        }
        Commands::Versions => json!(vsm::game_releases::catalog().await),
        Commands::Status => api(args, reqwest::Method::GET, "/summary", None).await?,
        Commands::Servers => api(args, reqwest::Method::GET, "/servers", None).await?,
        Commands::Accounts => api(args, reqwest::Method::GET, "/accounts", None).await?,
        Commands::AccountAdd { file } => {
            api(
                args,
                reqwest::Method::POST,
                "/accounts",
                Some(read(file.clone())?),
            )
            .await?
        }
        Commands::AccountValidate { id } => {
            api(
                args,
                reqwest::Method::POST,
                &format!("/accounts/{id}/validate"),
                Some(json!({})),
            )
            .await?
        }
        Commands::AccountDiscover { id } => {
            api(
                args,
                reqwest::Method::GET,
                &format!("/accounts/{id}/discover"),
                None,
            )
            .await?
        }
        Commands::AccountAdopt {
            id,
            vm_id,
            ssh_private_key,
            name,
        } => {
            let mut body = json!({ "vm_id": vm_id });
            if let Some(key) = ssh_private_key {
                body["ssh_private_key"] = json!(key);
            }
            if let Some(name) = name {
                body["name"] = json!(name);
            }
            api(
                args,
                reqwest::Method::POST,
                &format!("/accounts/{id}/adopt"),
                Some(body),
            )
            .await?
        }
        Commands::ServerConnection {
            id,
            ssh_private_key,
        } => {
            api(
                args,
                reqwest::Method::PATCH,
                &format!("/servers/{id}/connection"),
                Some(json!({ "ssh_private_key": ssh_private_key })),
            )
            .await?
        }
        Commands::ServerReconnect { id } => {
            api(
                args,
                reqwest::Method::POST,
                &format!("/servers/{id}/reconnect"),
                Some(json!({})),
            )
            .await?
        }
        Commands::Catalog { id } => {
            api(
                args,
                reqwest::Method::GET,
                &format!("/accounts/{id}/catalog"),
                None,
            )
            .await?
        }
        Commands::Quote { id, region, size } => {
            api(
                args,
                reqwest::Method::GET,
                &format!("/accounts/{id}/quote?region={region}&size={size}"),
                None,
            )
            .await?
        }
        Commands::Plan { file } => {
            api(
                args,
                reqwest::Method::POST,
                "/plans",
                Some(read(file.clone())?),
            )
            .await?
        }
        Commands::Apply {
            plan_id,
            yes,
            accept_incomplete_estimate,
        } => {
            if !yes {
                use std::io::Write;
                eprint!("Create billable resources for plan {plan_id}? Type deploy: ");
                std::io::stderr().flush()?;
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                anyhow::ensure!(answer.trim() == "deploy", "Deployment cancelled");
            }
            api(args,reqwest::Method::POST,"/deployments",Some(json!({"plan_id":plan_id,"accept_incomplete_estimate":accept_incomplete_estimate}))).await?
        }
        Commands::Action {
            id,
            action,
            body,
            wait,
        } => {
            let value = api(
                args, reqwest::Method::POST, &format!("/servers/{id}/actions/{action}"),
                Some(body.clone().map(read).transpose()?.unwrap_or(json!({}))),
            ).await?;
            if *wait { wait_for_operation(args, &admin_client(args)?, value).await? } else { value }
        }
        Commands::Operations => api(args, reqwest::Method::GET, "/operations", None).await?,
        Commands::Logs { id, follow } => {
            let mut last = String::new();
            loop {
                let value = api(
                    args,
                    reqwest::Method::GET,
                    &format!("/servers/{id}/agent/logs"),
                    None,
                )
                .await?;
                for line in value.as_array().into_iter().flatten() {
                    let at = line["at"].as_str().unwrap_or("");
                    if at > last.as_str() {
                        if args.json {
                            println!("{line}");
                        } else {
                            println!("{} {}", at, line["message"].as_str().unwrap_or(""));
                        }
                    }
                }
                if let Some(at) = value
                    .as_array()
                    .and_then(|v| v.last())
                    .and_then(|l| l["at"].as_str())
                {
                    last = at.into();
                }
                if !follow {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            return Ok(());
        }
        Commands::Request { method, path, body } => {
            api(
                args,
                method.parse()?,
                path,
                body.clone().map(read).transpose()?,
            )
            .await?
        }
        Commands::SecretSet { name, vault } => {
            use std::io::Read;
            let mut secret = String::new();
            std::io::stdin().read_to_string(&mut secret)?;
            if *vault {
                vsm::auth::vault_set(name, secret.trim())?;
            } else {
                keyring::Entry::new("vsm", name)?.set_password(secret.trim())?;
            }
            json!({"credential_ref":format!("{}:{name}",if *vault{"vault"}else{"keyring"})})
        }
        Commands::Openapi => vsm::service::api_document(),
        Commands::UserAdd { data } => {
            use std::io::Read;
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let body: Value = serde_json::from_str(&input)?;
            let db = vsm::db::Db::open(&data.join("controller.db"))?;
            let servers: Vec<String> =
                serde_json::from_value(body["servers"].clone()).unwrap_or_default();
            vsm::auth::create_user(
                &db,
                body["name"].as_str().context("name required")?,
                body["password"].as_str().context("password required")?,
                body["role"].as_str().unwrap_or("owner"),
                &servers,
                body["totp_secret"].as_str(),
            )?;
            json!({"created":true})
        }
        Commands::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(*shell, &mut Args::command(), "vsm", &mut std::io::stdout());
            return Ok(());
        }
        Commands::Probe {
            host,
            port,
            quote_id,
            location,
            players,
        } => {
            let mut samples = vec![];
            for _ in 0..5 {
                let start = std::time::Instant::now();
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    tokio::net::TcpStream::connect((host.as_str(), *port)),
                )
                .await??;
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            samples.sort_by(f64::total_cmp);
            json!({"quote_id":quote_id,"location":location,"players":players,"milliseconds":samples[2],"source":"measured","measured_at":vsm::domain::now(),"endpoint":format!("{host}:{port}"),"note":"TCP connection latency from this computer; not in-game latency."})
        }
    };
    println!(
        "{}",
        if args.json {
            serde_json::to_string(&value)?
        } else {
            serde_json::to_string_pretty(&value)?
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_and_explicit_connections_cannot_be_combined() {
        assert!(Args::try_parse_from([
            "vsm",
            "--desktop",
            "--url",
            "https://example.com",
            "servers"
        ])
        .is_err());
        assert!(
            Args::try_parse_from(["vsm", "--desktop", "--token-file", "token", "servers"]).is_err()
        );
        assert!(Args::try_parse_from([
            "vsm",
            "--token",
            "secret",
            "--token-file",
            "token",
            "servers"
        ])
        .is_err());
    }

    #[test]
    fn administration_cli_supports_common_workflows_and_global_flags() {
        let args = Args::try_parse_from([
            "vsm",
            "mods",
            "install",
            "server-id",
            "4425",
            "55847",
            "--acknowledge-unknown-compatibility",
            "--wait",
            "--json",
            "--desktop",
            "--idempotency-key",
            "request-1",
        ])
        .unwrap();
        assert!(args.desktop && args.json);
        assert_eq!(args.idempotency_key.as_deref(), Some("request-1"));
        assert!(matches!(
            args.command,
            Commands::Mods {
                command: ModsCommand::Install {
                    acknowledge_unknown_compatibility: true,
                    wait: true,
                    ..
                }
            }
        ));
        assert!(Args::try_parse_from([
            "vsm",
            "admin",
            "run",
            "config_set",
            "--input",
            "{}",
            "--file",
            "input.json"
        ])
        .is_err());
        assert!(Args::try_parse_from(["vsm", "activity", "--wait"]).is_err());
        assert!(Args::try_parse_from(["vsm", "--wait-timeout", "0", "servers"]).is_err());
        assert!(Args::try_parse_from(["vsm", "--wait-timeout", "601", "servers"]).is_err());
    }

    #[test]
    fn failed_operation_has_nonzero_shared_exit_code_and_operation_id() {
        let error = ensure_operation_succeeded(
            json!({"id":"operation-123", "status":"failed", "message":"Disk full"}),
        )
        .unwrap_err();
        let failure = error.downcast_ref::<OperationFailed>().unwrap();
        assert_eq!(failure.0["id"], "operation-123");
        assert!(failure.to_string().contains("operation-123"));
        assert!(
            ensure_operation_succeeded(json!({"id":"operation-123", "status":"succeeded"})).is_ok()
        );
    }

    #[test]
    fn discovery_supports_global_json_after_command() {
        let args =
            Args::try_parse_from(["vsm", "account-discover", "azure-account", "--json"]).unwrap();
        assert!(args.json);
        assert!(matches!(args.command, Commands::AccountDiscover { id } if id == "azure-account"));
    }

    #[test]
    fn adoption_can_restore_cloud_inventory_without_a_private_key() {
        let args = Args::try_parse_from([
            "vsm",
            "--json",
            "account-adopt",
            "azure-account",
            "--vm-id",
            "/subscriptions/sub/resourceGroups/group/providers/Microsoft.Compute/virtualMachines/vm",
        ])
        .unwrap();
        assert!(args.json);
        assert!(matches!(
            args.command,
            Commands::AccountAdopt {
                ssh_private_key: None,
                ..
            }
        ));
        assert!(Args::try_parse_from(["vsm", "account-adopt", "azure-account"]).is_err());
    }

    #[test]
    fn recovered_connection_requires_a_key_path() {
        assert!(Args::try_parse_from(["vsm", "server-connection", "server"]).is_err());
        let args = Args::try_parse_from([
            "vsm",
            "server-connection",
            "server",
            "--ssh-private-key",
            "C:\\Users\\Player Name\\.ssh\\vsm",
            "--json",
        ])
        .unwrap();
        assert!(args.json);
        assert!(
            matches!(args.command, Commands::ServerConnection { ssh_private_key, .. }
            if ssh_private_key == "C:\\Users\\Player Name\\.ssh\\vsm")
        );
    }

    #[test]
    fn reconnect_accepts_server_id_and_json() {
        let args = Args::try_parse_from(["vsm", "server-reconnect", "server", "--json"]).unwrap();
        assert!(args.json);
        assert!(matches!(args.command, Commands::ServerReconnect { id } if id == "server"));
    }
}
