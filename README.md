<p align="center">
  <img src="docs/logos/monty-256.png" alt="Monty the Monitor Lizard" width="256" />
</p>

<h1 align="center">monitor-agent</h1>

<p align="center">
  <strong>Monty watches your systems so you don't have to.</strong><br/>
  A local-first daemon that alerts you before problems become crises.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.80+-orange?style=flat-square" alt="Rust 1.80+" />
  <img src="https://img.shields.io/badge/coverage-80%25-brightgreen?style=flat-square" alt="Coverage 80%+" />
  <img src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square" alt="Apache-2.0" />
</p>

---

## What is Monty?

**Monty** is the Monitor Lizard — a systems monitoring daemon and ratatui TUI
that watches your machines, evaluates configurable alert rules, and notifies you
when something is about to go wrong.

Unlike hosted monitoring SaaS, Monty runs entirely on your hardware. No
accounts, no cloud, no egress. It reads your systems, applies your rules, and
speaks — literally, if you want — when thresholds are crossed.

```
┌─────────────────────────────────────────────────────────┐
│  ● daemon:ok  ⚠ 2 alert(s)  12:34:07                  │
├─────────────────────────────────────────────────────────┤
│  [Alerts ⚠2]  [Metrics]  [History]  [Rules]           │
├─────────────────────────────────────────────────────────┤
│  ACTIVE ALERTS                                          │
│  ● CRIT  nuc     disk./.used_pct    96%  firing 5m     │
│  ⚠ WARN  gnuc    cpu.percent        92%  firing 2m     │
├─────────────────────────────────────────────────────────┤
│  gnuc  CPU ████████░░ 82%  MEM █████░ 64%  DSK ██░ 42%│
│  nuc   CPU ██░░░░░░░  21%  MEM ███░░░ 48%  DSK ████ 96%│
├─────────────────────────────────────────────────────────┤
│  daemon:ok  collectors:4  q:quit  1-4:tabs  ↑↓:scroll  │
└─────────────────────────────────────────────────────────┘
```

---

## Why do you need Monty?

Because problems don't announce themselves. Disks fill silently. CPU spikes
happen at 3 AM. Memory leaks grow for days before something crashes. By the
time you notice, the damage is done — a job failed, a service dropped requests,
a deadline was missed.

Monty breaks that pattern. It watches continuously, evaluates rules you define,
and interrupts you *before* the threshold becomes a disaster:

- **Disk at 90%?** Voice alert. Terminal bell. Before it hits 100% and
  corrupts data mid-write.
- **CPU pinned at 95% for 5 minutes?** Notification. Before the build
  queue backs up and engineers start filing tickets.
- **Remote machine stopped heartbeating?** Alert. Before your users notice
  the service is down.

The difference between Monty and a dashboard is *who initiates contact*.
A dashboard waits for you to look. Monty comes to find you.

### What makes Monty different

| | Monty | Grafana Alerting | PagerDuty | cron + scripts |
|---|---|---|---|---|
| Local-first, no cloud | ✓ | ✗ | ✗ | ✓ |
| Zero config to start | ✓ | ✗ | ✗ | ✗ |
| Voice alerts | ✓ | ✗ | ✗ | maybe |
| Prometheus integration | ✓ | ✓ | ✓ | ✗ |
| NATS / swarm-aware | ✓ | ✗ | ✗ | ✗ |
| SSH-polled remotes | ✓ | ✗ | ✗ | ✓ |
| Ratatui live dashboard | ✓ | browser | browser | ✗ |
| Single static binary | ✓ | ✗ | ✗ | ✓ |

---

## Quick start

```bash
# Build and install to ~/bin
just install

# Run the live dashboard with default config (monitors localhost)
monitor-agent tui

# Check collectors and voice engine
monitor-agent doctor

# Print active alerts as JSON
monitor-agent alerts
```

Zero config needed to get started. By default Monty monitors the local machine
with three built-in rules: CPU > 85%, disk > 90%, memory > 90%.

---

## Data sources

Monty pulls metrics from wherever your systems live:

| Collector | How it works | Refresh |
|---|---|---|
| **Local** | `sysinfo` crate + `nvidia-smi` subprocess | 2s |
| **Prometheus** | HTTP `/api/v1/query` against any Prometheus endpoint | 30s |
| **SSH** | Runs one command over SSH, parses `/proc` + `df` | 30s |
| **NATS** | Subscribes to subjects; parses JSON metric payloads | real-time |

