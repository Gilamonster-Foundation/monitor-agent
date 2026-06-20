//! egui GUI skin for monitor-agent — the caster station's premium surface.
//!
//! One *skin* over the shared [`monitor_presence`] core (the same contract as
//! the ratatui skin): it clones a [`SharedPresence`], renders a
//! [`PresenceModel`] snapshot each frame, and submits intents. Agent output
//! reaches it via an [`EguiSink`] attached as an Observer.
//!
//! **Layout** mirrors the monty-tui "Monitor Lizard" dashboard:
//!
//! ```text
//! ┌ Monty (art/GIF) ∣ chat + voice waveform ────┐  top panel (fixed)
//! ├ 1:Metrics 2:Swarm 3:Board 4:Shell ──────────┤  status tabs
//! │ per-tab grid of big bordered blocks          │  content
//! ├ daemon:ok │ ⚠ N active │ clock … key hints ──┤  status bar
//! └──────────────────────────────────────────────┘
//! ```
//!
//! **P4b** fills the blocks: `egui_plot` sparklines off the metric history rings;
//! an **animated Monty** that loads `assets/monty/<state>.gif|png` at runtime
//! (ASCII fallback); an animated **voice waveform**; and a **Shell** console.
//! Swarm/Board remain layout stubs pending the swarm rearchitecture.

use std::path::Path;

use eframe::egui::{self, Color32, RichText};
use egui_plot::{Line, Plot, PlotBounds, PlotPoints};
use monitor_presence::{
    AttachRole, DataEvent, Intent, OutputChunk, OutputSink, PresenceModel, SharedPresence,
};
use monitor_voice::{MicCapture, StubVoiceEngine, VoiceEngine};
use tokio::sync::mpsc;

/// Monty accent (the lizard green), reused for headings and the active tab.
const ACCENT: Color32 = Color32::from_rgb(0x6a, 0xc6, 0x6a);
/// "You" chat lines + voice waveform (cyan), matching monty-tui's user color.
const USER_CYAN: Color32 = Color32::from_rgb(0x4e, 0xc9, 0xb0);

/// Built-in placeholder Monty, used when no image asset is present.
const MONTY_ART: &str = "   .--.\n  / o o\\\n  \\  ^ /\n _/`-'\\_\n(_/   \\_)";

/// Runtime directory for Monty's per-state art (see `assets/monty/README.md`).
const MONTY_ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/monty");

/// Placeholder cards for the Board tab until the board feed is wired.
const SAMPLE_CARDS: &[(&str, &str, &str)] = &[
    (
        "P0",
        "gilabot-pipeline-redesign",
        "plan worker · review authority · multi-model dispatch",
    ),
    ("P0", "mads-data720-burst", "Units 04–05 + quizzes"),
    (
        "P1",
        "caster-gui-monty-layout",
        "this view — big blocks + status tabs",
    ),
    (
        "P2",
        "caster-voice-talk-timeout",
        "bound record_until_silence so talk can't hang",
    ),
];

/// The GUI-local status tabs, mirroring monty-tui plus a Shell console.
///
/// A *view* concern of this skin, intentionally separate from the shared
/// [`monitor_presence::Tab`] data model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuiTab {
    Metrics,
    Swarm,
    Board,
    Shell,
}

impl GuiTab {
    const ALL: &'static [Self] = &[Self::Metrics, Self::Swarm, Self::Board, Self::Shell];

    fn label(self) -> &'static str {
        match self {
            Self::Metrics => "Metrics",
            Self::Swarm => "Swarm",
            Self::Board => "Board",
            Self::Shell => "Shell",
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// Monty's mood, derived from the model. Drives which animation plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MontyState {
    Sleeping,
    Idle,
    Listening,
    /// Reserved: selected once an agent-turn-in-progress signal reaches the model.
    #[allow(dead_code)]
    Thinking,
    Active,
    SuperActive,
}

impl MontyState {
    fn filename(self) -> &'static str {
        match self {
            Self::Sleeping => "sleeping",
            Self::Idle => "idle",
            Self::Listening => "listening",
            Self::Thinking => "thinking",
            Self::Active => "active",
            Self::SuperActive => "superactive",
        }
    }

