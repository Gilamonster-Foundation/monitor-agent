# monitor-agent Roadmap

Each phase is a PR-sized unit of work. Current status follows each phase title.

## Phase 0 — Workspace scaffold ✓
Cargo workspace, justfile, pre-push hooks, git init.

## Phase 1 — monitor-core types ✓
MetricSet, MetricPath, MetricValue, Collector trait, AlertRule, Alert, AlertState,
Severity, Condition, AlertEngine, AlertDispatcher trait, Config.

## Phase 2 — LocalCollector ✓
sysinfo-based CPU/memory/disk/network collector. nvidia-smi subprocess for GPU.

## Phase 3 — Alert engine ✓
Rule evaluation, lifecycle (Firing/Resolved), cooldown, dedup, hysteresis.

## Phase 4 — Alert dispatchers ✓
TerminalBellDispatcher, VoiceDispatcher (auto-detect: say/espeak-ng/piper/powershell),
WebhookDispatcher (HTTP POST), NatsPublishDispatcher.

## Phase 5 — Daemon event loop ✓
monitor-cli: daemon subcommand, spawn_collectors, IPC-ready structure.

## Phase 6 — PrometheusCollector ✓
HTTP /api/v1/query polling for node_exporter metrics (CPU, mem, disk, GPU via DCGM).

## Phase 7 — NatsCollector ✓
Real-time NATS subscription, JSON payload → MetricSet mapping.

## Phase 8 — SshCollector ✓
SSH subprocess, parses /proc/loadavg + /proc/meminfo + df -P output.

## Phase 9 — monitor-tui ✓
ratatui dashboard: Alerts/Metrics/History/Rules tabs, status bar, embedded Monty splash.

## Phase 10 — CLI subcommands (next)
`status`, `alerts`, `doctor`, `config` fully wired. JSON output for scripting.

## Phase 11 — IPC socket daemon/TUI split
Daemon writes state to Unix socket. TUI attaches as a client. Allows the daemon
to run as a systemd service while TUI attaches on demand.

## Phase 12 — Coverage gate + release binary
`cargo llvm-cov` gate at 75%, `just install` to ~/bin, Homebrew formula template.

## Phase 13 — Grafana alert webhook receiver (optional)
Receive Grafana/Alertmanager webhook payloads and surface them as Alert objects —
bridges existing Prometheus alerting into the monitor-agent ecosystem.

## Phase 14 — systemd service template
`/etc/systemd/system/monitor-agent.service` for boot-persistent daemon on gnuc/nuc.
