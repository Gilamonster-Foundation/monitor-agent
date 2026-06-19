//! `monitor-tui` — the ratatui *skin* over the shared [`monitor_presence`]
//! core. It owns terminal lifecycle, the splash, rendering, and crossterm
//! input translation; the canonical state lives in [`monitor_presence`].

pub mod ansi;
mod sink;
mod skin;
mod ui;

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
use monitor_presence::{AttachRole, DataEvent, SharedPresence};
use ratatui::{backend::CrosstermBackend, Terminal};
use skin::TuiViewState;
use std::io::{self, Write};
use tokio::sync::mpsc;

/// 32-col × 16-line square-cropped portrait for the dashboard header panel.
pub(crate) const PORTRAIT: &str = include_str!("../../docs/logos/monty-ansi-portrait-32.txt");

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
/// `timeout_secs`: auto-dismiss after this many seconds (0 = wait forever).
/// Returns `false` only if the user pressed q / Esc / Ctrl-C (quit).
fn show_ansi_splash(timeout_secs: u64) -> anyhow::Result<bool> {
    use std::time::{Duration, Instant};

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

    let brand_col = art_cols + 3;
    let has_brand_room = brand_col + 28 < cols;
    let mid = if has_brand_room {
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

        out.flush()?;
        mid
    } else {
        0
    };

    // Helper: write the dismiss hint line (overwrites in place each second).
    let write_hint = |out: &mut io::Stdout, remaining: Option<u64>| -> anyhow::Result<()> {
        if !has_brand_room {
            return Ok(());
        }
        let hint = match remaining {
            Some(0) | None => "↵ start  ·  q quit".to_owned(),
            Some(s) => format!("↵ start  ·  q quit  ·  {s}s"),
        };
        // Pad to a fixed width so old digits get overwritten cleanly.
        let padded = format!("{hint:<35}");
        queue!(out, MoveTo(brand_col, mid + 4))?;
        queue!(
            out,
            SetForegroundColor(CtColor::DarkGrey),
            Print(padded),
            ResetColor
        )?;
        out.flush()?;
        Ok(())
    };

    let deadline = if timeout_secs > 0 {
        Some(Instant::now() + Duration::from_secs(timeout_secs))
    } else {
        None
    };

    // Write the initial hint.
    write_hint(
        &mut out,
        deadline.map(|d| d.saturating_duration_since(Instant::now()).as_secs()),
    )?;

    let mut quit = false;
    let mut last_secs_remaining = timeout_secs;

    loop {
        // How long until the next event poll should return at the latest.
        let poll_dur = Duration::from_millis(100);

        // Check timeout.
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                break; // auto-dismiss
            }
            // Update countdown once per second.
            let secs_left = dl.saturating_duration_since(Instant::now()).as_secs();
            if secs_left != last_secs_remaining {
                last_secs_remaining = secs_left;
                write_hint(&mut out, Some(secs_left))?;
            }
        }

        if ct_event::poll(poll_dur)? {
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
/// `splash_timeout_secs`: passed to the splash screen (0 = wait forever).
/// Accepts an `mpsc::Receiver<DataEvent>` fed by the daemon's data pipeline.
/// This is one *skin*: it renders the shared [`MontyPresence`] and feeds
/// frontend-neutral intents into it. The canonical state lives in the presence.
pub async fn run(
    mut data_rx: mpsc::Receiver<DataEvent>,
    splash_timeout_secs: u64,
) -> anyhow::Result<()> {
    let cont = tokio::task::block_in_place(move || show_ansi_splash(splash_timeout_secs))?;
    if !cont {
        return Ok(());
    }

    enable_raw_mode().context("enable_raw_mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    // One shared presence: the collector feed and this skin both touch it
    // through `SharedPresence` (a second egui skin will share the same handle).
    let shared = SharedPresence::new();
    let mut view = TuiViewState::new();
    let mut key_stream = crossterm::event::EventStream::new();

    // Attach this skin as an Observer of the session fan-out. Agent output
    // (from the Monty mind, a later phase) is delivered to `sink`, forwarded
    // over `transcript_rx`, and folded into the transcript here. A second
    // (egui) skin attaches its own sink the same way — one turn, every skin.
    let (sink, mut transcript_rx) = sink::RatatuiSink::new();
    shared.with_mut(|p| p.attach_sink(AttachRole::Observer, Box::new(sink)));

    loop {
        // Render from a cheap snapshot — never hold the shared lock across draw.
        let snapshot = shared.snapshot();
        terminal.draw(|frame| ui::draw(frame, &snapshot, &view))?;

        tokio::select! {
            Some(event) = data_rx.recv() => {
                shared.apply(event);
                if shared.should_quit() { break; }
            }
            Some(Ok(ce)) = tokio_stream::StreamExt::next(&mut key_stream) => {
                if let CtEvent::Key(key) = ce {
                    if let Some(intent) = skin::translate_key(&mut view, key) {
                        shared.submit_intent(intent);
                    }
                }
                if shared.should_quit() { break; }
            }
            Some(chunk) = transcript_rx.recv() => {
                shared.with_mut(|p| p.fold_output(&chunk));
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
