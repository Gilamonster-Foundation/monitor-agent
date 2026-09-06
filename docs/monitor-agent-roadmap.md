# monitor-agent — Development Backlog (Epics → Stories → Tasks)

> Companion to [`ROADMAP.md`](ROADMAP.md), which tracks high-level phase status.
> This document decomposes the **planned-but-unfinished** work into a
> test-driven backlog: Epics → Stories → Tasks. Every story carries a TDD note
> and an acceptance bar; the workspace-wide floor is **≥80% line coverage**
> (enforced by `just check` / `just cov-ci`).
>
> **Reading the marks:** ✅ done · 🔄 in review · ⬜ planned · ⏸ on hold
>
> **Provenance — what informed this backlog:**
> - `monitor-agent` current crates (`monitor-core`, `-collect`, `-alert`,
>   `-presence`, `-tui`, `-gui`, `-voice`, `-station`, `-cli`) and
>   [`design/inhabit-both-surfaces.md`](design/inhabit-both-surfaces.md),
>   [`design/caster-station.md`](design/caster-station.md).
> - `gilabot/gila-monitor-tui` — the swarm-dashboard reference implementation
>   (board, sessions, budget, decisions, github, infra, swarm, chat, voice,
>   settings, character tabs). Its feature surface is the *aspiration* for the
>   caster GUI/TUI once the presence seam is complete.
> - `newt-agent` / `gilamonster-agent` — the inherit-and-extend "descendant of
>   newt-agent" pattern: object-capability identity, a single read-only
>   `Driver`, turn loop, output fan-out. caster reuses this shape.

---

## How stories are sized

- **Epic** = a coherent outcome that ships user-visible value (maps to a
  ROADMAP.md section).
- **Story** = a vertical slice, independently mergeable, with its own
  acceptance criteria and coverage target.
- **Task** = a single commit-sized unit. Every task that touches code is
  written test-first (see *TDD discipline* below).

### TDD discipline (applies to every code task)

1. **Red** — write a failing test that pins the desired behavior. Run `cargo
   test -p <crate> <test>` and watch it fail for the right reason.
2. **Green** — write the minimum code to make it pass.
3. **Refactor** — extract / de-duplicate; tests stay green.
4. **Lint + coverage** — `cargo clippy -p <crate> -- -D warnings` and confirm
   the crate's coverage did not drop below the story's target. The
   workspace floor is **80%**; stories in `monitor-core`, `monitor-presence`,
   and `monitor-alert` target **90%** (pure logic, no I/O).

Tests that require a live NATS / Prometheus / SSH / microphone endpoint live
behind a `#[cfg(test)]` integration module gated by the `live` feature flag and
do **not** count toward CI coverage (they are skipped unless `--features live`
is passed). Wiremock / `mockall` / `tempfile` stand in for networked deps.

---

## Epic 1 — Complete the dual-surface presence
*ROADMAP §3, P4b + P5. Goal: caster renders Monty with real graphs, an embedded
terminal, and late-join replay — on both the ratatui and egui skins.*

### Story 1.1 — History-ring graphs via `egui_plot`
⬜ · crate: `monitor-gui` · coverage target 85%

The TUI already keeps history rings; the GUI shows static numbers. Render them.

- **Task 1.1.1** *(red)* Add a `gui_plot_test` that feeds a `SharedPresence`
  snapshot with N metric history points and asserts the plot line data
  matches (ordered, clamped to the visible window).
- **Task 1.1.2** *(green)* Add `egui_plot` dep; render a `Line` per metric
  series from the history ring in the metrics panel.
- **Task 1.1.3** *(refactor)* Extract a `MetricPlotModel` pure struct
  (`Vec<HistoryPoint> → Vec<[f64;2]>`) so the transform is unit-tested in
  isolation from egui rendering.
- **Task 1.1.4** *(red→green)* Time-axis windowing: pin the visible window to
  the last `T` seconds; test the clamp at the ring head and tail.
- **Acceptance** — `monitor-gui` shows live CPU/mem/disk sparkline+line graphs
  driven off the same `SharedPresence` the TUI reads. `cargo test -p
  monitor-gui` green; coverage ≥85%.

### Story 1.2 — Animated Monty mascot in the GUI
⬜ · crate: `monitor-gui` · coverage target 80% (rendering is visual; logic 90%)

