use monitor_core::alert::{Alert, AlertDispatcher, AlertState, Severity};

/// Writes an alert line to stderr and rings the terminal bell on Warn/Critical.
pub struct TerminalBellDispatcher {
    bell_enabled: bool,
}

impl TerminalBellDispatcher {
    pub fn new(bell_enabled: bool) -> Self {
        Self { bell_enabled }
    }
}

#[async_trait::async_trait]
impl AlertDispatcher for TerminalBellDispatcher {
    fn name(&self) -> &str {
        "terminal-bell"
    }

    async fn fire(&self, alert: &Alert) -> anyhow::Result<()> {
        let icon = match alert.severity {
            Severity::Critical => "●",
            Severity::Warn => "⚠",
            Severity::Info => "ℹ",
        };
        eprintln!(
            "\r{icon} [{sev}] {msg}",
            sev = alert.severity,
            msg = alert.message
        );

        if self.bell_enabled && alert.severity >= Severity::Warn {
            // BEL character — rings the terminal bell.
            eprint!("\x07");
        }
        Ok(())
    }

    async fn resolve(&self, alert: &Alert) -> anyhow::Result<()> {
        debug_assert_eq!(alert.state, AlertState::Resolved);
        eprintln!("\r✓ [resolved] {}", alert.message);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::alert::{Alert, AlertId, AlertState, Severity};
    use monitor_core::metrics::MetricPath;
    use uuid::Uuid;

    fn test_alert(severity: Severity) -> Alert {
        Alert {
            id: AlertId::for_rule("gnuc", "high-cpu"),
            uuid: Uuid::new_v4(),
            rule_name: "high-cpu".into(),
            target: "gnuc".into(),
            metric: MetricPath::new("cpu.percent"),
            value: 90.0,
            severity,
            state: AlertState::Firing,
            message: "gnuc: CPU at 90%".into(),
            fired_at_secs: None,
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        }
    }

    #[tokio::test]
    async fn terminal_dispatcher_fires_without_error() {
        let d = TerminalBellDispatcher::new(false);
        d.fire(&test_alert(Severity::Warn)).await.unwrap();
        d.fire(&test_alert(Severity::Critical)).await.unwrap();
    }

    #[tokio::test]
    async fn terminal_dispatcher_resolves_without_error() {
        let d = TerminalBellDispatcher::new(false);
        let mut a = test_alert(Severity::Info);
        a.state = AlertState::Resolved;
        d.resolve(&a).await.unwrap();
    }
}
