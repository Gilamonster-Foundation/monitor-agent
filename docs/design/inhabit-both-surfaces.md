# Monty Inhabits Both Surfaces: One `MontyPresence` Core, a ratatui Skin and an egui Skin

> Status: **proposal** · 2026-06-18 · companion to `caster-station.md` §5/§10.
> Scope: one Monty presence inhabiting BOTH the ratatui TUI and a future egui GUI. Swarm-free.


> Scope: how a single "Monty" presence on the caster box renders into and acts through **both** the existing ratatui TUI and a future egui GUI from one frontend-agnostic core, under the read-only ocap leash already minted in `monitor-station`. **Swarm-free** by construction — the canonical model carries only `MetricSet`/`Alert`/chat, which are ecosystem-independent. No `Swarm`/`Board` tabs, no swarm collectors.

## 1. TL;DR + diagram

The seam is already 95% cut. `monitor_tui::App` and `App::update` (`monitor-tui/src/app.rs:44-66`, `:114-163`) are a clean immediate-mode reducer with **zero ratatui imports** — `app.rs:1-5` imports only `crossterm::event` key types and `monitor_core`. The only frontend coupling is:

1. **Input**: `App::update` consumes `Event::Key(crossterm::event::KeyEvent)` (`event.rs:11`) and three `handle_*_key` fns match `crossterm::KeyCode` directly (`app.rs:165-226`).
2. **Render**: `ui.rs` (540 lines) is pure ratatui `Frame`/`Rect` (`ui.rs:5-11`) — the only genuinely non-portable file.
3. **Lifecycle**: `run()` constructs `App` *inside* the loop (`lib.rs:222`) and owns the crossterm terminal + `tokio::select!` (`lib.rs:207-245`) — so today the model cannot be shared with a second skin.

The plan: hoist a **`MontyPresence`** above the loop that owns (a) the canonical state (the data subset of `App`), (b) the read-only `AgentKey` from `monitor_station::identity` (`identity.rs:59-66`), (c) a `newt_core::session::SessionState` as the output fan-out (`session.rs:175-183`), and (d) a frontend-neutral `Intent` intake. Both skins attach as **`Observer`** sinks; the Monty mind drives as the sole **`Driver`** under read-only caveats. The Observer role *structurally* refuses input from skins (`session.rs:268-270`), which is exactly what guarantees "the mind acts under the leash" identically across both surfaces.

```
                       ┌──────────────────────────────────────────────────────┐
   collectors          │                  MontyPresence (core)                 │
   spawn_collectors    │  ┌────────────────────────────────────────────────┐  │
   (daemon.rs:97)      │  │  PresenceModel  (canonical, render-agnostic)     │  │
        │  mpsc<Event> │  │  metrics / alerts / history rings / chat_log /   │  │
        └─────────────►│  │  daemon_status   (pure monitor_core types)       │  │
        apply(Event)   │  └────────────────────────────────────────────────┘  │
                       │  AgentKey (read-only leash, identity.rs:59)           │
   skin --Intent-->    │  SessionState (newt-core fan-out, session.rs:175)     │
   submit_intent()     │     attachments: { ratatui=Observer, egui=Observer,  │
                       │                    Monty-mind=Driver }                │
                       └───────┬───────────────────────────────┬──────────────┘
                  emit() fans  │ OutputChunk                    │ OutputChunk
                       ┌───────▼────────┐               ┌───────▼────────┐
                       │  RatatuiSink   │               │   EguiSink     │  (Observer)
                       │  (Observer)    │               │  push->chan,   │
                       └───────┬────────┘               │  request_repaint
       reads &PresenceModel    │                        └───────┬────────┘
       each frame              ▼                                ▼ reads snapshot/frame
                  ┌────────────────────────┐         ┌────────────────────────┐
                  │  ratatui skin (ui.rs)  │         │  egui skin (NEW)       │
                  │  terminal.draw(|f|…)   │         │  eframe update(ctx)    │
                  │  crossterm Key→Intent  │         │  egui Event→Intent     │
                  └────────────────────────┘         └────────────────────────┘
                         ONE MIND. ONE STATE. TWO SKINS.
```

Both loops keep the identical `draw → recv → apply` shape they have today (`lib.rs:225-240`); only the *draw call*, the *input source*, and the *locking discipline* differ.

---

## 2. The shared presence/session core

