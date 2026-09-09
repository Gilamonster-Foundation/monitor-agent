# AGENTS.md — monitor-agent

This file guides coding agents working in this repository. `CLAUDE.md` is a
symlink to it, matching the convention in `herdr/` and the workspace root.

**Read §"Design doctrine" before proposing anything.** It is not preamble; it
is the standard a change is judged against here.

## Project Purpose

monitor-agent is a standalone Rust daemon + ratatui TUI for monitoring systems
and alerting a human operator of coming problems — before they become crises.

It is NOT a replacement for gila-monitor-tui (monty-tui), which owns the
gilabot swarm/agent ecosystem view. monitor-agent is a general-purpose,
ecosystem-independent operator alert tool.

## Workspace Structure

```
monitor-agent/
├── monitor-core/        Types, traits, config, alert engine
├── monitor-collect/     Collectors: local, prometheus, nats, ssh
├── monitor-alert/       Dispatchers: terminal bell, voice, webhook, nats-pub
├── monitor-journal/     Append-only, hash-linked journal of metric snapshots
├── monitor-presence/    Frontend-agnostic presence model (state + intents)
├── monitor-station/     Caster station (Phase 0/1 scaffold)
├── monitor-tui/         ratatui dashboard
├── monitor-gui/         GUI skin over the same presence model
├── monitor-voice/       Speech dispatch backends
└── monitor-cli/         Binary entry point (monitor-agent)
```

Each name is a sentence about one responsibility — see the doctrine's closing
section. Keep it that way.

## Build Commands

```bash
just check              # fmt + clippy + test (full local gate)
just test               # cargo test --workspace
just install            # release binary to ~/bin
just install-hooks      # wire .githooks/pre-push
cargo run --bin monitor-agent -- tui    # launch TUI
cargo run --bin monitor-agent -- doctor # check collectors
```

## Key Design Rules

- **Zero-warnings policy**: `cargo clippy -- -D warnings` must be clean before any merge.
- **Faux PR workflow**: branch → TDD → all tests pass → merge to main.
- **No push without hooks**: `just install-hooks` after any fresh clone.
- **Config search path**: `MONITOR_CONFIG` env → `./monitor-agent.toml`
  → `~/.config/monitor-agent/config.toml` → `/etc/monitor-agent/config.toml`
- **Collector trait**: add new data sources by implementing `monitor_core::metrics::Collector`.
- **Dispatcher trait**: add new notification channels by implementing
  `monitor_core::alert::AlertDispatcher`.

## Roadmap

See `docs/ROADMAP.md`. Next phase: IPC socket daemon/TUI split (Phase 11).

## Logos

Source image: `docs/logos/Monty_Lizard_Large.png`.
Regenerate ANSI/ASCII art via `chafa` — see `docs/logos/README.md`.
The TUI splash selects width automatically via `monitor_tui::splash_for_width()`.

## Dependencies of Note

| Crate | Why |
|---|---|
| `sysinfo 0.33` | Local CPU/mem/disk/process metrics |
| `ratatui 0.29` + `crossterm 0.28` | TUI framework (match monty-tui versions) |
| `async-nats 0.37` | NATS subscriber + publisher |
| `reqwest 0.12` (rustls-native-roots) | Prometheus HTTP, webhook — respects OS CA store |

---

# Design doctrine

Two sources, and they are not decoration. Every design question in this repo is
answered by asking them in order.

