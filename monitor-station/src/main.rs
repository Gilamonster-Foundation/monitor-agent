//! caster station — lab-wide monitoring (Phase 0/1 scaffold).
//!
//! Thin entry point: parse args (so `--help` / `--version` work), then hand
//! off to [`monitor_station::run`].

use clap::Parser;

/// caster station — lab-wide monitoring (Phase 0/1 scaffold).
#[derive(Parser)]
#[command(name = "monitor-station", version, about)]
struct Cli {}

fn main() -> anyhow::Result<()> {
    Cli::parse();
    monitor_station::run()
}
