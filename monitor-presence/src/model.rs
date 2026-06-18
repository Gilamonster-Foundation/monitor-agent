//! The canonical, render-agnostic application model shared by every skin.

use std::collections::{HashMap, VecDeque};

use monitor_core::alert::{Alert, AlertState};
use monitor_core::metrics::MetricSet;

use crate::DataEvent;

/// A single line in the chat log.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub from: String,
    pub text: String,
}

/// The dashboard tabs. The *identity* is shared across skins; each skin renders
/// them however it likes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Alerts,
    Metrics,
    History,
    Rules,
}

impl Tab {
    pub const ALL: &'static [Self] = &[Self::Alerts, Self::Metrics, Self::History, Self::Rules];

    pub fn label(self) -> &'static str {
        match self {
            Self::Alerts => "Alerts",
            Self::Metrics => "Metrics",
            Self::History => "History",
            Self::Rules => "Rules",
        }
    }
}

/// Canonical application state — everything a skin renders, and nothing about
/// *how* it is rendered. Holds only ecosystem-independent `monitor-core` types,
/// so it is swarm-free and frontend-agnostic.
///
/// `Clone` is cheap-enough for a per-frame snapshot (a few KB of metrics /
/// alerts / history); skins render from a snapshot rather than holding the
/// shared lock (see [`SharedPresence`](crate::SharedPresence)).
#[derive(Clone)]
pub struct PresenceModel {
    pub active_tab: Tab,
    pub quit: bool,
    /// Current metric snapshot per target name.
    pub metrics: HashMap<String, MetricSet>,
    /// Active (Firing) alerts.
    pub active_alerts: Vec<Alert>,
    /// Recently resolved alerts (last 100).
    pub resolved_alerts: Vec<Alert>,
    pub daemon_connected: bool,
    pub daemon_status: String,
    /// Pre-formatted clock string, refreshed on [`DataEvent::Tick`].
    pub now: String,
    pub active_alert_count: usize,
    /// Chat log — capped at 200 entries.
    pub chat_log: Vec<ChatMessage>,
    /// Rolling metric history: target → metric_path → last 60 values.
    pub metrics_history: HashMap<String, HashMap<String, VecDeque<f64>>>,
}

impl PresenceModel {
    pub fn new() -> Self {
        Self {
            active_tab: Tab::Metrics,
            quit: false,
            metrics: HashMap::new(),
            active_alerts: Vec::new(),
            resolved_alerts: Vec::new(),
            daemon_connected: false,
            daemon_status: "connecting…".into(),
            now: chrono::Local::now().format("%H:%M:%S").to_string(),
            active_alert_count: 0,
            chat_log: Vec::new(),
            metrics_history: HashMap::new(),
        }
    }

    /// Apply a frontend-neutral data event. Pure reducer — no rendering, no
    /// input handling.
    pub fn apply(&mut self, event: DataEvent) {
        match event {
            DataEvent::MetricsUpdate(metrics) => {
                // Push each value into its rolling history (cap 60).
                let target_hist = self
                    .metrics_history
                    .entry(metrics.target.clone())
                    .or_default();
                for (path, value) in &metrics.values {
                    let dq = target_hist.entry(path.0.clone()).or_default();
                    dq.push_back(value.value);
                    if dq.len() > 60 {
                        dq.pop_front();
                    }
                }
                self.metrics.insert(metrics.target.clone(), metrics);
            }
            DataEvent::AlertFired(alert) => {
                self.active_alerts.retain(|a| a.id != alert.id);
                self.active_alerts.push(alert);
                self.active_alert_count = self.active_alerts.len();
            }
            DataEvent::AlertResolved(alert) => {
                self.active_alerts.retain(|a| a.id != alert.id);
                self.active_alert_count = self.active_alerts.len();
                self.resolved_alerts.insert(0, alert);
                self.resolved_alerts.truncate(100);
            }
            DataEvent::AlertsSnapshot(alerts) => {
                self.active_alerts = alerts
                    .into_iter()
                    .filter(|a| a.state == AlertState::Firing)
                    .collect();
                self.active_alert_count = self.active_alerts.len();
            }
            DataEvent::DaemonConnected => {
                self.daemon_connected = true;
                self.daemon_status = "ok".into();
            }
            DataEvent::DaemonDisconnected(reason) => {
                self.daemon_connected = false;
                self.daemon_status = format!("disconnected: {reason}");
            }
            DataEvent::Tick => {
                self.now = chrono::Local::now().format("%H:%M:%S").to_string();
            }
        }
    }

    /// Return up to `width` recent samples for `(target, metric_path)`.
    pub fn history_for(&self, target: &str, metric: &str, width: usize) -> Vec<f64> {
        let dq = self.metrics_history.get(target).and_then(|t| t.get(metric));
        match dq {
            None => vec![],
            Some(dq) => {
                let v: Vec<f64> = dq.iter().copied().collect();
                if v.len() > width {
                    v[v.len() - width..].to_vec()
                } else {
                    v
                }
            }
        }
    }

