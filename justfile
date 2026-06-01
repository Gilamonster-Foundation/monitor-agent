# monitor-agent — task runner
#
# PIPELINE PARITY: This justfile is the local mirror of the CI pipeline
# defined at .github/workflows/ci.yml. The pre-push hook at
# .githooks/pre-push calls `just check` — keep both in lock-step.
#
# Quick reference:
#   just              — list available recipes
#   just check        — full local gate (fmt + clippy + test)
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

# --- Hook installation ---

install-hooks:
    git config core.hooksPath .githooks
    @echo "core.hooksPath -> .githooks"
