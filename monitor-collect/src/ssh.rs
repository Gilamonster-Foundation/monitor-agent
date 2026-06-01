use anyhow::Context;
use monitor_core::metrics::{Collector, MetricSet};

/// Collects metrics from a remote host via SSH subprocess.
///
/// Runs a one-shot command that reads `/proc/loadavg`, `/proc/meminfo`,
/// and `df -P` over an existing SSH connection. Uses the system `ssh`
/// binary — no libssh2 dependency, works on all platforms.
pub struct SshCollector {
    target_name: String,
    host: String,
    user: Option<String>,
    key: Option<String>,
}

impl SshCollector {
    pub fn new(
        target_name: impl Into<String>,
        host: impl Into<String>,
        user: Option<String>,
        key: Option<String>,
    ) -> Self {
        Self {
            target_name: target_name.into(),
            host: host.into(),
            user,
            key,
        }
    }

    fn ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".into(),
            "ConnectTimeout=5".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "StrictHostKeyChecking=accept-new".into(),
        ];
        if let Some(ref key) = self.key {
            args.push("-i".into());
            args.push(key.clone());
        }
        let host = match &self.user {
            Some(u) => format!("{u}@{}", self.host),
            None => self.host.clone(),
        };
        args.push(host);
        args.push(
            // One command string: emit loadavg, meminfo, and df in one SSH round-trip.
            "cat /proc/loadavg; echo '---meminfo---'; cat /proc/meminfo; echo '---df---'; df -P"
                .into(),
        );
        args
    }
}

#[async_trait::async_trait]
impl Collector for SshCollector {
    fn name(&self) -> &str {
        &self.target_name
    }

    async fn collect(&self) -> anyhow::Result<MetricSet> {
        let output = tokio::process::Command::new("ssh")
            .args(self.ssh_args())
            .output()
            .await
            .context("ssh subprocess failed")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ssh to {} failed: {}", self.host, stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_ssh_output(&self.target_name, &stdout)
    }
}

fn parse_ssh_output(target: &str, output: &str) -> anyhow::Result<MetricSet> {
    let mut m = MetricSet::new(target);
    let mut section = "loadavg";

    let mut mem_total_kb: f64 = 0.0;
    let mut mem_available_kb: f64 = 0.0;
    let mut worst_disk_pct: f64 = 0.0;

    for line in output.lines() {
        match line {
            "---meminfo---" => {
                section = "meminfo";
                continue;
            }
            "---df---" => {
                section = "df";
                continue;
            }
            _ => {}
        }

        match section {
            "loadavg" => {
                // Format: "0.52 0.58 0.59 1/312 12345"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    if let (Ok(l1), Ok(l5), Ok(l15)) = (
                        parts[0].parse::<f64>(),
                        parts[1].parse::<f64>(),
                        parts[2].parse::<f64>(),
                    ) {
                        m.insert("load.1m", l1);
                        m.insert("load.5m", l5);
                        m.insert("load.15m", l15);
                    }
                }
            }
            "meminfo" => {
                // Format: "MemTotal:       32768000 kB"
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Ok(kb) = rest.split_whitespace().next().unwrap_or("").parse::<f64>() {
                        mem_total_kb = kb;
                    }
                } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    if let Ok(kb) = rest.split_whitespace().next().unwrap_or("").parse::<f64>() {
                        mem_available_kb = kb;
                    }
                }
            }
            "df" => {
                // POSIX df -P format: Filesystem 1024-blocks Used Available Use% Mounted
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 6 && parts[0] != "Filesystem" {
                    if let Some(pct_str) = parts[4].strip_suffix('%') {
                        if let Ok(pct) = pct_str.parse::<f64>() {
                            worst_disk_pct = worst_disk_pct.max(pct);
                            let mount = parts[5]
                                .replace('/', "_")
                                .trim_start_matches('_')
                                .to_owned();
                            let mount = if mount.is_empty() {
                                "root".to_owned()
                            } else {
                                mount
                            };
                            m.insert(format!("disk.{mount}.used_pct"), pct);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if mem_total_kb > 0.0 {
        let used_kb = mem_total_kb - mem_available_kb;
        m.insert("memory.percent", (used_kb / mem_total_kb) * 100.0);
        m.insert_with_unit("memory.total_bytes", mem_total_kb * 1024.0, "bytes");
        m.insert_with_unit("memory.used_bytes", used_kb * 1024.0, "bytes");
    }
    m.insert("disk.used_pct", worst_disk_pct);

    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OUTPUT: &str = "0.52 0.58 0.59 1/312 99999
---meminfo---
MemTotal:       32768000 kB
MemFree:         8192000 kB
MemAvailable:   16384000 kB
Buffers:          512000 kB
Cached:          4096000 kB
---df---
Filesystem     1024-blocks      Used Available Use% Mounted on
/dev/sda1        100000000  70000000  30000000  70% /
tmpfs              8192000         0   8192000   0% /dev/shm
";

    #[test]
    fn parse_loadavg() {
        let m = parse_ssh_output("gnuc", SAMPLE_OUTPUT).unwrap();
        assert_eq!(m.get(&"load.1m".into()), Some(0.52));
        assert_eq!(m.get(&"load.5m".into()), Some(0.58));
        assert_eq!(m.get(&"load.15m".into()), Some(0.59));
    }

    #[test]
    fn parse_memory() {
        let m = parse_ssh_output("gnuc", SAMPLE_OUTPUT).unwrap();
        let pct = m.get(&"memory.percent".into()).unwrap();
        assert!(
            (pct - 50.0).abs() < 1.0,
            "expected ~50% memory used, got {pct}"
        );
    }

    #[test]
    fn parse_disk() {
        let m = parse_ssh_output("gnuc", SAMPLE_OUTPUT).unwrap();
        assert_eq!(m.get(&"disk.used_pct".into()), Some(70.0));
    }
}
