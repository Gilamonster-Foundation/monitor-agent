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
    /// Launch the egui GUI (caster station). Requires the `gui` build feature.
    #[cfg(feature = "gui")]
    Gui,
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
        // The GUI is dispatched from `main()` on the main thread (eframe/winit
        // require it), before the async runtime — so it never reaches here.
        #[cfg(feature = "gui")]
        Command::Gui => anyhow::bail!("the `gui` command must be launched on the main thread"),
    }
}

async fn run_tui(cfg: monitor_core::Config) -> anyhow::Result<()> {
    use monitor_presence::DataEvent;
    use tokio::sync::mpsc;

    let splash_timeout = cfg.tui.splash_timeout_secs;
    let (tx, rx) = mpsc::channel::<DataEvent>(256);

    daemon::spawn_collectors(cfg, tx).await?;

    monitor_tui::run(rx, splash_timeout).await
}

/// Launch the egui GUI (caster station) on the calling (main) thread.
///
/// `eframe`/`winit` require the main thread, so this is dispatched from `main`
/// *before* the async runtime — unlike every other command. Collectors run on a
/// background tokio runtime whose data feed a pump task drains into the shared
/// presence; the GUI reads snapshots of that presence each frame.
///
/// # Errors
///
/// Returns an error if config resolution, the runtime, the collectors, or the
/// GUI window / graphics backend fail to start.
#[cfg(feature = "gui")]
pub fn run_gui() -> anyhow::Result<()> {
    use monitor_presence::{DataEvent, SharedPresence};
    use tokio::sync::mpsc;

    let cfg = monitor_core::Config::resolve()?;
    let rt = tokio::runtime::Runtime::new()?;
    let shared = SharedPresence::new();

    // Collector feed + a pump that drains data events into the shared presence.
    let (tx, mut rx) = mpsc::channel::<DataEvent>(256);
    rt.block_on(async { daemon::spawn_collectors(cfg, tx).await })?;
    let pump = shared.clone();
    rt.spawn(async move {
        while let Some(ev) = rx.recv().await {
            pump.apply(ev);
        }
    });

    // eframe owns this (the main) thread; keep the runtime alive in scope so the
    // collector + pump tasks keep feeding the shared presence while it runs.
    let _guard = rt.enter();
    monitor_gui::run(shared).map_err(|e| anyhow::anyhow!("gui failed: {e:?}"))
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
