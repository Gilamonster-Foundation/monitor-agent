use crate::app::{App, Tab};
use monitor_core::alert::Severity;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Length(1), // tab bar
            Constraint::Min(5),    // content
            Constraint::Length(1), // status bar
        ])
        .split(area);

    draw_header(frame, app, chunks[0]);
    draw_tabs(frame, app, chunks[1]);
    draw_content(frame, app, chunks[2]);
    draw_status_bar(frame, app, chunks[3]);
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let daemon_indicator = if app.daemon_connected {
        Span::styled("● daemon:ok", Style::default().fg(Color::Green))
    } else {
        Span::styled("● daemon:disconnected", Style::default().fg(Color::Red))
    };

    let alert_badge = if app.active_alert_count > 0 {
        Span::styled(
            format!("  ⚠ {} alert(s)", app.active_alert_count),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("  ✓ no alerts", Style::default().fg(Color::Green))
    };

    let time = Span::styled(
        format!("  {}", app.now),
        Style::default().fg(Color::DarkGray),
    );

    let line = Line::from(vec![daemon_indicator, alert_badge, time]);
    frame.render_widget(Paragraph::new(line), area);
}

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

    let tabs = Tabs::new(titles)
        .select(selected)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .divider("|");

    frame.render_widget(tabs, area);
}

fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    match app.active_tab {
        Tab::Alerts => draw_alerts_tab(frame, app, area),
        Tab::Metrics => draw_metrics_tab(frame, app, area),
        Tab::History => draw_history_tab(frame, app, area),
        Tab::Rules => draw_rules_tab(frame, app, area),
    }
}

fn draw_alerts_tab(frame: &mut Frame, app: &App, area: Rect) {
    if app.active_alerts.is_empty() {
        let p = Paragraph::new("No active alerts.")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Active Alerts"),
            )
            .style(Style::default().fg(Color::Green));
        frame.render_widget(p, area);
        return;
    }

    let header = Row::new(vec!["Sev", "Target", "Metric", "Value", "Firing for"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .active_alerts
        .iter()
        .map(|a| {
            let color = severity_color(a.severity);
            let sev_icon = match a.severity {
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
                Cell::from(format!("{} {}", sev_icon, a.severity))
                    .style(Style::default().fg(color)),
                Cell::from(a.target.as_str()),
                Cell::from(a.metric.as_str()),
                Cell::from(format!("{:.1}", a.value)),
                Cell::from(duration),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(16),
            Constraint::Min(20),
            Constraint::Length(8),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Active Alerts"),
    )
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_widget(table, area);
}

fn draw_metrics_tab(frame: &mut Frame, app: &App, area: Rect) {
    if app.metrics.is_empty() {
        let p = Paragraph::new("No metrics collected yet. Waiting for first poll interval…")
            .block(Block::default().borders(Borders::ALL).title("Metrics"))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(p, area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    let mut targets: Vec<&str> = app.metrics.keys().map(String::as_str).collect();
    targets.sort();

    for target in targets {
        let m = &app.metrics[target];
        let cpu = m
            .get(&"cpu.percent".into())
            .map(|v| format!("{v:.0}%"))
            .unwrap_or("n/a".into());
        let mem = m
            .get(&"memory.percent".into())
            .map(|v| format!("{v:.0}%"))
            .unwrap_or("n/a".into());
        let dsk = m
            .get(&"disk.used_pct".into())
            .map(|v| format!("{v:.0}%"))
            .unwrap_or("n/a".into());
        let gpu = m
            .get(&"gpu.util_pct".into())
            .map(|v| format!("{v:.0}%"))
            .unwrap_or_default();
        let gpu_str = if gpu.is_empty() {
            String::new()
        } else {
            format!("  GPU {gpu}")
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("{target:<16}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("CPU {cpu:<6}  MEM {mem:<6}  DSK {dsk:<6}{gpu_str}")),
        ]));
    }

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Metrics"))
        .scroll((app.scroll_offset as u16, 0));
    frame.render_widget(p, area);
}

fn draw_history_tab(frame: &mut Frame, app: &App, area: Rect) {
    if app.resolved_alerts.is_empty() {
        let p = Paragraph::new("No resolved alerts in this session.")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Alert History"),
            )
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(p, area);
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

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Min(24),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Alert History (last 100)"),
    );

    frame.render_widget(table, area);
}

fn draw_rules_tab(frame: &mut Frame, _app: &App, area: Rect) {
    let p = Paragraph::new("Rules are defined in monitor-agent.toml. Reload with 'r'.")
        .block(Block::default().borders(Borders::ALL).title("Alert Rules"))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(p, area);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let collectors_ok = !app.metrics.is_empty();
    let collector_str = if collectors_ok {
        format!("collectors:{}", app.metrics.len())
    } else {
        "collectors:0".into()
    };

    let line = Line::from(vec![
        Span::styled(
            format!("daemon:{}", if app.daemon_connected { "ok" } else { "err" }),
            Style::default().fg(if app.daemon_connected {
                Color::Green
            } else {
                Color::Red
            }),
        ),
        Span::raw("  "),
        Span::raw(&collector_str),
        Span::raw("  "),
        Span::styled("q:quit", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled("1-4:tabs", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled("↑↓/jk:scroll", Style::default().fg(Color::DarkGray)),
    ]);

    frame.render_widget(Paragraph::new(line), area);
}

fn severity_color(s: Severity) -> Color {
    match s {
        Severity::Critical => Color::Red,
        Severity::Warn => Color::Yellow,
        Severity::Info => Color::Blue,
    }
}

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
        let mut term = test_terminal(80, 24);
        let app = App::new();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn draw_narrow_terminal_does_not_panic() {
        let mut term = test_terminal(20, 10);
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
        // No gpu.util_pct — should not crash
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
}
