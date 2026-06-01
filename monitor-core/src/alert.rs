use crate::metrics::{MetricPath, MetricSet};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Alert rule types
// ---------------------------------------------------------------------------

/// Glob-like target pattern: "*" matches all, otherwise exact name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetPattern(pub String);

impl TargetPattern {
    pub fn matches(&self, target: &str) -> bool {
        self.0 == "*" || self.0 == target
    }
}

impl From<&str> for TargetPattern {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Threshold condition for an alert rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// Fires when the metric value is strictly greater than the threshold.
    GreaterThan(f64),
    /// Fires when the metric value is strictly less than the threshold.
    LessThan(f64),
    /// Fires when the metric value equals the threshold (within f64 epsilon).
    Equals(f64),
}

impl Condition {
    pub fn evaluate(&self, value: f64) -> bool {
        match self {
            Self::GreaterThan(threshold) => value > *threshold,
            Self::LessThan(threshold) => value < *threshold,
            Self::Equals(threshold) => (value - threshold).abs() < f64::EPSILON,
        }
    }

    pub fn threshold(&self) -> f64 {
        match self {
            Self::GreaterThan(t) | Self::LessThan(t) | Self::Equals(t) => *t,
        }
    }
}

/// Alert severity — determines urgency of notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warn,
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warn => write!(f, "WARN"),
            Self::Critical => write!(f, "CRIT"),
        }
    }
}

/// A single threshold rule loaded from config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub target: TargetPattern,
    pub metric: MetricPath,
    pub condition: Condition,
    pub severity: Severity,
    /// Minimum time between repeated firings of the same alert.
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: u64,
    /// Message template. Placeholders: `{target}`, `{metric}`, `{value:.1}`.
    #[serde(default)]
    pub message: String,
}

fn default_cooldown_secs() -> u64 {
    300
}

impl AlertRule {
    pub fn cooldown(&self) -> Duration {
        Duration::from_secs(self.cooldown_secs)
    }

