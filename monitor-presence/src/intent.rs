/// A frontend-neutral input intent.
///
/// Both the ratatui and egui skins translate their native input events
/// (crossterm keys, egui events) into this enum, so the core never sees a
/// frontend-specific type. Purely local view edits (scrolling, text-buffer
/// editing, input-mode toggles) stay in each skin and never become an
/// `Intent` — only changes to the shared model do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Quit the application.
    Quit,
    /// Select a tab by index into [`Tab::ALL`](crate::Tab::ALL).
    SelectTab(usize),
    /// Cycle the active tab by a (wrapping) delta.
    CycleTab(i32),
    /// Submit a line of chat text. In a later phase this drives the Monty
    /// mind; today it is a local echo into the chat log.
    SubmitChat(String),
}
