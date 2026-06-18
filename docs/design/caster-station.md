# caster Station — Architecture Proposal

> Lab-wide monitoring station for the homelab/agent farm, built as an **inherit-and-extend descendant** of `newt-agent`.
> Drop-in path: `monitor-agent/docs/design/caster-station.md`
> Status: **proposal** · Author: NUC01 agent · Evidence base: on-disk source in `C:\workspaces\{newt-agent,brush,monitor-agent,gilabot}`

---

## 1. TL;DR + Layered Architecture

**What caster is.** `caster` is the premium monitoring station that runs on a Windows GPU box. It is *not a new framework* — it is the **descendant** of two ancestors:

- **`newt-agent`** contributes the reusable **library spine**: object-capability identity (`newt-identity`), the availability-adaptive farm-health model (`newt-scheduler`), the multi-attach fan-out session/`OutputSink` model and config/store/router (`newt-core`), the generic mesh transport (`newt-mesh`), and the MCP client/server plumbing.
- **`monitor-agent`** contributes the **structural ancestor of the station itself**: it already owns the two extension traits (`Collector`, `AlertDispatcher`), the `AlertEngine` lifecycle, the `Event`-driven ratatui `App`, the width-selecting ANSI splash, and a working Windows SAPI `VoiceDispatcher`.

caster **extends both in place**: it keeps `monitor-agent`'s five crates verbatim and *adds* new crates (`monitor-swarm`, `monitor-gui`, `monitor-station`), pulling in `newt-*` library crates behind cargo features. The rich ratatui TUI and the egui GPU GUI are built **in our repo** — never by modifying `newt-tui`, which is deliberately a "plain scroller" by ADR #304 (`C:\workspaces\newt-agent\docs\decisions\plain_scroller_tui.md:73-88`, `:207-211`).

**Locked decisions honored.** Hybrid frontend (ratatui for headless farm boxes + egui for caster). Generic status pull (NATS JSON and/or newt-mesh — no hard `gilamonster-swarm-core` dependency). Native-Windows voice (SAPI TTS + whisper.cpp STT). brush as the embedded shell/tool-runner. Voice modules extracted into a standalone `gilavox` library under `github.com/Gilamonster-Foundation`.

```
                        ┌──────────────────────────────────────────────────────────────────────┐
                        │                       CASTER  STATION  (our repo)                      │
                        │                                                                        │
   ┌────────────────┐   │   ┌─────────────── monitor-station (caster binary) ───────────────┐    │
   │  gilavox        │   │   │  runtime mode select:  TUI (headless farm)  |  GUI (caster)   │    │
   │  (Python lib +  │◄──┼───┤  wires collectors → AlertEngine → dispatchers → frontends     │    │
   │  gilavox-daemon)│IPC│   └───────┬───────────────────────┬───────────────────┬──────────┘    │
   │  SAPI / whisper │   │           │                       │                   │               │
   └────────────────┘   │   ┌────────┴────────┐   ┌──────────┴────────┐  ┌───────┴───────────┐   │
                        │   │  monitor-tui     │   │   monitor-gui     │  │  monitor-swarm    │   │
   ┌────────────────┐   │   │  (ratatui;       │   │   (egui + wgpu;   │  │  (generic swarm   │   │
   │  brush          │◄──┼───┤   redesigned     │   │   egui_plot,      │  │   model; NATS +   │   │
   │  brush_core::   │   │   │   pilot surface) │   │   Monty, term)    │  │   feature=mesh)   │   │
   │  Shell<SE> +    │   │   └────────┬─────────┘   └─────────┬─────────┘  └────────┬──────────┘   │
   │  CommandInter-  │   │            └───────────┬───────────┘                    │              │
   │  ceptor hook    │   │              ┌─────────┴──────────────── shared core ───┴────────┐     │
   └────────────────┘   │              │  monitor-core   monitor-collect   monitor-alert    │     │
                        │              │  Collector  AlertDispatcher  AlertEngine  MetricSet │     │
                        │              └──────────────────────────┬──────────────────────────┘    │
                        └─────────────────────────────────────────┼─────────────────────────────┘
                                                                  │  (inherits, feature-gated)
   ════════════════════════════════════════════════════════════════════════════════════════════
                        ┌─────────────────────────── newt-agent FRAMEWORK (ancestor) ───────────┐
                        │                          library crates only — never modified          │
                        │  ┌────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐ │
                        │  │newt-identity│ │newt-scheduler│ │  newt-core   │ │ newt-mcp-client/ │ │
                        │  │ UserKey →   │ │ BackendPool  │ │ Config Router│ │  -server/-data   │ │
                        │  │ session_root│ │ PoolSource   │ │ SessionState │ │  McpTools seam   │ │
                        │  │ attenuate() │ │ Prober       │ │ OutputSink   │ │                  │ │
                        │  │ (read-only) │ │ DispatchStr. │ │ ConvStore    │ │                  │ │
                        │  └────────────┘ └──────────────┘ └──────────────┘ └──────────────────┘ │
                        │         │ (registry dep, pinned 0.6)        │ (path-dep, off-workspace) │
                        │  ┌──────┴───────────────────┐      ┌────────┴─────────┐                 │
                        │  │ agent-mesh-protocol 0.6  │      │ newt-mesh 0.6.8  │                 │
                        │  │ Caveats UserKey AgentKey │      │ MeshAsker /Service│                │
                        │  │ CertChain (crates.io)    │      │ (EXCLUDED member) │                │
                        │  └──────────────────────────┘      └──────────────────┘                 │
                        └────────────────────────────────────────────────────────────────────────┘
   ┌─────────────────────────────── ABSENT SIBLINGS (gaps) ────────────────────────────────────┐
   │  agent-mesh/{protocol,bus,discovery,transport}  ·  agent-bridle{,-tool-shell,-tool-web}    │
   │  hermes-thoon   —  not on disk; only contracts inferred from call sites & docs             │
   └────────────────────────────────────────────────────────────────────────────────────────────┘
```

