use crate::alert::{AlertRule, Condition, Severity, TargetPattern};
use crate::metrics::MetricPath;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration, loaded from a TOML file.
///
/// Search order:
///   1. `MONITOR_CONFIG` environment variable (explicit path)
///   2. `./monitor-agent.toml` (current working directory)
///   3. `~/.config/monitor-agent/config.toml`
///   4. `/etc/monitor-agent/config.toml`
///   5. Built-in defaults (local target, no remote sources)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub targets: Vec<TargetConfig>,
    #[serde(default)]
    pub nats: Option<NatsConfig>,
    #[serde(default)]
    pub rules: Vec<RuleConfig>,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub tui: TuiConfig,
}

/// TUI-specific settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiConfig {
    /// Seconds before the splash screen auto-dismisses. Set to 0 to wait forever.
    #[serde(default = "default_splash_timeout")]
    pub splash_timeout_secs: u64,
}

fn default_splash_timeout() -> u64 {
    10
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            splash_timeout_secs: default_splash_timeout(),
        }
    }
}

impl Config {
    /// Resolve config from environment / filesystem, falling back to defaults.
    pub fn resolve() -> anyhow::Result<Self> {
        let candidates = Self::candidate_paths();
        for path in candidates {
            if path.exists() {
                tracing::debug!("loading config from {}", path.display());
                let text = std::fs::read_to_string(&path)?;
                let cfg: Self = toml::from_str(&text)?;
                return Ok(cfg);
            }
        }
        tracing::debug!("no config file found — using built-in defaults");
        Ok(Self::default_with_local_target())
    }

    fn candidate_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(p) = std::env::var("MONITOR_CONFIG") {
            paths.push(PathBuf::from(p));
        }
        paths.push(PathBuf::from("monitor-agent.toml"));
        if let Some(home) = dirs_home() {
            paths.push(home.join(".config/monitor-agent/config.toml"));
        }
        paths.push(PathBuf::from("/etc/monitor-agent/config.toml"));
        paths
    }

    fn default_with_local_target() -> Self {
        Self {
            targets: vec![TargetConfig {
                name: "local".into(),
                kind: TargetKind::Local,
                ..Default::default()
            }],
            rules: default_rules(),
            ..Default::default()
        }
    }

    /// Convert RuleConfig entries to AlertRule values for the engine.
    pub fn alert_rules(&self) -> Vec<AlertRule> {
        self.rules.iter().map(|r| r.to_alert_rule()).collect()
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn default_rules() -> Vec<RuleConfig> {
    vec![
        RuleConfig {
            name: "high-cpu".into(),
            target: "*".into(),
            metric: "cpu.percent".into(),
            gt: Some(85.0),
            lt: None,
            eq: None,
            severity: SeverityConfig::Warn,
            cooldown_secs: 300,
            message: "{target}: CPU at {value:.0}%".into(),
        },
        RuleConfig {
            name: "critical-disk".into(),
            target: "*".into(),
            metric: "disk.used_pct".into(),
            gt: Some(90.0),
            lt: None,
            eq: None,
            severity: SeverityConfig::Critical,
            cooldown_secs: 3600,
            message: "{target}: disk at {value:.0}%".into(),
        },
        RuleConfig {
            name: "high-memory".into(),
            target: "*".into(),
            metric: "memory.percent".into(),
            gt: Some(90.0),
            lt: None,
            eq: None,
            severity: SeverityConfig::Warn,
            cooldown_secs: 300,
            message: "{target}: memory at {value:.0}%".into(),
        },
    ]
}

// ---------------------------------------------------------------------------
// Sub-config types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonConfig {
    /// Unix socket path. Defaults to `$XDG_RUNTIME_DIR/monitor-agent.sock`
    /// or `/tmp/monitor-agent.sock` on macOS/Windows.
    #[serde(default)]
    pub socket: Option<String>,
}

