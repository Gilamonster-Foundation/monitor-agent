# ADR 0001 — caster station toolchain & inheritance strategy (Phase 0)

Status: accepted · Date: 2026-06-18 · Scope: `monitor-station` and the
in-`monitor-agent` ("hybrid") phase of the [caster station](../design/caster-station.md).

## Context

We are growing `monitor-agent` into the caster station as an inherit-and-extend
descendant of `newt-agent` (Model: *hybrid — start in monitor-agent, split
later*). Phase 0 reconciles the toolchain and establishes the feature-gating
strategy so the default workspace stays clean while optional capabilities are
inherited incrementally.

## Decisions

### 1. Toolchain standard

The station standardizes on `monitor-agent`'s pins (the descendant owns the
build; ancestors are consumed):

| Axis | Standard | Notes |
|---|---|---|
| MSRV | **1.80** | `> newt-agent`'s 1.75, so consuming newt crates is safe on the MSRV axis. |
| Edition | **2021** | `monitor-agent` + `newt-agent`. The monty-tui port must shed its edition-2024 features. |
| `ratatui` | **0.29** | monty-tui (0.28) ports *up* to 0.29; the egui GUI sidesteps it entirely. |
| `crossterm` | **0.28** | matches gilamonster-agent. |
| `async-nats` | **0.37** | matches the inherited `monitor-collect` NATS collector (monty-tui is 0.38 — port down). |

### 2. Feature gates (all OFF by default)

`monitor-station` defines three optional capabilities so `cargo build
--workspace` is green and ecosystem-clean with no siblings compiled:

- **`newt`** — the object-capability identity layer (Phase 1).
- **`mesh`** — authenticated farm transport via newt-mesh / agent-mesh (Phase 9).
- **`shell`** — embedded brush shell / agent-bridle confined tools (Phase 5).

### 3. Inheritance level for Phase 1 (deviation from the design doc, recorded)

The design doc's Phase 1 says "mint via `newt-identity`." Building `newt-identity`
on this machine showed it drags **`newt-core` → `agent-bridle`** (a git-patched,
**unpublished** crate on the `feat/stub-shell` branch) **+ `reqwest`/`hickory`/
`htmd`**. Wiring that into `monitor-agent` would force the same
`[patch.crates-io]` agent-bridle block gilamonster-agent carries, and make even
the *default* `cargo build --workspace` fetch the agent-bridle git repo — directly
against Phase 0's "clean default workspace" goal.

**Decision:** in this hybrid phase, inherit the capability lattice one level
lower — at **`agent-mesh-protocol`** (the published, pure-Rust ed25519+blake3
crate that `newt-identity` itself wraps). The station re-implements the thin
`session_root` / `attenuate` / `read-only caveats` helpers (~30 lines) over
`AgentKey::issue` / `delegate`, inheriting the *same* signed, attenuation-only
machinery without the newt-core/agent-bridle drag.

**On the standalone-repo split (Model B):** adopt `newt-identity` proper over a
**pinned git-dep** plus the mirrored `[patch.crates-io]` agent-bridle block and a
git-ignored `.cargo/config.toml` overlay — exactly the gilamonster-agent recipe.
At that point the station is its own repo and the patch/heavy-dep cost is paid in
isolation, not imposed on `monitor-agent`'s default workspace.

## Consequences

- Default `cargo build --workspace` / `cargo test --workspace` compile no sibling
  code and need no `[patch.crates-io]` block.
- The read-only ocap key (Phase 1) is real and verifiable today (`--features
  newt`), using the authentic capability lattice — only the *wrapper* differs
  from `newt-identity`, not the security model.
- The `monitor-core` crate keeps **zero** newt dependency, preserving its
  standalone, ecosystem-independent identity.
