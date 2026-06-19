//! egui GUI skin for monitor-agent — the caster station's premium surface.
//!
//! One *skin* over the shared [`monitor_presence`] core (the same contract as
//! the ratatui skin): it clones a [`SharedPresence`], renders a
//! [`PresenceModel`] snapshot each frame, and submits intents. Agent output
//! reaches it via an [`EguiSink`] attached as an Observer.
//!
//! **P4a** (this) renders the core dashboard — status bar, tabs, and the
//! Alerts / Metrics / History / Rules content. **P4b** adds `egui_plot`
//! graphs, the embedded brush terminal, the animated Monty, and the voice
//! waveform.

use eframe::egui;
use monitor_presence::{AttachRole, OutputChunk, OutputSink, PresenceModel, SharedPresence, Tab};
use tokio::sync::mpsc;

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
    /// Per-skin view state (egui-local): which tab is shown.
    active_tab: Tab,
}

impl EguiSkin {
    pub fn new(
        shared: SharedPresence,
        transcript_rx: mpsc::UnboundedReceiver<OutputChunk>,
    ) -> Self {
        Self {
            shared,
            transcript_rx,
            active_tab: Tab::Metrics,
        }
    }

    /// Render the dashboard from a model snapshot. Kept separate from the
    /// eframe `App` impl so it is headless-testable.
    pub fn dashboard(&mut self, ctx: &egui::Context, model: &PresenceModel) {
        egui::TopBottomPanel::top("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(if model.daemon_connected {
                    "● daemon:ok"
                } else {
                    "● daemon:…"
                });
                ui.separator();
                ui.label(format!("⚠ {} active", model.active_alert_count));
                ui.separator();
                ui.label(&model.now);
            });
        });

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for &tab in Tab::ALL {
                    if ui
                        .selectable_label(self.active_tab == tab, tab.label())
                        .clicked()
                    {
                        self.active_tab = tab;
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Alerts => draw_alerts(ui, model),
            Tab::Metrics => draw_metrics(ui, model),
            Tab::History => draw_history(ui, model),
            Tab::Rules => {
                ui.label("Rules are defined in monitor-agent.toml.");
            }
        });
    }
}

fn draw_alerts(ui: &mut egui::Ui, model: &PresenceModel) {
    if model.active_alerts.is_empty() {
        ui.label("No active alerts.");
        return;
    }
    for a in &model.active_alerts {
        ui.label(format!("{}  {}  {:.1}", a.severity, a.message, a.value));
    }
}

fn draw_metrics(ui: &mut egui::Ui, model: &PresenceModel) {
    if model.metrics.is_empty() {
        ui.label("Waiting for first poll…");
        return;
    }
    let mut targets: Vec<&String> = model.metrics.keys().collect();
    targets.sort();
    egui::Grid::new("metrics").striped(true).show(ui, |ui| {
        for h in ["target", "cpu", "mem", "disk"] {
            ui.strong(h);
        }
        ui.end_row();
        for t in targets {
            let m = &model.metrics[t];
            let pct = |k: &str| {
                m.get(&k.into())
                    .map(|v| format!("{v:.0}%"))
                    .unwrap_or_else(|| "n/a".into())
            };
            ui.label(t);
            ui.label(pct("cpu.percent"));
            ui.label(pct("memory.percent"));
            ui.label(pct("disk.used_pct"));
            ui.end_row();
        }
    });
}

fn draw_history(ui: &mut egui::Ui, model: &PresenceModel) {
    if model.resolved_alerts.is_empty() {
        ui.label("No resolved alerts this session.");
        return;
    }
    for a in &model.resolved_alerts {
        ui.label(format!("{}  {}", a.target, a.metric.as_str()));
    }
}

impl eframe::App for EguiSkin {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain agent output into the shared transcript.
        while let Ok(chunk) = self.transcript_rx.try_recv() {
            self.shared.with_mut(|p| p.fold_output(&chunk));
        }
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
        for tab in Tab::ALL {
            skin.active_tab = *tab;
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
