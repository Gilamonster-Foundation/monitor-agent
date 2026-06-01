use crate::ansi::ansi_to_text;
use crate::app::{App, ChatMessage, Mode, Tab};
use crate::PORTRAIT;
use monitor_core::alert::Severity;
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame,
};

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

/// Lines reserved for the header (portrait + speech panel).
/// Must match the line count of docs/logos/monty-ansi-portrait-32.txt.
const HEADER_H: u16 = 13;
/// Width of the portrait column.
const PORTRAIT_W: u16 = 33;
/// Minimum terminal width before the portrait is hidden.
const PORTRAIT_MIN_WIDTH: u16 = 55;

// ---------------------------------------------------------------------------
// Top-level draw
// ---------------------------------------------------------------------------

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let show_portrait = area.width >= PORTRAIT_MIN_WIDTH;

    // Outer: [header] [tabs] [content] [status]
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_H),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    let header_area = outer[0];
    let tabs_area = outer[1];
    let content_area = outer[2];
    let status_area = outer[3];

    // Header: [portrait] [speech]
    let portrait_w = if show_portrait { PORTRAIT_W } else { 0 };
    let header_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(portrait_w), Constraint::Min(20)])
        .split(header_area);

    if show_portrait {
        draw_portrait(frame, header_split[0]);
    }
    draw_speech_panel(frame, app, header_split[1]);
    draw_tabs(frame, app, tabs_area);
    draw_content(frame, app, content_area);
    draw_status_bar(frame, app, status_area);
}

// ---------------------------------------------------------------------------
// Portrait
// ---------------------------------------------------------------------------

fn draw_portrait(frame: &mut Frame, area: Rect) {
    frame.render_widget(Paragraph::new(ansi_to_text(PORTRAIT)), area);
}

// ---------------------------------------------------------------------------
// Speech panel (right of portrait): status + commentary + chat log + input
// ---------------------------------------------------------------------------

fn draw_speech_panel(frame: &mut Frame, app: &App, area: Rect) {
    // Vertical split inside the speech panel:
    //   [status 1] [commentary 3] [chat log fills] [input 1]
    let splits = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    draw_speech_status(frame, app, splits[0]);
    draw_commentary(frame, app, splits[1]);
    draw_chat_log(frame, app, splits[2]);
    draw_chat_input(frame, app, splits[3]);
}

