use monitor_core::alert::Alert;
use monitor_core::metrics::MetricSet;

/// All events that flow through the TUI's unified event channel.
///
/// Terminal key events come from crossterm; data events come from the
/// daemon's collector pipeline (or direct in standalone mode).
#[derive(Debug)]
pub enum Event {
    // --- Terminal events ---
    Key(crossterm::event::KeyEvent),
    Resize(u16, u16),

    // --- Metrics data ---
    /// Fresh metric snapshot from any collector.
    MetricsUpdate(MetricSet),

    // --- Alert lifecycle ---
    /// An alert transitioned to Firing.
    AlertFired(Alert),
    /// An alert transitioned to Resolved.
    AlertResolved(Alert),
    /// Full replacement of the active alert list (e.g. on daemon reconnect).
    AlertsSnapshot(Vec<Alert>),

    // --- Connectivity ---
    DaemonConnected,
    DaemonDisconnected(String),

    /// Clock tick — triggers header timestamp refresh.
    Tick,
}
