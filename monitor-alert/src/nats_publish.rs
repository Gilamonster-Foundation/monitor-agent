use monitor_core::alert::{Alert, AlertDispatcher};

/// Publishes alert events to a NATS subject so downstream consumers
/// (monty-tui, drake-herald, custom handlers) can react.
pub struct NatsPublishDispatcher {
    subject: String,
    client: Option<async_nats::Client>,
}

impl NatsPublishDispatcher {
    /// Creates a dispatcher with an already-connected NATS client.
    pub fn new(subject: impl Into<String>, client: async_nats::Client) -> Self {
        Self {
            subject: subject.into(),
            client: Some(client),
        }
    }

    /// Creates a no-op dispatcher (no NATS connection configured).
    pub fn disabled() -> Self {
        Self {
            subject: String::new(),
            client: None,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.client.is_some() && !self.subject.is_empty()
    }

    async fn publish(&self, event: &str, alert: &Alert) -> anyhow::Result<()> {
        let Some(ref client) = self.client else {
            return Ok(());
        };
        if self.subject.is_empty() {
            return Ok(());
        }
        let payload = serde_json::to_vec(&serde_json::json!({
            "event": event,
            "rule": alert.rule_name,
            "target": alert.target,
            "metric": alert.metric.as_str(),
            "value": alert.value,
            "severity": format!("{}", alert.severity),
            "message": alert.message,
            "uuid": alert.uuid,
        }))?;
        client
            .publish(self.subject.clone(), payload.into())
            .await
            .map_err(|e| anyhow::anyhow!("NATS publish failed: {e}"))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl AlertDispatcher for NatsPublishDispatcher {
    fn name(&self) -> &str {
        "nats-publish"
    }

    async fn fire(&self, alert: &Alert) -> anyhow::Result<()> {
        self.publish("firing", alert).await
    }

    async fn resolve(&self, alert: &Alert) -> anyhow::Result<()> {
        self.publish("resolved", alert).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_dispatcher_is_no_op() {
        use monitor_core::alert::{Alert, AlertId, AlertState, Severity};
        use monitor_core::metrics::MetricPath;
        use uuid::Uuid;
        let d = NatsPublishDispatcher::disabled();
        assert!(!d.is_configured());
        let alert = Alert {
            id: AlertId::for_rule("gnuc", "test"),
            uuid: Uuid::new_v4(),
            rule_name: "test".into(),
            target: "gnuc".into(),
            metric: MetricPath::new("cpu.percent"),
            value: 90.0,
            severity: Severity::Warn,
            state: AlertState::Firing,
            message: "test".into(),
            fired_at_secs: None,
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        };
        d.fire(&alert).await.unwrap();
        d.resolve(&alert).await.unwrap();
    }
}