    /// Derive the current mood from the snapshot. Listening/Thinking await voice
    /// + agent-turn signals on the model and aren't selected yet.
    fn from_model(m: &PresenceModel) -> Self {
        if m.listening {
            return Self::Listening;
        }
        if !m.daemon_connected {
            return Self::Sleeping;
        }
        let hot = m
            .metrics
            .values()
            .any(|ms| ms.get(&"cpu.percent".into()).is_some_and(|v| v > 80.0));
        if hot {
            return Self::SuperActive;
        }
        if m.active_alert_count > 0 {
            return Self::Active;
        }
        Self::Idle
    }
}

/// Resolve a `file://` URI for `state`'s art, trying `.gif` then `.png`, then
/// the same for `idle`, then `None` (caller falls back to ASCII). Checking the
/// file on disk first keeps missing assets out of egui's loader error path.
fn monty_uri(state: MontyState) -> Option<String> {
    for name in [state.filename(), "idle"] {
        for ext in ["gif", "png"] {
            let path = Path::new(MONTY_ASSET_DIR).join(format!("{name}.{ext}"));
            if path.exists() {
                return Some(format!(
                    "file:///{}",
                    path.to_string_lossy().replace('\\', "/")
                ));
            }
        }
    }
    None
}

/// An [`OutputSink`] that forwards agent output to the egui paint loop and
/// requests a repaint. It never touches egui state inside `deliver` — it only
/// enqueues + wakes the paint thread (the same discipline as the ratatui sink).
pub struct EguiSink {
    tx: mpsc::UnboundedSender<OutputChunk>,
    ctx: egui::Context,
}

impl EguiSink {
    pub fn new(ctx: egui::Context) -> (Self, mpsc::UnboundedReceiver<OutputChunk>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx, ctx }, rx)
    }
}

impl OutputSink for EguiSink {
    fn deliver(&mut self, chunk: &OutputChunk) {
        let _ = self.tx.send(chunk.clone());
        self.ctx.request_repaint();
    }
}

/// A minimal embedded console. Runs commands on a background thread via the
/// system shell and streams output into the scrollback. This is a placeholder
/// for real `brush-core` embedding (which is async + needs custom stdout/stderr
/// capture — tracked as a follow-up).
struct ShellConsole {
    input: String,
    lines: Vec<String>,
    rx: Option<std::sync::mpsc::Receiver<String>>,
    running: bool,
}

impl ShellConsole {
    fn new() -> Self {
        Self {
            input: String::new(),
            lines: vec!["brush-core embedding pending — commands run via the system shell.".into()],
            rx: None,
            running: false,
        }
    }

    fn submit(&mut self) {
        let cmd = self.input.trim().to_owned();
        if cmd.is_empty() {
            return;
        }
        self.input.clear();
        self.lines.push(format!("$ {cmd}"));
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.running = true;
        std::thread::spawn(move || {
            let _ = tx.send(run_command(&cmd));
        });
    }

    /// Pull finished command output into the scrollback (called each frame).
    fn drain(&mut self) {
        let mut done = false;
        if let Some(rx) = self.rx.as_ref() {
            match rx.try_recv() {
                Ok(out) => {
                    for line in out.lines() {
                        self.lines.push(line.to_owned());
                    }
                    done = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => done = true,
            }
        }
        if done {
            self.running = false;
            self.rx = None;
        }
        if self.lines.len() > 500 {
            let n = self.lines.len() - 500;
            self.lines.drain(0..n);
        }
    }
}

/// Run `cmd` through the platform shell, returning combined stdout+stderr.
fn run_command(cmd: &str) -> String {
    use std::process::Command;
    #[cfg(windows)]
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
        .output();
    #[cfg(not(windows))]
    let output = Command::new("sh").args(["-c", cmd]).output();
    match output {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() {
                s.push_str(&err);
            }
            if s.trim().is_empty() {
                s = format!("(exit {})", o.status.code().unwrap_or(-1));
            }
            s
        }
        Err(e) => format!("error: {e}"),
    }
}