### 2.1 What the core owns

`MontyPresence` is a net-new type (lands in a new `monitor-presence` crate, see §6) that binds four things the two skins must share:

```rust
pub struct MontyPresence {
    /// Canonical, render-agnostic state (the data subset of today's App).
    model: PresenceModel,
    /// Read-only operating key — the leash. From monitor_station::identity.
    key: AgentKey,                       // identity.rs:59-66
    /// Output fan-out + per-driver authority + replay. From newt-core.
    session: SessionState,               // session.rs:175-183
    /// Handle of the Monty-mind Driver attachment (the only Driver).
    mind: AttachId,                      // session.rs:107
}
```

### 2.2 The canonical state model — `PresenceModel`

This is `monitor_tui::App` with the four render/input-bound fields **removed**. From `app.rs:44-66`, the canonical (frontend-agnostic) subset is:

```rust
pub struct PresenceModel {
    pub active_tab: Tab,                                           // app.rs:45 (shared identity)
    pub metrics: HashMap<String, MetricSet>,                       // app.rs:48
    pub active_alerts: Vec<Alert>,                                 // app.rs:50
    pub resolved_alerts: Vec<Alert>,                               // app.rs:52
    pub active_alert_count: usize,                                 // app.rs:56
    pub daemon_connected: bool,                                    // app.rs:53
    pub daemon_status: String,                                     // app.rs:54
    pub chat_log: Vec<ChatMessage>,                                // app.rs:63
    pub metrics_history: HashMap<String, HashMap<String, VecDeque<f64>>>, // app.rs:65 (60-sample rings)
    pub quit: bool,                                                // app.rs:46
}
```

**Stays out of the core (per-skin view state).** These four fields are TUI viewport/editor concerns and must live in each skin's own layer, not in `PresenceModel`:

| Field | `app.rs` | Why it is per-skin |
|---|---|---|
| `scroll_offset: usize` | `:57` | TUI viewport row offset; egui scrolls via `ScrollArea` and ignores it. Already consumed only by `ui.rs:395` `.scroll(...)`. |
| `now: String` | `:55` | Pre-formatted clock string refreshed on `Event::Tick` (`app.rs:159-161`); each skin formats its own clock from a timestamp. |
| `chat_input: String` | `:61` | Editor buffer; egui's `TextEdit` owns its own buffer. |
| `mode: Mode` | `:59` | Input-mode (Normal/Chat). *Borderline* — keep it skin-local: it gates which native keys mean what, and egui expresses focus differently (a focused text field == "chat mode"). |

The pure data accessors move with the model **verbatim**: `history_for(target, metric, width) -> Vec<f64>` (`app.rs:89-102`) and `recent_chat(n) -> &[ChatMessage]` (`app.rs:105-112`) are already allocation-light, `monitor_core`-typed, and directly consumable by `egui_plot`.

### 2.3 How it stays render-agnostic — the data half of `update` splits cleanly

Today `App::update` (`app.rs:114-163`) does two jobs in one match: **(a)** apply data events (`MetricsUpdate`/`AlertFired`/`AlertResolved`/`AlertsSnapshot`/`DaemonConnected`/`DaemonDisconnected`/`Tick`, `app.rs:118-161`) and **(b)** dispatch `Event::Key` to crossterm handlers (`app.rs:116`). These split along the existing arm boundary:

- **`PresenceModel::apply(&mut self, ev: DataEvent)`** — the data arms `app.rs:118-161` moved verbatim. This is a pure reducer, no crossterm, no ratatui. `DataEvent` is `Event` (`event.rs:9-32`) with the two terminal variants (`Key`, `Resize`, `event.rs:11-12`) removed — the remaining 7 variants are *already* frontend-neutral (`monitor_core` types only, `event.rs:1-2`).
- **`MontyPresence::submit_intent(&mut self, intent: Intent)`** — replaces the crossterm key handlers. `Intent` is a frontend-neutral enum (§5.1) that *both* skins produce; the body is `app.rs:204-226` rewritten to match `Intent` instead of `KeyCode`.

The `Tab` enum (`app.rs:22-41`, `Tab::ALL`/`label()`) moves into the core unchanged so both skins share tab *identity* while rendering differently — directly answering the doc's hard-coded-in-two-places observation (`ui.rs:247-254` dispatch + `app.rs:212-215` number-key select). Adding tabs later touches one enum, not two skins.

