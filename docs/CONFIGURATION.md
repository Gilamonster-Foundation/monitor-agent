# Full configuration example

Combines every section from the README (targets, NATS, alert rules, notify)
into one file. See the README for what each section does.

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