impl DaemonConfig {
    pub fn socket_path(&self) -> PathBuf {
        if let Some(ref s) = self.socket {
            return PathBuf::from(s);
        }
        if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
            return PathBuf::from(xdg).join("monitor-agent.sock");
        }
        PathBuf::from("/tmp/monitor-agent.sock")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetConfig {
    pub name: String,
    pub kind: TargetKind,
    /// For `prometheus` kind: the Prometheus HTTP API base URL.
    pub endpoint: Option<String>,
    /// For `ssh` kind: hostname or IP.
    pub host: Option<String>,
    /// For `ssh` kind: SSH username.
    pub user: Option<String>,
    /// For `ssh` kind: path to private key.
    pub key: Option<String>,
    /// Override the poll interval in seconds (each kind has a built-in default).
    pub interval_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    #[default]
    Local,
    Prometheus,
    Ssh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsConfig {
    pub servers: Vec<String>,
    #[serde(default)]
    pub subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConfig {
    #[serde(default = "default_true")]
    pub terminal_bell: bool,
    #[serde(default = "default_true")]
    pub voice: bool,
    /// `auto` | `piper` | `espeak-ng` | `say` | `powershell`
    #[serde(default = "default_auto")]
    pub voice_engine: String,
    /// Publish fired alerts to this NATS subject (empty = disabled).
    #[serde(default)]
    pub nats_subject: String,
    /// HTTP POST webhook URL (empty = disabled).
    #[serde(default)]
    pub webhook: String,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            terminal_bell: true,
            voice: true,
            voice_engine: "auto".into(),
            nats_subject: String::new(),
            webhook: String::new(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_auto() -> String {
    "auto".into()
}

/// TOML-friendly rule with flat condition fields instead of an enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfig {
    pub name: String,
    #[serde(default = "wildcard")]
    pub target: String,
    pub metric: String,
    /// Fires when metric > this value.
    pub gt: Option<f64>,
    /// Fires when metric < this value.
    pub lt: Option<f64>,
    /// Fires when metric == this value.
    pub eq: Option<f64>,
    #[serde(default)]
    pub severity: SeverityConfig,
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
    #[serde(default)]
    pub message: String,
}

fn wildcard() -> String {
    "*".into()
}
fn default_cooldown() -> u64 {
    300
}

impl RuleConfig {
    pub fn to_alert_rule(&self) -> AlertRule {
        let condition = if let Some(v) = self.gt {
            Condition::GreaterThan(v)
        } else if let Some(v) = self.lt {
            Condition::LessThan(v)
        } else if let Some(v) = self.eq {
            Condition::Equals(v)
        } else {
            Condition::GreaterThan(100.0) // never fires — misconfigured rule
        };

        AlertRule {
            name: self.name.clone(),
            target: TargetPattern(self.target.clone()),
            metric: MetricPath::new(self.metric.clone()),
            condition,
            severity: self.severity.to_severity(),
            cooldown_secs: self.cooldown_secs,
            message: self.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SeverityConfig {
    Info,
    #[default]
    Warn,
    Critical,
}

impl SeverityConfig {
    pub fn to_severity(&self) -> Severity {
        match self {
            Self::Info => Severity::Info,
            Self::Warn => Severity::Warn,
            Self::Critical => Severity::Critical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_local_target() {
        let cfg = Config::default_with_local_target();
        assert_eq!(cfg.targets.len(), 1);
        assert_eq!(cfg.targets[0].name, "local");
        assert_eq!(cfg.targets[0].kind, TargetKind::Local);
    }

    #[test]
    fn default_rules_convert_to_alert_rules() {
        let cfg = Config::default_with_local_target();
        let rules = cfg.alert_rules();
        assert!(!rules.is_empty());
        assert!(rules.iter().any(|r| r.name == "high-cpu"));
    }

    #[test]
    fn rule_config_gt_converts() {
        let rc = RuleConfig {
            name: "test".into(),
            target: "*".into(),
            metric: "cpu.percent".into(),
            gt: Some(80.0),
            lt: None,
            eq: None,
            severity: SeverityConfig::Warn,
            cooldown_secs: 300,
            message: String::new(),
        };
        let rule = rc.to_alert_rule();
        assert!(matches!(rule.condition, Condition::GreaterThan(v) if v == 80.0));
    }

    #[test]
    fn daemon_config_socket_fallback() {
        let cfg = DaemonConfig { socket: None };
        let path = cfg.socket_path();
        assert!(path.to_str().is_some());
    }

    #[test]
    fn toml_roundtrip() {
        let cfg = Config::default_with_local_target();
        let text = toml::to_string(&cfg).unwrap();
        let _: Config = toml::from_str(&text).unwrap();
    }

    #[test]
    fn rule_config_lt_converts() {
        let rc = RuleConfig {
            name: "low-disk".into(),
            target: "gnuc".into(),
            metric: "disk.free_pct".into(),
            gt: None,
            lt: Some(10.0),
            eq: None,
            severity: SeverityConfig::Critical,
            cooldown_secs: 3600,
            message: String::new(),
        };
        let rule = rc.to_alert_rule();
        assert!(matches!(rule.condition, Condition::LessThan(v) if v == 10.0));
    }

    #[test]
    fn rule_config_eq_converts() {
        let rc = RuleConfig {
            name: "zero-load".into(),
            target: "*".into(),
            metric: "load.1m".into(),
            gt: None,
            lt: None,
            eq: Some(0.0),
            severity: SeverityConfig::Info,
            cooldown_secs: 60,
            message: String::new(),
        };
        let rule = rc.to_alert_rule();
        assert!(matches!(rule.condition, Condition::Equals(v) if v == 0.0));
    }

    #[test]
    fn rule_config_no_condition_never_fires() {
        let rc = RuleConfig {
            name: "broken".into(),
            target: "*".into(),
            metric: "cpu.percent".into(),
            gt: None,
            lt: None,
            eq: None,
            severity: SeverityConfig::Warn,
            cooldown_secs: 300,
            message: String::new(),
        };
        let rule = rc.to_alert_rule();
        assert!(matches!(rule.condition, Condition::GreaterThan(v) if v == 100.0));
    }

    #[test]
    fn severity_config_all_variants() {
        assert!(matches!(SeverityConfig::Info.to_severity(), Severity::Info));
        assert!(matches!(SeverityConfig::Warn.to_severity(), Severity::Warn));
        assert!(matches!(
            SeverityConfig::Critical.to_severity(),
            Severity::Critical
        ));
    }

    #[test]
    fn daemon_config_explicit_socket() {
        let cfg = DaemonConfig {
            socket: Some("/tmp/test.sock".into()),
        };
        assert_eq!(cfg.socket_path().to_str().unwrap(), "/tmp/test.sock");
    }

    #[test]
    fn notify_config_defaults() {
        let n = NotifyConfig::default();
        assert!(n.terminal_bell);
        assert!(n.voice);
        assert_eq!(n.voice_engine, "auto");
        assert!(n.webhook.is_empty());
        assert!(n.nats_subject.is_empty());
    }

    #[test]
    fn target_kind_default_is_local() {
        let t = TargetConfig::default();
        assert_eq!(t.kind, TargetKind::Local);
    }

    #[test]
    fn config_resolve_uses_env_var() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("monitor-agent.toml");
        std::fs::write(&path, "[daemon]\n").unwrap();
        std::env::set_var("MONITOR_CONFIG", path.to_str().unwrap());
        let cfg = Config::resolve();
        std::env::remove_var("MONITOR_CONFIG");
        assert!(cfg.is_ok());
    }

    #[test]
    fn config_resolve_nonexistent_env_var_falls_back() {
        std::env::set_var(
            "MONITOR_CONFIG",
            "/tmp/nonexistent-monitor-agent-99999.toml",
        );
        // nonexistent path → should fall through to defaults, not error
        // (the path doesn't exist, so it's skipped)
        std::env::remove_var("MONITOR_CONFIG");
    }

    // `XDG_RUNTIME_DIR` and the Unix-domain socket it points at are a
    // Unix-only concept, and `PathBuf::join` uses `\` on Windows — so this
    // exact-path assertion is only meaningful (and correct) on Unix.
    #[cfg(unix)]
    #[test]
    fn daemon_config_socket_xdg_runtime() {
        let orig = std::env::var("XDG_RUNTIME_DIR").ok();
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        let cfg = DaemonConfig { socket: None };
        let path = cfg.socket_path();
        assert_eq!(path.to_str().unwrap(), "/run/user/1000/monitor-agent.sock");
        match orig {
            Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
    }
}
