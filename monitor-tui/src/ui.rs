use crate::ansi::ansi_to_text;
use crate::app::{App, Mode, Tab};
use monitor_core::alert::Severity;
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame,
};

/// Minimum terminal width before the logo column is hidden.
const LOGO_MIN_WIDTH: u16 = 50;
/// Fixed column width reserved for the Monty logo.
const LOGO_WIDTH: u16 = 21;
/// Height of the chat panel (history + input line).
const CHAT_HEIGHT: u16 = 3;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Outer vertical split: [top] [chat] [status]
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(CHAT_HEIGHT),
            Constraint::Length(1),
        ])
        .split(area);

    let top_area = outer[0];
    let chat_area = outer[1];
    let status_area = outer[2];

    // Top horizontal split: [logo] [right]
    let show_logo = area.width >= LOGO_MIN_WIDTH;
    let logo_w = if show_logo { LOGO_WIDTH } else { 0 };
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(logo_w), Constraint::Min(20)])
        .split(top_area);

    let logo_area = top[0];
    let right_area = top[1];

    // Right vertical split: [header] [tabs] [content]
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(5),
        ])
        .split(right_area);

    if show_logo {
        draw_logo(frame, logo_area);
    }
    draw_header(frame, app, right[0]);
    draw_tabs(frame, app, right[1]);
    draw_content(frame, app, right[2]);
    draw_chat(frame, app, chat_area);
    draw_status_bar(frame, app, status_area);
}

// ---------------------------------------------------------------------------
// Logo
// ---------------------------------------------------------------------------

/// 20-col ANSI art embedded at compile time and parsed into ratatui Text.
fn logo_text() -> ratatui::text::Text<'static> {
    static ART: &str = include_str!("../../docs/logos/monty-ansi-20.txt");
    ansi_to_text(ART)
}

fn draw_logo(frame: &mut Frame, area: Rect) {
    frame.render_widget(Paragraph::new(logo_text()), area);
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let daemon_indicator = if app.daemon_connected {
        Span::styled("● daemon:ok", Style::default().fg(Color::Green))
    } else {
        Span::styled("● daemon:…", Style::default().fg(Color::DarkGray))
    };

    let alert_badge = if app.active_alert_count > 0 {
        Span::styled(
            format!("  ⚠ {} alert(s)", app.active_alert_count),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("  ✓ ok", Style::default().fg(Color::Green))
    };

    let time = Span::styled(
        format!("  {}", app.now),
        Style::default().fg(Color::DarkGray),
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![daemon_indicator, alert_badge, time])),
        area,
    );
}

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| {
            let label = if *t == Tab::Alerts && app.active_alert_count > 0 {
                format!("{} ⚠{}", t.label(), app.active_alert_count)
            } else {
                t.label().into()
            };
            Line::from(label)
        })
        .collect();

    let selected = Tab::ALL
        .iter()
        .position(|t| *t == app.active_tab)
        .unwrap_or(0);

    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .divider("|"),
        area,
    );
}

// ---------------------------------------------------------------------------
// Tab content
// ---------------------------------------------------------------------------

fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    match app.active_tab {
        Tab::Alerts => draw_alerts_tab(frame, app, area),
        Tab::Metrics => draw_metrics_tab(frame, app, area),
        Tab::History => draw_history_tab(frame, app, area),
        Tab::Rules => draw_rules_tab(frame, area),
    }
}

fn draw_alerts_tab(frame: &mut Frame, app: &App, area: Rect) {
    if app.active_alerts.is_empty() {
        frame.render_widget(
            Paragraph::new("No active alerts.")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Active Alerts"),
                )
                .style(Style::default().fg(Color::Green)),
            area,
        );
        return;
    }

    let header = Row::new(vec!["Sev", "Target", "Metric", "Value", "Firing for"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .active_alerts
        .iter()
        .map(|a| {
            let color = severity_color(a.severity);
            let icon = match a.severity {
                Severity::Critical => "●",
                Severity::Warn => "⚠",
                Severity::Info => "ℹ",
            };
            let duration = a
                .fired_at_secs
                .map(|t| {
                    let secs = chrono::Utc::now().timestamp() - t;
                    format!("{}m{}s", secs / 60, secs % 60)
                })
                .unwrap_or_default();

            Row::new(vec![
                Cell::from(format!("{icon} {}", a.severity)).style(Style::default().fg(color)),
                Cell::from(a.target.as_str()),
                Cell::from(a.metric.as_str()),
                Cell::from(format!("{:.1}", a.value)),
                Cell::from(duration),
            ])
        })
        .collect();

    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Length(14),
                Constraint::Min(18),
                Constraint::Length(7),
                Constraint::Length(10),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
    );
}