### 2.4 How it wraps newt-core's `SessionState` + `OutputSink`

`newt_core::session` is the correct substrate and is **frontend- and transport-free by construction** — its module doc (`session.rs:46-64`) explicitly describes this exact two-skin fan-out ("the same `OutputChunk` stream fans out to every attachment… a human at the keyboard and a phone observing… see the same turn") and states it has "NO transport dependency." Nothing ratatui-specific moves *into* it; the work is wiring it *up*.

The presence wraps it as follows:

- **Attach each skin as an `Observer`** via `SessionState::attach(AttachRole::Observer, observer_caveats, sink)` (`session.rs:208-225`). The skin supplies a `Box<dyn OutputSink>` whose `deliver(&OutputChunk)` (`session.rs:118-120`) routes a chunk to that skin's render path (ratatui: into `chat_log`; egui: onto a channel — §4).
- **Attach the Monty mind as the sole `Driver`** carrying `read_only_caveats()` (`identity.rs:37-46`). When the mind produces a turn it calls `session.emit(stream, data, last)` (`session.rs:291-310`), which buffers into the bounded ring and fans the chunk to **every** attachment in one pass (`session.rs:307-309`).
- **Replay for late join**: a skin attaching mid-session calls `session.replay_from(seq)` (`session.rs:326-332`) and gets the retained transcript tail (ring cap 256, `session.rs:357`). This is what makes "two skins over one core" survive a skin attaching after turns have already streamed — free, no new code.

### 2.5 How the agent acts under the read-only ocap key

The leash is enforced at the session layer, structurally, identically for both skins:

- The Monty mind is the only `Driver`. Authority for an in-flight turn is the **active driver's** caveats, surfaced by `effective_caveats()` (`session.rs:251-254`) — proven by `effective_caveats_tracks_the_active_driver` (`session.rs:607-640`). Because the mind's caveats are `read_only_caveats()` (`fs_write/exec/net = Scope::none()`, `identity.rs:38-45`), every turn runs deny-by-default on all mutating axes.
- Both skins are `Observer`s, and `submit_input` from an Observer returns `Err(InputRefused::NotADriver)` (`session.rs:268-270`), proven by `observer_receives_output_but_cannot_drive` (`session.rs:517-544`). **A skin cannot mutate the farm even if it wanted to** — the role is the gate, not the caveats (`session.rs:99-103`).

So "the human types into a skin" and "Monty acts" are two different paths: human input becomes an `Intent` that drives *local UI* (tab switch, scroll) or, for chat, becomes a **request to the mind**, which is the Driver that submits the actual session turn. The skins never hold `Driver`. The key is minted exactly once in the presence (via `read_only_operating_key`, `identity.rs:59-66`) and both skins inherit its leash transitively because they only ever observe.

---

## 3. Refactor of the existing `monitor-tui` into a skin

The existing TUI becomes a thin skin that borrows `&PresenceModel` each frame and feeds `Intent` in. Concretely:

### 3.1 Moves OUT of `monitor-tui` into the core

| Item | Current location | Destination |
|---|---|---|
| Canonical fields of `App` | `app.rs:44-66` (minus the 4 view fields) | `PresenceModel` |
| Data arms of `update` | `app.rs:118-161` | `PresenceModel::apply` |
| `history_for`, `recent_chat` | `app.rs:89-112` | `PresenceModel` (verbatim) |
| `Tab` + `Tab::ALL`/`label` | `app.rs:22-41` | core (shared identity) |
| `Mode` enum | `app.rs:8-13` | core type, but the *active mode* stays a per-skin field |
| `ChatMessage` | `app.rs:16-20` | core |
| Data variants of `Event` | `event.rs:14-31` | `DataEvent` in core |
| `sparkline`/`spark_char`/`pct_color` | `ui.rs:485-532` | shared `presence::viz` util (pure: `f64`→`String`/threshold; only the ratatui `Color` return of `pct_color` is skin-specific, so split it into `pct_bucket()->Bucket` in core + `bucket->Color` in the skin) |
| SGR-parse logic `apply_sgr` | `ansi.rs:68-105` | shared parser returning neutral `(text, fg, bg, mods)`; the ratatui `Span` assembly (`ansi.rs:15-65`) stays in the skin |

### 3.2 Stays IN `monitor-tui` (the ratatui skin proper)

