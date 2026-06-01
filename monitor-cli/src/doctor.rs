use monitor_core::config::Config;

pub async fn run_doctor(cfg: Config) -> anyhow::Result<()> {
    println!("monitor-agent doctor");
    println!("====================");

    // Config check.
    println!("\n[config]");
    println!("  targets:    {}", cfg.targets.len());
    println!("  rules:      {}", cfg.rules.len());
    println!(
        "  nats:       {}",
        if cfg.nats.is_some() {
            "configured"
        } else {
            "not configured"
        }
    );
    println!(
        "  voice:      {}",
        if cfg.notify.voice {
            &cfg.notify.voice_engine
        } else {
            "disabled"
        }
    );
    println!(
        "  webhook:    {}",
        if cfg.notify.webhook.is_empty() {
            "not configured"
        } else {
            &cfg.notify.webhook
        }
    );

    // Collector health.
    println!("\n[collectors]");
    let collectors = crate::daemon::build_collectors(&cfg).await?;
    for c in &collectors {
        match c.collect().await {
            Ok(m) => println!("  ✓  {}  ({} metrics)", c.name(), m.values.len()),
            Err(e) => println!("  ✗  {}  ERROR: {e}", c.name()),
        }
    }

    // Voice engine detection.
    println!("\n[voice]");
    if cfg.notify.voice {
        let engine = monitor_alert::voice::VoiceEngine::detect();
        println!("  detected engine: {engine:?}");
        if engine == monitor_alert::voice::VoiceEngine::Disabled {
            println!("  (no TTS binary found — install piper, espeak-ng, or use macOS 'say')");
        }
    } else {
        println!("  voice disabled in config");
    }

    println!("\n[done]");
    Ok(())
}