- **[ponytail](https://github.com/DietrichGebert/ponytail)** — *how much code
  should exist.* "The best code is the code you never wrote."
- **[The UNIX philosophy](https://en.wikipedia.org/wiki/Unix_philosophy)** —
  *what shape it takes.* "Make each program do one thing well. Expect the
  output of every program to become the input to another."

They agree far more than they differ, and where they differ it is worth knowing
why (see "The one real tension").

## The ladder — run it before writing code, not instead of reading it

Stop at the first rung that holds:

1. **Does this need to exist at all?** Speculative need → skip it, say so in one
   line.
2. **Already in this repo?** A trait, type, collector, or dispatcher that exists
   → reuse it. Re-implementing what lives one crate over is the most common
   waste.
3. **Stdlib?** Use it.
4. **Native platform feature?** A systemd timer over a scheduler thread; the
   kernel's own accounting over a sampler.
5. **A dependency already in `Cargo.toml`?** Use it. Never add one for what a
   few lines can do.
6. **One line?** One line.
7. **Only then**, the minimum that works.

**The ladder shortens the solution, never the reading.** Trace the real flow
first — every crate the change touches — then climb. The smallest diff in the
wrong place is not laziness, it is a second bug.

Worked example, from this repo's own design work: *"the monitor should clean up
full disks."* Rung 2 stops it. `~/bin/cleanup-stale-targets.sh` and
`weekly-disk-cleanup.sh` already encode the "is this safe to delete" judgement
and have been exercised. The correct change is **invoke the existing reaper**,
not grow a second `rm` implementation. One implementation of a destructive
decision, never two.

## The UNIX rules that bind hardest *here*

All of them apply. Four decide most arguments in a monitoring daemon.

**Rule of Silence — when there is nothing surprising to say, say nothing.**
This is the single most important rule for this project. A monitor that chatters
trains its operator to ignore it, and an ignored monitor is worse than none
because it also consumes the belief that something is watching. Every alert must
earn its interruption. Default to quiet; make noise the exception you can
justify.

**Rule of Composition — output is another program's input.** Alerts and metric
snapshots are structured events (NATS/JSON) that another tool can consume, not
prose formatted for a human eye. The TUI is *a* consumer of that stream, never
the only way to get at the data. If a fact is reachable only by looking at a
dashboard, it is not composable and does not count as reported.

**Rule of Separation — mechanism apart from policy.** Collectors gather;
dispatchers deliver; **config decides what is worth alerting on**. A threshold
compiled into a collector is a bug. This is already the shape (`Collector` and
`AlertDispatcher` traits + TOML); keep it.

**Rule of Repair — repair what you can, but when you must fail, fail noisily and
as soon as possible.** A collector that cannot reach its source must say so
loudly. Which leads directly to the next section.

## Where laziness is forbidden

ponytail carves these out and they are absolute. In this repo they are:

**Anything that deletes.** Remediation is not a place to be clever or brief.
Only ever act on what is provably reconstructible (a build cache rebuilds; a
worktree with uncommitted changes does not). Destructive acts — evicting a
model, removing dirty state, rebooting a host — are **never autonomous**; they
are proposed and wait.

**Detecting absence.** A rule of the form *"alert when disk > 90%"* never fires
if the exporter is dead. Our own incident is the proof: the kernel OOM-killed
`promtail`, and the signature was `nvidia-smi` returning `N/A` and a scrape gap,
not a threshold crossing. **Staleness of a series is a first-class alert
condition, not a nice-to-have.** A check that reports nothing must never be
indistinguishable from a system that is fine.

**Alert delivery.** A dispatcher that silently drops an alert is the worst
possible failure — it manufactures the appearance of health. Delivery failure is
itself an alert-worthy event.

Input validation at trust boundaries, error handling that prevents data loss,
and security posture are never simplified away.

## Every non-trivial change leaves one runnable check

A branch, a parser, a threshold comparison, a dispatch path: leave the smallest
thing that fails if the logic breaks. No frameworks, no fixtures, no per-function
suites unless asked. Trivial one-liners need none — YAGNI applies to tests too.

**Monitoring-specific corollary: a rule that cannot fire is worse than no rule.**
When adding an alert rule, prove it fires — feed it the condition and watch it
trigger. An untested rule is a promise of coverage that does not exist, and it
will be believed.

## `ponytail:` markers

A deliberate simplification with a known ceiling gets a comment naming the
ceiling and the upgrade path:

```rust
// ponytail: linear scan over rules; index by metric name if rule count grows
```

Harvest them before planning work. A shortcut that is written down is a
decision; one that is not is a trap.

## The one real tension, and how it resolves

UNIX says *"build afresh rather than complicate old programs by adding new
features."* ponytail's rung 2 says *"already in this codebase? reuse it."* These
can pull apart.

**Resolution: reuse a component; never bolt a feature onto something that does a
different job.** Adding a tenth responsibility to `monitor-core` because it is
already there is the failure both philosophies warn about — it violates "one
thing well" *and* produces more code than a new crate would. Reuse means calling
the thing that already does the job. It does not mean widening a thing that
does a different job until it covers yours.

The ten crates here are the evidence this is already understood:
`monitor-collect` gathers, `monitor-alert` dispatches, `monitor-journal` keeps
an append-only hash-linked record. Each name is a sentence about one
responsibility. **A new crate whose purpose cannot be stated in one clause is
the wrong shape.**