- **All of `ui.rs`** (`ui.rs:1-544`) — `draw`/`draw_portrait`/`draw_speech_panel`/`draw_metrics_tab`/`draw_alerts_tab`/`draw_history_tab`/etc. These are irreducibly `Frame`/`Rect`/`Line`/`Span` (`ui.rs:5-11`); the doc confirms they are not reusable for egui (`caster-station.md:218`). Change: `draw(frame, app)` becomes `draw(frame, model: &PresenceModel, view: &TuiViewState)` where `TuiViewState` carries `scroll_offset`/`mode`/`chat_input`/`now`.
- **`ansi.rs` ratatui assembly** (`ansi.rs:15-65`) — produces ratatui `Text`/`Span` for the Monty portrait.
- **The terminal lifecycle + loop** in `lib.rs` — raw mode, `EnterAlternateScreen`, `CrosstermBackend`, `EventStream`, the `tokio::select!` (`lib.rs:207-245`) and the splash (`lib.rs:62-201`). All TUI-only.
- **`TuiViewState`** — new tiny per-skin struct holding `scroll_offset`, `mode: Mode`, `chat_input: String`, `now: String`. The handlers that mutated these (`app.rs:177-226`) become methods on `TuiViewState` that, after updating local view state, emit `Intent`s for anything that touches canonical state (tab change, chat submit, quit).

### 3.3 The input translation — the one real refactor

`handle_key`/`handle_normal_key`/`handle_chat_key` (`app.rs:165-226`) become a **translator**: crossterm `KeyEvent` → `Intent` (+ purely-local view edits stay local). Examples, mapping today's arms:

- `Ctrl+C` / `q` / `Q` (`app.rs:167-169`, `:206`) → `Intent::Quit`
- `'/'` (`app.rs:208-211`) → set local `mode = Chat` (view-only) — no Intent
- `'1'..'4'` / `Tab` / `BackTab` (`app.rs:212-217`) → `Intent::SelectTab(n)` / `Intent::CycleTab(±1)`
- `Down`/`j`, `Up`/`k` (`app.rs:218-223`) → local `scroll_offset` edit (view-only) — no Intent
- Chat-mode `Char`/`Backspace`/`Esc` (`app.rs:178-200`) → local `chat_input` edits (view-only)
- Chat-mode `Enter` (`app.rs:183-193`) → `Intent::SubmitChat(text)` — **this is where the stub dies** (see §3.4)

The existing 18 unit tests in `app.rs` (`:249-539`) split cleanly: data-event tests (`alert_lifecycle_in_app`, `metrics_update_stored_by_target`, `alerts_snapshot_replaces_active`) move with `PresenceModel::apply`; input tests (`quit_on_q`, `tab_switching`, `slash_enters_chat_mode`, `chat_mode_*`) become Intent-translation tests in the TUI skin. Every test keeps its assertion; only the constructor call changes (`App::new()` → presence + view).

### 3.4 The chat stub is the pre-cut, empty mind seam

Today chat is local echo: on `Enter`, `handle_chat_key` pushes the *user's own* line to `chat_log` (`app.rs:183-193`) — there is **no agent response path, no OutputSink**. This is exactly the seam where the mind attaches. Post-refactor, `Intent::SubmitChat(text)` routes to `MontyPresence`, which (when the mind is wired, a later phase) becomes `session.submit_input(mind, text)` → mind turn → `session.emit(AgentThought/Stdout, …)` → fans to both skins' `OutputSink`s → each appends to its rendered transcript. Until the mind lands, `SubmitChat` keeps the local-echo behavior so the refactor is behavior-preserving.

---

## 4. The new egui skin as the second attachment

The egui skin is 100% new (a `monitor-gui` crate, `caster-station.md:216-218`). It renders the **same** `PresenceModel` immediate-mode and attaches a second `OutputSink`.

### 4.1 Same shape, different draw + input source

eframe's `update(&mut self, ctx, _)` mirrors the ratatui loop body (`lib.rs:225-240`) exactly:

```rust
impl eframe::App for EguiSkin {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // 1. Drain inbound: data events + OutputChunks delivered by the sink.
        while let Ok(ev) = self.data_rx.try_recv() { self.presence.lock().apply(ev); }
        // 2. Render from a borrowed/snapshotted &PresenceModel.
        let model = self.presence.lock();              // see locking, §4.3
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_tabs(ui, &model);
            match model.active_tab {                    // same Tab identity as TUI
                Tab::Metrics => self.draw_metrics(ui, &model),   // egui_plot off history_for()
                Tab::Alerts  => self.draw_alerts(ui, &model),
                /* … */
            }
        });
        // 3. egui native input → Intent → presence.submit_intent(...).
    }
}
```

- **Graphs**: `egui_plot` time-series off the same `history_for(target, metric, width)` rings (`app.rs:89-102`, `caster-station.md:238`) — real axes/zoom that ratatui sparklines can't do. `history_for` returns a `Vec<f64>`; for egui's per-frame paint, add a borrowing accessor `history_iter(target, metric) -> impl Iterator<Item=f64>` to the model to avoid per-frame alloc (the ratatui `Vec` path stays for sparklines).
- **Monty portrait**: instead of `ansi_to_text` (`ansi.rs:15`), egui renders the source PNG directly (`docs/logos/Monty_Lizard_Large.png` per `monitor-agent/CLAUDE.md`) or maps the shared SGR-parse output to `egui::Color32`. The parse logic is shared (§3.1); only the output type differs.
- **Input**: egui key/text events translate to the *same* `Intent` enum (§5.1). A focused chat `TextEdit` losing/gaining focus is the egui expression of `Mode::Chat`; submit on Enter → `Intent::SubmitChat`.

### 4.2 The egui `OutputSink` must not touch egui inside `deliver`

`OutputSink::deliver` (`session.rs:118-120`) is `Send` but **not** `Sync` and is synchronous, called from inside `session.emit` on whatever thread owns the session. The egui paint thread is separate. So the `EguiSink` must, inside `deliver`, **push the chunk onto an mpsc channel and (optionally) request a repaint** — never call into egui directly:

```rust
struct EguiSink { tx: mpsc::UnboundedSender<OutputChunk>, repaint: egui::Context }
impl OutputSink for EguiSink {
    fn deliver(&mut self, chunk: &OutputChunk) {
        let _ = self.tx.send(chunk.clone());
        self.repaint.request_repaint();   // wake the paint loop; safe cross-thread
    }
}
```

The skin drains that channel in `update()` and folds chunks into its rendered transcript. This is the exact discipline the design doc flags (`caster-station.md:243`, and the threading note `:326`).

### 4.3 The threading/locking wrapper — the genuinely new work

`SessionState` is single-threaded/in-process — it owns `Box<dyn OutputSink>` + a `BTreeMap` and has no interior mutability (`session.rs:175-183`), exactly as the doc warns (`caster-station.md:326`). Likewise `App` today is owned by a single stack frame and mutated by `&mut self` (`lib.rs:222`, `app.rs:114`). To fan one presence to a tokio collector task **and** an egui paint thread **and** a ratatui loop, the presence needs a concurrency wrapper. Two options:

- **A. `Arc<Mutex<MontyPresence>>`** — simplest. Collectors lock to `apply`; each skin locks briefly to read for a frame and to `submit_intent`. Acceptable because critical sections are tiny (one event apply / one frame snapshot). Risk: a skin holding the lock across a whole egui frame can stall the collector — mitigate by snapshotting cheaply (see C).
- **B. Actor task owning the presence** — one tokio task owns `MontyPresence` by value and receives `Attach`/`SubmitIntent`/`Apply`/`Emit` commands over an mpsc channel (the same shape `session.rs` already assumes). Each sink forwards to its surface channel. This matches the doc's recommended wrapper (`caster-station.md:326`) and keeps `SessionState` single-threaded (one owner) while still serving N surfaces. **Recommended** — it preserves the substrate's single-threaded invariant rather than fighting it with locks.
- **C. arc-swap snapshot for reads** (compatible with A or B): publish an immutable `Arc<PresenceModel>` snapshot after each mutation; skins read the snapshot lock-free each frame and only take the lock/send a command to *submit*. Best when egui repaints at 60fps and must never block the collector.

Whichever wins, it is **net-new code in the presence crate** — neither `App` nor `SessionState` provides it.

---

## 5. Input + output routing across both skins

### 5.1 Frontend-neutral `Intent` (the input contract)

Both skins translate native events into one enum; the core never sees crossterm or egui types:

```rust
pub enum Intent {
    Quit,
    SelectTab(usize),            // ← '1'..'4' (app.rs:212-215) / egui tab click
    CycleTab(i32),               // ← Tab/BackTab (app.rs:216-217)
    SubmitChat(String),          // ← chat Enter (app.rs:183-193) → mind turn (when wired)
    Cancel,                      // ← Esc / Ctrl-C in-turn → session.cancel_turn (session.rs:319)
    // Note: Scroll and text-edit are NOT here — they are per-skin view state
    //       (scroll_offset app.rs:57, chat_input app.rs:61), never canonical.
}
```

`MontyPresence::submit_intent` consumes `Intent` (the body is `app.rs:204-226` rewritten to match `Intent`), so the canonical reducer is skin-independent. This is the single biggest refactor to make the model skin-agnostic — and it is small (one enum + one match).

### 5.2 Driver/observer routing — who can drive

| Path | Role | Mechanism |
|---|---|---|
| Collector data feed | (not an attachment) | `spawn_collectors` mpsc `Event` (`daemon.rs:97-130`) → `presence.apply(DataEvent)`. Render-agnostic; **same source fans to both skins** (`daemon.rs:97` takes only `mpsc::Sender<Event>`, zero ratatui knowledge). |
| Monty mind (chat turns) | **`Driver`** | `session.submit_input(mind, text)` (`session.rs:259-287`) under `read_only_caveats()`. Sole Driver. |
| ratatui skin | **`Observer`** | attach `RatatuiSink`; cannot drive (`session.rs:268-270`). Human keystrokes → `Intent` → local view OR a *request* relayed to the mind. |
| egui skin | **`Observer`** | attach `EguiSink`; same as above. |
| (future) NATS-publish / TTS sinks | `Observer` | additional sinks on the same session — the capstone (`caster-station.md:243,306`). |

Because authority binds to the *active driver* (`effective_caveats`, `session.rs:251-254`) and only the mind is a Driver, every emitted turn is read-only regardless of which skin the human typed into. The model is symmetric: a human at the keyboard and a human at the GPU GUI co-observe the *same* mind turn.

### 5.3 Output fan-out + replay for late-join

`session.emit` fans one `OutputChunk` to every attachment in a single pass (`session.rs:307-309`), proven by `observer_receives_output_but_cannot_drive` (both sinks get the same count, `session.rs:542-543`) and `detach_stops_fanout` (`session.rs:667-688`). A skin attaching late (egui opened after the TUI has been running, or a TUI reconnecting after the daemon/TUI IPC split) calls `replay_from(seq)` (`session.rs:326-332`) and renders the retained tail before live chunks resume — `replay_returns_the_buffered_tail` proves the bounded-ring semantics (`session.rs:642-664`). **Caveat**: the ring is lossy at cap 256 (`session.rs:357`), so full scrollback for a long-absent skin needs a durable store later; the tail is enough for "open the GUI and see the recent transcript."

---

## 6. PR-sized phased roadmap

Conventions inherited from both repos: TDD, `cargo clippy --workspace --all-targets -- -D warnings` clean, ≥80% coverage (ratchet up, never down), branch `step-NN.M-kebab` / `feat/…`, PR body with *What this PR does / Test plan / Out of scope*, **never push to main**, run `just install-hooks` then `just check` green first (`monitor-agent/CLAUDE.md` Key Design Rules; `caster-station.md:286`).

**Crate home (decision point, see §7).** These phases assume a new **`monitor-presence`** crate that depends on `monitor-core` (model types) and, behind the `newt` feature, `newt-core` (`SessionState`/`OutputSink`). It is consumed by `monitor-tui` (and later `monitor-gui`). This is feature-gated so the default `cargo build --workspace` stays green and newt-free (`caster-station.md:290`, `monitor-station/src/lib.rs:23-32`).