/// The egui skin.
pub struct EguiSkin {
    shared: SharedPresence,
    transcript_rx: mpsc::UnboundedReceiver<OutputChunk>,
    /// Per-skin view state (egui-local): which status tab is shown.
    active_tab: GuiTab,
    /// Per-skin view state: which machine sub-tab is selected on the Metrics tab.
    active_machine: usize,
    console: ShellConsole,
    /// Chat input buffer (egui-local) — submitted as `Intent::SubmitChat`.
    chat_input: String,
    /// The (swappable) speech engine — a stub until native STT/TTS land.
    voice: Box<dyn VoiceEngine>,
    /// The live microphone, present only while push-to-talk is active.
    mic: Option<MicCapture>,
    /// Rolling RMS buffer published to the model for the voice waveform.
    voice_rms: Vec<f32>,
}

impl EguiSkin {
    pub fn new(
        shared: SharedPresence,
        transcript_rx: mpsc::UnboundedReceiver<OutputChunk>,
    ) -> Self {
        Self {
            shared,
            transcript_rx,
            active_tab: GuiTab::Metrics,
            active_machine: 0,
            console: ShellConsole::new(),
            chat_input: String::new(),
            voice: Box::new(StubVoiceEngine),
            mic: None,
            voice_rms: Vec::new(),
        }
    }

    /// Push-to-talk: open the mic if idle, or stop it, transcribe the captured
    /// audio, and submit the transcript as chat. (The stub engine returns
    /// instantly; a real engine will transcribe on a background thread.)
    fn toggle_listening(&mut self) {
        if let Some(mic) = self.mic.take() {
            let samples = mic.take_samples();
            let sample_rate = mic.sample_rate();
            drop(mic); // stop the stream
            self.voice_rms.clear();
            self.shared.apply(DataEvent::Listening(false));
            let reply = match self.voice.transcribe(&samples, sample_rate) {
                Ok(text) => text,
                Err(e) => format!("[voice error: {e}]"),
            };
            self.shared.submit_intent(Intent::SubmitChat(reply));
        } else {
            match MicCapture::start() {
                Ok(mic) => {
                    self.mic = Some(mic);
                    self.shared.apply(DataEvent::Listening(true));
                }
                Err(e) => self
                    .shared
                    .submit_intent(Intent::SubmitChat(format!("[mic error: {e}]"))),
            }
        }
    }

    /// Drain mic RMS into the rolling buffer and publish it for the waveform.
    fn pump_voice(&mut self) {
        if let Some(mic) = &self.mic {
            let new = mic.drain_rms();
            if !new.is_empty() {
                self.voice_rms.extend(new);
                let n = self.voice_rms.len();
                if n > 64 {
                    self.voice_rms.drain(0..n - 64);
                }
                self.shared
                    .apply(DataEvent::VoiceLevels(self.voice_rms.clone()));
            }
        }
    }

