use monitor_core::alert::Alert;
use monitor_core::metrics::MetricSet;

/// Frontend-neutral data events that mutate the canonical [`PresenceModel`].
///
/// These come from the collector/daemon pipeline and carry only `monitor-core`
/// types. Terminal/input events (key presses) are a per-skin concern and are
/// deliberately NOT represented here — each skin translates its native input
/// into an [`Intent`](crate::Intent) instead.
#[derive(Debug)]
pub enum DataEvent {
    /// Fresh metric snapshot from any collector.
    MetricsUpdate(MetricSet),
    /// An alert transitioned to Firing.
    AlertFired(Alert),
    /// An alert transitioned to Resolved.
    AlertResolved(Alert),
    /// Full replacement of the active alert list (e.g. on daemon reconnect).
    AlertsSnapshot(Vec<Alert>),
    /// The collector/daemon pipeline is running.
    DaemonConnected,
    /// The collector/daemon pipeline dropped, with a reason.
    DaemonDisconnected(String),
    /// Clock tick — refreshes the header timestamp.
    Tick,
    /// Latest microphone RMS levels (newest last) for the voice waveform.
    VoiceLevels(Vec<f32>),
    /// Whether the microphone is open (drives Monty's listening state).
    Listening(bool),
}
