use anyhow::Context;
use monitor_core::metrics::{Collector, MetricSet};
use serde::Deserialize;
use std::collections::HashMap;

/// Options for [`PrometheusCollector`], for endpoints behind a TLS ingress
/// reached by IP (e.g. over Tailscale) and non-default series labeling.
#[derive(Debug, Clone, Default)]
pub struct PrometheusOptions {
    /// HTTP `Host` header to send — route an ingress by IP, no DNS needed.
    pub host_header: Option<String>,
    /// Accept invalid/self-signed TLS certs (internal endpoint over a tunnel).
    pub insecure_tls: bool,
    /// Raw PromQL label matcher selecting this target's series, e.g.
    /// `instance_name="nuc"`. Defaults to `instance=~"<name>.*"`.
    pub instance_matcher: Option<String>,
}

/// Polls a Prometheus HTTP API for node_exporter metrics.
///
/// Uses standard queries already used by gila-monitor-tui:
/// `node_cpu_seconds_total`, `node_memory_*`, `node_disk_*`, `DCGM_FI_*`.
pub struct PrometheusCollector {
    target_name: String,
    endpoint: String,
    client: reqwest::Client,
    host_header: Option<String>,
    instance_matcher: Option<String>,
}

impl PrometheusCollector {
    pub fn new(target_name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self::with_options(target_name, endpoint, PrometheusOptions::default())
    }

    pub fn with_options(
        target_name: impl Into<String>,
        endpoint: impl Into<String>,
        opts: PrometheusOptions,
    ) -> Self {
        let client = if opts.insecure_tls {
            reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .unwrap_or_default()
        } else {
            reqwest::Client::new()
        };
        Self {
            target_name: target_name.into(),
            endpoint: endpoint.into().trim_end_matches('/').to_owned(),
            client,
            host_header: opts.host_header,
            instance_matcher: opts.instance_matcher,
        }
    }