    /// Last N chat messages (for display).
    pub fn recent_chat(&self, n: usize) -> &[ChatMessage] {
        let len = self.chat_log.len();
        if len <= n {
            &self.chat_log
        } else {
            &self.chat_log[len - n..]
        }
    }

    /// Cycle the active tab by `delta` (wrapping). Resetting per-skin scroll on
    /// a tab change is the skin's job, not the model's.
    pub(crate) fn cycle_tab(&mut self, delta: i32) {
        let idx = Tab::ALL
            .iter()
            .position(|t| *t == self.active_tab)
            .unwrap_or(0);
        let next = (idx as i32 + delta).rem_euclid(Tab::ALL.len() as i32) as usize;
        self.active_tab = Tab::ALL[next];
    }
}

impl Default for PresenceModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::alert::{AlertId, Severity};
    use monitor_core::metrics::MetricPath;
    use uuid::Uuid;

    fn firing_alert(rule: &str) -> Alert {
        Alert {
            id: AlertId::for_rule("gnuc", rule),
            uuid: Uuid::new_v4(),
            rule_name: rule.into(),
            target: "gnuc".into(),
            metric: MetricPath::new("cpu.percent"),
            value: 90.0,
            severity: Severity::Warn,
            state: AlertState::Firing,
            message: format!("gnuc: {rule}"),
            fired_at_secs: None,
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        }
    }

    #[test]
    fn alert_lifecycle() {
        let mut m = PresenceModel::new();
        let alert = firing_alert("high-cpu");
        m.apply(DataEvent::AlertFired(alert.clone()));
        assert_eq!(m.active_alerts.len(), 1);
        assert_eq!(m.active_alert_count, 1);

        let mut resolved = alert;
        resolved.state = AlertState::Resolved;
        m.apply(DataEvent::AlertResolved(resolved));
        assert_eq!(m.active_alerts.len(), 0);
        assert_eq!(m.resolved_alerts.len(), 1);
    }

    #[test]
    fn metrics_update_stored_by_target() {
        let mut m = PresenceModel::new();
        let mut ms = MetricSet::new("gnuc");
        ms.insert("cpu.percent", 42.0);
        m.apply(DataEvent::MetricsUpdate(ms));
        assert!(m.metrics.contains_key("gnuc"));
    }

    #[test]
    fn metrics_history_caps_at_60() {
        let mut m = PresenceModel::new();
        for i in 0..70 {
            let mut ms = MetricSet::new("gnuc");
            ms.insert("cpu.percent", i as f64);
            m.apply(DataEvent::MetricsUpdate(ms));
        }
        assert_eq!(m.history_for("gnuc", "cpu.percent", 100).len(), 60);
    }

    #[test]
    fn alerts_snapshot_replaces_active() {
        let mut m = PresenceModel::new();
        m.apply(DataEvent::AlertFired(firing_alert("old")));
        assert_eq!(m.active_alerts.len(), 1);
        m.apply(DataEvent::AlertsSnapshot(vec![
            firing_alert("new1"),
            firing_alert("new2"),
        ]));
        assert_eq!(m.active_alerts.len(), 2);
        assert_eq!(m.active_alert_count, 2);
    }

    #[test]
    fn daemon_connect_then_disconnect() {
        let mut m = PresenceModel::new();
        m.apply(DataEvent::DaemonConnected);
        assert!(m.daemon_connected);
        assert_eq!(m.daemon_status, "ok");
        m.apply(DataEvent::DaemonDisconnected("timeout".into()));
        assert!(!m.daemon_connected);
        assert!(m.daemon_status.contains("timeout"));
    }

    #[test]
    fn tick_refreshes_clock() {
        let mut m = PresenceModel::new();
        m.apply(DataEvent::Tick);
        assert!(!m.now.is_empty());
    }

    #[test]
    fn history_for_unknown_is_empty() {
        let m = PresenceModel::new();
        assert!(m.history_for("nope", "cpu.percent", 10).is_empty());
    }

    #[test]
    fn recent_chat_returns_last_n() {
        let mut m = PresenceModel::new();
        for i in 0..5 {
            m.chat_log.push(ChatMessage {
                from: "you".into(),
                text: format!("msg {i}"),
            });
        }
        let recent = m.recent_chat(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[2].text, "msg 4");
    }

    #[test]
    fn tab_all_covers_all_variants() {
        assert_eq!(Tab::ALL.len(), 4);
        for t in [Tab::Alerts, Tab::Metrics, Tab::History, Tab::Rules] {
            assert!(Tab::ALL.contains(&t));
        }
    }

    #[test]
    fn tab_labels_non_empty() {
        for tab in Tab::ALL {
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn cycle_tab_wraps_both_ways() {
        let mut m = PresenceModel::new();
        m.active_tab = Tab::Rules;
        m.cycle_tab(1);
        assert_eq!(m.active_tab, Tab::Alerts); // wrap forward off the end
        m.cycle_tab(-1);
        assert_eq!(m.active_tab, Tab::Rules); // wrap back off the front
    }
}