**One-paragraph essence.** caster mints a *read-only* operating key (`newt_identity::session_root` → `attenuate` to a `ReadOnly` `Caveats`) so the station is structurally incapable of mutating the farm it watches. It pulls farm status generically through `monitor_core::Collector` impls (a NATS-JSON collector already exists; a feature-gated mesh collector is the main new ingest work). Each metric flows `Collector → AlertEngine → AlertDispatcher`, and the same `App`/`Event` state feeds **both** a redesigned ratatui TUI (headless boxes) and a new egui GUI (caster). brush is embedded as the in-process tool-runner under that same read-only leash. Voice is a `gilavox` sidecar over a JSON-lines socket.

---

## 2. The OOP "inherit & extend" mapping

`caster` is the subclass. `newt-agent` and `monitor-agent` crates are the base classes. The station **depends on** (inherits) the left column and **adds/overrides** the right.

| Ancestor crate / type (inherited) | Evidence | What caster ADDS / OVERRIDES |
|---|---|---|
| `monitor_core::metrics::Collector` (async trait: `name()`, `collect()->MetricSet`) | `monitor-core/src/metrics.rs:90-95` | **ADD** `MeshStatusCollector` and `SwarmJsonCollector` impls in `monitor-swarm` — folds NATS/mesh peer status into a `MetricSet`. No core change. |
| `monitor_core::alert::AlertDispatcher` (`fire`/`resolve`) | `monitor-core/src/alert.rs:191-196` | **ADD** a `GilavoxDispatcher` (full loop) alongside the existing `VoiceDispatcher`; **ADD** a mesh-pub dispatcher for cross-farm alert fan-out. |
| `monitor_core::alert::AlertEngine` (`evaluate(&MetricSet)->Vec<Alert>`, Pending/Firing/Resolved, cooldown) | `monitor-core/src/alert.rs:203-317` | **INHERIT verbatim.** Shared by both frontends. Add swarm-specific `AlertRule`s (budget exceeded, gatekeeper-deny spike, agent-down) via config — no code change. |
| `monitor_tui::{Event, App, run, splash_for_width}` (Crush loop) | `monitor-tui/src/app.rs:22-163`, `lib.rs:207-245` | **EXTEND** the `Event` enum and `Tab` set (`+Swarm`, `+Board`); keep the splash, the `draw→recv→update` loop, the 4 existing tabs. |
| `monitor_alert::VoiceDispatcher` (Windows→PowerShell SAPI) | `monitor-alert/src/voice.rs:40-64`, `:129-139` | **KEEP** as fire-and-forget fallback; **OVERRIDE** the interactive path by routing to `gilavox` (full record→STT→LLM→TTS loop). |
| `newt_identity::{load_or_generate, session_root, attenuate, enforced_caveats}` | `newt-identity/src/lib.rs:101,133,145,156` | **COMPOSE, don't fork.** caster calls `session_root` then `attenuate(root, &ReadOnly.to_caveats())` to mint its own provably-narrowed operating key. The ocap lattice guarantees it can only narrow (`attenuate` rejects amplification, `lib.rs:141-144`). |
| `newt_core::config::{ToolPermissions, PermissionPreset::ReadOnly, to_caveats}` | (map: `config.rs:1192-1289`) | **REUSE** the `ReadOnly` preset → `Caveats` lowering to derive the station's observer authority and the brush interceptor's leash. |
| `newt_core::session::{SessionState, OutputSink, AttachRole}` | `newt-core/src/session.rs:118` (`OutputSink::deliver`), `:97-103`, `:208-225` | **IMPLEMENT** `OutputSink` four ways: a ratatui sink, an egui sink, a NATS-publish sink, a TTS sink — all attach as `Observer`s to one `SessionState`. This *is* the station's display fan-out. |
| `newt_scheduler::{BackendPool, PoolSource, Prober}` | `newt-scheduler/src/lib.rs:151-176`, `probe.rs` | **IMPLEMENT** a `MeshSource`/`NatsSource` `PoolSource` (the documented-but-unimplemented extension, `lib.rs:149-150`) so farm liveness (`Up/Busy/Down` + model inventory) renders as a "breathing pool"; supply a richer `Prober`. |
| `newt_core::agentic::execute_tool` / `run_command` → agent-bridle → `brush_core::Shell` | `newt-core/src/agentic/tools.rs:667,724` | **REUSE** the confined-shell tool-call path (when bridle is non-stub); **OR** embed `brush_core::Shell` directly with a station `CommandInterceptor` for the GUI/TUI terminal panel. |
| `newt_core::agentic::McpTools` seam | `newt-core/src/agentic/mcp.rs:16-43` | **IMPLEMENT** for a caster pool (clone the `newt-tui::mcp::Mcp` reference impl) to drive remote farm MCP servers; **ADD** a `farm-status-mcp` server via the `newt-mcp-data` thin-adapter template. |
| `newt_mesh::{MeshAsker, NewtMeshService, InferenceRequest, caveats_for_peer}` | `newt-mesh/src/lib.rs:62-67`, `caveats.rs:87-90` | **REUSE** for authenticated peer status pull; **ADD** new capability tags/topics (`newt-status`, `farm/status/v1`) — net-new wire types, no agent-mesh change for LAN. |
| `newt_core::agent_identity::AgentIdentity` (committable persona) | `agent_identity.rs:213-261`, docs `:9-11` | **OVERRIDE** by shipping `caster[bot]` `agent-identity.toml` (the documented descendant override mechanism). |
| `newt-tui` `InputSurface`/`ReadOutcome`/`rich_input` (pub(crate)) | `newt-tui/src/lib.rs:1220-1255`, `rich_input.rs:1-42` | **COPY the design**, not the dependency (it is crate-private). Lift the gutter/vi-nano-emacs inline editor as the starting point and extend with panes/dashboards. |
| `run_pilot(flight_id)` stub | `newt-tui/src/lib.rs:420-422` | **IMPLEMENT** the ADR-sanctioned full-screen pilot/monitor surface — *in our repo*, satisfying this signature's intent. |

**Override rule of thumb:** anything in the *data/identity/transport spine* is **inherited and composed** (call the function, implement the trait). Anything in the *rich frontend, voice loop, or swarm view* is **added** in our crates. We never edit `newt-core`/`newt-tui`/`newt-scheduler` — that is forbidden by ADR #304 and the "no trait-based tool registry to fork" reality (tools are a hardcoded JSON table + match, map area `newt-tools-skills-bridle`).

---

## 3. Proposed repo / crate layout

