use anyhow::Context;
use monitor_core::metrics::{Collector, MetricSet};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};
use tokio::sync::Mutex;

/// Collects metrics from the local machine via the `sysinfo` crate.
///
/// Also attempts to read GPU metrics from `nvidia-smi` if available.
pub struct LocalCollector {
    target_name: String,
    system: Mutex<System>,
    disks: Mutex<Disks>,
    networks: Mutex<Networks>,
}

impl LocalCollector {
    pub fn new(target_name: impl Into<String>) -> Self {
        let system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        Self {
            target_name: target_name.into(),
            system: Mutex::new(system),
            disks: Mutex::new(Disks::new_with_refreshed_list()),
            networks: Mutex::new(Networks::new_with_refreshed_list()),
        }
    }
}

#[async_trait::async_trait]
impl Collector for LocalCollector {
    fn name(&self) -> &str {
        &self.target_name
    }

    async fn collect(&self) -> anyhow::Result<MetricSet> {
        let mut m = MetricSet::new(&self.target_name);

        // Refresh system data.
        {
            let mut sys = self.system.lock().await;
            sys.refresh_cpu_all();
            sys.refresh_memory();

            // CPU — global average across all cores.
            let cpu_pct = sys.global_cpu_usage() as f64;
            m.insert("cpu.percent", cpu_pct);

            // Per-core CPU.
            for (i, cpu) in sys.cpus().iter().enumerate() {
                m.insert(format!("cpu.core.{i}.percent"), cpu.cpu_usage() as f64);
            }

            // Memory.
            let total = sys.total_memory() as f64;
            let used = (sys.total_memory() - sys.available_memory()) as f64;
            if total > 0.0 {
                m.insert("memory.percent", (used / total) * 100.0);
            }
            m.insert_with_unit("memory.used_bytes", used, "bytes");
            m.insert_with_unit("memory.total_bytes", total, "bytes");

            // Load average (Unix only — 0.0 on Windows).
            let load = System::load_average();
            m.insert("load.1m", load.one);
            m.insert("load.5m", load.five);
            m.insert("load.15m", load.fifteen);
        }

        // Disk usage.
        {
            let mut disks = self.disks.lock().await;
            disks.refresh(true);
            let mut worst_pct: f64 = 0.0;
            for disk in disks.list() {
                let total = disk.total_space() as f64;
                let avail = disk.available_space() as f64;
                if total == 0.0 {
                    continue;
                }
                let used_pct = ((total - avail) / total) * 100.0;
                worst_pct = worst_pct.max(used_pct);
                let mount = disk.mount_point().to_string_lossy();
                let safe = mount.replace('/', "_").trim_start_matches('_').to_owned();
                let safe = if safe.is_empty() {
                    "root".to_owned()
                } else {
                    safe
                };
                m.insert(format!("disk.{safe}.used_pct"), used_pct);
                m.insert_with_unit(format!("disk.{safe}.total_bytes"), total, "bytes");
                m.insert_with_unit(format!("disk.{safe}.free_bytes"), avail, "bytes");
            }
            // Convenience: worst disk utilization across all mounts.
            m.insert("disk.used_pct", worst_pct);
        }

        // Network RX/TX rates.
        {
            let mut nets = self.networks.lock().await;
            nets.refresh(true);
            let mut total_rx: f64 = 0.0;
            let mut total_tx: f64 = 0.0;
            for data in nets.list().values() {
                total_rx += data.received() as f64;
                total_tx += data.transmitted() as f64;
            }
            m.insert_with_unit("net.rx_bytes", total_rx, "bytes");
            m.insert_with_unit("net.tx_bytes", total_tx, "bytes");
        }

        // GPU metrics via nvidia-smi subprocess (optional — no-op if absent).
        if let Ok(gpu) = collect_gpu().await {
            for (k, v) in gpu {
                m.insert(k, v);
            }
        }

        Ok(m)
    }
}

/// Run `nvidia-smi` and parse utilization, VRAM, and temperature.
/// Returns an empty vec if nvidia-smi is not found or fails.
async fn collect_gpu() -> anyhow::Result<Vec<(String, f64)>> {
    let output = tokio::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .await
        .context("nvidia-smi not found")?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for (gpu_idx, line) in stdout.lines().enumerate() {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 4 {
            continue;
        }
        let prefix = if gpu_idx == 0 {
            "gpu".to_owned()
        } else {
            format!("gpu.{gpu_idx}")
        };

        if let Ok(util) = parts[0].parse::<f64>() {
            results.push((format!("{prefix}.util_pct"), util));
        }
        if let (Ok(used), Ok(total)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>()) {
            if total > 0.0 {
                results.push((format!("{prefix}.vram_pct"), (used / total) * 100.0));
            }
            results.push((format!("{prefix}.vram_used_mb"), used));
            results.push((format!("{prefix}.vram_total_mb"), total));
        }
        if let Ok(temp) = parts[3].parse::<f64>() {
            results.push((format!("{prefix}.temp_c"), temp));
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::metrics::Collector;

    #[tokio::test]
    async fn local_collector_produces_cpu_and_memory() {
        let collector = LocalCollector::new("test-local");
        let metrics = collector.collect().await.expect("collect failed");
        assert_eq!(metrics.target, "test-local");
        assert!(
            metrics
                .get(&monitor_core::metrics::MetricPath::new("cpu.percent"))
                .is_some(),
            "expected cpu.percent in metric set"
        );
        assert!(
            metrics
                .get(&monitor_core::metrics::MetricPath::new("memory.percent"))
                .is_some(),
            "expected memory.percent in metric set"
        );
    }

    #[tokio::test]
    async fn local_collector_produces_disk_metrics() {
        let collector = LocalCollector::new("test-local");
        let metrics = collector.collect().await.expect("collect failed");
        assert!(
            metrics
                .get(&monitor_core::metrics::MetricPath::new("disk.used_pct"))
                .is_some(),
            "expected disk.used_pct in metric set"
        );
    }
}