| Phase / PR | Branch | What this PR does | Out of scope | Gate |
|---|---|---|---|---|
| **P1 — Extract the seam; TUI is a skin (single-skin proof)** | `step-P1-presence-seam` | Create `monitor-presence`. Move the canonical subset of `App` → `PresenceModel`; move data arms of `update` → `PresenceModel::apply`; move `history_for`/`recent_chat`/`Tab`/`Mode`/`ChatMessage`; split `Event`→`DataEvent`. Introduce `Intent` + `MontyPresence::submit_intent`. Refactor `monitor-tui` so `ui.rs` reads `&PresenceModel` + a new `TuiViewState` (`scroll_offset`/`mode`/`chat_input`/`now`), and crossterm keys translate to `Intent`. **No `SessionState`, no egui, no mind, no swarm.** Behavior identical (local chat echo preserved). | egui; newt-core; the mind; fan-out; swarm tabs | Migrate every `app.rs` test (`:249-539`) and `ui.rs` render test (`:611-732`); all pass. Prove `monitor-tui` builds & runs identically. |
| **P2 — Wire `SessionState` as the fan-out, TUI as an Observer sink** | `step-P2-session-fanout` (feature `newt`) | Add `newt-core` dep behind `newt`. `MontyPresence` owns a `SessionState`; implement `RatatuiSink: OutputSink` that folds delivered `OutputChunk`s into the transcript. Attach the TUI sink as `Observer`. Mint the read-only `AgentKey` (reuse `monitor_station::identity::read_only_operating_key`, `identity.rs:59`) and store it in the presence (not yet a Driver). | egui; the mind submitting real turns; locking wrapper | Unit: a synthetic `emit` reaches the TUI sink and appears in the transcript; Observer `submit_input` returns `NotADriver` (mirror `session.rs:517-544`). |
| **P3 — Concurrency wrapper (`Arc<Mutex>` or actor)** | `step-P3-presence-actor` | Add the wrapper from §4.3 (recommend the actor) so collectors (`spawn_collectors`, `daemon.rs:97`) and the TUI loop share one presence without `&mut` stack ownership. Move `App::new()`-inside-`run` (`lib.rs:222`) above the loop. | egui | Concurrency tests: collector `apply` + skin read interleave without data race; one `emit` still fans correctly. |
| **P4 — egui skin skeleton (second attachment)** | `step-P4-egui-skeleton` (new `monitor-gui`) | New `monitor-gui` crate; eframe `update` drains the shared presence and renders tabs from `&PresenceModel`; `egui_plot` graphs off `history_for`. Implement `EguiSink` (channel + `request_repaint`, §4.2), attach as a second `Observer`. Native egui input → `Intent`. | animated Monty; voice; brush; mind | Headless egui test harness (`caster-station.md:303`): render empty + populated model without panic; `EguiSink::deliver` enqueues a chunk; both sinks receive the same `emit`. |
| **P5 — Late-join replay across skins** | `step-P5-replay-resume` | On skin attach, call `replay_from(last_seen_seq)` (`session.rs:326`) and render the tail before live chunks. Exercise: TUI running, egui attaches late, sees recent transcript. | durable scrollback store | Test: attach a fresh sink mid-session, assert it receives the buffered tail then live chunks (mirror `session.rs:642-664`). |
| **P6 — Wire the Monty mind as the Driver (read-only leash)** | `step-P6-mind-driver` | Attach the mind as the sole `Driver` with `read_only_caveats()`. `Intent::SubmitChat` → `session.submit_input(mind, text)` → mind turn → `session.emit(...)` → fans to both skins. Replace the local-echo stub (§3.4). | inference backend selection; tools/brush | Test: `effective_caveats` during a mind turn denies exec/write/net (mirror `session.rs:607-640` + `read_only_caveats`); both skins render the same turn. |

P1 is the small, provable first slice the brief asks for: **extract the presence/core seam and make the existing `monitor-tui` a skin attached to it, before any egui**, with the whole existing test suite migrated and green as the proof.

---

## 7. Open decisions & risks

1. **Reuse newt-core `SessionState` vs a lean monitor-local presence type.** `SessionState` (`session.rs:175-396`) is fully tested (8 attach tests, `session.rs:473-688`), enforces driver-only/serialized-turns/replay structurally, and is the substrate the §10 capstone unifies onto. Cost: it pulls the `newt` feature edge (`agent-mesh-protocol::Caveats`, `caveats.rs:26`) and is single-threaded. **Recommendation: reuse it behind `newt`** for the fan-out/authority/replay, and keep `PresenceModel` (the data) monitor-local and newt-free so the default build needs no newt. A lean local fan-out would duplicate proven, tested machinery for no gain.