`monitor-agent` **becomes** the caster station repo. It keeps its five existing crates and gains three new ones. It takes **optional, feature-gated** `newt-*` deps — not the whole newt workspace.

```
monitor-agent/                         (origin: hartsock/monitor-agent — stays its own git repo)
├── monitor-core/      [INHERIT, unchanged]  Collector + AlertDispatcher + AlertEngine + MetricSet.
│                                            Takes NO newt dep — stays ecosystem-independent.
├── monitor-collect/   [INHERIT, extend]     local/prometheus/nats/ssh collectors (verbatim).
├── monitor-alert/     [INHERIT, extend]     bell/voice/webhook/nats-pub; +gilavox dispatcher.
├── monitor-tui/       [INHERIT, redesign]   headless-farm ratatui; +Swarm/Board tabs, pilot surface.
├── monitor-cli/       [INHERIT, extend]     daemon wiring (build_collectors/dispatchers/spawn).
│
├── monitor-swarm/     [NEW]  generic swarm model (SwarmSession/SwarmBudget/GatekeeperDecision
│                             deserialized from NATS JSON) + Collector impls:
│                               · SwarmJsonCollector  (NATS JSON — default)
│                               · MeshStatusCollector (newt_mesh — feature = "mesh")
│                             ports monty-tui's swarm/board/character logic — NO gilamonster-swarm-core.
│                             board reader (data/board.rs) ports verbatim (pure fs).
│
├── monitor-gui/       [NEW]  egui + wgpu premium station. Renders the SAME App/Event/AlertEngine.
│                             egui_plot graphs, animated Monty, embedded brush terminal, voice waveform.
│                             impls PermissionGate as native allow/deny dialogs.
│
└── monitor-station/   [NEW]  caster binary. Selects TUI vs GUI at runtime; mints the read-only
                              operating key (newt-identity); wires monitor-collect + monitor-swarm
                              collectors into the shared AlertEngine; embeds brush_core::Shell.
```

**Dependency edges (and why they are gated):**

| Edge | Default? | Reason |
|---|---|---|
| `monitor-swarm → newt-mesh` (via `--manifest-path`) | **feature `mesh`, OFF** | `newt-mesh` is workspace-EXCLUDED (`newt-agent/Cargo.toml:24-34`) and path-deps `../agent-mesh` which is **not on disk**. Default build must stay green without it. |
| `monitor-station → newt-identity`, `newt-scheduler`, `newt-core` (registry/path) | **feature `newt`, ON for caster, OFF for headless** | Drags in `agent-mesh-protocol 0.6` (crates.io, resolvable) but `newt-scheduler` also pulls `newt-inference` (reqwest/tokio) — heavier than a pure status monitor needs. |
| `monitor-station → brush_core` (git, hartsock fork) | **feature `shell`, OFF** | The `CommandInterceptor` cap-hook lives only in the hartsock fork + open upstream PR `reubeno/brush#1184`; not on crates.io. Must git/path-dep or vendor. |
| `monitor-alert → agent-bridle` | **NOT taken** | bridle's brush-backed shell tool **fails closed** on the `feat/stub-shell` branch (no brush git deps in newt's `Cargo.lock`). caster embeds brush *directly* rather than routing through stub bridle until bridle PR #21/#20 lands. |

**Why `monitor-core` takes no newt dep:** the locked "generic data" decision. The spine stays ecosystem-independent; only `monitor-swarm`/`monitor-station` reach for `newt-*`, and always behind a feature so the default `cargo build --workspace` compiles on any box without the absent siblings.

**MSRV / edition reconciliation (required first PR):** `monitor-agent` pins `rust-version 1.80`, edition 2021, `toml 0.8`, `ratatui 0.29` (`monitor-agent/Cargo.toml`). `newt-agent` pins `1.75`/2021/`toml 1.0`. `gila-monitor-tui` is `ratatui 0.28`, `async-nats 0.38`, **edition 2024** (`gila-monitor-tui/Cargo.toml:3-29`). The station standardizes on `monitor-agent`'s pins; the monty-tui port must bump `ratatui 0.28→0.29` and drop edition-2024 features. `1.80 > 1.75`, so consuming `newt-*` is fine on the MSRV axis.

---

## 4. Data flow — pulling the whole farm + firing alerts/voice

caster gets data off the farm **two complementary ways**, both already half-built, unified behind the one `Collector` trait so the rest of the system can't tell them apart.

```
   FARM BOXES                      INGEST (monitor-swarm + monitor-collect)         STATION CORE
 ┌──────────────┐
 │ gnuc agent   │── NATS JSON ───►┌──────────────────────┐
 │ {cpu,budget, │  heartbeat      │ SwarmJsonCollector    │  collect()
 │  decisions}  │                 │ (NatsCollector shape) │──► MetricSet ──┐
 └──────────────┘                 └──────────────────────┘                │
 ┌──────────────┐                                                          ▼
 │ nuc agent    │── newt-mesh ───►┌──────────────────────┐         ┌───────────────┐   transitions
 │ AgentKey-    │  signed         │ MeshStatusCollector   │         │  AlertEngine   │──► Vec<Alert>
 │ signed status│  pub/sub        │ caveats_for_peer()    │──► MetricSet  evaluate()  │
 └──────────────┘  (feature=mesh) │ verifies every datum  │         └──────┬────────┘
 ┌──────────────┐                 └──────────────────────┘                │ firing / resolved
 │ caster local │── sysinfo ─────►┌──────────────────────┐                ▼
 │ (this box)   │                 │ LocalCollector        │──► MetricSet   ┌────────────────────┐
 └──────────────┘                 └──────────────────────┘                │  AlertDispatcher[]  │
                                                                          │  bell · gilavox-TTS │
   Backend liveness ("breathing pool"):                                   │  webhook · nats-pub │
   newt_scheduler::BackendPool ◄── MeshSource/NatsSource (PoolSource)      └─────────┬──────────┘
        refresh_health(&Prober) on a timer ──► Up/Busy/Down + model pin            │
                                                       │                            │ mpsc::Event
                                                       └────────────► Event::* ─────┤
                                                                                    ▼
                                                                 ┌──────────────────────────────┐
                                                                 │  App::update(event)  (shared)  │
                                                                 │  → ratatui draw  |  egui repaint│
                                                                 └──────────────────────────────┘
```

