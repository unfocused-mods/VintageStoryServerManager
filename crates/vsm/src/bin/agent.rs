use clap::Parser;
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "/var/lib/vsm")]
    root: std::path::PathBuf,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    vsm::agent::run(Args::parse().root).await
}