    /// Render the dashboard from a model snapshot. Kept separate from the
    /// eframe `App` impl so it is headless-testable.
    pub fn dashboard(&mut self, ctx: &egui::Context, model: &PresenceModel) {
        self.top_panel(ctx, model);
        self.tab_bar(ctx);
        status_bar(ctx, model);
        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            GuiTab::Metrics => self.metrics_tab(ui, model),
            GuiTab::Swarm => swarm_tab(ui),
            GuiTab::Board => board_tab(ui),
            GuiTab::Shell => self.shell_tab(ui),
        });
    }

    /// Top panel: animated Monty (left, fixed) ∣ chat + input + voice waveform.
    fn top_panel(&mut self, ctx: &egui::Context, model: &PresenceModel) {
        egui::TopBottomPanel::top("monty")
            .exact_height(160.0)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(150.0, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            match monty_uri(MontyState::from_model(model)) {
                                Some(uri) => {
                                    ui.add(
                                        egui::Image::from_uri(uri)
                                            .fit_to_exact_size(egui::vec2(120.0, 120.0)),
                                    );
                                }
                                None => {
                                    ui.label(RichText::new(MONTY_ART).monospace().color(ACCENT));
                                }
                            }
                            ui.label(RichText::new("Monty · caster").small().color(Color32::GRAY));
                        },
                    );
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Chat").strong().color(ACCENT));
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height(78.0);
                            egui::ScrollArea::vertical()
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    if model.chat_log.is_empty() {
                                        ui.label(
                                            RichText::new("No messages yet — press Enter to chat.")
                                                .italics()
                                                .color(Color32::GRAY),
                                        );
                                    } else {
                                        for msg in model.recent_chat(30) {
                                            let you = msg.from.eq_ignore_ascii_case("you");
                                            let color = if you { USER_CYAN } else { ACCENT };
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}: {}",
                                                    msg.from, msg.text
                                                ))
                                                .color(color),
                                            );
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            let listening = self.mic.is_some();
                            let (talk, talk_col) = if listening {
                                ("● stop", Color32::from_rgb(0xd9, 0x4f, 0x4f))
                            } else {
                                ("talk", ACCENT)
                            };
                            if ui
                                .add(egui::Button::new(RichText::new(talk).color(talk_col)))
                                .clicked()
                            {
                                self.toggle_listening();
                            }
                            ui.label(RichText::new("›").strong().color(ACCENT));
                            let w = (ui.available_width() - 150.0).max(80.0);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.chat_input)
                                    .hint_text("talk to Monty…")
                                    .desired_width(w),
                            );
                            voice_waveform(ui, &model.voice_levels, model.listening);
                            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                let text = std::mem::take(&mut self.chat_input);
                                self.shared.submit_intent(Intent::SubmitChat(text));
                                resp.request_focus();
                            }
                        });
                    });
                });
            });
    }

    /// Status-tab bar: `1:Metrics 2:Swarm 3:Board 4:Shell`, active in bold green.
    fn tab_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, &tab) in GuiTab::ALL.iter().enumerate() {
                    let active = self.active_tab == tab;
                    let text = RichText::new(format!("{}:{}", i + 1, tab.label()));
                    let text = if active {
                        text.strong().color(ACCENT)
                    } else {
                        text.color(Color32::GRAY)
                    };
                    if ui.selectable_label(active, text).clicked() {
                        self.active_tab = tab;
                    }
                }
            });
        });
    }

    /// Metrics tab: machine sub-tabs + a btop-style grid of big blocks with
    /// live gauges and `egui_plot` sparklines off the metric history rings.
    fn metrics_tab(&mut self, ui: &mut egui::Ui, model: &PresenceModel) {
        let mut machines: Vec<String> = model.metrics.keys().cloned().collect();
        machines.sort();
        if machines.is_empty() {
            machines = vec!["gnuc".into(), "nuc".into(), "nuc2".into()];
        }
        if self.active_machine >= machines.len() {
            self.active_machine = 0;
        }
        ui.horizontal(|ui| {
            for (i, m) in machines.iter().enumerate() {
                let active = i == self.active_machine;
                let connected = model.metrics.contains_key(m);
                let text = RichText::new(m.as_str());
                let text = if active {
                    text.strong().color(ACCENT)
                } else if connected {
                    text.color(Color32::WHITE)
                } else {
                    text.color(Color32::GRAY)
                };
                if ui.selectable_label(active, text).clicked() {
                    self.active_machine = i;
                }
            }
        });
        ui.separator();

        let machine = machines[self.active_machine].as_str();
        let ms = model.metrics.get(machine);
        let pct = |key: &str| ms.and_then(|m| m.get(&key.into()));

        ui.columns(2, |cols| {
            metric_box(&mut cols[0], "CPU", |ui| {
                spark_with_value(
                    ui,
                    "spark_cpu",
                    pct("cpu.percent"),
                    &model.history_for(machine, "cpu.percent", 60),
                    ACCENT,
                );
            });
            metric_box(&mut cols[0], "NET", |ui| {
                let rx = first_history(model, machine, &["net.rx_bytes_sec", "net.rx_bytes"]);
                let tx = first_history(model, machine, &["net.tx_bytes_sec", "net.tx_bytes"]);
                if rx.is_empty() && tx.is_empty() {
                    ui.label(RichText::new("RX / TX — waiting…").color(Color32::GRAY));
                } else {
                    butterfly_net(ui, "spark_net", &rx, &tx);
                    ui.label(
                        RichText::new("rx ▲ green · tx ▼ cyan")
                            .small()
                            .color(Color32::GRAY),
                    );
                }
            });
            metric_box(&mut cols[1], "MEM", |ui| {
                spark_with_value(
                    ui,
                    "spark_mem",
                    pct("memory.percent"),
                    &model.history_for(machine, "memory.percent", 60),
                    USER_CYAN,
                );
                ui.add_space(2.0);
                ui.label(RichText::new("disk").small().color(Color32::GRAY));
                spark_with_value(
                    ui,
                    "spark_disk",
                    pct("disk.used_pct"),
                    &model.history_for(machine, "disk.used_pct", 60),
                    ACCENT,
                );
            });
            metric_box(&mut cols[1], "GPU", |ui| match pct("gpu.util_pct") {
                Some(_) => spark_with_value(
                    ui,
                    "spark_gpu",
                    pct("gpu.util_pct"),
                    &model.history_for(machine, "gpu.util_pct", 60),
                    ACCENT,
                ),
                None => {
                    ui.label(RichText::new("no GPU on this host").color(Color32::GRAY));
                }
            });
        });
        metric_box(ui, "PROC", |ui| {
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .show(ui, |ui| {
                    egui::Grid::new("proc")
                        .striped(true)
                        .num_columns(5)
                        .show(ui, |ui| {
                            for h in ["PID", "USER", "CPU%", "MEM%", "COMMAND"] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            ui.label(RichText::new("— process table stub —").color(Color32::GRAY));
                            ui.end_row();
                        });
                });
        });
    }

    /// Shell tab: scrollback + command input. Runs via the system shell on a
    /// background thread (placeholder for real brush embedding).
    fn shell_tab(&mut self, ui: &mut egui::Ui) {
        self.console.drain();
        ui.label(
            RichText::new("Shell — system shell (brush-core embedding pending)")
                .italics()
                .color(Color32::GRAY),
        );
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &self.console.lines {
                        let color = if line.starts_with("$ ") {
                            ACCENT
                        } else {
                            Color32::GRAY
                        };
                        ui.label(RichText::new(line).monospace().color(color));
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("›").strong().color(ACCENT));
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.console.input)
                    .hint_text("command…")
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.console.submit();
                resp.request_focus();
            }
            if self.console.running {
                ui.spinner();
            }
        });
    }
}

