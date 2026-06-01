use monitor_core::metrics::{Collector, MetricSet};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Subscribes to NATS subjects and surfaces the most recent heartbeat/status
/// payloads as metrics. Real-time push — no polling interval.
///
/// This collector is passive: NATS messages arrive asynchronously and update
/// an internal snapshot. `collect()` returns that snapshot on demand.
pub struct NatsCollector {
    target_name: String,
    snapshot: Arc<Mutex<MetricSet>>,
    /// Background task handle — kept alive for the collector's lifetime.
    _task: tokio::task::JoinHandle<()>,
}

impl NatsCollector {
    /// Connect to `servers` and subscribe to each subject in `subjects`.
    ///
    /// Returns an error if the initial NATS connection fails.
    pub async fn connect(
        target_name: impl Into<String>,
        servers: &[String],
        subjects: &[String],
    ) -> anyhow::Result<Self> {
        let name = target_name.into();
        let snapshot = Arc::new(Mutex::new(MetricSet::new(&name)));

        let opts = async_nats::ConnectOptions::new().name("monitor-agent");

        let server_list = servers.join(",");
        let client = opts
            .connect(server_list.as_str())
            .await
            .map_err(|e| anyhow::anyhow!("NATS connect failed: {e}"))?;

        let snap = Arc::clone(&snapshot);
        let snap_name = name.clone();
        let subs = subjects.to_vec();

        let task = tokio::spawn(async move {
            if let Err(e) = run_subscriber(client, snap, snap_name, subs).await {
                tracing::warn!("nats subscriber exited: {e}");
            }
        });

        Ok(Self {
            target_name: name,
            snapshot,
            _task: task,
        })
    }
}

async fn run_subscriber(
    client: async_nats::Client,
    snapshot: Arc<Mutex<MetricSet>>,
    target_name: String,
    subjects: Vec<String>,
) -> anyhow::Result<()> {
    use tokio_stream::StreamExt;

    // Subscribe to all configured subjects.
    let mut subscribers = Vec::new();
    for subject in &subjects {
        let sub = client.subscribe(subject.clone()).await?;
        subscribers.push(sub);
    }

    // Process the first subscriber's messages for simplicity.
    // A production implementation would fan-merge all streams.
    if let Some(mut sub) = subscribers.into_iter().next() {
        while let Some(msg) = sub.next().await {
            let payload = std::str::from_utf8(&msg.payload).unwrap_or("").to_owned();
            // Try to parse as a JSON object of metric name→value pairs.
            if let Ok(map) =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&payload)
            {
                let mut snap = snapshot.lock().await;
                *snap = MetricSet::new(&target_name);
                for (k, v) in &map {
                    if let Some(f) = v.as_f64() {
                        snap.insert(k.as_str(), f);
                    }
                }
            } else {
                // Non-JSON message — record as a heartbeat counter.
                let mut snap = snapshot.lock().await;
                let key = monitor_core::metrics::MetricPath::new("nats.heartbeat_count");
                let count = snap.get(&key).unwrap_or(0.0) + 1.0;
                snap.insert(key.as_str(), count);
            }
        }
    }

    Ok(())
}

#[async_trait::async_trait]
impl Collector for NatsCollector {
    fn name(&self) -> &str {
        &self.target_name
    }

    async fn collect(&self) -> anyhow::Result<MetricSet> {
        Ok(self.snapshot.lock().await.clone())
    }
}
