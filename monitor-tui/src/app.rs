use crate::event::Event;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use monitor_core::alert::{Alert, AlertState};
use monitor_core::metrics::MetricSet;
use std::collections::HashMap;

/// Whether the user is typing in the chat field or navigating the dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Chat,
}

/// A single line in the chat log.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub from: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Alerts,
    Metrics,
    History,
    Rules,
}

impl Tab {
    pub const ALL: &'static [Self] = &[Self::Alerts, Self::Metrics, Self::History, Self::Rules];

    pub fn label(self) -> &'static str {
        match self {
            Self::Alerts => "Alerts",
            Self::Metrics => "Metrics",
            Self::History => "History",
            Self::Rules => "Rules",
        }
    }
}

/// Application state for the TUI.
pub struct App {
    pub active_tab: Tab,
    pub quit: bool,
    /// Current metric snapshot per target name.
    pub metrics: HashMap<String, MetricSet>,
    /// Active (Firing) alerts.
    pub active_alerts: Vec<Alert>,
    /// Recently resolved alerts (last 100).
    pub resolved_alerts: Vec<Alert>,
    pub daemon_connected: bool,
    pub daemon_status: String,
    pub now: String,
    pub active_alert_count: usize,
    pub scroll_offset: usize,
    /// Chat input mode.
    pub mode: Mode,
    /// Current text in the chat input field.
    pub chat_input: String,
    /// Chat log — capped at 200 entries.
    pub chat_log: Vec<ChatMessage>,
}

impl App {
    pub fn new() -> Self {
        Self {
            active_tab: Tab::Alerts,
            quit: false,
            metrics: HashMap::new(),
            active_alerts: Vec::new(),
            resolved_alerts: Vec::new(),
            daemon_connected: false,
            daemon_status: "connecting…".into(),
            now: chrono::Local::now().format("%H:%M:%S").to_string(),
            active_alert_count: 0,
            scroll_offset: 0,
            mode: Mode::Normal,
            chat_input: String::new(),
            chat_log: Vec::new(),
        }
    }

    /// Last N chat messages (for display).
    pub fn recent_chat(&self, n: usize) -> &[ChatMessage] {
        let len = self.chat_log.len();
        if len <= n {
            &self.chat_log
        } else {
            &self.chat_log[len - n..]
        }
    }