/// Swarm tab: the monty 3×2 grid of panels — all layout stubs for now.
fn swarm_tab(ui: &mut egui::Ui) {
    ui.label(
        RichText::new(
            "Swarm — layout stub (data wiring deferred pending the swarm rearchitecture)",
        )
        .italics()
        .color(Color32::GRAY),
    );
    ui.add_space(4.0);
    ui.columns(3, |c| {
        stub_box(&mut c[0], "Machines", "per-host CPU / MEM / NET meters");
        stub_box(
            &mut c[1],
            "Repo Status",
            "active repo · commits · languages",
        );
        stub_box(&mut c[2], "Budget", "daily / weekly / monthly LLM spend");
    });
    ui.columns(3, |c| {
        stub_box(&mut c[0], "gnuc · PROC", "PID  USER  CPU%  MEM%  COMMAND");
        stub_box(&mut c[1], "nuc · POD", "NS  POD  CPU  MEM");
        stub_box(&mut c[2], "nuc2 · POD", "NS  POD  CPU  MEM");
    });
}

/// Board tab: full-width scrollable list of `[PRIORITY] id — summary` cards.
fn board_tab(ui: &mut egui::Ui) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(format!("Board · {} cards (sample)", SAMPLE_CARDS.len()))
                .strong()
                .color(ACCENT),
        );
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for card in SAMPLE_CARDS {
                let (prio, id, summary) = *card;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("[{prio}]"))
                            .strong()
                            .color(prio_color(prio)),
                    );
                    ui.label(RichText::new(id).color(Color32::WHITE));
                    ui.label(RichText::new(format!("— {summary}")).color(Color32::GRAY));
                });
            }
        });
    });
}