    /// The PromQL label matcher selecting this target's series.
    fn selector(&self) -> String {
        self.instance_matcher
            .clone()
            .unwrap_or_else(|| format!(r#"instance=~"{}.*""#, self.target_name))
    }

    async fn query(&self, promql: &str) -> anyhow::Result<Vec<PromSample>> {
        let url = format!("{}/api/v1/query", self.endpoint);
        let mut req = self.client.get(&url).query(&[("query", promql)]);
        if let Some(ref host) = self.host_header {
            req = req.header(reqwest::header::HOST, host);
        }
        let resp: PromResponse = req
            .send()
            .await
            .context("prometheus request failed")?
            .json()
            .await
            .context("prometheus response parse failed")?;

        if resp.status != "success" {
            anyhow::bail!("prometheus returned status: {}", resp.status);
        }

        Ok(resp.data.result)
    }
}

#[async_trait::async_trait]
impl Collector for PrometheusCollector {
    fn name(&self) -> &str {
        &self.target_name
    }

    async fn collect(&self) -> anyhow::Result<MetricSet> {
        let mut m = MetricSet::new(&self.target_name);
        let sel = self.selector();

        // CPU utilization — 1 - idle rate averaged across all cores.
        if let Ok(samples) = self
            .query(&format!(
                r#"100 - (avg by(instance) (rate(node_cpu_seconds_total{{mode="idle",{sel}}}[2m])) * 100)"#
            ))
            .await
        {
            if let Some(s) = samples.first() {
                if let Some(v) = s.parse_value() {
                    m.insert("cpu.percent", v);
                }
            }
        }

        // Memory utilization.
        if let Ok(samples) = self
            .query(&format!(
                r#"100 * (1 - node_memory_MemAvailable_bytes{{{sel}}} / node_memory_MemTotal_bytes{{{sel}}})"#
            ))
            .await
        {
            if let Some(s) = samples.first() {
                if let Some(v) = s.parse_value() {
                    m.insert("memory.percent", v);
                }
            }
        }

        // Disk utilization — worst mount.
        if let Ok(samples) = self
            .query(&format!(
                r#"max by(instance) (100 * (1 - node_filesystem_avail_bytes{{{sel},fstype!~"tmpfs|overlay"}} / node_filesystem_size_bytes{{{sel},fstype!~"tmpfs|overlay"}}))"#
            ))
            .await
        {
            if let Some(s) = samples.first() {
                if let Some(v) = s.parse_value() {
                    m.insert("disk.used_pct", v);
                }
            }
        }

        // GPU utilization via DCGM exporter (optional).
        if let Ok(samples) = self
            .query(&format!(r#"DCGM_FI_DEV_GPU_UTIL{{{sel}}}"#))
            .await
        {
            if let Some(s) = samples.first() {
                if let Some(v) = s.parse_value() {
                    m.insert("gpu.util_pct", v);
                }
            }
        }

        // Network RX/TX bytes/sec.
        for (metric_path, promql) in [
            (
                "net.rx_bytes_sec",
                format!(r#"sum by(instance)(rate(node_network_receive_bytes_total{{{sel}}}[2m]))"#),
            ),
            (
                "net.tx_bytes_sec",
                format!(
                    r#"sum by(instance)(rate(node_network_transmit_bytes_total{{{sel}}}[2m]))"#
                ),
            ),
        ] {
            if let Ok(samples) = self.query(&promql).await {
                if let Some(s) = samples.first() {
                    if let Some(v) = s.parse_value() {
                        m.insert(metric_path, v);
                    }
                }
            }
        }

        Ok(m)
    }
}

// ---------------------------------------------------------------------------
// Prometheus API response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PromResponse {
    status: String,
    data: PromData,
}

#[derive(Debug, Deserialize)]
struct PromData {
    result: Vec<PromSample>,
}

#[derive(Debug, Deserialize)]
struct PromSample {
    metric: HashMap<String, String>,
    value: (f64, String), // (timestamp, value_string)
}

impl PromSample {
    fn parse_value(&self) -> Option<f64> {
        self.value.1.parse().ok()
    }

    #[allow(dead_code)]
    fn label(&self, key: &str) -> Option<&str> {
        self.metric.get(key).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn prom_response(value: f64) -> serde_json::Value {
        serde_json::json!({
            "status": "success",
            "data": {
                "resultType": "vector",
                "result": [{
                    "metric": {"instance": "gnuc:9100"},
                    "value": [1_700_000_000.0, value.to_string()]
                }]
            }
        })
    }

    #[test]
    fn selector_defaults_to_instance_prefix() {
        let c = PrometheusCollector::new("nuc", "http://x");
        assert_eq!(c.selector(), r#"instance=~"nuc.*""#);
    }

    #[test]
    fn selector_uses_custom_matcher_when_set() {
        let c = PrometheusCollector::with_options(
            "nuc",
            "http://x",
            PrometheusOptions {
                instance_matcher: Some(r#"instance_name="nuc""#.into()),
                insecure_tls: true,
                host_header: Some("prometheus.home.lab".into()),
            },
        );
        assert_eq!(c.selector(), r#"instance_name="nuc""#);
    }

    #[tokio::test]
    async fn prometheus_collector_parses_cpu() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(prom_response(42.0)))
            .mount(&server)
            .await;

        let collector = PrometheusCollector::new("gnuc", server.uri());
        let metrics = collector.collect().await.expect("collect failed");
        // At least one metric should have been populated.
        assert!(!metrics.values.is_empty());
    }

    #[tokio::test]
    async fn prometheus_collector_tolerates_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let collector = PrometheusCollector::new("gnuc", server.uri());
        // Individual query failures are tolerated — collect() succeeds with empty metrics.
        let result = collector.collect().await;
        assert!(
            result.is_ok(),
            "collect() should not propagate individual query errors"
        );
        assert!(
            result.unwrap().values.is_empty(),
            "no metrics on 500 responses"
        );
    }
}
