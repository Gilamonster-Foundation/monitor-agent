# CLAUDE.md — monitor-agent

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
├── monitor-tui/         ratatui dashboard
└── monitor-cli/         Binary entry point (monitor-agent)
```

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
