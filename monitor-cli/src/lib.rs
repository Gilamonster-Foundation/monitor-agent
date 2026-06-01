mod daemon;
mod doctor;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "monitor-agent",
    about = "Systems monitor and alert daemon with ratatui TUI",
    version
)]
pub struct Cli {
    /// Path to config file (overrides MONITOR_CONFIG and search path).
    #[arg(short, long, env = "MONITOR_CONFIG")]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the background monitoring daemon.
    Daemon,
    /// Launch the TUI (attaches to running daemon or runs in-process).
    Tui,
    /// Print current state (active alerts + target health) as JSON.
    Status,
    /// List active alerts.
    Alerts,
    /// Check collectors, config validity, and daemon connectivity.
    Doctor,
    /// Print the resolved configuration.
    Config,
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let cfg = monitor_core::Config::resolve()?;

    match cli.command.unwrap_or(Command::Tui) {
        Command::Daemon => daemon::run_daemon(cfg).await,
        Command::Tui => run_tui(cfg).await,
        Command::Status => print_status(cfg).await,
        Command::Alerts => print_alerts(cfg).await,
        Command::Doctor => doctor::run_doctor(cfg).await,
        Command::Config => {
            println!("{}", toml::to_string_pretty(&cfg)?);
            Ok(())
        }
    }
}

async fn run_tui(cfg: monitor_core::Config) -> anyhow::Result<()> {
    use monitor_tui::Event;
    use tokio::sync::mpsc;

    let (tx, rx) = mpsc::channel::<Event>(256);

    // Start collectors in background and feed the TUI's event channel.
    daemon::spawn_collectors(cfg, tx).await?;

    monitor_tui::run(rx).await
}

async fn print_status(cfg: monitor_core::Config) -> anyhow::Result<()> {
    let collectors = daemon::build_collectors(&cfg).await?;
    let mut results = Vec::new();
    for c in &collectors {
        match c.collect().await {
            Ok(m) => results.push(serde_json::json!({
                "target": m.target,
                "metrics": m.values.len(),
                "status": "ok",
            })),
            Err(e) => results.push(serde_json::json!({
                "target": c.name(),
                "status": "error",
                "error": e.to_string(),
            })),
        }
    }
    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}

async fn print_alerts(cfg: monitor_core::Config) -> anyhow::Result<()> {
    use monitor_core::alert::AlertEngine;

    let rules = cfg.alert_rules();
    let mut engine = AlertEngine::new(rules);
    let collectors = daemon::build_collectors(&cfg).await?;

    for c in &collectors {
        if let Ok(metrics) = c.collect().await {
            engine.evaluate(&metrics);
        }
    }

    let active: Vec<serde_json::Value> = engine
        .active_alerts()
        .iter()
        .map(|a| {
            serde_json::json!({
                "rule": a.rule_name,
                "target": a.target,
                "metric": a.metric.as_str(),
                "value": a.value,
                "severity": format!("{}", a.severity),
                "message": a.message,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&active)?);
    Ok(())
}
