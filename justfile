# monitor-agent — task runner
#
# PIPELINE PARITY: This justfile is the local mirror of the CI pipeline
# defined at .github/workflows/ci.yml. The pre-push hook at
# .githooks/pre-push calls `just check` and `just cov-ci` — keep all
# three in lock-step.
#
# Quick reference:
#   just              — list available recipes
#   just check        — full local gate (fmt + clippy + test)
#   just cov          — HTML coverage report (local review)
#   just cov-ci       — coverage with 80% floor, lcov output (CI mode)
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
    cargo test --workspace

# --- Lint & format ---

fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Full local gate — must match .github/workflows/ci.yml.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# --- Coverage ---

# Generate an HTML coverage report for local review.
cov:
    cargo llvm-cov --workspace --html
    @echo "Report: target/llvm-cov/html/index.html"

# CI-mode coverage: enforce 80% line coverage floor, emit lcov.
# PIPELINE PARITY: must match the coverage job in .github/workflows/ci.yml.
#
# On macOS (Homebrew Rust), llvm-tools-preview is unavailable via rustup.
# Set LLVM_COV and LLVM_PROFDATA to the Homebrew LLVM binaries:
#   export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
#   export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
#
# The floor RATCHETS UP — never down. Current baseline: 80%.
cov-ci:
    cargo llvm-cov --workspace --lcov --output-path lcov.info --fail-under-lines 80

# --- Hook installation ---

install-hooks:
    git config core.hooksPath .githooks
    @echo "core.hooksPath -> .githooks"
