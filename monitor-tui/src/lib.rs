mod app;
mod event;
mod ui;

pub use app::{App, Tab};
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
