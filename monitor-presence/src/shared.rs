//! [`SharedPresence`] — a thread-safe handle to one [`MontyPresence`].
//!
//! The collector tasks and the active skin (ratatui or egui) share a single
//! presence through this. Mutations lock briefly; a read takes a cheap
//! [`PresenceModel`] *snapshot* under the lock and then renders lock-free, so a
//! skin never holds the lock across a render frame. This is the concurrency
//! wrapper from `docs/design/inhabit-both-surfaces.md` §4.3 (option A —
//! `Arc<Mutex>` + snapshot reads); it keeps this crate dependency-free (std
//! only). The single-owner actor model (option B) remains a drop-in alternative
//! if egui frame-lock contention is ever measured.

use std::sync::{Arc, Mutex};

use crate::{DataEvent, Intent, MontyPresence, PresenceModel};

/// A cloneable, thread-safe handle to one shared [`MontyPresence`].
#[derive(Clone)]
pub struct SharedPresence(Arc<Mutex<MontyPresence>>);

impl SharedPresence {
    /// Wrap a fresh presence.
    pub fn new() -> Self {
        Self::from_presence(MontyPresence::new())
    }

    /// Wrap an existing presence (e.g. one with sinks already attached).
    pub fn from_presence(presence: MontyPresence) -> Self {
        Self(Arc::new(Mutex::new(presence)))
    }

    /// Apply a data event from the collector/daemon pipeline (locks briefly).
    pub fn apply(&self, event: DataEvent) {
        self.0.lock().expect("presence mutex poisoned").apply(event);
    }

    /// Submit a frontend-neutral intent from a skin (locks briefly).
    pub fn submit_intent(&self, intent: Intent) {
        self.0
            .lock()
            .expect("presence mutex poisoned")
            .submit_intent(intent);
    }

    /// Take a cheap snapshot of the canonical model for rendering — cloned
    /// under the lock, so the caller renders without holding it.
    pub fn snapshot(&self) -> PresenceModel {
        self.0
            .lock()
            .expect("presence mutex poisoned")
            .model()
            .clone()
    }

    /// Whether the application has been asked to quit.
    pub fn should_quit(&self) -> bool {
        self.0
            .lock()
            .expect("presence mutex poisoned")
            .should_quit()
    }

    /// Run a closure with exclusive access to the presence — for operations
    /// beyond apply/intent (attach a sink, fold agent output, drive the
    /// session). Holds the lock only for the closure's duration.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut MontyPresence) -> R) -> R {
        f(&mut self.0.lock().expect("presence mutex poisoned"))
    }
}

impl Default for SharedPresence {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{AttachRole, OutputChunk, OutputSink, OutputStream};
    use monitor_core::metrics::MetricSet;
    use std::thread;

    #[test]
    fn snapshot_reflects_applied_events() {
        let shared = SharedPresence::new();
        shared.apply(DataEvent::DaemonConnected);
        assert!(shared.snapshot().daemon_connected);
    }

    #[test]
    fn submit_intent_mutates_through_the_handle() {
        let shared = SharedPresence::new();
        shared.submit_intent(Intent::Quit);
        assert!(shared.should_quit());
    }

    #[test]
    fn concurrent_apply_and_snapshot_do_not_race() {
        // A writer thread floods the presence with metric updates while the
        // main thread snapshots concurrently. The Mutex serializes access, so
        // this completes without a data race or panic.
        let shared = SharedPresence::new();
        let writer = shared.clone();
        let h = thread::spawn(move || {
            for i in 0..1000 {
                let mut m = MetricSet::new("gnuc");
                m.insert("cpu.percent", i as f64);
                writer.apply(DataEvent::MetricsUpdate(m));
            }
        });
        for _ in 0..1000 {
            let _ = shared.snapshot();
        }
        h.join().unwrap();
        assert!(shared.snapshot().metrics.contains_key("gnuc"));
    }

    #[test]
    fn with_mut_drives_the_session_fanout() {
        // The session still fans out correctly through the shared handle: an
        // attached observer sink receives an emitted chunk.
        #[derive(Clone, Default)]
        struct Collector(Arc<Mutex<Vec<OutputChunk>>>);
        impl OutputSink for Collector {
            fn deliver(&mut self, chunk: &OutputChunk) {
                self.0.lock().unwrap().push(chunk.clone());
            }
        }

        let shared = SharedPresence::new();
        let collector = Collector::default();
        let seen = collector.0.clone();
        shared.with_mut(|p| {
            p.attach_sink(AttachRole::Observer, Box::new(collector));
            let driver = p
                .session_mut()
                .attach(AttachRole::Driver, Box::new(Collector::default()));
            p.session_mut().submit_input(driver, "go").unwrap();
            p.session_mut().emit(OutputStream::AgentThought, "hi", true);
        });
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
