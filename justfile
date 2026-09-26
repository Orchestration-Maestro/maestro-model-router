# Optional convenience task runner (https://just.systems).
# Every command below works standalone -- just is never required.

# Our runtime, ahead of whatever the ambient PATH carries. WSL appends the
# Windows PATH by default, which put a broken `just` npm shim in front of the
# real one once already. Recipes resolve tools from our own install first, so
# an inherited PATH cannot decide which binary a gate runs.
#
# The pinned toolbelt goes first of all: `rust-gate setup` links every tool
# rust-workflows pins into one per-user directory, so a gate runs the release
# the gate verified rather than whichever copy an earlier install left in
# ~/.cargo/bin.
#
# Derived, never hardcoded: `home_directory()` resolves on Windows, macOS and
# Linux alike, and the separator follows the OS rather than assuming Unix.
path_sep := if os_family() == "windows" { ";" } else { ":" }
unix_cache := env("XDG_CACHE_HOME", home_directory() / ".cache")
cache_home := if os_family() == "windows" { env("LOCALAPPDATA") } else { unix_cache }
tools_bin := cache_home / "maestro" / "tools" / "bin"
cargo_bin := home_directory() / ".cargo" / "bin"
local_bin := home_directory() / ".local" / "bin"
export PATH := tools_bin + path_sep + cargo_bin + path_sep + local_bin + path_sep + env('PATH')

# The service unit that runs the router, for the recipes that act on it.
#
# Named rather than spelled out at each use, and a name rather than a pid:
# the unit is what starts the router at login and what restarts it, so acting
# on anything else leaves systemd's idea of the service and the process that
# is actually running disagreeing with each other.
#
# `reload`, `serving` and `deploy` are the recipes in this file that are not
# cross-platform: they act on a running router, and all but `reload` on its
# service, which on this machine is a systemd user unit. Every gate above them
# runs anywhere its tools are on the PATH; `rust-gate setup` provisions those
# tools on Linux, macOS and Windows.
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

# Rust alone. The gate tools are the pinned toolbelt, which `rust-gate setup`
# installs: a tool fetched here at whatever version was newest is the unpinned
# input the gate's pins exist to remove.

# Install the Rust toolchain this repository needs. Idempotent.
install:
    rustup toolchain install --profile minimal 1.98.1
    rustup component add clippy rustfmt llvm-tools-preview

# One check, the one CI runs: `rust-gate ci --local` runs the checks job of
# rust-workflows' CI step by step, in its order and in a runner's environment,
# over the commits a push sends, so CI confirms what a push already passed
# rather than discovering it. A step only GitHub can run, the other platforms
# and the uploads among them, says it is not applied locally and why. The
# pre-push hook runs the same command.
#
# The gate itself comes from `cargo install`, at rust-workflows' latest release;
# every tool it runs comes from the toolbelt `rust-gate setup` installs.

# Run the checks CI runs.
check:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v rust-gate >/dev/null ||
      { echo "Missing rust-gate; install it, then run rust-gate setup" >&2; exit 1; }
    rust-gate ci --local

# Format in place. `check` only verifies.
fmt:
    cargo fmt --all -- --config style_edition=2024

# CI mutates only a pull request's diff; this mutates the whole crate, and runs
# for hours. `CARGO_TARGET_DIR` is cleared because the tests run the binaries
# they build: with one target directory shared across jobs, a test would run
# another job's mutant, and the result would be wrong in both directions.

# Mutate every function; a surviving mutant fails.
mutants jobs="4":
    env -u CARGO_TARGET_DIR cargo mutants --jobs {{ jobs }} --cargo-arg=--locked

# Prove the gates do not depend on the ambient PATH.
doctor:
    @echo "just    $(command -v just)"
    @echo "cargo   $(command -v cargo)"
    @echo "prek    $(command -v prek)"
    @echo "rustc   $(rustc --version)"

# `POST /reload` rather than a restart: the router reads the catalog file
# again and nothing running is stopped. The reply names what changed, and
# `superseded` names the entries whose running child keeps the arguments it
# was started with until it is next loaded. A catalog that does not parse is
# refused and the one serving is untouched, so a file saved half-way through
# cannot take the router down.
#
# The address is a parameter because the unit file, not this one, says where
# the router listens. A key in `MAESTRO_API_KEY` is passed on standard input
# rather than as an argument, where any process on the machine could read it.

# Make the running router read its catalog again.
reload address="127.0.0.1:8080":
    #!/usr/bin/env sh
    set -eu
    { [ -z "${MAESTRO_API_KEY:-}" ] || printf 'Authorization: Bearer %s\n' "$MAESTRO_API_KEY"; } \
        | curl --silent --show-error --fail-with-body -X POST -H @- "http://{{ address }}/reload"
    echo

# The two facts a restart costs you: whether the unit is up, and what it is
# holding. A restart with nothing loaded interrupts nothing.
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
    MAESTRO_MODEL_ROUTER_COMMIT="$commit" cargo build --release --locked --bin model-router
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
    systemctl --user restart {{ unit }}
    sleep 1
    systemctl --user is-active {{ unit }}
    "$bin/model-router" --version

# What the router is holding, before deciding whether to interrupt it.
serving:
    #!/usr/bin/env sh
    systemctl --user is-active {{ unit }} | sed 's/^/unit: /'
    held=$(pgrep -af llama-server | grep -o -- '--alias [A-Za-z0-9._-]*' | sed 's/--alias //')
    if [ -n "$held" ]; then
        echo "$held" | sed 's/^/loaded: /'
    else
        echo "loaded: nothing"
    fi
