use monitor_alert::{
    NatsPublishDispatcher, TerminalBellDispatcher, VoiceDispatcher, WebhookDispatcher,
};
use monitor_collect::{LocalCollector, NatsCollector, PrometheusCollector, SshCollector};
use monitor_core::{
    alert::{AlertDispatcher, AlertEngine},
    config::{Config, TargetKind},
    metrics::Collector,
    Config as MonitorConfig,
};
use monitor_tui::Event;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Build the list of collectors from config.
pub async fn build_collectors(cfg: &MonitorConfig) -> anyhow::Result<Vec<Arc<dyn Collector>>> {
    let mut collectors: Vec<Arc<dyn Collector>> = Vec::new();

    for target in &cfg.targets {
        match target.kind {
            TargetKind::Local => {
                collectors.push(Arc::new(LocalCollector::new(&target.name)));
            }
            TargetKind::Prometheus => {
                let endpoint = target.endpoint.clone().ok_or_else(|| {
                    anyhow::anyhow!("prometheus target '{}' missing endpoint", target.name)
                })?;
                collectors.push(Arc::new(PrometheusCollector::new(&target.name, endpoint)));
            }
            TargetKind::Ssh => {
                let host = target
                    .host
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("ssh target '{}' missing host", target.name))?;
                collectors.push(Arc::new(SshCollector::new(
                    &target.name,
                    host,
                    target.user.clone(),
                    target.key.clone(),
                )));
            }
        }
    }

    // NATS collector — one shared subscriber for all NATS subjects.
    if let Some(ref nats_cfg) = cfg.nats {
        if !nats_cfg.servers.is_empty() && !nats_cfg.subjects.is_empty() {
            match NatsCollector::connect("nats", &nats_cfg.servers, &nats_cfg.subjects).await {
                Ok(c) => collectors.push(Arc::new(c)),
                Err(e) => tracing::warn!("NATS collector failed to connect: {e} — skipping"),
            }
        }
    }

    Ok(collectors)
}

/// Build the list of alert dispatchers from config.
async fn build_dispatchers(cfg: &MonitorConfig) -> Vec<Arc<dyn AlertDispatcher>> {
    let mut dispatchers: Vec<Arc<dyn AlertDispatcher>> = Vec::new();

    dispatchers.push(Arc::new(TerminalBellDispatcher::new(
        cfg.notify.terminal_bell,
    )));

    if cfg.notify.voice {
        dispatchers.push(Arc::new(VoiceDispatcher::new(&cfg.notify.voice_engine)));
    }

    if !cfg.notify.webhook.is_empty() {
        dispatchers.push(Arc::new(WebhookDispatcher::new(&cfg.notify.webhook)));
    }

    if !cfg.notify.nats_subject.is_empty() {
        // Try to connect to NATS for publishing.
        if let Some(ref nats_cfg) = cfg.nats {
            match async_nats::connect(nats_cfg.servers.join(",")).await {
                Ok(client) => {
                    dispatchers.push(Arc::new(NatsPublishDispatcher::new(
                        &cfg.notify.nats_subject,
                        client,
                    )));
                }
                Err(e) => {
                    tracing::warn!("NATS publish dispatcher: connect failed: {e}");
                }
            }
        }
    }

    dispatchers
}

/// Spawn background tasks that collect metrics, evaluate rules, and dispatch.
/// Sends `Event`s to the TUI channel.
pub async fn spawn_collectors(cfg: Config, tx: mpsc::Sender<Event>) -> anyhow::Result<()> {
    let collectors = build_collectors(&cfg).await?;
    let rules = cfg.alert_rules();
    let dispatchers = build_dispatchers(&cfg).await;

    // Tick task — sends periodic clock events.
    let tick_tx = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            if tick_tx.send(Event::Tick).await.is_err() {
                break;
            }
        }
    });

    // Signal daemon is running (in-process mode).
    let _ = tx.send(Event::DaemonConnected).await;

    // Collector / alert-engine task.
    tokio::spawn(async move {
        let mut engine = AlertEngine::new(rules);
        let poll_secs = 2u64;
        let mut interval = tokio::time::interval(Duration::from_secs(poll_secs));

        loop {
            interval.tick().await;

            for collector in &collectors {
                match collector.collect().await {
                    Ok(metrics) => {
                        // Send raw metrics to TUI.
                        let _ = tx.send(Event::MetricsUpdate(metrics.clone())).await;

                        // Evaluate alert rules.
                        let transitions = engine.evaluate(&metrics);
                        for alert in transitions {
                            if alert.is_firing() {
                                // Dispatch notifications.
                                for d in &dispatchers {
                                    if let Err(e) = d.fire(&alert).await {
                                        tracing::warn!("dispatcher '{}' fire error: {e}", d.name());
                                    }
                                }
                                let _ = tx.send(Event::AlertFired(alert)).await;
                            } else if alert.is_resolved() {
                                for d in &dispatchers {
                                    if let Err(e) = d.resolve(&alert).await {
                                        tracing::warn!(
                                            "dispatcher '{}' resolve error: {e}",
                                            d.name()
                                        );
                                    }
                                }
                                let _ = tx.send(Event::AlertResolved(alert)).await;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("collector '{}' error: {e}", collector.name());
                    }
                }
            }
        }
    });

    Ok(())
}

/// Long-running daemon loop (no TUI — collects + dispatches forever).
pub async fn run_daemon(cfg: Config) -> anyhow::Result<()> {
    tracing::info!("monitor-agent daemon starting");

    let (tx, mut rx) = mpsc::channel::<Event>(256);
    spawn_collectors(cfg, tx).await?;

    // Drain events — the daemon doesn't need to render them, just keep running.
    while let Some(event) = rx.recv().await {
        match event {
            Event::AlertFired(ref a) => {
                tracing::info!("ALERT FIRING: {}", a.message);
            }
            Event::AlertResolved(ref a) => {
                tracing::info!("ALERT RESOLVED: {}", a.message);
            }
            _ => {}
        }
    }

    Ok(())
}