**Path 1 — raw NATS (default, broker-backed).** `monitor-collect/src/nats.rs` *already* ships a passive `NatsCollector` that subscribes to subjects and parses JSON `name→f64` maps into a `MetricSet` (`nats.rs:55-97`). `monitor-swarm`'s `SwarmJsonCollector` is the same shape but decodes richer swarm payloads (sessions/budgets/decisions) into a station-local generic model — **no `gilamonster-swarm-core`**. Path of least resistance, sub-ms localhost latency, works the instant a broker exists, reaches non-mesh producers.

**Path 2 — the mesh (authenticated, zero-broker).** caster joins as a peer under the farm `UserKey`; a `MeshStatusCollector` either polls peers (`MeshAsker::ask`, `newt-mesh/src/ask.rs:30-67`) or subscribes to status topics they publish. Every datum carries an ed25519 per-envelope signature + monotonic-sequence/nonce replay defense, and is attributed to a specific `AgentKey`. Ingest gates through `caveats_for_peer(cert)` (`newt-mesh/src/caveats.rs:87-90`) so the station trusts only verified, properly-attenuated peers.

**Verdict (from the mesh subsystem map):** mesh is the right *authenticated default* for a trusted home-lab farm (identity/provenance/zero-broker-ops, mDNS auto-discovery); raw NATS is the *fallback* for per-message latency, cross-subnet reach (mesh is mDNS-only → single broadcast domain today), and non-mesh agents. Both surface as `Collector`s, so the `AlertEngine` and both frontends are identical regardless of source.

**Liveness model.** `newt_scheduler::BackendPool` driven by a custom `PoolSource` (the documented `MeshSource`, `lib.rs:149-150`) + a `Prober` on a timer gives the "breathing pool": `Up/Busy/Down` + model inventory, with `model-pin` load-bearing (a 16GB box cannot host a 30B planner). This renders directly as a farm-health panel.

**Alert + voice fire path.** Verbatim from `monitor-cli/src/daemon.rs:97-165`: each collector's `MetricSet` → `engine.evaluate()` → for each firing transition, every `AlertDispatcher::fire` runs (bell, voice/gilavox, webhook, nats-pub), and an `Event::AlertFired` goes to the frontend channel. Voice escalates by severity (`voice.rs:86-92`: `Critical`→"WARNING.", `Warn`→"Heads up."). The interactive voice path (mic→STT→LLM→TTS) is the `gilavox` sidecar (§7).

---

## 5. Frontend plan — hybrid ratatui TUI + egui GUI

Both frontends are **skins over one core**: they share `App`/`Event`/`AlertEngine`/`Collector`. Only the render layer differs.

### 5a. Redesigned ratatui TUI (headless farm boxes)

Implements the empty `run_pilot` intent in our repo. Lifts monty-tui's tab layout (`gila-monitor-tui/src/ui/mod.rs:26-67`) and the inline-editor design from `newt-tui/rich_input.rs` (copied, since it is `pub(crate)`).

```
┌─ caster · NUC01 ──────────────────────────────────── 14:32:07 ─ ●NATS ●mesh ─┐
│ ┌─ Monty ──┐  ┌─ chat / agent transcript (OutputSink → ratatui) ───────────┐ │
│ │  ___      │  │ gnuc/worker: planning unit-05 quiz…                       │ │
│ │ (o o)  !! │  │ nuc/iggy: applied patch, tests green                      │ │
│ │  \_/      │  │ > _ (vi/nano/emacs inline editor, gutter, live clock)     │ │
│ └──────────┘  └────────────────────────────────────────────────────────────┘ │
├──[Alerts]─[Metrics]─[History]─[Rules]─[Swarm]─[Board]────────────────────────┤
│  Swarm tab:                                                                   │
│   agent       state     budget      last-decision     model-pin    health     │
│   gnuc/worker RUNNING   1.2k/5k tok  allow (gate)      qwen2.5:30b  ● Up       │
│   nuc/iggy    BUSY      4.9k/5k tok  DENY  (budget)    llama3.1:8b  ◐ Busy     │
│   nv/envy     DOWN      —            —                 —            ○ Down      │
│  Metrics tab: sparkline graphs from App::history_for() (60-sample ring)       │
├───────────────────────────────────────────────────────────────────────────────┤
│ q:quit  1-6:tabs  /:chat  v:voice  ↑↓:scroll                                  │
└───────────────────────────────────────────────────────────────────────────────┘
```

- **Tabs:** keep the 4 existing (`monitor-tui/src/app.rs:22-41`); **add** `Swarm` and `Board` from monty-tui. The `Tab` enum and `Event` enum merge (monty-tui's `SessionUpdate/BudgetUpdate/Decision/Heartbeat` from `event.rs:13-77` fold into monitor-tui's `Event`).
- **Graphs:** ratatui sparklines/charts off the existing `App::history_for(target, metric, width)` 60-sample ring buffers (`app.rs:88-102`).
- **Input editor:** copy `rich_input.rs`'s gutter + vi/nano/emacs `Edit` modes + `InputSurface`/`ReadOutcome` seam as the starting design, then grow it (history recall, model/plan-mode status tokens — all listed as v1 gaps in `rich_input.rs:27-42`).
- **Reused verbatim:** `color_supported()` degradation ladder (`newt-tui/lib.rs:428-439`), the splash RAII alt-screen idiom, the `draw→recv→update` loop (`monitor-tui/lib.rs:225-240`).

### 5b. egui GUI (caster's premium GPU station)