- **Task 1.2.1** *(red)* Test a `MascotFrame::advance(dt, severity)` state
  machine: idle blink cadence, alert wiggle when severity ≥ warn.
- **Task 1.2.2** *(green)* Implement the frame selector; render the existing
  ANSI/ASCII Monty frames as a texture or procedural sprite.
- **Task 1.2.3** Map `AlertEngine` worst-severity → mascot mood each tick.
- **Acceptance** — Monty blinks when idle, reacts to firing alerts. State logic
  ≥90% covered.

### Story 1.3 — Embedded brush terminal in the GUI
⬜ · crate: `monitor-gui` (+ `monitor-station`) · coverage target 85%

The TUI has no input prompt; gila-monitor-tui's `ui/chat.rs` shows the pattern.

- **Task 1.3.1** *(red)* Test an `InputBuffer` (insert, backspace, history
  recall, submit→`Intent`) as a pure unit — no egui.
- **Task 1.3.2** *(green)* Add the `InputBuffer` to `monitor-presence` (it is
  surface-agnostic) and wire it into the GUI's chat/brush panel.
- **Task 1.3.3** *(red→green)* Paste / cursor / `Ctrl-C` interrupt handling,
  each with a unit test.
- **Acceptance** — user can type into the GUI, submit produces an `Intent` on
  the session, output fans back to the panel. Input logic ≥90% covered.

### Story 1.4 — Voice waveform visualization
⬜ · crate: `monitor-gui` + `monitor-voice` · coverage target 80%

- **Task 1.4.1** *(red)* Test a `WaveformSamples::push/pop_window` ring at
  fixed sample size.
- **Task 1.4.2** *(green)* Pipe mic RMS or dispatcher amplitude into the ring;
  render an `egui_plot::Bar` series.
- **Acceptance** — GUI shows a live waveform while Monty speaks/listens.

### Story 1.5 — Late-join replay across skins (P5)
⬜ · crate: `monitor-presence` + skins · coverage target 90%

- **Task 1.5.1** *(red)* Test `SharedPresence::replay_from(offset)` returns the
  bounded event tail in order; out-of-range offset clamps to head.
- **Task 1.5.2** *(green)* Implement `replay_from` over the event ring; have
  each `Observer` sink call it on `attach`.
- **Task 1.5.3** *(red→green)* Both skins render the replayed tail before
  switching to live; test the "replay then live" handoff ordering.
- **Acceptance** — a GUI launched 30s after the TUI shows the last N events,
  then continues live. `monitor-presence` ≥90% covered.

---

## Epic 2 — The Monty mind as the session's Driver
*ROADMAP §3, P6. Goal: a single read-only `Driver` turns chat input into
fan-out to both skins, reusing the newt-agent turn loop.*

### Story 2.1 — `Driver` trait + read-only capability mint
⬜ · crate: `monitor-station` · coverage target 90%

- **Task 2.1.1** *(red)* Test `Driver::turn(intent) -> Vec<DataEvent>` with a
  stub driver; assert the session fans events to all attached sinks.
- **Task 2.1.2** *(green)* Define the `Driver` trait in `monitor-station`;
  mint a read-only capability via `agent-mesh-protocol` (mirror `monitor-station`
  Phase 0/1 identity path) and pass it attenuated to the driver.
- **Task 2.1.3** *(red→green)* Reject any `Intent` that requires write
  capability the driver doesn't hold; test the denial path.
- **Acceptance** — a driver can be swapped in behind the session without skin
  changes; capability denials are testable. ≥90% covered.

### Story 2.2 — Monty mind driver (LLM-backed)
⬜ · crate: `monitor-station` · coverage target 80% (network behind `live`)

- **Task 2.2.1** *(red)* Test a `MindDriver` against a mock LLM client
  (`mockall`) — prompt assembly includes current `PresenceModel` snapshot
  (worst alert, top metrics).
- **Task 2.2.2** *(green)* Implement prompt templating + response parsing into
  `DataEvent`s (status text, alert ack, rule query).
- **Task 2.2.3** *(red→green)* Streaming: yield partial `DataEvent::Chunk`
  tokens so skins render incrementally.
- **Task 2.2.4** *(live, skipped in CI)* End-to-end against a local ollama model
  behind `--features live`.
- **Acceptance** — "why is nuc's disk full?" → Monty answers with live data and
  the reply streams into both skins.

