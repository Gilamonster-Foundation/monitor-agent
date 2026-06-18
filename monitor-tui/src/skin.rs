//! The ratatui skin's per-frontend input + view state.
//!
//! `monitor-tui` is one *skin* over the shared [`monitor_presence`] core: it
//! renders [`PresenceModel`](monitor_presence::PresenceModel) and translates
//! crossterm key events into frontend-neutral [`Intent`]s. Viewport and editor
//! state that never belongs in the shared model lives here in [`TuiViewState`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use monitor_presence::Intent;

/// Whether the user is typing in the chat field or navigating the dashboard.
///
/// A ratatui-input concept (egui expresses the same idea via widget focus), so
/// it lives in the skin, not the shared model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Chat,
}

/// Per-skin view state — viewport offset, input mode, and the chat editor
/// buffer. None of this belongs in the shared [`PresenceModel`].
pub struct TuiViewState {
    pub scroll_offset: usize,
    pub mode: Mode,
    pub chat_input: String,
}

impl TuiViewState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            mode: Mode::Normal,
            chat_input: String::new(),
        }
    }
}

impl Default for TuiViewState {
    fn default() -> Self {
        Self::new()
    }
}

/// Translate a crossterm key into an optional [`Intent`], applying any purely
/// local (per-skin) view edits in place. Returns `Some(intent)` only when the
/// shared model must change.
pub fn translate_key(view: &mut TuiViewState, key: KeyEvent) -> Option<Intent> {
    // Ctrl+C always quits, regardless of mode.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Intent::Quit);
    }
    match view.mode {
        Mode::Chat => translate_chat_key(view, key),
        Mode::Normal => translate_normal_key(view, key),
    }
}

fn translate_chat_key(view: &mut TuiViewState, key: KeyEvent) -> Option<Intent> {
    match key.code {
        KeyCode::Esc => {
            view.mode = Mode::Normal;
            view.chat_input.clear();
            None
        }
        KeyCode::Enter => {
            let text = view.chat_input.trim().to_owned();
            view.chat_input.clear();
            if text.is_empty() {
                None
            } else {
                Some(Intent::SubmitChat(text))
            }
        }
        KeyCode::Backspace => {
            view.chat_input.pop();
            None
        }
        KeyCode::Char(c) => {
            view.chat_input.push(c);
            None
        }
        _ => None,
    }
}