/// Bottom status bar: connection state + active-alert count + clock, with key
/// hints pinned to the right.
fn status_bar(ctx: &egui::Context, model: &PresenceModel) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let (label, color) = if model.daemon_connected {
                ("daemon:ok", ACCENT)
            } else {
                ("daemon:…", Color32::GRAY)
            };
            ui.colored_label(color, format!("● {label}"));
            ui.separator();
            let alert_color = if model.active_alert_count > 0 {
                Color32::from_rgb(0xd7, 0xbf, 0x5a)
            } else {
                Color32::GRAY
            };
            ui.colored_label(
                alert_color,
                format!("⚠ {} active", model.active_alert_count),
            );
            ui.separator();
            ui.colored_label(Color32::GRAY, model.now.as_str());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(
                    Color32::GRAY,
                    "q:quit  1-4:tabs  Tab:next  Enter:chat  Space:talk",
                );
            });
        });
    });
}

/// A titled, bordered "big block" — the GUI analog of a ratatui bordered Block.
fn metric_box(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong().color(ACCENT));
        ui.separator();
        body(ui);
    });
}

/// A bordered block with a one-line description — used for not-yet-wired panels.
fn stub_box(ui: &mut egui::Ui, title: &str, body: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(92.0);
        ui.label(RichText::new(title).strong().color(ACCENT));
        ui.separator();
        ui.label(RichText::new(body).color(Color32::GRAY));
    });
}

/// Current value (latest sample) shown small + right-aligned, above a filled
/// sparkline — the change-over-time view we prefer over a snapshot gauge.
fn spark_with_value(
    ui: &mut egui::Ui,
    id: &str,
    current: Option<f64>,
    history: &[f64],
    color: Color32,
) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let (text, col) = match current {
            Some(v) => (format!("{v:.0}%"), pct_color(v)),
            None => ("—".to_owned(), Color32::GRAY),
        };
        ui.label(RichText::new(text).strong().color(col));
    });
    sparkline(ui, id, history, color, Some(100.0));
}

