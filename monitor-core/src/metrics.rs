use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dot-separated path to a metric value, e.g. `"cpu.percent"`, `"disk./.used_pct"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MetricPath(pub String);

impl MetricPath {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MetricPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for MetricPath {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for MetricPath {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// A single metric value — always f64 internally for rule evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricValue {
    pub value: f64,
    pub unit: Option<String>,
}

impl MetricValue {
    pub fn new(value: f64) -> Self {
        Self { value, unit: None }
    }

    pub fn with_unit(value: f64, unit: impl Into<String>) -> Self {
        Self {
            value,
            unit: Some(unit.into()),
        }
    }
}

/// A snapshot of all metrics for one target at one point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSet {
    /// The target that produced these metrics (matches a target name in config).
    pub target: String,
    /// Collected at this wall-clock time (Unix timestamp seconds).
    pub collected_at: i64,
    /// Flat map of MetricPath → MetricValue.
    pub values: HashMap<MetricPath, MetricValue>,
}

impl MetricSet {
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            collected_at: chrono::Utc::now().timestamp(),
            values: HashMap::new(),
        }
    }

    pub fn insert(&mut self, path: impl Into<MetricPath>, value: f64) {
        self.values.insert(path.into(), MetricValue::new(value));
    }

    pub fn insert_with_unit(&mut self, path: impl Into<MetricPath>, value: f64, unit: &str) {
        self.values
            .insert(path.into(), MetricValue::with_unit(value, unit));
    }

    pub fn get(&self, path: &MetricPath) -> Option<f64> {
        self.values.get(path).map(|v| v.value)
    }
}

/// Trait for anything that can produce a MetricSet on demand.
#[async_trait::async_trait]
pub trait Collector: Send + Sync {
    fn name(&self) -> &str;
    async fn collect(&self) -> anyhow::Result<MetricSet>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_path_display() {
        let p = MetricPath::new("cpu.percent");
        assert_eq!(p.to_string(), "cpu.percent");
    }

    #[test]
    fn metric_set_insert_get_roundtrip() {
        let mut m = MetricSet::new("gnuc");
        m.insert("cpu.percent", 42.5);
        assert_eq!(m.get(&MetricPath::new("cpu.percent")), Some(42.5));
        assert_eq!(m.get(&MetricPath::new("missing")), None);
    }

    #[test]
    fn metric_set_target_name() {
        let m = MetricSet::new("nuc");
        assert_eq!(m.target, "nuc");
    }
}