Mix and match. A typical homelab config monitors the local machine via
`sysinfo`, remote machines via Prometheus node_exporter, and the
gilabot agent swarm via NATS heartbeats — all at once.

---

## Alert rules

Rules live in `monitor-agent.toml`:

```toml
[[rules]]
name       = "high-cpu"
target     = "*"           # "*" matches all targets
metric     = "cpu.percent"
condition  = { gt = 85.0 }
severity   = "warn"
cooldown_secs = 300
message    = "{target}: CPU at {value:.0}%"

[[rules]]
name       = "critical-disk"
target     = "*"
metric     = "disk.used_pct"
condition  = { gt = 90.0 }
severity   = "critical"
cooldown_secs = 3600
```

**Conditions:** `{ gt = N }`, `{ lt = N }`, `{ eq = N }`
**Severities:** `info`, `warn`, `critical`
**Cooldown:** minimum seconds between repeated alerts for the same rule + target

---

## Notifications

When a rule fires, Monty dispatches to every configured channel:

| Channel | What happens |
|---|---|
| **Terminal bell** | `\x07` BEL + printed alert line — works in any terminal |
| **Voice** | Auto-detects `say` (macOS), `espeak-ng`, `piper`, or PowerShell TTS |
| **NATS publish** | Publishes a JSON alert event to a configurable subject |
| **Webhook** | HTTP POST to any endpoint (Slack, Discord, custom handler) |

Configure in `monitor-agent.toml`:

```toml
[notify]
terminal_bell = true
voice         = true
voice_engine  = "auto"          # auto-detects best available engine
nats_subject  = "monitor.alerts"
webhook       = ""              # set to a URL to enable
```

---

## Full config example

```toml
[daemon]
socket = ""    # default: $XDG_RUNTIME_DIR/monitor-agent.sock

[[targets]]
name = "local"
kind = "local"

[[targets]]
name = "gnuc"
kind = "prometheus"
endpoint = "http://192.168.0.104:9090"

[[targets]]
name = "nuc"
kind = "ssh"
host = "192.168.0.104"
user = "hartsock"
key  = "~/.ssh/id_ed25519"

[nats]
servers  = ["nats://192.168.0.104:4222"]
subjects = ["swarm.heartbeat", "monitor.>"]

[[rules]]
name      = "high-cpu"
target    = "*"
metric    = "cpu.percent"
condition = { gt = 85.0 }
severity  = "warn"
message   = "{target}: CPU at {value:.0}%"

[[rules]]
name      = "critical-disk"
target    = "*"
metric    = "disk.used_pct"
condition = { gt = 90.0 }
severity  = "critical"

[notify]
terminal_bell = true
voice         = true
voice_engine  = "auto"
nats_subject  = "monitor.alerts"
```

Config is searched in order: `MONITOR_CONFIG` env → `./monitor-agent.toml`
→ `~/.config/monitor-agent/config.toml` → `/etc/monitor-agent/config.toml`
→ built-in defaults.

---

## Build

```bash
# Prerequisites: Rust 1.80+, just
cargo install just

# Full quality gate (fmt + clippy + test + 80% coverage floor)
just check
LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov \
LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata \
  just cov-ci

# Install release binary to ~/bin
just install

# Install pre-push hook (enforces the same gate before every push)
just install-hooks
```

---

## Project structure

```
monitor-agent/
├── monitor-core/     Types, config, alert engine, Collector/Dispatcher traits
├── monitor-collect/  Collectors: local, prometheus, nats, ssh
├── monitor-alert/    Dispatchers: terminal bell, voice, webhook, nats-pub
├── monitor-tui/      ratatui dashboard (Alerts, Metrics, History, Rules tabs)
└── monitor-cli/      Binary entry point — daemon, tui, status, alerts, doctor
```

---

## Roadmap

Next: Phase 11 — IPC socket daemon/TUI split (daemon runs as a background
service; TUI attaches on demand). See [`docs/ROADMAP.md`](docs/ROADMAP.md).

---

<p align="center">
  Built with Rust · Powered by <a href="https://github.com/ratatui-org/ratatui">ratatui</a> ·
  Mascot by the Gilamonster Foundation
</p>
