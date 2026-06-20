//! egui GUI skin for monitor-agent — the caster station's premium surface.
//!
//! One *skin* over the shared [`monitor_presence`] core (the same contract as
//! the ratatui skin): it clones a [`SharedPresence`], renders a
//! [`PresenceModel`] snapshot each frame, and submits intents. Agent output
//! reaches it via an [`EguiSink`] attached as an Observer.
//!
//! **Layout** deliberately mirrors the monty-tui "Monitor Lizard" dashboard so
//! caster's GUI feels like home:
//!
//! ```text
//! ┌ Monty art ∣ chat ───────────────────────────┐  (top panel, fixed)
//! ├ 1:Metrics  2:Swarm  3:Board ────────────────┤  (status tabs)
//! │ per-tab grid of big bordered blocks          │  (content)
//! ├ daemon:ok │ ⚠ N active │ clock … key hints ──┤  (status bar)
//! └──────────────────────────────────────────────┘
//! ```
//!
//! This pass lands the *layout* — the big blocks and the tabs. **Metrics**
//! renders live data from the shared presence; **Swarm** and **Board** are
//! labeled layout STUBS pending the swarm rearchitecture. Later work fills in
//! `egui_plot` graphs, the embedded brush terminal, the animated Monty, and the
//! voice waveform.

use eframe::egui::{self, Color32, RichText};
use monitor_presence::{AttachRole, OutputChunk, OutputSink, PresenceModel, SharedPresence};
use tokio::sync::mpsc;

/// Monty accent (the lizard green), reused for headings and the active tab.
const ACCENT: Color32 = Color32::from_rgb(0x6a, 0xc6, 0x6a);
/// "You" chat lines (cyan), matching monty-tui's user color.
const USER_CYAN: Color32 = Color32::from_rgb(0x4e, 0xc9, 0xb0);

/// A small placeholder Monty. The real ANSI art lives in `docs/logos/` and can
/// be wired in with the animated-character pass; this keeps the skin dep-free.
const MONTY_ART: &str = "   .--.\n  / o o\\\n  \\  ^ /\n _/`-'\\_\n(_/   \\_)";

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

/// The GUI-local status tabs, mirroring monty-tui (`1:Metrics 2:Swarm 3:Board`).
///
/// These are a *view* concern of this skin, intentionally separate from the
/// shared [`monitor_presence::Tab`] data model — the monty taxonomy is a layout
/// stub we can promote into the shared model once the swarm rearchitecture
/// settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuiTab {
    Metrics,
    Swarm,
    Board,
}

impl GuiTab {
    const ALL: &'static [Self] = &[Self::Metrics, Self::Swarm, Self::Board];

    fn label(self) -> &'static str {
        match self {
            Self::Metrics => "Metrics",
            Self::Swarm => "Swarm",
            Self::Board => "Board",
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
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

/// The egui skin.
pub struct EguiSkin {
    shared: SharedPresence,
    transcript_rx: mpsc::UnboundedReceiver<OutputChunk>,
    /// Per-skin view state (egui-local): which status tab is shown.
    active_tab: GuiTab,
    /// Per-skin view state: which machine sub-tab is selected on the Metrics tab.
    active_machine: usize,
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
        });
    }

    /// Top panel: Monty character art (left, fixed) ∣ chat (right, fills).
    fn top_panel(&self, ctx: &egui::Context, model: &PresenceModel) {
        egui::TopBottomPanel::top("monty")
            .exact_height(160.0)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(150.0, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.label(RichText::new(MONTY_ART).monospace().color(ACCENT));
                            ui.label(RichText::new("Monty · caster").small().color(Color32::GRAY));
                        },
                    );
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Chat").strong().color(ACCENT));
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height(86.0);
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
                        ui.label(RichText::new("Chat: Enter to chat").color(Color32::GRAY));
                    });
                });
            });
    }

    /// Status-tab bar: `1:Metrics  2:Swarm  3:Board`, active tab in bold green.
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

    /// Metrics tab: machine sub-tabs + a btop-style grid of big blocks.
    fn metrics_tab(&mut self, ui: &mut egui::Ui, model: &PresenceModel) {
        // Machine sub-tab bar — real targets if we have any, else placeholders.
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

        let machine = &machines[self.active_machine];
        let ms = model.metrics.get(machine);
        let pct = |key: &str| ms.and_then(|m| m.get(&key.into()));

        // Two columns of stacked boxes (CPU/NET ∣ MEM/GPU), then a wide PROC box.
        ui.columns(2, |cols| {
            metric_box(&mut cols[0], "CPU", |ui| gauge(ui, pct("cpu.percent")));
            metric_box(&mut cols[0], "NET", |ui| {
                ui.label(RichText::new("RX / TX history — stub").color(Color32::GRAY));
            });
            metric_box(&mut cols[1], "MEM", |ui| {
                gauge(ui, pct("memory.percent"));
                ui.add_space(2.0);
                ui.label(RichText::new("disk").small().color(Color32::GRAY));
                gauge(ui, pct("disk.used_pct"));
            });
            metric_box(&mut cols[1], "GPU", |ui| {
                ui.label(RichText::new("util / VRAM / temp — stub").color(Color32::GRAY));
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
/// hints pinned to the right (matching monty-tui's help line).
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
                    "q:quit  1-3:tabs  Tab:next  Enter:chat  Space:talk",
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

/// A percentage gauge colored green/yellow/red, the GUI analog of monty's heat
/// meters. Renders `n/a` when the metric is absent.
fn gauge(ui: &mut egui::Ui, value: Option<f64>) {
    match value {
        Some(v) => {
            let frac = (v / 100.0).clamp(0.0, 1.0) as f32;
            ui.add(
                egui::ProgressBar::new(frac)
                    .text(format!("{v:.0}%"))
                    .fill(pct_color(v)),
            );
        }
        None => {
            ui.label(RichText::new("n/a").color(Color32::GRAY));
        }
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
        // Drain agent output into the shared transcript.
        while let Ok(chunk) = self.transcript_rx.try_recv() {
            self.shared.with_mut(|p| p.fold_output(&chunk));
        }
        // Tab navigation: number keys jump, Tab cycles (matches monty-tui).
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
            if i.key_pressed(egui::Key::Tab) {
                self.active_tab = self.active_tab.next();
            }
        });
        let model = self.shared.snapshot();
        self.dashboard(ctx, &model);
        if self.shared.should_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Poll for collector updates even without user input.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

/// Launch the egui GUI. Attaches an [`EguiSink`] as an Observer so agent output
/// reaches this skin, then runs the native window (blocks the calling thread).
///
/// # Errors
///
/// Returns an error if the native window or graphics backend cannot start.
pub fn run(shared: SharedPresence) -> eframe::Result {
    eframe::run_native(
        "monitor-agent · caster",
        eframe::NativeOptions::default(),
        Box::new(move |cc| {
            let (sink, rx) = EguiSink::new(cc.egui_ctx.clone());
            shared.with_mut(|p| p.attach_sink(AttachRole::Observer, Box::new(sink)));
            Ok(Box::new(EguiSkin::new(shared, rx)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_presence::OutputStream;

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
}
