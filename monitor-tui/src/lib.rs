mod app;
mod event;
mod ui;

pub use app::{App, ChatMessage, Mode, Tab};
pub use event::Event;

use anyhow::Context;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self as ct_event, Event as CtEvent, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{Color as CtColor, Print, ResetColor, SetForegroundColor},
    terminal::{
        self as ct_terminal, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
        {disable_raw_mode, enable_raw_mode},
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Write};
use tokio::sync::mpsc;

/// Plain ASCII logo for the upper-left corner panel (20 cols × 7 lines).
pub(crate) const LOGO_CORNER: &str = include_str!("../../docs/logos/monty-ascii-20.txt");

/// Embedded ANSI splash art — selected by terminal width at runtime.
const SPLASH_10: &str = include_str!("../../docs/logos/monty-ansi-10.txt");
const SPLASH_20: &str = include_str!("../../docs/logos/monty-ansi-20.txt");
const SPLASH_40: &str = include_str!("../../docs/logos/monty-ansi-40.txt");
const SPLASH_80: &str = include_str!("../../docs/logos/monty-ansi-80.txt");
const SPLASH_120: &str = include_str!("../../docs/logos/monty-ansi-120.txt");
const SPLASH_160: &str = include_str!("../../docs/logos/monty-ansi-160.txt");

/// Select the widest splash art that still leaves `BRAND_MIN` cols for branding text.
fn splash_art_for_size(cols: u16) -> (&'static str, u16) {
    const BRAND_MIN: u16 = 32;
    if cols >= 160 + BRAND_MIN {
        (SPLASH_160, 160)
    } else if cols >= 120 + BRAND_MIN {
        (SPLASH_120, 120)
    } else if cols >= 80 + BRAND_MIN {
        (SPLASH_80, 80)
    } else if cols >= 40 + BRAND_MIN {
        (SPLASH_40, 40)
    } else if cols >= 20 + BRAND_MIN {
        (SPLASH_20, 20)
    } else {
        (SPLASH_10, 10)
    }
}

/// For test / splash_for_width API compatibility.
pub fn splash_for_width(cols: u16) -> &'static str {
    splash_art_for_size(cols).0
}

/// Full-screen ANSI splash — same pattern as newt-agent.
///
/// Enters the alternate screen, prints the colour logo flush to the top-left,
/// then prints branding text to the right. Blocks until the user presses any
/// key; q / Esc / Ctrl-C are treated identically to Enter (just dismiss).
/// Returns `false` only if we should quit immediately (currently unused —
/// caller always proceeds to the TUI).
fn show_ansi_splash() -> anyhow::Result<bool> {
    let (cols, _rows) = ct_terminal::size().unwrap_or((80, 24));
    let (art, art_cols) = splash_art_for_size(cols);
    let art_rows = art.lines().count() as u16;

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        Hide,
        Clear(ClearType::All),
        MoveTo(0, 0)
    )?;

    // Raw mode: \n is LF only; must use \r\n to reset column.
    write!(out, "{}", art.replace('\n', "\r\n"))?;
    out.flush()?;

    // Print branding to the right of the art if there's room.
    let brand_col = art_cols + 3;
    if brand_col + 28 < cols {
        let mid = art_rows.saturating_sub(4) / 2;

        queue!(out, MoveTo(brand_col, mid))?;
        queue!(
            out,
            SetForegroundColor(CtColor::Green),
            Print("monitor-agent"),
            ResetColor
        )?;

        queue!(out, MoveTo(brand_col, mid + 1))?;
        queue!(
            out,
            SetForegroundColor(CtColor::DarkGrey),
            Print(format!("v{}", env!("CARGO_PKG_VERSION"))),
            ResetColor
        )?;

        queue!(out, MoveTo(brand_col, mid + 2))?;
        queue!(
            out,
            SetForegroundColor(CtColor::DarkGrey),
            Print("Watches your systems."),
            ResetColor
        )?;

        queue!(out, MoveTo(brand_col, mid + 4))?;
        queue!(
            out,
            SetForegroundColor(CtColor::DarkGrey),
            Print("↵ start  ·  q quit"),
            ResetColor
        )?;

        out.flush()?;
    }

    // Block until any keypress.
    let mut quit = false;
    loop {
        if ct_event::poll(std::time::Duration::from_millis(100))? {
            match ct_event::read()? {
                CtEvent::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                })
                | CtEvent::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }) => {
                    quit = true;
                    break;
                }
                CtEvent::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => {
                    quit = true;
                    break;
                }
                CtEvent::Key(_) => break,
                _ => {}
            }
        }
    }

    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
    Ok(!quit)
}

/// Run the TUI in-process (standalone mode — no daemon required).
///
/// Accepts an `mpsc::Receiver<Event>` fed by the daemon's data pipeline.
pub async fn run(mut data_rx: mpsc::Receiver<Event>) -> anyhow::Result<()> {
    // Show the full-screen ANSI splash before entering the ratatui loop.
    // block_in_place lets us run the sync crossterm I/O without blocking
    // the tokio runtime thread.
    let cont = tokio::task::block_in_place(show_ansi_splash)?;
    if !cont {
        return Ok(());
    }

    enable_raw_mode().context("enable_raw_mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    let mut app = App::new();
    let mut key_stream = crossterm::event::EventStream::new();

    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        tokio::select! {
            Some(event) = data_rx.recv() => {
                app.update(event);
                if app.quit { break; }
            }
            Some(Ok(ce)) = tokio_stream::StreamExt::next(&mut key_stream) => {
                if let CtEvent::Key(key) = ce {
                    app.update(Event::Key(key));
                }
                if app.quit { break; }
            }
        }
    }

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splash_for_width_selects_correct_size() {
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
        assert_ne!(small.len(), large.len());
    }

    #[test]
    fn splash_art_for_size_leaves_brand_room() {
        // Each selected size should leave at least 32 cols for branding.
        for cols in [52u16, 72, 112, 152, 192, 250] {
            let (_art, art_cols) = splash_art_for_size(cols);
            assert!(
                cols >= art_cols + 32,
                "cols={cols} art_cols={art_cols}: not enough room for branding"
            );
        }
    }

    #[test]
    fn splash_art_for_size_narrow_picks_smallest() {
        let (_art, art_cols) = splash_art_for_size(30);
        assert_eq!(art_cols, 10);
    }
}
