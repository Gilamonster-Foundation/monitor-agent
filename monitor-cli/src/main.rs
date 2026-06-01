use clap::Parser;
use monitor_cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("monitor_agent=info".parse()?)
                .add_directive("monitor_cli=info".parse()?)
                .add_directive("monitor_collect=info".parse()?)
                .add_directive("monitor_alert=info".parse()?)
                .add_directive("monitor_tui=info".parse()?)
                .add_directive("monitor_core=info".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    monitor_cli::run(cli).await
}