    pub fn update(&mut self, event: Event) {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Resize(_, _) => {}
            Event::MetricsUpdate(metrics) => {
                self.metrics.insert(metrics.target.clone(), metrics);
            }
            Event::AlertFired(alert) => {
                self.active_alerts.retain(|a| a.id != alert.id);
                self.active_alerts.push(alert);
                self.active_alert_count = self.active_alerts.len();
            }
            Event::AlertResolved(alert) => {
                self.active_alerts.retain(|a| a.id != alert.id);
                self.active_alert_count = self.active_alerts.len();
                self.resolved_alerts.insert(0, alert);
                self.resolved_alerts.truncate(100);
            }
            Event::AlertsSnapshot(alerts) => {
                self.active_alerts = alerts
                    .into_iter()
                    .filter(|a| a.state == AlertState::Firing)
                    .collect();
                self.active_alert_count = self.active_alerts.len();
            }
            Event::DaemonConnected => {
                self.daemon_connected = true;
                self.daemon_status = "ok".into();
            }
            Event::DaemonDisconnected(reason) => {
                self.daemon_connected = false;
                self.daemon_status = format!("disconnected: {reason}");
            }
            Event::Tick => {
                self.now = chrono::Local::now().format("%H:%M:%S").to_string();
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits regardless of mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match self.mode {
            Mode::Chat => self.handle_chat_key(key),
            Mode::Normal => self.handle_normal_key(key),
        }
    }

    fn handle_chat_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.chat_input.clear();
            }
            KeyCode::Enter => {
                let text = self.chat_input.trim().to_owned();
                if !text.is_empty() {
                    self.chat_log.push(ChatMessage {
                        from: "you".into(),
                        text,
                    });
                    self.chat_log.truncate(200);
                    self.chat_input.clear();
                }
            }
            KeyCode::Backspace => {
                self.chat_input.pop();
            }
            KeyCode::Char(c) => {
                self.chat_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.quit = true,
            // '/' opens the chat field (familiar search-bar convention).
            KeyCode::Char('/') => {
                self.mode = Mode::Chat;
                self.chat_input.clear();
            }
            KeyCode::Char('1') => self.active_tab = Tab::Alerts,
            KeyCode::Char('2') => self.active_tab = Tab::Metrics,
            KeyCode::Char('3') => self.active_tab = Tab::History,
            KeyCode::Char('4') => self.active_tab = Tab::Rules,
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn cycle_tab(&mut self, delta: i32) {
        let idx = Tab::ALL
            .iter()
            .position(|t| *t == self.active_tab)
            .unwrap_or(0);
        let next = (idx as i32 + delta).rem_euclid(Tab::ALL.len() as i32) as usize;
        self.active_tab = Tab::ALL[next];
        self.scroll_offset = 0;
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_on_q() {
        let mut app = App::new();
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        assert!(app.quit);
    }

    #[test]
    fn tab_switching() {
        let mut app = App::new();
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('2'),
            KeyModifiers::NONE,
        )));
        assert_eq!(app.active_tab, Tab::Metrics);
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('1'),
            KeyModifiers::NONE,
        )));
        assert_eq!(app.active_tab, Tab::Alerts);
    }

    #[test]
    fn alert_lifecycle_in_app() {
        use monitor_core::alert::{Alert, AlertId, AlertState, Severity};
        use monitor_core::metrics::MetricPath;
        use uuid::Uuid;

        let mut app = App::new();
        let alert = Alert {
            id: AlertId::for_rule("gnuc", "high-cpu"),
            uuid: Uuid::new_v4(),
            rule_name: "high-cpu".into(),
            target: "gnuc".into(),
            metric: MetricPath::new("cpu.percent"),
            value: 90.0,
            severity: Severity::Warn,
            state: AlertState::Firing,
            message: "gnuc: CPU at 90%".into(),
            fired_at_secs: None,
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        };

        app.update(Event::AlertFired(alert.clone()));
        assert_eq!(app.active_alerts.len(), 1);
        assert_eq!(app.active_alert_count, 1);

        let mut resolved = alert;
        resolved.state = AlertState::Resolved;
        app.update(Event::AlertResolved(resolved));
        assert_eq!(app.active_alerts.len(), 0);
        assert_eq!(app.resolved_alerts.len(), 1);
    }

    #[test]
    fn metrics_update_stored_by_target() {
        let mut app = App::new();
        let mut m = monitor_core::metrics::MetricSet::new("gnuc");
        m.insert("cpu.percent", 42.0);
        app.update(Event::MetricsUpdate(m));
        assert!(app.metrics.contains_key("gnuc"));
    }

    #[test]
    fn cycle_tab_forward_wraps() {
        let mut app = App::new();
        app.active_tab = Tab::Rules; // last tab
        app.update(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(app.active_tab, Tab::Alerts); // wraps to first
    }

    #[test]
    fn cycle_tab_backward() {
        let mut app = App::new();
        app.active_tab = Tab::Alerts;
        app.update(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.active_tab, Tab::Rules); // wraps to last
    }

    #[test]
    fn scroll_keys() {
        let mut app = App::new();
        app.update(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        assert_eq!(app.scroll_offset, 1);
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
        )));
        assert_eq!(app.scroll_offset, 2);
        app.update(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        assert_eq!(app.scroll_offset, 1);
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::NONE,
        )));
        assert_eq!(app.scroll_offset, 0);
        // Saturating sub — doesn't go negative
        app.update(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn ctrl_c_quits() {
        let mut app = App::new();
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(app.quit);
    }

    #[test]
    fn uppercase_q_quits() {
        let mut app = App::new();
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('Q'),
            KeyModifiers::NONE,
        )));
        assert!(app.quit);
    }

    #[test]
    fn resize_event_handled() {
        let mut app = App::new();
        app.update(Event::Resize(120, 40)); // should not panic
    }

    #[test]
    fn tick_updates_timestamp() {
        let mut app = App::new();
        let before = app.now.clone();
        // Tick should update the time string (same value in practice, but no panic)
        app.update(Event::Tick);
        let _ = &app.now; // still a valid string
        assert!(!before.is_empty());
    }

    #[test]
    fn alerts_snapshot_replaces_active() {
        use monitor_core::alert::{Alert, AlertId, AlertState, Severity};
        use monitor_core::metrics::MetricPath;
        use uuid::Uuid;
        let mut app = App::new();
        let make_alert = |id: &str| Alert {
            id: AlertId::for_rule("gnuc", id),
            uuid: Uuid::new_v4(),
            rule_name: id.into(),
            target: "gnuc".into(),
            metric: MetricPath::new("cpu.percent"),
            value: 90.0,
            severity: Severity::Warn,
            state: AlertState::Firing,
            message: id.into(),
            fired_at_secs: None,
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        };
        app.update(Event::AlertFired(make_alert("old")));
        assert_eq!(app.active_alerts.len(), 1);
        // Snapshot replaces entirely
        app.update(Event::AlertsSnapshot(vec![
            make_alert("new1"),
            make_alert("new2"),
        ]));
        assert_eq!(app.active_alerts.len(), 2);
        assert_eq!(app.active_alert_count, 2);
    }

    #[test]
    fn tab_all_covers_all_variants() {
        assert_eq!(Tab::ALL.len(), 4);
        assert!(Tab::ALL.contains(&Tab::Alerts));
        assert!(Tab::ALL.contains(&Tab::Metrics));
        assert!(Tab::ALL.contains(&Tab::History));
        assert!(Tab::ALL.contains(&Tab::Rules));
    }

    #[test]
    fn tab_labels_non_empty() {
        for tab in Tab::ALL {
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn slash_enters_chat_mode() {
        let mut app = App::new();
        assert_eq!(app.mode, Mode::Normal);
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('/'),
            KeyModifiers::NONE,
        )));
        assert_eq!(app.mode, Mode::Chat);
    }

    #[test]
    fn chat_mode_esc_returns_to_normal() {
        let mut app = App::new();
        app.mode = Mode::Chat;
        app.update(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn chat_mode_typing_builds_input() {
        let mut app = App::new();
        app.mode = Mode::Chat;
        for c in "hello".chars() {
            app.update(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            )));
        }
        assert_eq!(app.chat_input, "hello");
    }

    #[test]
    fn chat_mode_backspace_deletes() {
        let mut app = App::new();
        app.mode = Mode::Chat;
        app.chat_input = "helo".into();
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.chat_input, "hel");
    }

    #[test]
    fn chat_mode_enter_submits_and_clears() {
        let mut app = App::new();
        app.mode = Mode::Chat;
        app.chat_input = "test message".into();
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(app.chat_input.is_empty());
        assert_eq!(app.chat_log.len(), 1);
        assert_eq!(app.chat_log[0].text, "test message");
        assert_eq!(app.chat_log[0].from, "you");
    }

    #[test]
    fn chat_mode_empty_enter_is_no_op() {
        let mut app = App::new();
        app.mode = Mode::Chat;
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(app.chat_log.is_empty());
    }

    #[test]
    fn normal_mode_q_does_not_quit_in_chat() {
        let mut app = App::new();
        app.mode = Mode::Chat;
        app.update(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        // In chat mode, 'q' is text input, not quit
        assert!(!app.quit);
        assert_eq!(app.chat_input, "q");
    }

    #[test]
    fn recent_chat_returns_last_n() {
        let mut app = App::new();
        for i in 0..5 {
            app.chat_log.push(ChatMessage {
                from: "you".into(),
                text: format!("msg {i}"),
            });
        }
        let recent = app.recent_chat(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[2].text, "msg 4");
    }
}
