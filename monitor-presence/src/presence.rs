//! [`MontyPresence`] — one Monty presence that every skin renders and acts
//! through.
//!
//! It owns the canonical [`PresenceModel`], the [`Intent`] intake, and the
//! [`SessionState`] output fan-out that lets one agent turn reach every
//! attached skin. Later phases bind the read-only object-capability key and the
//! agent "mind" (as the session's sole `Driver`) here — without changing the
//! skins, which only ever observe the model + the fan-out and submit intents.

use crate::session::{AttachId, AttachRole, OutputChunk, OutputSink, SessionState};
use crate::{ChatMessage, DataEvent, Intent, PresenceModel, Tab};

/// The chat "brain" seam. The active responder turns a user line into Monty's
/// reply. [`StubResponder`] is the plumbing-first default; a real brain (an LLM,
/// or gilabot's agent) swaps in via [`MontyPresence::set_responder`]. A
/// streaming brain can instead drive the session fan-out and let chunks fold in.
pub trait ChatResponder: Send {
    /// Reply to `user_text` (may use live `model` context), or `None` to stay
    /// silent. Must return promptly — a slow/real brain should stream via the
    /// session rather than block here.
    fn respond(&mut self, user_text: &str, model: &PresenceModel) -> Option<String>;
}

/// Plumbing-first placeholder: a canned, context-aware acknowledgement so the
/// chat loop is visible end-to-end before a real brain is wired.
#[derive(Debug, Default, Clone, Copy)]
pub struct StubResponder;

impl ChatResponder for StubResponder {
    fn respond(&mut self, user_text: &str, model: &PresenceModel) -> Option<String> {
        Some(format!(
            "(stub) heard \"{user_text}\" — watching {} host(s), {} active alert(s). A real brain wires in next.",
            model.metrics.len(),
            model.active_alert_count,
        ))
    }
}

/// The shared presence: canonical state, the frontend-neutral intent intake,
/// and the session output fan-out. Skins read [`model`](Self::model), call
/// [`submit_intent`](Self::submit_intent), and attach a sink via
/// [`attach_sink`](Self::attach_sink); they never mutate the model directly.
pub struct MontyPresence {
    model: PresenceModel,
    session: SessionState,
    responder: Box<dyn ChatResponder>,
}

impl MontyPresence {
    pub fn new() -> Self {
        Self {
            model: PresenceModel::new(),
            session: SessionState::default(),
            responder: Box::new(StubResponder),
        }
    }

    /// Borrow the canonical model for rendering.
    pub fn model(&self) -> &PresenceModel {
        &self.model
    }

    /// Whether the application has been asked to quit.
    pub fn should_quit(&self) -> bool {
        self.model.quit
    }

    /// Apply a data event from the collector/daemon pipeline.
    pub fn apply(&mut self, event: DataEvent) {
        self.model.apply(event);
    }

    /// Consume a frontend-neutral intent, mutating the shared model.
    pub fn submit_intent(&mut self, intent: Intent) {
        match intent {
            Intent::Quit => self.model.quit = true,
            Intent::SelectTab(i) => {
                if let Some(&tab) = Tab::ALL.get(i) {
                    self.model.active_tab = tab;
                }
            }
            Intent::CycleTab(delta) => self.model.cycle_tab(delta),
            Intent::SubmitChat(text) => {
                let text = text.trim();
                if !text.is_empty() {
                    self.model.chat_log.push(ChatMessage {
                        from: "you".into(),
                        text: text.to_owned(),
                    });
                    // The current responder is the chat "brain". The stub
                    // replies inline; a streaming brain will instead drive the
                    // session and fold chunks in via [`fold_output`].
                    if let Some(reply) = self.responder.respond(text, &self.model) {
                        self.model.chat_log.push(ChatMessage {
                            from: "monty".into(),
                            text: reply,
                        });
                    }
                    self.model.chat_log.truncate(200);
                }
            }
        }
    }

    /// Swap the chat brain. The plumbing-first default is [`StubResponder`];
    /// a real brain (LLM / gilabot agent) replaces it here.
    pub fn set_responder(&mut self, responder: Box<dyn ChatResponder>) {
        self.responder = responder;
    }

    // --- session fan-out -----------------------------------------------------

    /// Borrow the session fan-out (read-only: counts, replay).
    pub fn session(&self) -> &SessionState {
        &self.session
    }

    /// Borrow the session fan-out mutably (drive emits / submit input). The
    /// Monty mind (a later phase) drives turns through this as the sole Driver.
    pub fn session_mut(&mut self) -> &mut SessionState {
        &mut self.session
    }

    /// Attach a skin's output sink to the session as `role`. Returns its handle.
    pub fn attach_sink(&mut self, role: AttachRole, sink: Box<dyn OutputSink>) -> AttachId {
        self.session.attach(role, sink)
    }