100% new (`monitor-gui` crate). Renders the **same** `App` immediate-mode via egui; the ratatui `ui/*.rs` render fns are **not** reusable, but the state model, `Event`, `AlertEngine`, and `Collector` pipeline are shared.

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│  caster                                                          ● mesh   ● NATS   │
│ ┌─────────────┬──────────────────────────────────────────────┬──────────────────┐ │
│ │  [Monty]    │  Alerts | Metrics | Swarm | Board | Logs      │  Farm health      │ │
│ │  animated   │ ┌──────────────────────────────────────────┐ │  gnuc  ● Up qwen  │ │
│ │  egui sprite│ │  CPU %  (egui_plot real-time line)        │ │  nuc   ◐ Busy 8b  │ │
│ │  state ←    │ │  ╱╲    ╱╲                                  │ │  nv    ○ Down     │ │
│ │  CharacterS-│ │ ╱  ╲__╱  ╲___                             │ │ ─────────────────│ │
│ │  tate(cpu)  │ └──────────────────────────────────────────┘ │  Voice waveform   │ │
│ │             │ ┌──────────────────────────────────────────┐ │  ▁▃▅█▅▃▁  ●REC    │ │
│ │  budget bar │ │  embedded brush terminal (brush_core::    │ │ ─────────────────│ │
│ │  decisions  │ │  Shell, fds → egui text; Reedline-style)  │ │  allow / deny     │ │
│ │             │ │  caster$ tail -n5 /var/log/iggy.log       │ │  (PermissionGate │ │
│ └─────────────┴─└──────────────────────────────────────────┘─┴──native dialog)──┘ │
└──────────────────────────────────────────────────────────────────────────────────┘
```

- **Real graphs:** `egui_plot` time-series from the same history rings — proper axes/zoom, the thing ratatui sparklines can't do.
- **Animated Monty:** port monty-tui's `CharacterState{Sleeping,Idle,Listening,Thinking,Active,SuperActive}` + `AttentionLevel::from_cpu` (`gila-monitor-tui/src/ui/character.rs:27-80`, attention engine `app.rs:889-986`) to egui sprite frames driven by farm CPU + voice state.
- **Embedded brush terminal:** a long-lived `ShellRef<StationShellExtensions>` (Arc<Mutex<Shell>>); GUI reads/writes the shell's `fds` (set via `.fds()`/`replace_open_files`) to render output and feed keystrokes (§6).
- **Voice waveform:** live mic level from the `gilavox` capture path; REC indicator tied to `Listening` state.
- **PermissionGate:** implement `newt_core`'s `PermissionGate` (map area `newt-tools-skills-bridle`) as a native egui allow-once/session-allow dialog when a tool call hits a denial.
- **OutputSink fan-out:** the egui chat/transcript pane is an `OutputSink` attached as an `Observer` to a `SessionState`; the ratatui TUI is a second sink on the same session — one stream, two skins, with `replay_from(seq)` for reconnect resume (`newt-core/src/session.rs:118`, `:208-225`).

---

## 6. brush + tools integration

caster embeds `brush_core::Shell` as its **in-process terminal / tool-runner** rather than shelling out to PowerShell.

**Embedding surface (verified on disk):**

```rust
// monitor-station: compose station extensions
type StationShellExtensions =
    brush_core::extensions::ShellExtensionsImpl<StationFormatter, StationInterceptor>;

let shell = brush_core::Shell::builder_with_extensions::<StationShellExtensions>()
    .do_not_inherit_env(true)            // confined env, seed own PATH (newt's pattern, tools.rs:207)
    .command_interceptor(StationInterceptor::from(read_only_caveats))
    .build().await?;
