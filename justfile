# Optional convenience task runner (https://just.systems).
# Every command below works standalone -- just is never required.

# Our runtime, ahead of whatever the ambient PATH carries. WSL appends the
# Windows PATH by default, which put a broken `just` npm shim in front of the
# real one once already. Recipes resolve tools from our own install first, so
# an inherited PATH cannot decide which binary a gate runs.
#
# Derived, never hardcoded: `home_directory()` resolves on Windows, macOS and
# Linux alike, and the separator follows the OS rather than assuming Unix.
path_sep := if os_family() == "windows" { ";" } else { ":" }
export PATH := home_directory() / ".cargo" / "bin" + path_sep + home_directory() / ".local" / "bin" + path_sep + env('PATH')

# The service unit that runs the router, for the recipes that act on it.
#
# Named rather than spelled out at each use, and a name rather than a pid:
# the unit is what starts the router at login and what restarts it, so acting
# on anything else leaves systemd's idea of the service and the process that
# is actually running disagreeing with each other.
#
# `reload`, `serving` and `deploy` are the recipes in this file that are not
# cross-platform: they act on a running service, which on this machine is a
# systemd user unit. Every gate above them runs anywhere.
unit := "model-router"

# First in the file, deliberately: `just` with no arguments runs the first
# recipe, and before this that was `install`, which installs a toolchain. A
# reader typing `just` to find out what is here should not get one for asking.
#
# `--list` reads the last comment line above a recipe as its description, so
# in this file the reasoning goes above and the summary goes immediately
# before the recipe. That is why these blocks read upside down.

# Every recipe, with what it does. Bare `just` shows this.
help:
    @just --list --unsorted

# Install the toolchain this repository needs. Idempotent.
install:
    rustup toolchain install --profile minimal 1.98.1
    rustup component add clippy rustfmt llvm-tools-preview
    cargo binstall -y prek cargo-deny cargo-machete cargo-llvm-cov similarity-rs

# Wire the local hooks. Both types come from default_install_hook_types.
setup:
    prek install --install-hooks

# The quality commands rust-workflows' CI runs, with its flags: Clippy with the
# scaffolding and `unsafe` denied, strict rustdoc, the 90% coverage floor. CI
# also runs what needs its own runners or the network: the other platforms,
# the release build, the SBOMs, the secret scan and mutation testing.

# Run the quality gates CI runs.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings -D clippy::todo -D clippy::dbg_macro -D unsafe_code
    cargo test --workspace --all-targets --locked
    cargo test --workspace --doc --locked
    RUSTDOCFLAGS='-D warnings -D missing_docs' cargo doc --workspace --no-deps --locked
    cargo llvm-cov --workspace --locked --fail-under-lines 90 --summary-only
    cargo machete
    cargo deny check

# Format in place. `check` only verifies.
fmt:
    cargo fmt --all

# Prove the gates do not depend on the ambient PATH.
doctor:
    @echo "just    $(command -v just)"
    @echo "cargo   $(command -v cargo)"
    @echo "prek    $(command -v prek)"
    @echo "rustc   $(rustc --version)"

# A restart, because that is what re-reading the catalog costs today: the
# router reads the file once at startup and holds it for the life of the
# process. Every loaded model is unloaded and the next request for one starts
# it again -- cheap when nothing is loaded, not cheap when something is, and
# `just serving` says which before you run this.
#
# `systemctl restart` rather than a signal: SIGHUP already ends this process
# rather than reloading it, which is why this is a recipe and not a kill.

# Restart the router so it serves the catalog as it now reads.
reload:
    systemctl --user restart {{unit}}
    @systemctl --user --no-pager --lines=0 status {{unit}} | head -3

# The two facts `reload` costs you: whether the unit is up, and what it is
# holding. A reload with nothing loaded interrupts nothing.
#
# Read from the processes rather than from the router, because this has to
# answer when the router is the thing that is wrong, and because the address
# to ask on lives in the unit file rather than here.
#
# One script rather than a line per shell: the empty case needs a branch, and
# a pipeline that ends in `sed` reports the exit status of `sed`, which
# succeeds on no input -- so `|| echo nothing` never fires and the caller is
# told nothing at all instead of "nothing".

# Installing from a working tree nobody committed is how a router came to run
# code that no commit held, so this refuses one. The binary is built with its
# commit stamped in -- `model-router --version` and `/props` report it -- and
# the one it replaces is kept as `model-router.prev` for a rollback. The
# restart waits until no connection is open to the router, so nothing is cut
# off mid-answer; a loaded model loads again on its next request.

# Build HEAD, install it, and restart the router once nothing is in flight.
deploy:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! git diff --quiet HEAD --; then
        echo "refusing: tracked files differ from HEAD; commit first" >&2
        exit 1
    fi
    commit=$(git rev-parse --short=12 HEAD)
    MODEL_ROUTER_COMMIT="$commit" cargo build --release --locked --bin model-router
    for _ in $(seq 600); do
        open=$(ss -Htnp state established | grep -c '"model-router"' || true)
        [ "$open" -eq 0 ] && break
        sleep 1
    done
    if [ "$open" -ne 0 ]; then
        echo "refusing: $open connections still open after ten minutes" >&2
        exit 1
    fi
    bin="$HOME/.local/bin"
    cp -p "$bin/model-router" "$bin/model-router.prev"
    install -m 0755 target/release/model-router "$bin/model-router.new"
    mv -f "$bin/model-router.new" "$bin/model-router"
    systemctl --user restart {{unit}}
    sleep 1
    systemctl --user is-active {{unit}}
    "$bin/model-router" --version

# What the router is holding, before deciding whether to interrupt it.
serving:
    #!/usr/bin/env sh
    systemctl --user is-active {{unit}} | sed 's/^/unit: /'
    held=$(pgrep -af llama-server | grep -o -- '--alias [A-Za-z0-9._-]*' | sed 's/--alias //')
    if [ -n "$held" ]; then
        echo "$held" | sed 's/^/loaded: /'
    else
        echo "loaded: nothing"
    fi