/// A compact, non-interactive **filled** line graph (sparkline) of `values`.
/// `fixed_max` clamps the y-axis (e.g. `Some(100.0)` for percentages); `None`
/// auto-fits to the data range. Shows `waiting…` until history accumulates.
fn sparkline(ui: &mut egui::Ui, id: &str, values: &[f64], color: Color32, fixed_max: Option<f64>) {
    if values.is_empty() {
        ui.label(RichText::new("waiting…").small().color(Color32::GRAY));
        return;
    }
    let pts: PlotPoints = values
        .iter()
        .enumerate()
        .map(|(i, &v)| [i as f64, v])
        .collect();
    let line = Line::new(pts).color(color).width(1.5).fill(0.0);
    let xmax = (values.len() as f64 - 1.0).max(1.0);
    let (ymin, ymax) = match fixed_max {
        Some(m) => (0.0, m),
        None => {
            let mn = values.iter().copied().fold(f64::INFINITY, f64::min);
            let mx = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let pad = ((mx - mn).abs() * 0.1).max(1.0);
            (mn - pad, mx + pad)
        }
    };
    Plot::new(id)
        .height(46.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show_axes(false)
        .show_grid(false)
        .show_background(false)
        .show(ui, |pui| {
            pui.set_plot_bounds(PlotBounds::from_min_max([0.0, ymin], [xmax, ymax]));
            pui.line(line);
        });
}

/// A NET "butterfly": rx filled upward, tx filled downward, mirrored about a
/// zero center line — change-over-time for both directions at a glance.
fn butterfly_net(ui: &mut egui::Ui, id: &str, rx: &[f64], tx: &[f64]) {
    let mx = rx
        .iter()
        .chain(tx.iter())
        .copied()
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let xmax = (rx.len().max(tx.len()) as f64 - 1.0).max(1.0);
    let wing = |data: &[f64], sign: f64, color: Color32| {
        let pts: PlotPoints = data
            .iter()
            .enumerate()
            .map(|(i, &v)| [i as f64, sign * v])
            .collect();
        Line::new(pts).color(color).width(1.2).fill(0.0)
    };
    let rx_line = wing(rx, 1.0, ACCENT);
    let tx_line = wing(tx, -1.0, USER_CYAN);
    Plot::new(id)
        .height(56.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show_axes(false)
        .show_grid(false)
        .show_background(false)
        .show(ui, |pui| {
            pui.set_plot_bounds(PlotBounds::from_min_max([0.0, -mx * 1.1], [xmax, mx * 1.1]));
            pui.line(rx_line);
            pui.line(tx_line);
        });
}

/// Return the first non-empty metric history among `keys` for `machine`.
fn first_history(model: &PresenceModel, machine: &str, keys: &[&str]) -> Vec<f64> {
    for k in keys {
        let h = model.history_for(machine, k, 60);
        if !h.is_empty() {
            return h;
        }
    }
    Vec::new()
}

/// A voice level-meter. While listening it renders the live mic RMS `levels`
/// (newest at the right); otherwise it idles with a gentle frame-time animation.
fn voice_waveform(ui: &mut egui::Ui, levels: &[f32], listening: bool) {
    let time = ui.input(|i| i.time) as f32;
    let (resp, painter) = ui.allocate_painter(egui::vec2(140.0, 18.0), egui::Sense::hover());
    let rect = resp.rect;
    let bars = 18usize;
    let bw = rect.width() / bars as f32;
    let live = listening && !levels.is_empty();
    let color = if live {
        USER_CYAN
    } else {
        Color32::from_gray(80)
    };
    for i in 0..bars {
        let amp = if live {
            // Map bar i to a recent level, newest on the right; boost for visibility.
            let idx = levels.len().saturating_sub(bars - i);
            (levels.get(idx).copied().unwrap_or(0.0) * 6.0).clamp(0.03, 1.0)
        } else {
            let phase = time * 6.0 + i as f32 * 0.5;
            ((phase.sin() * 0.5 + 0.5) * 0.6 + 0.05).clamp(0.0, 1.0)
        };
        let h = amp * rect.height();
        let x = rect.left() + (i as f32 + 0.5) * bw;
        let bar = egui::Rect::from_center_size(
            egui::pos2(x, rect.center().y),
            egui::vec2((bw * 0.6).max(1.0), h),
        );
        painter.rect_filled(bar, egui::Rounding::same(1.0), color);
    }
}

/// Utilization → color: <70% green, 70–90% yellow, >90% red.
fn pct_color(v: f64) -> Color32 {
    if v >= 90.0 {
        Color32::from_rgb(0xd9, 0x4f, 0x4f)
    } else if v >= 70.0 {
        Color32::from_rgb(0xd7, 0xbf, 0x5a)
    } else {
        Color32::from_rgb(0x5a, 0xb0, 0x6a)
    }
}

/// Board priority → color: P0 red, P1 yellow, P2 blue, else gray.
fn prio_color(prio: &str) -> Color32 {
    match prio {
        "P0" => Color32::from_rgb(0xd9, 0x4f, 0x4f),
        "P1" => Color32::from_rgb(0xd7, 0xbf, 0x5a),
        "P2" => Color32::from_rgb(0x5a, 0x9b, 0xd7),
        _ => Color32::GRAY,
    }
}

impl eframe::App for EguiSkin {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(chunk) = self.transcript_rx.try_recv() {
            self.shared.with_mut(|p| p.fold_output(&chunk));
        }
        self.pump_voice();
        // Tab hotkeys, but only when no text field has focus (so the Shell
        // console can type digits without switching tabs).
        if ctx.memory(|m| m.focused().is_none()) {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::Num1) {
                    self.active_tab = GuiTab::Metrics;
                }
                if i.key_pressed(egui::Key::Num2) {
                    self.active_tab = GuiTab::Swarm;
                }
                if i.key_pressed(egui::Key::Num3) {
                    self.active_tab = GuiTab::Board;
                }
                if i.key_pressed(egui::Key::Num4) {
                    self.active_tab = GuiTab::Shell;
                }
                if i.key_pressed(egui::Key::Tab) {
                    self.active_tab = self.active_tab.next();
                }
            });
        }
        let model = self.shared.snapshot();
        self.dashboard(ctx, &model);
        if self.shared.should_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // ~30fps so the Monty GIF and the voice waveform animate smoothly.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