```

- **The leash hook** is `brush_core::extensions::CommandInterceptor` with `before_exec(program,&[args])->ExecDecision::{Allow,Deny}` and `before_open(&Path,write)->OpenDecision::{Allow,Deny}` (`brush-core/src/extensions.rs:104-136`). It is enforced at the *single* external-spawn funnel and the *single* file-open chokepoint, so a name-based policy can't be defeated by `/bin/rm` or `./x` (`extensions.rs:97-103`). caster's `StationInterceptor` lowers the station's **read-only `Caveats`** (the same key minted in §2) into deny-by-default exec/write, granting only the inspection commands the station needs (`tail`, `cat`, `ls`, `t10` over logs/config).
- **Two drive modes:** (a) **tool-runner** — long-lived `Shell`, call `run_string`/`run_dash_c_command` per tool invocation, read `ExecutionResult.exit_code` (`brush-core/src/shell/execution.rs:163,184`); (b) **REPL panel** — `brush_interactive::InteractiveShell` + a Reedline backend for the live terminal pane in the GUI/TUI.
- **agent-bridle / MCP path:** when non-stub `agent-bridle` lands, the *same* interceptor is exactly what `agent-bridle-tool-shell` wraps `Caveats` around — caster inherits that wiring pattern. Until then, bridle's `shell` tool **fails closed** on the `feat/stub-shell` branch (`newt-agent/Cargo.toml:218-222`; brush absent from newt's `Cargo.lock`), so caster embeds brush **directly**. For *remote* command execution on farm boxes, caster acts as an **MCP client** (clone `newt-tui::mcp::Mcp` over `newt_mcp_client`, or implement `McpTools`) and drives a `newt-mcp-server` running on each box under its own `Caveats` leash. A new `farm-status-mcp` server (NATS/mesh reads as MCP tools: `list_agents`, `node_health`, `tail_log`) is built by copying the `newt-mcp-data` thin-adapter template — one config line, zero core changes.
- **Skills as runbooks:** `newt-skills` is reusable verbatim — `SKILL.md` folders on a search path give operator-authored procedures (`restart-agent`, `drain-node`) with progressive disclosure into any LLM prompt the station runs (`newt-skills/src/lib.rs:332-416`).

**Caveat on the leash:** `CommandInterceptor` is synchronous and implements only `before_exec`/`before_open` — there is **no** `before_connect` network hook in code (`extensions.rs:104-136`); network-egress confinement is unbuilt in brush and deferred to a per-OS Layer-B proxy.

---

## 7. Extracted voice library — `gilavox`

The gilabot voice modules are extracted into a standalone reusable library `gilavox` under `github.com/Gilamonster-Foundation` (beside agent-bridle). caster consumes it as a **sidecar**, not a linked dependency.

- **Language strategy:** the working conversation loop, VAD, resampling, device handling, and HF auto-download already live and are 80%-covered in `gila-plugin-voice` (Python). **Python stays the implementation.** Rust is *not* rewritten. caster (Rust) drives a `gilavox-daemon` over a JSON-lines IPC socket (the protocol already defined in `services/daemon_client.py`: `ping/status/listen/speak/shutdown`; Windows transport already `127.0.0.1:9876`). No PyO3/cdylib needed — SAPI and whisper.cpp are reached via subprocess.
- **One interface, OS-selected impl:** `gilavox` defines `TtsEngine`/`SttEngine`/`VadEngine` ABCs; a runtime factory (`gilavox/factory.py`) picks concretes by `platform.system()` + config, exactly mirroring `monitor-alert`'s Rust `VoiceEngine::detect()` (`voice.rs:40-64`). On **caster (Windows):** `TtsEngine=SapiTtsEngine` (the same `System.Speech.Synthesis.SpeechSynthesizer` call at `voice.rs:129-139`, or `comtypes SAPI.SpVoice` to avoid spawning PowerShell per phrase) and `SttEngine=WhisperCppSttEngine` (shelling `whisper.cpp`/`whisper-cli` with a ggml model, GPU `-ngl 99` per `gila-plugin-whisper/services.py:447-480`). On **Linux/mac:** piper + faster-whisper. Both STT paths return the same `ListenResult`; both TTS paths consume the same `text`.
- **Stable API:** `VoiceSession(config).run()` (record→VAD→STT→LLM→TTS→play), `.listen(timeout)->ListenResult`, `.say(text)->SayResult`; plus `VoiceDaemonClient` (JSON-lines IPC) for non-Python consumers. Packaging: base deps `numpy+sounddevice+requests`, extras `[piper]/[faster-whisper]/[silero]/[whispercpp]/[windows]/[all]` so **caster installs only the `[windows]` subset** (no torch/piper).
- **Two consumption faces:** gilabot reduces `gila voice`/`gila whisper` to thin Click shims over `gilavox`; caster either repoints `VoiceDispatcher` to call `gilavox say` (consistency) **or** spawns `gilavox-daemon` and drives `listen/speak` over the socket for the egui GUI's bidirectional voice (the mic→STT round-trip is the main new Rust work). The station stays ecosystem-independent — voice is a sidecar over a documented socket, no Python linkage, no gilabot dependency.

---

## 8. Phased build roadmap

Each phase is **one PR-sized step** in dependency order, matching `newt-agent`/`monitor-agent` conventions: TDD, `cargo clippy --workspace --all-targets -- -D warnings` clean, ≥80% coverage (ratchet, never lower), branch `step-NN.M-kebab` (or `feat/…`), PR body with *What this PR does / Test plan / Out of scope*, **never push to main**, run `just install-hooks` then `just check` green first.

| Phase | Step | Scope (one PR) | Gates |
|---|---|---|---|
| **0** | 0.1 | **Workspace reconciliation.** Add `mesh`/`newt`/`shell` cargo features (all OFF by default). Confirm `cargo build --workspace` green with siblings absent. Document MSRV (1.80) / edition / `ratatui 0.29` as the station standard. No behavior change. | build+clippy+fmt |
| **1** | 1.1 | **Read-only identity.** `monitor-station` skeleton binary; mint `session_root`→`attenuate(ReadOnly)` operating key via `newt-identity` (feature `newt`). Ship `caster[bot]` `agent-identity.toml`. Test: key cannot widen (assert `CaveatAmplification` on amplify). | +cov 80% |
| **2** | 2.1 | **Generic swarm model.** `monitor-swarm` crate: station-local `SwarmSession/SwarmBudget/GatekeeperDecision` structs (serde from NATS JSON). Port monty-tui `data/board.rs` board reader verbatim. **No** `gilamonster-swarm-core`. | unit tests on JSON fixtures |
| | 2.2 | **SwarmJsonCollector** impl (`monitor_core::Collector`) folding swarm JSON → `MetricSet`. Wire into `build_collectors`. Mock NATS. | mock NATS, 80% |
| **3** | 3.1 | **TUI extension.** Merge `Event`/`Tab` enums; add `Swarm` + `Board` tabs to `monitor-tui`; bump any monty-tui-ported render to `ratatui 0.29`. Keep splash + Crush loop. | snapshot-ish render tests |
| | 3.2 | **Inline editor port.** Copy `rich_input.rs` design (gutter, vi/nano/emacs, `InputSurface`/`ReadOutcome`) into `monitor-tui`; extend with history recall + status tokens. | unit tests on edit modes |
| **4** | 4.1 | **Breathing pool.** Implement `PoolSource` (`NatsSource`) + a status `Prober`; drive `BackendPool::refresh_health` on a timer; render `Up/Busy/Down`+model-pin in Swarm tab. | timer/health unit tests |
| **5** | 5.1 | **brush embedding (tool-runner).** Embed `brush_core::Shell` (feature `shell`, git-dep hartsock fork) with `StationInterceptor` lowering read-only `Caveats`. `run_string` path + `ExecutionResult`. Test deny on disallowed exec/open. | interceptor deny tests |
| | 5.2 | **brush REPL panel** via `brush_interactive::InteractiveShell` + Reedline; fds piped for capture. | |
| **6** | 6.1 | **gilavox extraction (separate repo PRs).** Scaffold `Gilamonster-Foundation/gilavox`; relocate models (drop pandas), introduce engine ABCs, factory, native-Windows SAPI + whisper.cpp engines, daemon. Migrate the already-mocked tests (≥80%). | gilavox CI, mocked |
| | 6.2 | **caster voice wiring.** `GilavoxDispatcher` (`AlertDispatcher`) calling `gilavox say`; doctor check for SAPI + whisper.cpp + ggml model. | mock subprocess |
| **7** | 7.1 | **MCP client pool.** Implement `McpTools` (clone `newt-tui::mcp::Mcp`) over `newt_mcp_client`; reuse `newt_core::mcp::discover`. Drive remote farm `newt-mcp-server`s. | MockTransport tests |
| | 7.2 | **farm-status-mcp server** via the `newt-mcp-data` thin-adapter template (`list_agents`/`node_health`/`tail_log`). | in-band envelope tests |
| **8** | 8.1 | **egui GUI skeleton** (`monitor-gui`): render shared `App` state; tabs; `egui_plot` graphs off history rings. | headless egui test harness |
| | 8.2 | **Animated Monty** (port `CharacterState`+`AttentionLevel::from_cpu` to egui) + **voice waveform** + **embedded brush terminal pane** + **PermissionGate** native dialog. | |
| **9** | 9.1 | **Mesh collector** (feature `mesh`, off-workspace `--manifest-path` newt-mesh): `MeshStatusCollector` over `MeshAsker`/published topics; gate ingest through `caveats_for_peer`. New `newt-status`/`farm/status/v1` tags+topics. | only runs when agent-mesh checked out |
| **10** | 10.1 | **OutputSink fan-out + daemon/TUI IPC split** (monitor-agent Phase 11): one `SessionState`, ratatui + egui + NATS-publish + TTS sinks as `Observer`s; `replay_from` reconnect. | fan-out unit tests |

Ordering rationale: identity (2) gates everything authority-bearing; generic model (2) precedes any collector; TUI extension (3) is independent and parallelizable; brush (5) and voice (6) are leaf capabilities; egui (8) needs the shared `App` stable; mesh (9) is last because it depends on absent siblings; the OutputSink unification (10) is the capstone that makes "two skins over one core" real.

---

## 9. Open decisions & risks

**Absent sibling repos (hard build/CI constraint).** `agent-mesh/{protocol,bus,discovery,transport}`, `agent-bridle{,-tool-shell,-tool-web}`, and `hermes-thoon` are **not on disk** (confirmed: `C:\workspaces\agent-mesh`, `\agent-bridle`, `\hermes-thoon` all absent).
- Only `agent-mesh-protocol 0.6` is on crates.io (`newt-agent/Cargo.toml:156`); `agent-mesh-bus`/`-discovery` are **not published**, so any mesh-binding crate must path-dep a local checkout and live **outside** its workspace — the same exclusion tax `newt-mesh` pays. → **Mitigation:** feature-gate `mesh`/`newt`/`shell`; default build stays green without them. *(Naming wrinkle to resolve: `newt-mesh/src/lib.rs:15` imports `agent_mesh_core` in its doc example while the rest uses `agent_mesh_protocol` — confirm the actual crate name before path-dep.)*
- `agent-bridle`'s brush-backed shell **fails closed** on `feat/stub-shell` (brush absent from newt's `Cargo.lock`). The confined-shell-over-brush capability is **not yet live anywhere** — caster would be among the first real consumers. → caster embeds brush **directly** until bridle PR #21/#20 + `reubeno/brush#1184` land.