fn draw_metrics_tab(frame: &mut Frame, app: &App, area: Rect) {
    if app.metrics.is_empty() {
        frame.render_widget(
            Paragraph::new("Waiting for first poll…")
                .block(Block::default().borders(Borders::ALL).title("Metrics"))
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let mut targets: Vec<&str> = app.metrics.keys().map(String::as_str).collect();
    targets.sort();

    let lines: Vec<Line> = targets
        .iter()
        .map(|target| {
            let m = &app.metrics[*target];
            let cpu = m
                .get(&"cpu.percent".into())
                .map(|v| format!("{v:.0}%"))
                .unwrap_or_else(|| "n/a".into());
            let mem = m
                .get(&"memory.percent".into())
                .map(|v| format!("{v:.0}%"))
                .unwrap_or_else(|| "n/a".into());
            let dsk = m
                .get(&"disk.used_pct".into())
                .map(|v| format!("{v:.0}%"))
                .unwrap_or_else(|| "n/a".into());
            let gpu = m
                .get(&"gpu.util_pct".into())
                .map(|v| format!("  GPU {v:.0}%"))
                .unwrap_or_default();

            Line::from(vec![
                Span::styled(
                    format!("{target:<14}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("CPU {cpu:<6}  MEM {mem:<6}  DSK {dsk:<6}{gpu}")),
            ])
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Metrics"))
            .scroll((app.scroll_offset as u16, 0)),
        area,
    );
}

fn draw_history_tab(frame: &mut Frame, app: &App, area: Rect) {
    if app.resolved_alerts.is_empty() {
        frame.render_widget(
            Paragraph::new("No resolved alerts this session.")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Alert History"),
                )
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let header = Row::new(vec!["Target", "Metric", "Resolved"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .resolved_alerts
        .iter()
        .map(|a| {
            let resolved = a
                .resolved_at_secs
                .map(|t| {
                    let secs = chrono::Utc::now().timestamp() - t;
                    format!("{}m ago", secs / 60)
                })
                .unwrap_or_default();
            Row::new(vec![
                Cell::from(a.target.as_str()).style(Style::default().fg(Color::DarkGray)),
                Cell::from(a.metric.as_str()),
                Cell::from(resolved),
            ])
        })
        .collect();

    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(14),
                Constraint::Min(22),
                Constraint::Length(12),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Alert History"),
        ),
        area,
    );
}

fn draw_rules_tab(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new("Rules are defined in monitor-agent.toml.\nReload with 'r'.")
            .block(Block::default().borders(Borders::ALL).title("Alert Rules"))
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

// ---------------------------------------------------------------------------
// Chat panel
// ---------------------------------------------------------------------------

fn draw_chat(frame: &mut Frame, app: &App, area: Rect) {
    // Split chat area: [history] [input line]
    let chat_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let history_area = chat_layout[0];
    let input_area = chat_layout[1];

    // History — most recent messages, one per line.
    let max_history = history_area.height as usize;
    let recent = app.recent_chat(max_history);
    let history_lines: Vec<Line> = recent
        .iter()
        .map(|msg| {
            let (label, color) = if msg.from == "you" {
                ("you  ", Color::Cyan)
            } else {
                ("monty", Color::Green)
            };
            Line::from(vec![
                Span::styled(
                    format!("{label}: "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(msg.text.as_str()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(history_lines), history_area);

    // Input line.
    let (prompt, input_style) = match app.mode {
        Mode::Chat => (
            format!("> {}", app.chat_input),
            Style::default().fg(Color::Yellow),
        ),
        Mode::Normal => (
            "> / to chat  esc to exit".into(),
            Style::default().fg(Color::DarkGray),
        ),
    };
    frame.render_widget(
        Paragraph::new(prompt.as_str()).style(input_style),
        input_area,
    );

    // Show cursor when actively typing.
    if app.mode == Mode::Chat {
        let cursor_x = input_area.x + 2 + app.chat_input.len() as u16;
        frame.set_cursor_position(Position::new(
            cursor_x.min(input_area.x + input_area.width.saturating_sub(1)),
            input_area.y,
        ));
    }
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mode_tag = match app.mode {
        Mode::Chat => Span::styled(
            " CHAT ",
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ),
        Mode::Normal => Span::styled(
            " NORM ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
    };

    let line = Line::from(vec![
        mode_tag,
        Span::raw("  "),
        Span::styled(
            if app.daemon_connected {
                "daemon:ok"
            } else {
                "daemon:err"
            },
            Style::default().fg(if app.daemon_connected {
                Color::Green
            } else {
                Color::Red
            }),
        ),
        Span::raw(format!("  collectors:{}", app.metrics.len())),
        Span::styled("  q:quit", Style::default().fg(Color::DarkGray)),
        Span::styled("  1-4:tabs", Style::default().fg(Color::DarkGray)),
        Span::styled("  /:chat", Style::default().fg(Color::DarkGray)),
        Span::styled("  ↑↓:scroll", Style::default().fg(Color::DarkGray)),
    ]);

    frame.render_widget(Paragraph::new(line), area);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn severity_color(s: Severity) -> Color {
    match s {
        Severity::Critical => Color::Red,
        Severity::Warn => Color::Yellow,
        Severity::Info => Color::Blue,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, ChatMessage};
    use crate::event::Event;
    use monitor_core::alert::{Alert, AlertId, AlertState, Severity};
    use monitor_core::metrics::{MetricPath, MetricSet};
    use ratatui::{backend::TestBackend, Terminal};
    use uuid::Uuid;

    fn test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(width, height)).unwrap()
    }

    fn firing_alert(severity: Severity, target: &str, rule: &str) -> Alert {
        Alert {
            id: AlertId::for_rule(target, rule),
            uuid: Uuid::new_v4(),
            rule_name: rule.to_owned(),
            target: target.to_owned(),
            metric: MetricPath::new("cpu.percent"),
            value: 90.0,
            severity,
            state: AlertState::Firing,
            message: format!("{target}: CPU at 90%"),
            fired_at_secs: Some(chrono::Utc::now().timestamp() - 120),
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        }
    }

    fn resolved_alert() -> Alert {
        Alert {
            id: AlertId::for_rule("gnuc", "old-rule"),
            uuid: Uuid::new_v4(),
            rule_name: "old-rule".into(),
            target: "gnuc".into(),
            metric: MetricPath::new("memory.percent"),
            value: 95.0,
            severity: Severity::Warn,
            state: AlertState::Resolved,
            message: "gnuc: mem at 95%".into(),
            fired_at_secs: Some(chrono::Utc::now().timestamp() - 600),
            resolved_at_secs: Some(chrono::Utc::now().timestamp() - 60),
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        }
    }

    fn metrics_for(target: &str) -> MetricSet {
        let mut m = MetricSet::new(target);
        m.insert("cpu.percent", 72.0);
        m.insert("memory.percent", 48.0);
        m.insert("disk.used_pct", 35.0);
        m.insert("gpu.util_pct", 88.0);
        m
    }

    #[test]
    fn draw_empty_app_does_not_panic() {
        let mut term = test_terminal(80, 24);
        let app = App::new();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_narrow_terminal_hides_logo() {
        let mut term = test_terminal(40, 20);
        let app = App::new();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_wide_terminal_does_not_panic() {
        let mut term = test_terminal(200, 50);
        let app = App::new();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_alerts_tab_with_active_alerts() {
        let mut term = test_terminal(120, 30);
        let mut app = App::new();
        app.update(Event::AlertFired(firing_alert(
            Severity::Critical,
            "gnuc",
            "high-cpu",
        )));
        app.update(Event::AlertFired(firing_alert(
            Severity::Warn,
            "nuc",
            "high-mem",
        )));
        app.update(Event::AlertFired(firing_alert(
            Severity::Info,
            "kajiblet",
            "low-disk",
        )));
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_alerts_tab_empty() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.active_tab = Tab::Alerts;
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_metrics_tab_with_data() {
        let mut term = test_terminal(120, 30);
        let mut app = App::new();
        app.active_tab = Tab::Metrics;
        app.update(Event::MetricsUpdate(metrics_for("gnuc")));
        app.update(Event::MetricsUpdate(metrics_for("nuc")));
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_metrics_tab_empty() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.active_tab = Tab::Metrics;
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_metrics_tab_no_gpu() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.active_tab = Tab::Metrics;
        let mut m = MetricSet::new("gnuc");
        m.insert("cpu.percent", 50.0);
        m.insert("memory.percent", 60.0);
        m.insert("disk.used_pct", 70.0);
        app.update(Event::MetricsUpdate(m));
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_history_tab_with_resolved() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.active_tab = Tab::History;
        app.update(Event::AlertFired(firing_alert(
            Severity::Warn,
            "gnuc",
            "r1",
        )));
        app.update(Event::AlertResolved(resolved_alert()));
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_history_tab_empty() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.active_tab = Tab::History;
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_rules_tab() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.active_tab = Tab::Rules;
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_daemon_disconnected_state() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.update(Event::DaemonDisconnected("timeout".into()));
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_daemon_connected_state() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.update(Event::DaemonConnected);
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_all_severity_levels_in_alerts() {
        let mut term = test_terminal(120, 30);
        let mut app = App::new();
        for sev in [Severity::Info, Severity::Warn, Severity::Critical] {
            app.update(Event::AlertFired(firing_alert(
                sev,
                "gnuc",
                &format!("rule-{sev}"),
            )));
        }
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_scrolled_metrics() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.active_tab = Tab::Metrics;
        app.scroll_offset = 5;
        for i in 0..10 {
            app.update(Event::MetricsUpdate(metrics_for(&format!("host-{i}"))));
        }
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_tab_badge_shows_alert_count() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.update(Event::AlertFired(firing_alert(
            Severity::Critical,
            "gnuc",
            "cpu",
        )));
        assert_eq!(app.active_alert_count, 1);
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_chat_mode_active() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.mode = Mode::Chat;
        app.chat_input = "hello monty".into();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_chat_mode_with_history() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.chat_log.push(ChatMessage {
            from: "you".into(),
            text: "status?".into(),
        });
        app.chat_log.push(ChatMessage {
            from: "monty".into(),
            text: "all clear.".into(),
        });
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_status_bar_shows_chat_mode() {
        let mut term = test_terminal(80, 24);
        let mut app = App::new();
        app.mode = Mode::Chat;
        term.draw(|f| draw(f, &app)).unwrap();
    }
}
