use monitor_core::alert::{Alert, AlertDispatcher};

/// POSTs alert JSON to a configured HTTP endpoint.
pub struct WebhookDispatcher {
    url: String,
    client: reqwest::Client,
}

impl WebhookDispatcher {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.url.is_empty()
    }
}

#[async_trait::async_trait]
impl AlertDispatcher for WebhookDispatcher {
    fn name(&self) -> &str {
        "webhook"
    }

    async fn fire(&self, alert: &Alert) -> anyhow::Result<()> {
        if !self.is_configured() {
            return Ok(());
        }
        let body = serde_json::json!({
            "event": "firing",
            "rule": alert.rule_name,
            "target": alert.target,
            "metric": alert.metric.as_str(),
            "value": alert.value,
            "severity": format!("{}", alert.severity),
            "message": alert.message,
        });
        self.client.post(&self.url).json(&body).send().await?;
        Ok(())
    }

    async fn resolve(&self, alert: &Alert) -> anyhow::Result<()> {
        if !self.is_configured() {
            return Ok(());
        }
        let body = serde_json::json!({
            "event": "resolved",
            "rule": alert.rule_name,
            "target": alert.target,
            "metric": alert.metric.as_str(),
            "severity": format!("{}", alert.severity),
            "message": alert.message,
        });
        self.client.post(&self.url).json(&body).send().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::alert::{Alert, AlertId, AlertState, Severity};
    use monitor_core::metrics::MetricPath;
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_alert() -> Alert {
        Alert {
            id: AlertId::for_rule("gnuc", "high-cpu"),
            uuid: Uuid::new_v4(),
            rule_name: "high-cpu".into(),
            target: "gnuc".into(),
            metric: MetricPath::new("cpu.percent"),
            value: 91.0,
            severity: Severity::Warn,
            state: AlertState::Firing,
            message: "gnuc: CPU at 91%".into(),
            fired_at_secs: None,
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        }
    }

    #[tokio::test]
    async fn webhook_fires_post_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/alert"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let d = WebhookDispatcher::new(format!("{}/alert", server.uri()));
        d.fire(&test_alert()).await.unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn webhook_empty_url_is_no_op() {
        let d = WebhookDispatcher::new("");
        d.fire(&test_alert()).await.unwrap();
    }
}