**The newt "opinionated, not extensible" stance.** ADR #304 forbids advanced TUI in `newt-tui`, and there is **no trait-based tool registry** to subclass — built-in tools are a hardcoded JSON table + `match` (`tools.rs:13-127`, `:666`). "Inherit its functions" must be read as *depend on the library crates + copy private designs*, **not** `use newt_tui::...` for the editor. The valuable `rich_input`/`InputSurface` code is `pub(crate)` and must be **copied, not imported**. Station-specific tools enter only via the `McpTools` seam or a parallel dispatcher.

**Version skew.** newt `1.75`/2021/`toml 1.0`/`ratatui` (via newt-tui) vs monitor-agent `1.80`/2021/`toml 0.8`/`ratatui 0.29` vs monty-tui `ratatui 0.28`/`async-nats 0.38`/**edition 2024**. Station standardizes on monitor-agent's pins; the monty-tui port must bump `ratatui 0.28→0.29` and shed edition-2024 features. `async-nats` differs (0.37 monitor vs 0.38 monty) — pick one (recommend 0.37 to match the inherited collector).

**ratatui 0.29 alignment.** monitor-agent already pins `0.29`/`crossterm 0.28` deliberately "to match monty-tui versions" (CLAUDE.md), but monty-tui itself is still `0.28` — the *port direction* is monty→monitor (up to 0.29), and the GUI sidesteps it entirely (egui).

**Other risks.**
- **`newt-scheduler` drags `newt-inference`** (reqwest/tokio HTTP) even for a pure status monitor (`dispatch.rs:17-20`). → caster may use only `newt-core::session` + a hand-rolled status pool if it never dispatches inference.
- **`SessionState` is single-threaded/in-process** (owns `Box<dyn OutputSink>` + `BTreeMap`, `session.rs:175-183`); the station must supply the async/locking wrapper to fan one session to a GUI thread + network peers. The `newt/session/v1` wire protocol is unbuilt.
- **mesh is mDNS-only → single broadcast domain.** A farm spanning subnets/VLANs/WireGuard needs the direct-dial path (`Endpoint::dial`, repo absent/unverified) or a rendezvous cache. WireGuard does not forward mDNS multicast.
- **No streaming/presence topic in newt-mesh today** — only single-shot `InferenceRequest`/`Reply`; a status *feed* is net-new wire types + the `bus.publish_to` half.
- **Windows path hygiene:** monty-tui persists to `~/.config/monitor-lizard` and `/var/lib/...` (`app.rs:27-39`) — needs Windows-appropriate paths (`%LOCALAPPDATA%`) for caster. `AgentIdentity`'s `cmd` secret source shells via COMSPEC on Windows (`agent_identity.rs:144`).
- **whisper.cpp binary drift** (`main`→`whisper-cli`, Make→CMake) and unverified Windows wheels for faster-whisper/piper — the `[windows]` extra must handle both binary names; Silero VAD needs torch (heavy) → a torch-free VAD fallback is desirable behind `VadEngine`.
- **"Descendant inherits newt" is architectural intent, not a cargo edge today.** `monitor-agent` (origin `hartsock/monitor-agent`) currently has **zero** newt dep (grep confirmed); the relationship is documented in CLAUDE.md scope, and this proposal is what makes it a real (feature-gated) cargo edge.

---

### Key file references (load-bearing)
- Ancestor traits: `C:\workspaces\monitor-agent\monitor-core\src\metrics.rs:90-95`, `alert.rs:191-196`, `:203-317`
- Daemon pipeline: `C:\workspaces\monitor-agent\monitor-cli\src\daemon.rs:97-165`
- NATS collector (Path 1 template): `C:\workspaces\monitor-agent\monitor-collect\src\nats.rs:55-108`
- Windows SAPI voice: `C:\workspaces\monitor-agent\monitor-alert\src\voice.rs:40-64,129-139`
- TUI Crush loop + Event/App: `C:\workspaces\monitor-agent\monitor-tui\src\app.rs:22-163`, `lib.rs:207-245`
- Identity attenuation: `C:\workspaces\newt-agent\newt-identity\src\lib.rs:133-159`
- Fan-out OutputSink: `C:\workspaces\newt-agent\newt-core\src\session.rs:97-103,118,208-225`
- Scheduler PoolSource seam: `C:\workspaces\newt-agent\newt-scheduler\src\lib.rs:149-176`
- brush cap-hook: `C:\workspaces\brush\brush-core\src\extensions.rs:104-136`
- mesh exports + caveats: `C:\workspaces\newt-agent\newt-mesh\src\lib.rs:62-67`, `caveats.rs:87-90`
- agent-bridle stub patch: `C:\workspaces\newt-agent\Cargo.toml:205-222`
- monty-tui swarm/board/character (to port): `C:\workspaces\gilabot\gila-monitor-tui\src\ui\mod.rs:6-67`, `event.rs:13-77`, `ui\character.rs:27-80`

---

## 10. Addendum — sibling repos now on disk (supersedes the §9 "absent" gaps)

After this proposal was synthesized, the related Gilamonster-Foundation repos were
cloned into `C:\workspaces\`. This section updates the gaps flagged in §1/§6/§9.

### 10.1 `agent-mesh` — present (7 crates)

`agent-mesh-protocol` (ed25519 identity, signed envelopes), `agent-mesh-discovery`
(mDNS LAN, `_agent-mesh._udp.local.`), `agent-mesh-transport` (authenticated QUIC
via iroh — the agent signing key doubles as the iroh `EndpointId`), `agent-mesh-bus`
(pub/sub + request/reply), `agent-mesh-ratchet`, `agent-mesh-cli` (`amesh`),
`agent-mesh-py`.

- The crate the report wanted is **`agent-mesh-protocol`** (crates.io `0.5`; newt pins
  `0.6` → reconcile). This resolves the `agent_mesh_core` vs `agent_mesh_protocol`
  naming wrinkle from §9: the published crate is `agent-mesh-protocol`.
- **`agent-mesh-bus` IS the status feed** §4/§9 called net-new — Path 2 status topics
  ride `Bus` pub/sub + request/reply instead of inventing wire types. Farm boxes
  `amesh announce --capability ollama --role inference-worker`; caster discovers, and
  the **auto-team rule** (`user_pubkey != ours` ⇒ reject, fail-closed at the QUIC
  handshake) gates ingest before any payload crosses.
- Cloning into `C:\workspaces\agent-mesh` **satisfies `newt-mesh`'s `../agent-mesh/`
  path-dep**, so the `mesh` feature builds locally now (Phase 9 unblocked).
- Unchanged caveat: mesh is mDNS-only (single broadcast domain); cross-VLAN/WireGuard
  still needs the direct-dial path.

### 10.2 `agent-bridle` — present; the confined brush shell is real, not a stub

Crates: `agent-bridle-core` (`Tool` trait, `Registry`, `Gate`, `Caveats` re-export,
`Sandbox`), `agent-bridle-tool-shell` (brush-backed confined shell, carried coreutils),
`agent-bridle-tool-web` (SSRF-screened `web_fetch`), `agent-bridle` (facade
`registry()`), `agent-bridle-mcp` (MCP stdio server over the confined tools). Head
commit = **Windows clean-build**.

- This **supersedes** §6/§9's "confined-shell-over-brush is not live anywhere → embed
  brush directly." caster can route shell tools through
  `registry().dispatch("shell", json!({program, args}), &granted)` and inherit the
  **Caveats leash** (`required ⊑ granted`, least-authority meet; `ToolContext` is a
  mint-token constructible only inside `Gate::authorize`) plus Linux Landlock —
  governed by the **same `agent-mesh-protocol::Caveats` lattice** `newt-identity`
  already uses. The station's read-only key (§2) lowers straight into the granted
  Caveats.
- **`agent-bridle-mcp`** is the drop-in for "other tools": any MCP client drives it over
  stdio with `$AGENT_BRIDLE_CAVEATS` selecting the leash.
- **Recommendation:** use **agent-bridle for confined tool dispatch** (the
  capability-governed, long-term-correct path) and keep **direct `brush_core::Shell`
  embed only for the live REPL/terminal pane** — bridle is one-shot dispatch, not an
  interactive terminal. The two coexist; §6's direct-embed becomes the terminal-pane
  story, bridle becomes the tool-call story.

### 10.3 `gilamonster-agent` — the working exemplar (and a structural fork)

`gila` is the **already-building** newt descendant; its `Cargo.toml` is the reference
recipe:

- a **separate single-binary repo**, inheriting newt over **pinned git-deps** —
  `newt-tui`/`newt-core`/`newt-identity`/`newt-mcp-client` at `rev = 81488ef…` — *not*
  crates.io (newt cannot publish while the agent-bridle git-patch stands; a binary never
  needs crates.io, so the git-dep is the permanent shape, not a stopgap).
- rich TUI lives **in the descendant**: `ratatui 0.29` + `crossterm 0.28` +
  **`portable-pty 0.9` + `vt100 0.15`** to host a real shell in a TUI pane — directly
  reusable for the embedded-brush terminal, and a concrete alternative to §5b's
  fds-piping approach.
- mirrors newt's `[patch.crates-io]` agent-bridle block byte-for-byte; uses a
  git-ignored `.cargo/config.toml` overlay for local dev against a live newt checkout
  (`just overlay-on` / `overlay-off`).

**The fork this raises (the key remaining decision):** §3 proposes growing
`monitor-agent` into the station (add `monitor-swarm`/`-gui`/`-station` crates,
feature-gated newt deps). The family precedent — `gilamonster-agent`, `mogul-agent` —
instead makes each newt descendant a **separate binary repo** that git-deps newt and
consumes `monitor-*` as a library. Both are valid:

| | Model A — grow monitor-agent (the §3 proposal) | Model B — new `caster` repo, git-dep newt (the exemplar) |
|---|---|---|
| Repo | one repo; add 3 crates | new Foundation repo; monitor-* consumed as a lib |
| newt inheritance | feature-gated path/registry deps | pinned git-dep rev (matches gila/mogul) |
| monitor-agent identity | becomes the station (blends "standalone monitor" + "newt descendant") | stays a pure, ecosystem-independent monitor library |
| Consistency w/ family | divergent | matches every other newt descendant |
| Rich TUI / PTY home | new `monitor-tui`/`monitor-gui` crates | the new repo (ratatui + portable-pty + vt100, like gila) |

Recommended: **Model B** — it matches the established pattern, keeps `monitor-agent`
clean and reusable, and inherits gila's proven git-dep + `.cargo/config.toml` overlay
wiring verbatim.
