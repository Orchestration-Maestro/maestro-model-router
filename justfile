# Optional convenience task runner (https://just.systems).
# Every command below works standalone -- just is never required.
#
# Every tool these recipes run comes from `mise.toml`, on the `PATH` mise
# supplies when it is enabled in the shell or a command is prefixed with
# `mise exec --`. Nothing here puts a directory of its own in front of that
# `PATH`: an unpinned executable would then win over the pinned one it shadows.

# The service unit that runs the router, for the recipes that act on it.
#
# Named rather than spelled out at each use, and a name rather than a pid:
# the unit is what starts the router at login and what restarts it, so acting
# on anything else leaves systemd's idea of the service and the process that
# is actually running disagreeing with each other.
#
# `reload`, `serving` and `deploy` are the recipes in this file that are not
# cross-platform: they act on a running router, and all but `reload` on its
# service, which on this machine is a systemd user unit. Every check above them
# runs anywhere its tools are on the PATH; `just setup` provisions those tools
# on Linux, macOS and Windows. Two are narrower: the duplication test inside
# `just check` needs Linux or macOS, and `just mutants` needs Linux, because
# `mise.toml` pins each of those two for the hosts its release builds for.
unit := "model-router"

# First in the file, deliberately: `just` with no arguments runs the first
# recipe, and a reader typing it to find out what is here should get the list,
# not an install.
#
# `--list` reads the last comment line above a recipe as its description, so
# in this file the reasoning goes above and the summary goes immediately
# before the recipe. That is why these blocks read upside down.

# Every recipe, with what it does. Bare `just` shows this.
help:
    @just --list --unsorted

# Every tool a hook or a recipe runs is pinned in mise.toml, and the commit
# hooks are installed from .pre-commit-config.yaml. Both are per clone.

# Install the pinned tools and the commit hooks. Idempotent.
setup:
    mise install
    mise exec -- prek install

# The four checks continuous integration runs on a pull request, in its order:
# formatting, Clippy with warnings denied on every target, the documentation
# with every public item required, and the tests.
#
# Advisories, coverage, the spelling and licence checks and the other two
# platforms stay with the shared workflow: they need its pinned tools and its
# runners, and a local copy that drifts is worse than none.

# Run the checks CI runs.
check:
    cargo fmt --all --check -- --config style_edition=2024
    cargo clippy --workspace --all-targets --locked -- -D warnings
    RUSTDOCFLAGS="-D warnings -D missing_docs" cargo doc --workspace --no-deps --locked
    cargo nextest run --workspace --locked

# Format in place and lint, for the commit hook. `check` is the whole of it and
# verifies instead of rewriting. A file formatting rewrites is left for its
# author to stage: the hook fails while it differs, and a commit carries what
# was staged rather than whatever a hook touched on its way.

# Prepare a commit: format and lint it.
pre-commit:
    cargo fmt --all -- --config style_edition=2024
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Format in place. `check` only verifies.
fmt:
    cargo fmt --all -- --config style_edition=2024

# Mutation testing of the whole crate: it runs for hours, and a surviving
# mutant fails the run. Continuous integration does not mutate a change, so
# nothing else catches one. `CARGO_TARGET_DIR` is cleared because the tests run
# the binaries they build: with one target directory shared across jobs, a test
# would run another job's mutant, and the result would be wrong in both
# directions. The tool's release ships x86_64 builds only, so `mise.toml` pins
# it for Linux, where this runs.

# Mutate every function; a surviving mutant fails.
mutants jobs="4":
    env -u CARGO_TARGET_DIR cargo mutants --jobs {{ jobs }} --cargo-arg=--locked

# Prove the hooks and the checks do not depend on the ambient PATH.
doctor:
    @echo "just    $(command -v just)"
    @echo "cargo   $(command -v cargo)"
    @echo "prek    $(command -v prek)"
    @echo "gitleaks $(command -v gitleaks)"
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
