pub mod local;
pub mod nats;
pub mod prometheus;
pub mod ssh;

pub use local::LocalCollector;
pub use nats::NatsCollector;
pub use prometheus::{PrometheusCollector, PrometheusOptions};
pub use ssh::SshCollector;