fn translate_normal_key(view: &mut TuiViewState, key: KeyEvent) -> Option<Intent> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(Intent::Quit),
        // '/' opens the chat field (familiar search-bar convention).
        KeyCode::Char('/') => {
            view.mode = Mode::Chat;
            view.chat_input.clear();
            None
        }
        KeyCode::Char('1') => Some(Intent::SelectTab(0)),
        KeyCode::Char('2') => Some(Intent::SelectTab(1)),
        KeyCode::Char('3') => Some(Intent::SelectTab(2)),
        KeyCode::Char('4') => Some(Intent::SelectTab(3)),
        KeyCode::Tab => {
            view.scroll_offset = 0;
            Some(Intent::CycleTab(1))
        }
        KeyCode::BackTab => {
            view.scroll_offset = 0;
            Some(Intent::CycleTab(-1))
        }
        KeyCode::Down | KeyCode::Char('j') => {
            view.scroll_offset = view.scroll_offset.saturating_add(1);
            None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            view.scroll_offset = view.scroll_offset.saturating_sub(1);
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_presence::Intent;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn q_translates_to_quit() {
        let mut v = TuiViewState::new();
        assert_eq!(
            translate_key(&mut v, key(KeyCode::Char('q'))),
            Some(Intent::Quit)
        );
    }

    #[test]
    fn uppercase_q_quits() {
        let mut v = TuiViewState::new();
        assert_eq!(
            translate_key(&mut v, key(KeyCode::Char('Q'))),
            Some(Intent::Quit)
        );
    }

    #[test]
    fn ctrl_c_quits_in_any_mode() {
        let mut v = TuiViewState::new();
        v.mode = Mode::Chat;
        let e = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(translate_key(&mut v, e), Some(Intent::Quit));
    }

    #[test]
    fn number_keys_select_tabs() {
        let mut v = TuiViewState::new();
        assert_eq!(
            translate_key(&mut v, key(KeyCode::Char('1'))),
            Some(Intent::SelectTab(0))
        );
        assert_eq!(
            translate_key(&mut v, key(KeyCode::Char('2'))),
            Some(Intent::SelectTab(1))
        );
        assert_eq!(
            translate_key(&mut v, key(KeyCode::Char('3'))),
            Some(Intent::SelectTab(2))
        );
        assert_eq!(
            translate_key(&mut v, key(KeyCode::Char('4'))),
            Some(Intent::SelectTab(3))
        );
    }

    #[test]
    fn tab_cycles_and_resets_scroll() {
        let mut v = TuiViewState::new();
        v.scroll_offset = 5;
        assert_eq!(
            translate_key(&mut v, key(KeyCode::Tab)),
            Some(Intent::CycleTab(1))
        );
        assert_eq!(v.scroll_offset, 0);
        assert_eq!(
            translate_key(&mut v, key(KeyCode::BackTab)),
            Some(Intent::CycleTab(-1))
        );
    }

    #[test]
    fn scroll_keys_edit_local_offset() {
        let mut v = TuiViewState::new();
        assert_eq!(translate_key(&mut v, key(KeyCode::Down)), None);
        assert_eq!(v.scroll_offset, 1);
        assert_eq!(translate_key(&mut v, key(KeyCode::Char('j'))), None);
        assert_eq!(v.scroll_offset, 2);
        translate_key(&mut v, key(KeyCode::Up));
        translate_key(&mut v, key(KeyCode::Char('k')));
        assert_eq!(v.scroll_offset, 0);
        // Saturating — does not go negative.
        translate_key(&mut v, key(KeyCode::Up));
        assert_eq!(v.scroll_offset, 0);
    }

    #[test]
    fn slash_enters_chat_mode() {
        let mut v = TuiViewState::new();
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(translate_key(&mut v, key(KeyCode::Char('/'))), None);
        assert_eq!(v.mode, Mode::Chat);
    }

    #[test]
    fn chat_esc_returns_to_normal() {
        let mut v = TuiViewState::new();
        v.mode = Mode::Chat;
        translate_key(&mut v, key(KeyCode::Esc));
        assert_eq!(v.mode, Mode::Normal);
    }

    #[test]
    fn chat_typing_builds_input() {
        let mut v = TuiViewState::new();
        v.mode = Mode::Chat;
        for c in "hello".chars() {
            translate_key(&mut v, key(KeyCode::Char(c)));
        }
        assert_eq!(v.chat_input, "hello");
    }

    #[test]
    fn chat_backspace_deletes() {
        let mut v = TuiViewState::new();
        v.mode = Mode::Chat;
        v.chat_input = "helo".into();
        translate_key(&mut v, key(KeyCode::Backspace));
        assert_eq!(v.chat_input, "hel");
    }

    #[test]
    fn chat_enter_submits_and_clears() {
        let mut v = TuiViewState::new();
        v.mode = Mode::Chat;
        v.chat_input = "test message".into();
        let intent = translate_key(&mut v, key(KeyCode::Enter));
        assert_eq!(intent, Some(Intent::SubmitChat("test message".into())));
        assert!(v.chat_input.is_empty());
    }

    #[test]
    fn chat_empty_enter_is_no_op() {
        let mut v = TuiViewState::new();
        v.mode = Mode::Chat;
        assert_eq!(translate_key(&mut v, key(KeyCode::Enter)), None);
    }

    #[test]
    fn q_in_chat_is_text_not_quit() {
        let mut v = TuiViewState::new();
        v.mode = Mode::Chat;
        assert_eq!(translate_key(&mut v, key(KeyCode::Char('q'))), None);
        assert_eq!(v.chat_input, "q");
    }
}