fn draw_speech_status(frame: &mut Frame, app: &App, area: Rect) {
    let conn = if app.daemon_connected {
        Span::styled("● daemon:ok", Style::default().fg(Color::Green))
    } else {
        Span::styled("● daemon:…", Style::default().fg(Color::DarkGray))
    };
    let alerts = if app.active_alert_count > 0 {
        Span::styled(
            format!("  ⚠ {}", app.active_alert_count),
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
    frame.render_widget(Paragraph::new(Line::from(vec![conn, alerts, time])), area);
}

fn draw_commentary(frame: &mut Frame, app: &App, area: Rect) {
    let lines = monty_says(app);
    frame.render_widget(Paragraph::new(lines), area);
}

fn monty_says(app: &App) -> Vec<Line<'static>> {
    if app.active_alerts.is_empty() {
        if app.metrics.is_empty() {
            vec![Line::from(Span::styled(
                "Watching... (no metrics yet)",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            vec![Line::from(Span::styled(
                "All clear. Systems nominal.",
                Style::default().fg(Color::Green),
            ))]
        }
    } else {
        let mut lines = Vec::new();
        for alert in app.active_alerts.iter().take(3) {
            let color = match alert.severity {
                Severity::Critical => Color::Red,
                Severity::Warn => Color::Yellow,
                Severity::Info => Color::Blue,
            };
            let icon = match alert.severity {
                Severity::Critical => "● ",
                Severity::Warn => "⚠ ",
                Severity::Info => "ℹ ",
            };
            lines.push(Line::from(vec![
                Span::styled(icon, Style::default().fg(color)),
                Span::styled(alert.message.clone(), Style::default().fg(color)),
            ]));
        }
        if app.active_alert_count > 3 {
            lines.push(Line::from(Span::styled(
                format!("  … and {} more", app.active_alert_count - 3),
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines
    }
}

fn draw_chat_log(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let n = area.height as usize;
    let recent = app.recent_chat(n);
    let lines: Vec<Line> = recent.iter().map(|msg| chat_line(msg)).collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn chat_line(msg: &ChatMessage) -> Line<'static> {
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
        Span::raw(msg.text.clone()),
    ])
}

fn draw_chat_input(frame: &mut Frame, app: &App, area: Rect) {
    let (prompt, style) = match app.mode {
        Mode::Chat => (
            format!("> {}", app.chat_input),
            Style::default().fg(Color::Yellow),
        ),
        Mode::Normal => ("> / to chat".into(), Style::default().fg(Color::DarkGray)),
    };
    frame.render_widget(Paragraph::new(prompt.as_str()).style(style), area);

    if app.mode == Mode::Chat {
        let x = area.x + 2 + app.chat_input.len() as u16;
        frame.set_cursor_position(Position::new(
            x.min(area.x + area.width.saturating_sub(1)),
            area.y,
        ));
    }
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
                .block(Block::default().borders(Borders::ALL).title("Alerts"))
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
                Constraint::Length(12),
                Constraint::Min(18),
                Constraint::Length(7),
                Constraint::Length(10),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Alerts"))
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

    // Width for sparklines — at least 10, at most 20.
    let spark_w = (area.width as usize).saturating_sub(60).clamp(10, 20);

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

            let cpu_hist = app.history_for(target, "cpu.percent", spark_w);
            let mem_hist = app.history_for(target, "memory.percent", spark_w);

            let cpu_spark = sparkline(&cpu_hist, spark_w);
            let mem_spark = sparkline(&mem_hist, spark_w);

            let cpu_color = pct_color(m.get(&"cpu.percent".into()).unwrap_or(0.0));
            let mem_color = pct_color(m.get(&"memory.percent".into()).unwrap_or(0.0));
            let dsk_color = pct_color(m.get(&"disk.used_pct".into()).unwrap_or(0.0));

            let gpu_part: Vec<Span> = {
                if let Some(g) = m.get(&"gpu.util_pct".into()) {
                    let gpu_hist = app.history_for(target, "gpu.util_pct", spark_w);
                    let gpu_spark = sparkline(&gpu_hist, spark_w);
                    vec![
                        Span::raw("  GPU "),
                        Span::styled(gpu_spark, Style::default().fg(pct_color(g))),
                        Span::styled(format!(" {g:.0}%"), Style::default().fg(pct_color(g))),
                    ]
                } else {
                    vec![]
                }
            };

            let mut spans = vec![
                Span::styled(
                    format!("{target:<12}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  CPU "),
                Span::styled(cpu_spark, Style::default().fg(cpu_color)),
                Span::styled(format!(" {cpu:<5}"), Style::default().fg(cpu_color)),
                Span::raw("  MEM "),
                Span::styled(mem_spark, Style::default().fg(mem_color)),
                Span::styled(format!(" {mem:<5}"), Style::default().fg(mem_color)),
                Span::raw("  DSK "),
                Span::styled(dsk, Style::default().fg(dsk_color)),
            ];
            spans.extend(gpu_part);
            Line::from(spans)
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
                .block(Block::default().borders(Borders::ALL).title("History"))
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
                Constraint::Length(12),
                Constraint::Min(22),
                Constraint::Length(12),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("History")),
        area,
    );
}

fn draw_rules_tab(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new("Rules are defined in monitor-agent.toml.")
            .block(Block::default().borders(Borders::ALL).title("Rules"))
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
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
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            mode_tag,
            Span::raw(format!("  collectors:{}", app.metrics.len())),
            Span::styled("  q:quit", Style::default().fg(Color::DarkGray)),
            Span::styled("  1-4:tabs", Style::default().fg(Color::DarkGray)),
            Span::styled("  ↑↓:scroll", Style::default().fg(Color::DarkGray)),
        ])),
        area,
    );
}

// ---------------------------------------------------------------------------
// Spark charts
// ---------------------------------------------------------------------------

const SPARK_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn spark_char(value: f64, min: f64, max: f64) -> char {
    if max <= min {
        return SPARK_CHARS[0];
    }
    let ratio = (value - min) / (max - min);
    let idx = (ratio * 7.0).round().clamp(0.0, 7.0) as usize;
    SPARK_CHARS[idx]
}

fn sparkline(history: &[f64], width: usize) -> String {
    if history.is_empty() {
        return " ".repeat(width);
    }
    let min = history
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
        .max(0.0);
    let max = history
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
        .min(100.0);

    // Pad left with the minimum bar if we have fewer samples than width.
    let pad = width.saturating_sub(history.len());
    let mut s = String::with_capacity(width * 3); // ▁ is 3 bytes UTF-8
    for _ in 0..pad {
        s.push(SPARK_CHARS[0]);
    }
    for &v in history {
        s.push(spark_char(v, min, max));
    }
    s
}

/// Map 0-100% to green→yellow→red.
fn pct_color(pct: f64) -> Color {
    if pct < 60.0 {
        Color::Green
    } else if pct < 80.0 {
        Color::Yellow
    } else {
        Color::Red
    }
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
    use crate::app::App;
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
        let mut term = test_terminal(80, 30);
        let app = App::new();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_narrow_hides_portrait() {
        let mut term = test_terminal(40, 30);
        let app = App::new();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_wide_terminal() {
        let mut term = test_terminal(200, 50);
        let app = App::new();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_with_active_alerts() {
        let mut term = test_terminal(120, 40);
        let mut app = App::new();
        app.update(Event::AlertFired(firing_alert(
            Severity::Critical,
            "gnuc",
            "cpu",
        )));
        app.update(Event::AlertFired(firing_alert(
            Severity::Warn,
            "nuc",
            "mem",
        )));
        app.update(Event::AlertFired(firing_alert(
            Severity::Info,
            "kajiblet",
            "dsk",
        )));
        app.update(Event::AlertFired(firing_alert(
            Severity::Critical,
            "gnuc",
            "cpu2",
        )));
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_metrics_tab_with_history() {
        let mut term = test_terminal(120, 40);
        let mut app = App::new();
        app.active_tab = Tab::Metrics;
        for i in 0..20 {
            let mut m = metrics_for("gnuc");
            m.insert("cpu.percent", i as f64 * 4.0);
            app.update(Event::MetricsUpdate(m));
        }
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_metrics_tab_empty() {
        let mut term = test_terminal(80, 30);
        let mut app = App::new();
        app.active_tab = Tab::Metrics;
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_history_tab() {
        let mut term = test_terminal(80, 30);
        let mut app = App::new();
        app.active_tab = Tab::History;
        app.update(Event::AlertFired(firing_alert(Severity::Warn, "gnuc", "r")));
        app.update(Event::AlertResolved(resolved_alert()));
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_history_tab_empty() {
        let mut term = test_terminal(80, 30);
        let mut app = App::new();
        app.active_tab = Tab::History;
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_rules_tab() {
        let mut term = test_terminal(80, 30);
        let mut app = App::new();
        app.active_tab = Tab::Rules;
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_chat_mode() {
        let mut term = test_terminal(80, 30);
        let mut app = App::new();
        app.mode = Mode::Chat;
        app.chat_input = "hello".into();
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
    fn draw_daemon_states() {
        let mut term = test_terminal(80, 30);
        let mut app = App::new();
        app.update(Event::DaemonConnected);
        term.draw(|f| draw(f, &app)).unwrap();
        app.update(Event::DaemonDisconnected("timeout".into()));
        term.draw(|f| draw(f, &app)).unwrap();
    }

    // Spark chart unit tests.
    #[test]
    fn sparkline_empty_is_spaces() {
        assert_eq!(sparkline(&[], 10), " ".repeat(10));
    }

    #[test]
    fn sparkline_all_same_value_uses_lowest_bar() {
        let s = sparkline(&[50.0, 50.0, 50.0], 3);
        assert_eq!(s.chars().count(), 3);
        // When min==max all bars are the same character.
        let chars: Vec<char> = s.chars().collect();
        assert!(chars.iter().all(|&c| c == chars[0]));
    }

    #[test]
    fn sparkline_rising_trend_ends_with_full_bar() {
        let data: Vec<f64> = (0..10).map(|i| i as f64 * 10.0).collect();
        let s = sparkline(&data, 10);
        assert_eq!(s.chars().last().unwrap(), '█');
    }

    #[test]
    fn sparkline_pads_when_fewer_samples_than_width() {
        let s = sparkline(&[80.0, 100.0], 10);
        assert_eq!(s.chars().count(), 10);
    }

    #[test]
    fn pct_color_thresholds() {
        assert_eq!(pct_color(0.0), Color::Green);
        assert_eq!(pct_color(59.9), Color::Green);
        assert_eq!(pct_color(60.0), Color::Yellow);
        assert_eq!(pct_color(79.9), Color::Yellow);
        assert_eq!(pct_color(80.0), Color::Red);
        assert_eq!(pct_color(100.0), Color::Red);
    }
}
