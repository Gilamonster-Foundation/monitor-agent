# monitor-agent / caster — Roadmap

> The product is **monitor-agent**; its flagship instance is **caster** — the
> lab monitoring station on the Windows GPU box. caster is being grown into an
> **inherit-and-extend descendant of `newt-agent`**: one "Monty" presence that
> *inhabits both* a ratatui TUI and an egui GUI over a single shared core, with
> voice, watching the whole farm.
>
> Design detail lives in:
> [`docs/design/caster-station.md`](design/caster-station.md) ·
> [`docs/design/inhabit-both-surfaces.md`](design/inhabit-both-surfaces.md) ·
> [`docs/decisions/0001-station-toolchain.md`](decisions/0001-station-toolchain.md)

**Status:** ✅ done / merged · 🔄 in review (open PR) · ⬜ planned · ⏸ on hold

---

## 1. Foundation — the standalone monitor ✅

The original monitor-agent: a general-purpose ops monitor, complete and shipping.

- ✅ `monitor-core` — `MetricSet` / `Collector` / `AlertEngine` / `AlertDispatcher` / `Config`
- ✅ Collectors — `LocalCollector` (sysinfo + nvidia-smi), `PrometheusCollector`, `NatsCollector`, `SshCollector`
- ✅ Alert engine — rule eval, Firing/Resolved lifecycle, cooldown, hysteresis
- ✅ Dispatchers — terminal bell, voice (SAPI/say/espeak/piper), webhook, NATS publish
- ✅ `monitor-tui` — ratatui dashboard (Alerts / Metrics / History / Rules + Monty splash)
- ✅ CLI — `daemon` / `tui` / `status` / `alerts` / `doctor` / `config`

> The original "Phase 11 — IPC socket daemon/TUI split" is **superseded** by the
> shared-presence + `OutputSink` fan-out architecture below (§3).

## 2. caster station — descendant of `newt-agent`

| Step | What | Status |
|---|---|---|
| Phase 0/1 | `monitor-station` crate; `newt`/`mesh`/`shell` feature gates (off by default); **read-only object-capability identity** (mint via `agent-mesh-protocol`, attenuate→ReadOnly) | ✅ [#2](https://github.com/Gilamonster-Foundation/monitor-agent/pull/2) |

## 3. "Monty inhabits both surfaces" — one core, two skins

The seam → fan-out → shared-handle → skins progression. See
[`inhabit-both-surfaces.md`](design/inhabit-both-surfaces.md).

| Step | What | Status |
|---|---|---|
| P1 | Presence seam — `monitor-presence` (`PresenceModel` / `Intent` / `DataEvent`); TUI becomes a skin | ✅ [#4](https://github.com/Gilamonster-Foundation/monitor-agent/pull/4) |
| P2 | Session **output fan-out** (`OutputSink` / `SessionState`); TUI attaches as an `Observer` sink | ✅ [#7](https://github.com/Gilamonster-Foundation/monitor-agent/pull/7) |
| P3 | `SharedPresence` concurrency wrapper (`Arc<Mutex>` + snapshot-on-read); TUI renders snapshots | ✅ [#6](https://github.com/Gilamonster-Foundation/monitor-agent/pull/6) |
| P4a | **egui skin** — `monitor-gui` renders the shared `PresenceModel` (status / tabs / metrics) | ✅ [#8](https://github.com/Gilamonster-Foundation/monitor-agent/pull/8) |
| P4a.2 | `gui` subcommand launches the egui window (main-thread eframe + tokio collectors, feature-gated) | 🔄 [#9](https://github.com/Gilamonster-Foundation/monitor-agent/pull/9) — *retarget base → `main`, then merge* |
| P4b | `egui_plot` graphs off the history rings · animated Monty · embedded **brush** terminal · voice waveform | ⬜ |
| P5 | Late-join **replay** across skins (`replay_from` on attach) | ⬜ |
| P6 | Wire the **Monty mind** as the session's sole read-only `Driver` (chat → turn → fan-out → both skins) | ⬜ |

## 4. Voice — TTS + STT

| Item | Status |
|---|---|
| **TTS + STT verified working on caster** — piper (`gila voice say`) + faster-whisper (`gila voice listen`), end-to-end | ✅ 2026-06-19 |
| Windows console **UTF-8 fix** — unblocks all gila output on cp1252 consoles (the `✓`-glyph crash) | 🔄 [gilabot #1915](https://github.com/hartsock/gilabot/pull/1915) |
| **PowerShell-injection hardening** in the Windows SAPI speech path | 🔄 [#10](https://github.com/Gilamonster-Foundation/monitor-agent/pull/10) |
| `talk` / `listen --vad` **timeout** — `record_until_silence` currently has no timeout and hangs if VAD never fires | ⬜ |
| Voice → station integration — extract the gilabot voice loop into a reusable `gilavox` lib; wire into `monitor-station` (mic→STT→mind→TTS) | ⬜ |

> Note: piper works on Windows, so the original design's native-SAPI-TTS plan is
> effectively moot for synthesis (SAPI remains the lightweight one-way *alert*
> dispatcher in `monitor-alert`).

## 5. Farm data plane

| Item | Status |
|---|---|
| Swarm status — generic `monitor-swarm` model + `SwarmJsonCollector`; Swarm / Board tabs | ⏸ on hold — **swarm architecture being reworked** |
| Authenticated **mesh** transport (`agent-mesh`: mDNS discovery + signed QUIC envelopes) | ⬜ |
| "Breathing pool" farm health (`newt-scheduler` `PoolSource` + prober) | ⬜ |

## 6. Repo & infrastructure

| Item | Status |
|---|---|
| **CI** — GitHub Actions (`fmt` + `clippy -D warnings` + `test`, Linux); none exists today | ⬜ (task filed) |
| Pre-existing **Windows-only** clippy/test failures (`monitor-alert/voice.rs`, `monitor-core/config.rs`) | ⬜ (task filed) |
| **Model A → B** — split caster into a standalone Foundation repo git-dep'ing `newt-*` (the gilamonster-agent pattern) | ⬜ |
| Coverage gate · `just install` · systemd service template (original Phases 12 / 14) | ⬜ |

---

## Now / Next / Later

- **Now** — retarget + merge [#9](https://github.com/Gilamonster-Foundation/monitor-agent/pull/9) (GUI launchable); merge the voice fixes ([gilabot #1915](https://github.com/hartsock/gilabot/pull/1915), [#10](https://github.com/Gilamonster-Foundation/monitor-agent/pull/10)); verify the egui window on caster.
- **Next** — P4b (real graphs + embedded brush terminal + voice waveform), P6 (the Monty mind as Driver), the `talk` timeout.
- **Later** — swarm (after the rearchitecture) + mesh transport; the standalone-repo split; CI + coverage + systemd.

## Open decisions

- **Swarm data model** — pending your rearchitecture; everything in §5 is provisional until then.
- **Model A vs B** — grow `monitor-agent` (current) vs split a standalone `caster` repo. Defer to before the mind/mesh work.
- **GUI graphics backend** — eframe defaults to glow/OpenGL; wgpu/DX12 may stream better over Moonlight. Revisit once the window is visually verified.
- **Voice-into-station shape** — drive piper/whisper via a `gilavox` sidecar (IPC) vs a native Rust path.