/// Launch the egui GUI. Installs image loaders (for the animated Monty), attaches
/// an [`EguiSink`] as an Observer, then runs the native window (blocks).
///
/// # Errors
///
/// Returns an error if the native window or graphics backend cannot start.
pub fn run(shared: SharedPresence) -> eframe::Result {
    eframe::run_native(
        "monitor-agent · caster",
        eframe::NativeOptions::default(),
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let (sink, rx) = EguiSink::new(cc.egui_ctx.clone());
            shared.with_mut(|p| p.attach_sink(AttachRole::Observer, Box::new(sink)));
            Ok(Box::new(EguiSkin::new(shared, rx)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_presence::{DataEvent, OutputStream};

    #[test]
    fn dashboard_renders_every_tab_without_panic() {
        let shared = SharedPresence::new();
        let (_tx, rx) = mpsc::unbounded_channel();
        let mut skin = EguiSkin::new(shared, rx);
        let ctx = egui::Context::default();
        let model = PresenceModel::new();
        for &tab in GuiTab::ALL {
            skin.active_tab = tab;
            let _ = ctx.run(egui::RawInput::default(), |ctx| skin.dashboard(ctx, &model));
        }
    }

    #[test]
    fn egui_sink_forwards_chunk_to_receiver() {
        let ctx = egui::Context::default();
        let (mut sink, mut rx) = EguiSink::new(ctx);
        let chunk = OutputChunk {
            turn: 1,
            stream: OutputStream::AgentThought,
            seq: 0,
            data: "hi".into(),
            last: true,
        };
        sink.deliver(&chunk);
        assert_eq!(rx.try_recv().unwrap().data, "hi");
    }

    #[test]
    fn monty_state_reflects_model() {
        let mut m = PresenceModel::new();
        assert_eq!(MontyState::from_model(&m), MontyState::Sleeping);
        m.apply(DataEvent::DaemonConnected);
        assert_eq!(MontyState::from_model(&m), MontyState::Idle);
    }
}
