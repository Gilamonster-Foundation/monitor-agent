# monitor-agent — task runner
#
# PIPELINE PARITY: this justfile is the SINGLE SOURCE OF TRUTH for the gate.
# .github/workflows/ci.yml invokes these same recipes as separate steps
# (`just fmt-check`, `just lint`, `just test`, `just test-features`,
# `just cov-ci`), and .githooks/pre-push runs `just check` + `just cov-ci`.
#
# The command strings and the 78% coverage floor live HERE and nowhere else.
# CI does not re-type them, so editing a recipe updates both gates at once —
# parity holds by construction rather than by comment.
#
# Quick reference:
#   just              — list available recipes
#   just check        — full local gate (fmt + clippy + test + features)
#   just cov          — HTML coverage report (local review)
#   just cov-ci       — coverage with 78% floor, lcov output (CI mode)
#   just install      — build release binary to ~/bin
#   just install-hooks — wire .githooks/ as the repo's hooks path

default:
    @just --list

# --- Build ---

build:
    cargo build --workspace

release:
    cargo build --workspace --release

install dest=`echo $HOME/bin`:
    cargo build --release --bin monitor-agent
    mkdir -p {{dest}}
    cp target/release/monitor-agent {{dest}}/monitor-agent
    @echo "Installed: {{dest}}/monitor-agent"
    @case ":$PATH:" in *":{{dest}}:"*) ;; *) echo "Note: {{dest}} is not in PATH — add:  export PATH={{dest}}:\$PATH" ;; esac

clean:
    cargo clean

# --- Test ---

test:
    cargo test --workspace --locked

# The object-capability identity layer is behind `--features newt`, so
# `cargo test --workspace` never compiles it: monitor-station/tests/identity.rs
# is `#![cfg(feature = "newt")]` and the default gate produces a test binary
# that is not even listed in the output. Four tests covering read-only key
# minting and caveat attenuation had therefore never run under ANY gate —
# a vacuous green on security code. This recipe is what makes them real.
#
# Run the feature-gated object-capability identity tests.
test-features:
    cargo test -p monitor-station --features newt --locked

# --- Lint & format ---

fmt:
    cargo fmt --all

# Check formatting without rewriting anything.
fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Full local gate. The recipe dependencies ARE the gate; ci.yml runs these
# same four recipes as separate steps so one failure cannot mask another.
#
# Full local gate: fmt + clippy + test + feature-gated tests.
check: fmt-check lint test test-features

# --- Coverage ---

# Generate an HTML coverage report for local review.
cov:
    cargo llvm-cov --workspace --html
    @echo "Report: target/llvm-cov/html/index.html"

# CI-mode coverage: enforce the line coverage floor, emit lcov.
# PIPELINE PARITY: ci.yml's `coverage` job runs THIS recipe verbatim. The floor
# lives here only; CI never restates the number.
#
# On macOS (Homebrew Rust), llvm-tools-preview is unavailable via rustup.
# Set LLVM_COV and LLVM_PROFDATA to the Homebrew LLVM binaries:
#   export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
#   export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
#
# The floor RATCHETS UP — never down. Current baseline: 78%.
#
# 76 -> 78, raised by this branch. See this recipe's history for why 80 was
# never actually enforced; main measures 76.58%. `monitor-journal` lands at
# 91.79% line coverage and carries the workspace to 78.06%, so 78 is a floor
# that holds the moment it is set.
#
# That is the rule this encodes: a new crate should RAISE this number, not
# coast underneath it. Raise it further as coverage improves; the largest
# single gap is monitor-gui/src/lib.rs at 58%.
#
# Coverage with the enforced line floor, lcov output (CI mode).
cov-ci:
    cargo llvm-cov --workspace --locked --lcov --output-path lcov.info --fail-under-lines 78

# --- Hook installation ---

install-hooks:
    git config core.hooksPath .githooks
    @echo "core.hooksPath -> .githooks"
