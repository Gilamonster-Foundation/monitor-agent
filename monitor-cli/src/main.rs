use clap::Parser;
use monitor_cli::Cli;
#[cfg(feature = "gui")]
use monitor_cli::Command;

fn main() -> anyhow::Result<()> {
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

    // The GUI must own the main thread (eframe/winit); collectors run on a
    // background tokio runtime. Every other command runs on the async path.
    #[cfg(feature = "gui")]
    if matches!(cli.command, Some(Command::Gui)) {
        return monitor_cli::run_gui();
    }

    tokio::runtime::Runtime::new()?.block_on(monitor_cli::run(cli))
}