### Story 2.3 — Alert→intent bridge
⬜ · crate: `monitor-station` · coverage target 90%

- **Task 2.3.1** *(red)* Test that a firing `Alert` synthesizes a high-priority
  `Intent` the driver must surface (preempts idle blink → alert mood).
- **Task 2.3.2** *(green)* Wire `AlertEngine` → `Intent` synthesis on the
  session bus.
- **Acceptance** — firing alerts interrupt the mind's idle state in both skins.

---

## Epic 3 — Voice into the station
*ROADMAP §4. Goal: a reusable voice loop (mic→STT→mind→TTS) wired into
`monitor-station`, with the `talk` timeout fixed.*

### Story 3.1 — `talk` / `listen --vad` timeout
⬜ · crate: `monitor-voice` · coverage target 90%

- **Task 3.1.1** *(red)* Test `record_until_silence` returns an
  `Err(Timeout)` after `max_secs` of silence/no-speech (use a fake clock /
  `tokio-test`).
- **Task 3.1.2** *(green)* Add the deadline; propagate the error to the caller
  instead of hanging.
- **Acceptance** — `listen --vad` never hangs; timeout is configurable and
  tested. ≥90% covered.

### Story 3.2 — Extract a `gilavox` library
⬜ · crate: new `monitor-voice` sub-lib · coverage target 85%

- **Task 3.2.1** *(red)* Port the gilabot piper/whisper loop's pure logic
  (VAD decision, segment concatenation) into unit-tested functions.
- **Task 3.2.2** *(green)* Expose `Gilavox::say(text)` and
  `Gilavox::listen(timeout) -> Result<Utterance>`; subprocess launch behind a
  trait so tests use a `RecordingCommander` mock.
- **Task 3.2.3** *(red→green)* Engine selection (piper > espeak > SAPI) tested
  via a fake `which`/PATH probe.
- **Acceptance** — voice is a library the station can depend on without pulling
  gilabot. ≥85% covered.

### Story 3.3 — Voice loop in the station
⬜ · crate: `monitor-station` · coverage target 80%

- **Task 3.3.1** *(red)* Test the loop: mic → STT → `Intent` → driver turn →
  TTS of the reply, using mocks for STT/TTS/driver.
- **Task 3.3.2** *(green)* Wire `Gilavox` into the station as a fifth sink +
  intent source; barge-in (speech while speaking) cancels current TTS.
- **Acceptance** — "Monty, status" spoken → Monty speaks a summary; barge-in
  works.

---

## Epic 4 — Farm data plane
*ROADMAP §5. Swarm items stay ⏸ pending the rearchitecture; mesh + breathing
pool proceed.*

### Story 4.1 — Authenticated mesh transport
⬜ · crate: `monitor-collect` (+ `agent-mesh`) · coverage target 80%

- **Task 4.1.1** *(red)* Test envelope sign/verify round-trip with a test
  keypair; reject tampered payloads.
- **Task 4.1.2** *(green)* Adopt `agent-mesh` signed QUIC envelopes; add a
  `MeshCollector` that subscribes to metric subjects.
- **Task 4.1.3** *(red→green)* mDNS discovery mock: discovered peer →
  collector added → first sample received (wiremock-style).
- **Acceptance** — a second caster instance's metrics appear in the first's
  dashboard over an authenticated channel. ≥80% covered.

### Story 4.2 — "Breathing pool" farm health
⬜ · crate: `monitor-collect` · coverage target 85%

- **Task 4.2.1** *(red)* Test a `PoolSource` that yields one probe per member
  on a cadence; failing probe → degraded member state.
- **Task 4.2.2** *(green)* Implement the prober; surface pool health as a
  metric series + an alert rule template.
- **Acceptance** — a farm member going quiet raises an alert within 2 cycles.

### Story 4.3 — Swarm tabs (⏸ deferred)
⏸ on hold — pending the swarm rearchitecture. Do not start. When unblocked,
port `gila-monitor-tui`'s `ui/board.rs`, `ui/sessions.rs`, `ui/budget.rs`,
`ui/decisions.rs`, `ui/swarm.rs` as `monitor-gui`/`monitor-tui` skins over the
new swarm model — each as its own story with the same TDD/coverage bar.

---

## Epic 5 — Hardening & infrastructure
*ROADMAP §6. Goal: CI green on Linux, Windows failures fixed, the coverage
gate live, a systemd unit shipped.*