2. **In-process / single-threaded constraint (the load-bearing risk).** `SessionState` and today's `App` are both single-owner (`session.rs:175-183`, `lib.rs:222`); egui paints on its own thread while collectors push from tokio (`daemon.rs:104-118`). The wrapper (§4.3) is mandatory and net-new. **Recommendation: actor model (B)** — one task owns the presence, preserving the substrate's single-threaded invariant, with skins as channel clients. Decide P3 between actor vs `Arc<Mutex>+arc-swap` based on measured egui frame-lock contention.

3. **`OutputSink: Send` but `!Sync`, synchronous `deliver` (`session.rs:118`).** The egui sink must enqueue-and-repaint, never touch egui inside `deliver` (§4.2). If a future sink blocks in `deliver`, it stalls `emit` for *all* attachments — sinks must be non-blocking. Enforce by convention + a test that `deliver` returns promptly.

4. **Where the presence/GUI crates live — Model A vs Model B (`caster-station.md:418-434`).** Model A grows `monitor-agent` (add `monitor-presence`/`monitor-gui`); Model B makes caster a separate binary repo git-dep'ing newt, consuming `monitor-*` as a library. The doc recommends **Model B** to keep `monitor-agent` a clean, ecosystem-independent monitor. The presence-seam refactor (P1) is **independent of this choice** — `PresenceModel`/`Intent`/skin-split are pure monitor-agent improvements that help either model. **Recommendation: do P1 in `monitor-agent` regardless; defer the A/B repo decision to before P4 (egui), when the newt/git-dep wiring actually bites.**

5. **The lossy replay ring (cap 256, `session.rs:357`).** Good enough for "open the GUI, see recent transcript," but a skin gone for many turns misses the head. Full scrollback needs a durable `ConversationStore` later — out of scope for two-skins.

6. **How the mind is wired (P6).** This design keeps the mind abstract: it is whatever Driver submits turns under `read_only_caveats()`. The actual inference backend (and whether it pulls `newt-scheduler`, which drags `newt-inference`/reqwest, `caster-station.md:325`) is deferred. The seam (`Intent::SubmitChat` → `submit_input` → `emit`) is fixed now (the chat stub `app.rs:183-193` is the pre-cut hole); the brain behind it lands later.

7. **`Mode`/focus mismatch between skins.** `Mode::Chat` (`app.rs:8-13`) is a TUI input-mode; egui expresses "I'm typing in chat" as widget focus. Keeping `mode` per-skin (§2.2) avoids forcing a TUI concept onto egui — but means "is the user composing?" is *not* in the shared model. If a future feature needs cross-skin awareness of composition state, that becomes a small canonical flag — flagged, not solved here.

---

### Files cited (all absolute)
- `C:\workspaces\monitor-agent\monitor-tui\src\app.rs` — `App`/`update`/handlers/accessors/`Tab`/`Mode` (the seam to split)
- `C:\workspaces\monitor-agent\monitor-tui\src\event.rs` — `Event` (split into `DataEvent` + terminal variants)
- `C:\workspaces\monitor-agent\monitor-tui\src\lib.rs:207-245` — `run()` Crush loop; `App` built inside loop (the blocker)
- `C:\workspaces\monitor-agent\monitor-tui\src\ui.rs` — ratatui render (stays in skin; not reusable)
- `C:\workspaces\monitor-agent\monitor-tui\src\ansi.rs` — SGR parse (shared) vs ratatui assembly (skin)
- `C:\workspaces\monitor-agent\monitor-cli\src\daemon.rs:97-165` — `spawn_collectors` render-agnostic `mpsc<Event>` feed
- `C:\workspaces\monitor-agent\monitor-core\src\metrics.rs:36-95` — `MetricSet`/`MetricPath`/`MetricValue` (frontend-neutral payload)
- `C:\workspaces\newt-agent\newt-core\src\session.rs` — `SessionState`/`OutputSink`/`OutputChunk`/`AttachRole`/`attach`/`emit`/`submit_input`/`effective_caveats`/`replay_from` (the fan-out substrate)
- `C:\workspaces\monitor-agent\monitor-station\src\identity.rs:37-66` — `read_only_caveats`/`read_only_operating_key` (the leash)
- `C:\workspaces\monitor-agent\monitor-station\src\lib.rs` — station scaffold (no presence code yet)
- `C:\workspaces\monitor-agent\docs\design\caster-station.md:184-243` (§5 frontend plan), `:306-308` (§8 capstone), `:312-331` (§9 risks), `:418-434` (§10 Model A/B)