    pub fn format_message(&self, target: &str, value: f64) -> String {
        if self.message.is_empty() {
            format!("{target}: {metric} = {value:.1}", metric = self.metric)
        } else {
            self.message
                .replace("{target}", target)
                .replace("{metric}", self.metric.as_str())
                .replace("{value:.0}", &format!("{value:.0}"))
                .replace("{value:.1}", &format!("{value:.1}"))
                .replace("{value:.2}", &format!("{value:.2}"))
                .replace("{value}", &format!("{value}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Alert lifecycle
// ---------------------------------------------------------------------------

/// Opaque unique ID for a firing alert.  Derived from (target, rule_name).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlertId(pub String);

impl AlertId {
    pub fn for_rule(target: &str, rule_name: &str) -> Self {
        Self(format!("{target}::{rule_name}"))
    }
}

impl std::fmt::Display for AlertId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lifecycle state of an alert instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertState {
    /// Condition met; waiting for `resolve_after` before auto-resolving.
    Pending,
    /// Condition met and dispatcher has been called.
    Firing,
    /// Condition no longer met.
    Resolved,
}

/// A live (or recently resolved) alert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: AlertId,
    pub uuid: Uuid,
    pub rule_name: String,
    pub target: String,
    pub metric: MetricPath,
    pub value: f64,
    pub severity: Severity,
    pub state: AlertState,
    pub message: String,
    /// Epoch-second timestamps for serialization/display.
    pub fired_at_secs: Option<i64>,
    pub resolved_at_secs: Option<i64>,
    /// Epoch-second until which repeat notifications are suppressed.
    pub cooldown_until_secs: Option<i64>,
    /// Used by the engine (not serialized).
    #[serde(skip)]
    pub fired_instant: Option<Instant>,
    #[serde(skip)]
    pub cooldown_until_instant: Option<Instant>,
}

impl Alert {
    pub fn is_firing(&self) -> bool {
        self.state == AlertState::Firing
    }

    pub fn is_resolved(&self) -> bool {
        self.state == AlertState::Resolved
    }

    pub fn in_cooldown(&self) -> bool {
        self.cooldown_until_instant
            .map(|t| Instant::now() < t)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Dispatcher trait
// ---------------------------------------------------------------------------

/// Anything that can notify a human operator when an alert fires or resolves.
#[async_trait::async_trait]
pub trait AlertDispatcher: Send + Sync {
    fn name(&self) -> &str;
    async fn fire(&self, alert: &Alert) -> anyhow::Result<()>;
    async fn resolve(&self, alert: &Alert) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// Alert engine
// ---------------------------------------------------------------------------

/// Evaluates MetricSets against AlertRules and drives the alert lifecycle.
pub struct AlertEngine {
    rules: Vec<AlertRule>,
    /// Active and recently resolved alerts, keyed by AlertId.
    alerts: HashMap<AlertId, Alert>,
    /// How long a condition must be absent before auto-resolving. Default 60s.
    resolve_after: Duration,
}

impl AlertEngine {
    pub fn new(rules: Vec<AlertRule>) -> Self {
        Self {
            rules,
            alerts: HashMap::new(),
            resolve_after: Duration::from_secs(60),
        }
    }

    pub fn with_resolve_after(mut self, d: Duration) -> Self {
        self.resolve_after = d;
        self
    }

    /// Evaluate a fresh MetricSet. Returns alerts that transitioned state
    /// this call (newly firing or newly resolved) — caller should dispatch.
    pub fn evaluate(&mut self, metrics: &MetricSet) -> Vec<Alert> {
        let mut transitions = Vec::new();
        let now_secs = chrono::Utc::now().timestamp();

        for rule in &self.rules {
            if !rule.target.matches(&metrics.target) {
                continue;
            }
            let Some(value) = metrics.get(&rule.metric) else {
                continue;
            };

            let id = AlertId::for_rule(&metrics.target, &rule.name);
            let condition_met = rule.condition.evaluate(value);

            if condition_met {
                let existing = self.alerts.get(&id);
                let in_cooldown = existing.map(|a| a.in_cooldown()).unwrap_or(false);
                if in_cooldown {
                    continue;
                }

                match existing.map(|a| a.state) {
                    Some(AlertState::Firing) => {
                        // Already firing — update value but don't re-dispatch.
                        if let Some(a) = self.alerts.get_mut(&id) {
                            a.value = value;
                        }
                    }
                    Some(AlertState::Pending) => {
                        // Transition to Firing.
                        if let Some(a) = self.alerts.get_mut(&id) {
                            a.state = AlertState::Firing;
                            a.value = value;
                            a.fired_at_secs = Some(now_secs);
                            a.fired_instant = Some(Instant::now());
                            a.cooldown_until_secs = Some(now_secs + rule.cooldown_secs as i64);
                            a.cooldown_until_instant = Some(Instant::now() + rule.cooldown());
                            transitions.push(a.clone());
                        }
                    }
                    None | Some(AlertState::Resolved) => {
                        // New alert — insert as Firing immediately.
                        let alert = Alert {
                            id: id.clone(),
                            uuid: Uuid::new_v4(),
                            rule_name: rule.name.clone(),
                            target: metrics.target.clone(),
                            metric: rule.metric.clone(),
                            value,
                            severity: rule.severity,
                            state: AlertState::Firing,
                            message: rule.format_message(&metrics.target, value),
                            fired_at_secs: Some(now_secs),
                            resolved_at_secs: None,
                            cooldown_until_secs: Some(now_secs + rule.cooldown_secs as i64),
                            fired_instant: Some(Instant::now()),
                            cooldown_until_instant: Some(Instant::now() + rule.cooldown()),
                        };
                        self.alerts.insert(id, alert.clone());
                        transitions.push(alert);
                    }
                }
            } else {
                // Condition NOT met — resolve if currently firing.
                if let Some(a) = self.alerts.get_mut(&id) {
                    if a.state == AlertState::Firing {
                        a.state = AlertState::Resolved;
                        a.resolved_at_secs = Some(now_secs);
                        transitions.push(a.clone());
                    }
                }
            }
        }

        transitions
    }

    /// All active (Firing) alerts.
    pub fn active_alerts(&self) -> Vec<&Alert> {
        self.alerts
            .values()
            .filter(|a| a.state == AlertState::Firing)
            .collect()
    }

    /// All alerts including recently resolved.
    pub fn all_alerts(&self) -> Vec<&Alert> {
        self.alerts.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricSet;

    fn cpu_rule(threshold: f64) -> AlertRule {
        AlertRule {
            name: "high-cpu".into(),
            target: TargetPattern("*".into()),
            metric: MetricPath::new("cpu.percent"),
            condition: Condition::GreaterThan(threshold),
            severity: Severity::Warn,
            cooldown_secs: 0, // no cooldown for tests
            message: String::new(),
        }
    }

    fn metrics(target: &str, cpu: f64) -> MetricSet {
        let mut m = MetricSet::new(target);
        m.insert("cpu.percent", cpu);
        m
    }

    #[test]
    fn condition_evaluate() {
        assert!(Condition::GreaterThan(80.0).evaluate(90.0));
        assert!(!Condition::GreaterThan(80.0).evaluate(70.0));
        assert!(Condition::LessThan(10.0).evaluate(5.0));
        assert!(Condition::Equals(42.0).evaluate(42.0));
    }

    #[test]
    fn engine_fires_on_threshold_breach() {
        let mut engine = AlertEngine::new(vec![cpu_rule(80.0)]);
        let transitions = engine.evaluate(&metrics("gnuc", 90.0));
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].state, AlertState::Firing);
        assert_eq!(transitions[0].target, "gnuc");
    }

    #[test]
    fn engine_no_fire_below_threshold() {
        let mut engine = AlertEngine::new(vec![cpu_rule(80.0)]);
        let transitions = engine.evaluate(&metrics("gnuc", 70.0));
        assert!(transitions.is_empty());
    }

    #[test]
    fn engine_resolves_when_condition_clears() {
        let mut engine = AlertEngine::new(vec![cpu_rule(80.0)]);
        engine.evaluate(&metrics("gnuc", 90.0));
        let transitions = engine.evaluate(&metrics("gnuc", 70.0));
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].state, AlertState::Resolved);
    }

    #[test]
    fn engine_no_duplicate_fire_while_active() {
        let mut engine = AlertEngine::new(vec![cpu_rule(80.0)]);
        engine.evaluate(&metrics("gnuc", 90.0));
        // Second evaluation at high CPU — already firing, no new transition.
        let transitions = engine.evaluate(&metrics("gnuc", 95.0));
        assert!(transitions.is_empty());
    }

    #[test]
    fn target_pattern_wildcard() {
        let p = TargetPattern("*".into());
        assert!(p.matches("gnuc"));
        assert!(p.matches("nuc"));
    }

    #[test]
    fn target_pattern_exact() {
        let p = TargetPattern("gnuc".into());
        assert!(p.matches("gnuc"));
        assert!(!p.matches("nuc"));
    }

    #[test]
    fn alert_id_format() {
        let id = AlertId::for_rule("gnuc", "high-cpu");
        assert_eq!(id.0, "gnuc::high-cpu");
    }

    #[test]
    fn rule_format_message_default() {
        let rule = cpu_rule(80.0);
        let msg = rule.format_message("gnuc", 92.3);
        assert!(msg.contains("gnuc"));
        assert!(msg.contains("cpu.percent"));
    }

    #[test]
    fn rule_format_message_template() {
        let mut rule = cpu_rule(80.0);
        rule.message = "{target}: CPU at {value:.0}%".into();
        let msg = rule.format_message("gnuc", 92.0);
        assert_eq!(msg, "gnuc: CPU at 92%");
    }
}