### Story 5.1 — GitHub Actions CI
⬜ · crate: workspace · coverage target n/a (gate)

- **Task 5.1.1** Add `.github/workflows/ci.yml`: `cargo fmt --check` +
  `cargo clippy -D warnings` + `cargo test` on Linux (rust-toolchain pinned to
  1.80).
- **Task 5.1.2** Add a Windows leg initially `continue-on-error: true` until
  Story 5.2 lands.
- **Acceptance** — PRs get a green Linux check.

### Story 5.2 — Fix Windows-only clippy/test failures
⬜ · crate: `monitor-alert`, `monitor-core` · coverage target 85%

- **Task 5.2.1** *(red)* Reproduce `monitor-alert/voice.rs` and
  `monitor-core/config.rs` Windows failures under a `#[cfg(windows)]` test.
- **Task 5.2.2** *(green)* Fix the SAPI/PowerShell path + config path
  resolution; flip the Windows CI leg to required.
- **Acceptance** — Windows CI green; no `#[cfg(windows)]` test is `#[ignore]`'d
  without a tracking task.

### Story 5.3 — Coverage gate at 80%
⬜ · crate: workspace · coverage target = the gate itself

- **Task 5.3.1** Add a `just cov-ci` target using `llvm-cov` with
  `--summary-only --fail-under-functions 80` (line + function).
- **Task 5.3.2** Wire it into CI as a separate job; upload the lcov artifact.
- **Acceptance** — a PR that drops workspace coverage below 80% fails CI.

### Story 5.4 — `just install` + systemd unit
⬜ · crate: `monitor-cli` · coverage target 80%

- **Task 5.4.1** *(red)* Test `InstallPlan` (binary → `~/bin`, config search
  order) as a pure unit.
- **Task 5.4.2** *(green)* Ship `deploy/monitor-agent.service` template +
  `just install` recipe.
- **Acceptance** — `just install && systemctl --user start monitor-agent`
  runs the daemon; TUI/GUI attach.

---

## Epic 6 — Standalone-repo split (Model A → B)
*ROADMAP §6. Goal: decide and execute the caster-as-Foundation-repo split,
git-depending `newt-*` the way `gilamonster-agent` does.*

### Story 6.1 — Decision record
⬜ · crate: docs · coverage target n/a

- **Task 6.1.1** Write `docs/decisions/0002-caster-repo-split.md` comparing
  Model A (grow in place) vs Model B (standalone repo git-dep'ing `newt-*`),
  referencing the `gilamonster-agent` pattern.
- **Acceptance** — decision merged; the chosen path updates ROADMAP §6.

### Story 6.2 — Execute the split (if Model B chosen)
⬜ · crate: workspace · coverage target unchanged

- **Task 6.2.1** Extract `newt-*` deps as git deps; verify `just check` still
  green.
- **Task 6.2.2** *(red→green)* Add a workspace-level import test that asserts
  the `Driver`/capability surface used by `monitor-station` still resolves.
- **Acceptance** — caster builds standalone against pinned `newt-*` commits.

---

## Cross-cutting acceptance (every story must satisfy)

1. **Tests first** — every code task has a red test before the green commit.
2. **Coverage floor** — crate coverage ≥ the story target; workspace ≥80%.
3. **No new `#[ignore]` without a task** — ignored tests carry a `// TODO(#N)`
   linking to a filed task.
4. **Lint clean** — `cargo clippy --workspace -- -D warnings` passes.
5. **Skins stay symmetric** — a `DataEvent` rendered by the TUI is also
   rendered by the GUI (or explicitly marked GUI-only/TUI-only with a reason).
6. **Capability discipline** — any new privileged action goes through the
   read-only-capability path from Phase 0/1; denials are tested.

---

## Suggested sequencing (Now / Next / Later)

- **Now** — finish the in-review work: retarget+merge GUI launch (P4a.2 / #9),
  voice fixes (gilabot #1915, #10). These unblock Stories 1.1 and 3.1.
- **Next** — Epic 1 (1.1 → 1.5), Story 3.1 (talk timeout), Epic 2 (2.1 → 2.3).
  These deliver the "Monty inhabits both surfaces + thinks" promise.
- **Later** — Epic 3.2/3.3 (voice loop), Epic 4.1/4.2 (mesh + pool), Epic 5
  (infra, parallelizable any time), Epic 6 (split, decide before mesh work).