    /// Fold an agent output chunk into the rendered transcript (a "monty" line).
    /// Skins call this as they drain their sink's delivered chunks.
    pub fn fold_output(&mut self, chunk: &OutputChunk) {
        self.model.chat_log.push(ChatMessage {
            from: "monty".into(),
            text: chunk.data.clone(),
        });
        self.model.chat_log.truncate(200);
    }
}

impl Default for MontyPresence {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{AttachRole, OutputStream};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Collector(Arc<Mutex<Vec<OutputChunk>>>);
    impl OutputSink for Collector {
        fn deliver(&mut self, chunk: &OutputChunk) {
            self.0.lock().unwrap().push(chunk.clone());
        }
    }
    impl Collector {
        fn count(&self) -> usize {
            self.0.lock().unwrap().len()
        }
    }

    #[test]
    fn quit_intent_sets_quit() {
        let mut p = MontyPresence::new();
        assert!(!p.should_quit());
        p.submit_intent(Intent::Quit);
        assert!(p.should_quit());
    }

    #[test]
    fn select_tab_sets_active_tab() {
        let mut p = MontyPresence::new();
        p.submit_intent(Intent::SelectTab(0));
        assert_eq!(p.model().active_tab, Tab::Alerts);
        p.submit_intent(Intent::SelectTab(1));
        assert_eq!(p.model().active_tab, Tab::Metrics);
    }

    #[test]
    fn select_tab_out_of_range_is_no_op() {
        let mut p = MontyPresence::new();
        let before = p.model().active_tab;
        p.submit_intent(Intent::SelectTab(99));
        assert_eq!(p.model().active_tab, before);
    }

    #[test]
    fn cycle_tab_intent_wraps() {
        let mut p = MontyPresence::new();
        p.submit_intent(Intent::SelectTab(0)); // Alerts
        p.submit_intent(Intent::CycleTab(-1));
        assert_eq!(p.model().active_tab, Tab::Rules); // wraps back off the front
    }

    #[test]
    fn submit_chat_appends_user_then_stub_reply() {
        let mut p = MontyPresence::new();
        p.submit_intent(Intent::SubmitChat("status?".into()));
        assert_eq!(p.model().chat_log.len(), 2);
        assert_eq!(p.model().chat_log[0].from, "you");
        assert_eq!(p.model().chat_log[0].text, "status?");
        assert_eq!(p.model().chat_log[1].from, "monty");
        assert!(!p.model().chat_log[1].text.is_empty());
    }

    #[test]
    fn submit_chat_blank_is_no_op() {
        let mut p = MontyPresence::new();
        p.submit_intent(Intent::SubmitChat("   ".into()));
        assert!(p.model().chat_log.is_empty());
    }

    #[test]
    fn set_responder_swaps_the_brain() {
        struct Silent;
        impl ChatResponder for Silent {
            fn respond(&mut self, _t: &str, _m: &PresenceModel) -> Option<String> {
                None
            }
        }
        let mut p = MontyPresence::new();
        p.set_responder(Box::new(Silent));
        p.submit_intent(Intent::SubmitChat("hi".into()));
        // Only the user line — the silent brain adds no reply.
        assert_eq!(p.model().chat_log.len(), 1);
        assert_eq!(p.model().chat_log[0].from, "you");
    }

    #[test]
    fn apply_delegates_to_model() {
        let mut p = MontyPresence::new();
        p.apply(DataEvent::DaemonConnected);
        assert!(p.model().daemon_connected);
    }

    #[test]
    fn fold_output_appends_monty_line() {
        let mut p = MontyPresence::new();
        let chunk = OutputChunk {
            turn: 1,
            stream: OutputStream::AgentThought,
            seq: 0,
            data: "all clear.".into(),
            last: true,
        };
        p.fold_output(&chunk);
        assert_eq!(p.model().chat_log.len(), 1);
        assert_eq!(p.model().chat_log[0].from, "monty");
        assert_eq!(p.model().chat_log[0].text, "all clear.");
    }

    #[test]
    fn attached_observer_receives_fanned_output() {
        // A synthetic agent turn fans out to an attached observer sink, the way
        // a skin's sink will once the Monty mind drives turns.
        let mut p = MontyPresence::new();
        let collector = Collector::default();
        p.attach_sink(AttachRole::Observer, Box::new(collector.clone()));
        let driver = p
            .session_mut()
            .attach(AttachRole::Driver, Box::new(Collector::default()));
        p.session_mut().submit_input(driver, "go").unwrap();
        p.session_mut()
            .emit(OutputStream::AgentThought, "thinking…", true);
        assert_eq!(collector.count(), 1, "observer sink received the chunk");
    }
}
