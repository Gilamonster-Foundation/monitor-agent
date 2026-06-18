//! [`MontyPresence`] — one Monty presence that every skin renders and acts
//! through.
//!
//! In this phase it owns the canonical [`PresenceModel`] and the
//! [`Intent`] intake. Later phases bind the read-only object-capability key, a
//! `newt-core` session output fan-out, and the agent "mind" here — without
//! changing the skins, which only ever observe the model and submit intents.

use crate::{ChatMessage, DataEvent, Intent, PresenceModel, Tab};

/// The shared presence: canonical state plus the frontend-neutral intent
/// intake. Skins hold a reference to read [`model`](Self::model) and call
/// [`submit_intent`](Self::submit_intent); they never mutate the model
/// directly.
pub struct MontyPresence {
    model: PresenceModel,
}

impl MontyPresence {
    pub fn new() -> Self {
        Self {
            model: PresenceModel::new(),
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
                // Local-echo stub: the Monty mind attaches here in a later phase.
                let text = text.trim();
                if !text.is_empty() {
                    self.model.chat_log.push(ChatMessage {
                        from: "you".into(),
                        text: text.to_owned(),
                    });
                    self.model.chat_log.truncate(200);
                }
            }
        }
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
    fn submit_chat_appends_user_line() {
        let mut p = MontyPresence::new();
        p.submit_intent(Intent::SubmitChat("status?".into()));
        assert_eq!(p.model().chat_log.len(), 1);
        assert_eq!(p.model().chat_log[0].from, "you");
        assert_eq!(p.model().chat_log[0].text, "status?");
    }

    #[test]
    fn submit_chat_blank_is_no_op() {
        let mut p = MontyPresence::new();
        p.submit_intent(Intent::SubmitChat("   ".into()));
        assert!(p.model().chat_log.is_empty());
    }

    #[test]
    fn apply_delegates_to_model() {
        let mut p = MontyPresence::new();
        p.apply(DataEvent::DaemonConnected);
        assert!(p.model().daemon_connected);
    }
}
