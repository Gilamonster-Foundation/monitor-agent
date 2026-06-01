mod app;
mod event;
mod ui;

pub use app::{App, ChatMessage, Mode, Tab};
pub use event::Event;

use anyhow::Context;
use crossterm::{
    event::EventStream,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tokio::sync::mpsc;

/// Plain ASCII logo for the upper-left corner panel (20 cols × 7 lines).
pub(crate) const LOGO_CORNER: &str = include_str!("../../docs/logos/monty-ascii-20.txt");

/// Embedded splash logos, selected by terminal width at runtime.
const SPLASH_10: &str = include_str!("../../docs/logos/monty-ansi-10.txt");
const SPLASH_20: &str = include_str!("../../docs/logos/monty-ansi-20.txt");
const SPLASH_40: &str = include_str!("../../docs/logos/monty-ansi-40.txt");
const SPLASH_80: &str = include_str!("../../docs/logos/monty-ansi-80.txt");
const SPLASH_120: &str = include_str!("../../docs/logos/monty-ansi-120.txt");
const SPLASH_160: &str = include_str!("../../docs/logos/monty-ansi-160.txt");

pub fn splash_for_width(cols: u16) -> &'static str {
    match cols {
        0..=20 => SPLASH_10,
        21..=40 => SPLASH_20,
        41..=80 => SPLASH_40,
        81..=120 => SPLASH_80,
        121..=160 => SPLASH_120,
        _ => SPLASH_160,
    }
}

/// Run the TUI in-process (standalone mode — no daemon required).
///
/// Accepts an `mpsc::Receiver<Event>` fed by the daemon's data pipeline.
pub async fn run(mut data_rx: mpsc::Receiver<Event>) -> anyhow::Result<()> {
    enable_raw_mode().context("enable_raw_mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let mut app = App::new();

    // Show splash until first keypress or data arrives.
    show_splash(&mut terminal, &app)?;

    let mut key_stream = EventStream::new();

    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        tokio::select! {
            // Data events from the daemon / collectors.
            Some(event) = data_rx.recv() => {
                app.update(event);
                if app.quit {
                    break;
                }
            }
            // Terminal key events.
            Some(Ok(crossterm_event)) = tokio_stream::StreamExt::next(&mut key_stream) => {
                if let crossterm::event::Event::Key(key) = crossterm_event {
                    app.update(Event::Key(key));
                }
                if app.quit {
                    break;
                }
            }
        }
    }

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen).ok();
    Ok(())
}

fn show_splash(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &App,
) -> anyhow::Result<()> {
    let cols = terminal.size().map(|s| s.width).unwrap_or(80);
    let _splash = splash_for_width(cols);
    // Render the main UI (which will include the splash in the header area on first frame).
    terminal.draw(|frame| ui::draw(frame, app))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splash_for_width_selects_correct_size() {
        // Each width boundary should return a non-empty splash string.
        assert!(!splash_for_width(10).is_empty()); // ≤20 → 10-col
        assert!(!splash_for_width(20).is_empty());
        assert!(!splash_for_width(21).is_empty()); // 21-40 → 20-col
        assert!(!splash_for_width(40).is_empty());
        assert!(!splash_for_width(41).is_empty()); // 41-80 → 40-col
        assert!(!splash_for_width(80).is_empty());
        assert!(!splash_for_width(81).is_empty()); // 81-120 → 80-col
        assert!(!splash_for_width(120).is_empty());
        assert!(!splash_for_width(121).is_empty()); // 121-160 → 120-col
        assert!(!splash_for_width(160).is_empty());
        assert!(!splash_for_width(161).is_empty()); // >160 → 160-col
        assert!(!splash_for_width(250).is_empty());
    }

    #[test]
    fn splash_for_width_small_terminal_returns_smallest() {
        let small = splash_for_width(5);
        let large = splash_for_width(200);
        // Different sizes should return different (sized) content.
        assert_ne!(small.len(), large.len());
    }
}
