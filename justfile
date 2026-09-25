# Optional convenience task runner (https://just.systems).
# Every command below works standalone -- just is never required.

# Our runtime, ahead of whatever the ambient PATH carries. WSL appends the
# Windows PATH by default, which put a broken `just` npm shim in front of the
# real one once already. Recipes resolve tools from our own install first, so
# an inherited PATH cannot decide which binary a gate runs.
#
# The pinned toolbelt goes first of all: `setup` links every tool mise.toml
# pins into the ignored .tools/bin, so a gate runs the release mise.lock
# verified rather than whichever copy an earlier install left in ~/.cargo/bin.
#
# Derived, never hardcoded: `home_directory()` resolves on Windows, macOS and
# Linux alike, and the separator follows the OS rather than assuming Unix.
path_sep := if os_family() == "windows" { ";" } else { ":" }
tools_bin := justfile_directory() / ".tools" / "bin"
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
# runs anywhere its tools are on the PATH; `setup` provisions those tools on
# Linux x64, the platform mise.lock records them for. `update-tools` and
# `_commit-as-bot`, last in the file, belong to the tool-updates workflow and
# run on its Linux runner.
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

# Rust alone. The gate tools are the pinned toolbelt, which `setup` installs:
# a tool fetched here at whatever version was newest is the unpinned input
# mise.lock exists to remove.

# Install the Rust toolchain this repository needs. Idempotent.
install:
    rustup toolchain install --profile minimal 1.98.1
    rustup component add clippy rustfmt llvm-tools-preview

# mise installs every tool mise.toml pins and refuses bytes that differ from
# mise.lock; scripts/bootstrap.sh verified mise itself, and runs this once.
# The links in .tools/bin are what put the toolbelt on this file's PATH. The
# hooks' two types come from default_install_hook_types.

# Install the pinned toolbelt into ignored .tools/bin and wire the local hooks.
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || {
      echo 'Pinned tooling supports Linux x64 only' >&2; exit 1;
    }
    mise trust mise.toml
    mise install --locked
    mkdir -p .tools/bin
    find .tools/bin -mindepth 1 ! -name mise -delete
    while IFS= read -r directory; do
      for file in "$directory"/*; do
        if [[ -f "$file" && -x "$file" ]]; then
          ln -s -- "$file" ".tools/bin/$(basename -- "$file")"
        fi
      done
    done < <(mise bin-paths)
    prek install --install-hooks

# The quality commands rust-workflows' CI runs, with its flags: Clippy with the
# scaffolding and `unsafe` denied, strict rustdoc, the 90% coverage floor; then
# the workflows, the formatting of every other file, the spelling, a secret
# scan of every file a commit could take, and the commit hooks. CI also runs
# what needs its own runners or the network: the other platforms, the release
# build, the SBOMs and mutation testing.
#
# A missing tool stops it before anything runs, because a gate that skips when
# its tool is absent reports green while looking at nothing.

# Run the quality gates CI runs.
check:
    #!/usr/bin/env bash
    set -euo pipefail
    for tool in mise just actionlint zizmor yamlfmt taplo shellcheck prek cargo rustup \
      gitleaks typos jaq cargo-deny cargo-llvm-cov cargo-machete similarity-rs; do
      command -v "$tool" >/dev/null ||
        { echo "Missing $tool; run scripts/bootstrap.sh" >&2; exit 1; }
    done
    just --unstable --fmt --check
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- \
        -D warnings -D clippy::todo -D clippy::dbg_macro -D unsafe_code
    cargo test --workspace --all-targets --locked
    cargo test --workspace --doc --locked
    RUSTDOCFLAGS='-D warnings -D missing_docs' cargo doc --workspace --no-deps --locked
    cargo llvm-cov --workspace --locked --fail-under-lines 90 --summary-only
    cargo machete
    cargo deny check
    actionlint
    zizmor --offline --persona=pedantic --no-progress .github/
    yamlfmt -no_global_conf -lint
    taplo fmt --check
    typos
    # Secrets in every file a commit could take, target/ and .tools/ aside.
    tree=$(mktemp -d)
    trap 'rm -rf "$tree"' EXIT
    while IFS= read -r -d '' file; do
      if [[ -f "$file" ]]; then cp --parents -- "$file" "$tree/"; fi
    done < <(git ls-files -z --cached --others --exclude-standard)
    gitleaks dir --no-banner --redact "$tree"
    prek run --all-files

# Format in place. `check` only verifies.
fmt:
    cargo fmt --all

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

# Move every tool mise.toml pins to its latest release, mise.lock with it (network).
[linux]
update-tools:
    #!/usr/bin/env bash
    set -euo pipefail
    # mise.toml first, then mise.lock through mise, which records each new
    # release's URL and checksum. One line per move; no output means all current.
    declare -A was=()
    version='^[0-9A-Za-z][0-9A-Za-z.+-]*$'
    while read -r name current; do
      [[ "$name" =~ ^[a-z0-9-]+$ ]] || { echo "unexpected tool name: ${name}" >&2; exit 1; }
      latest="$(mise latest "$name")"
      [[ "$latest" =~ $version ]] || { echo "${name}: no release version from mise" >&2; exit 1; }
      [[ "$latest" != "$current" ]] || continue
      sed -i -E "s/^(${name} = (\{ version = )?\")${current//./\\.}\"/\1${latest}\"/" mise.toml
      grep -q "^${name} = .*\"${latest}\"" mise.toml || {
        echo "${name}: cannot move its version in mise.toml" >&2; exit 1;
      }
      was[$name]="$current"
      major=''
      [[ "${current%%.*}" == "${latest%%.*}" ]] || major=' (major)'
      echo "${name} ${current} -> ${latest}${major}"
    done < <(jaq -r --from toml \
      '.tools | to_entries[] | "\(.key) \(.value | if type == "object" then .version else . end)"' \
      mise.toml)
    if (( ${#was[@]} )); then
      # Progress goes to stderr: stdout is the list of moves, a commit message.
      mise lock --platform linux-x64,linux-x64-musl "${!was[@]}" >&2
    fi

# Commit every changed file of the checkout onto $BRANCH as the organization's
# bot: through createCommitOnBranch, which GitHub signs, where a commit made on
# the runner would be unsigned and the organization refuses it. The new commit's
# parent is $HEAD, which must still be the branch's head. Reads GH_TOKEN,
# GITHUB_REPOSITORY, BRANCH, HEAD, TITLE, BODY, a file, and PATHS, the pathspecs
# a commit may take; unset, every changed file.
_commit-as-bot:
    #!/usr/bin/env bash
    set -euo pipefail
    : "${GH_TOKEN:?}" "${GITHUB_REPOSITORY:?}" "${BRANCH:?}" "${HEAD:?}" "${TITLE:?}" "${BODY:?}"
    read -r -a paths <<< "${PATHS:-}"
    files="$(mktemp)"
    while IFS= read -r path; do
      jaq -n --arg path "$path" --arg contents "$(base64 -w0 "$path")" \
        "{path: \$path, contents: \$contents}" >> "$files"
    done < <(git diff --name-only -- "${paths[@]}")
    if [[ ! -s "$files" ]]; then
      echo "Nothing changed; nothing to commit."
      exit 0
    fi
    mutation="mutation(\$input: CreateCommitOnBranchInput!) {"
    mutation+=" createCommitOnBranch(input: \$input) { commit { oid } } }"
    input="{branch: {repositoryNameWithOwner: \$repo, branchName: \$branch},"
    input+=" expectedHeadOid: \$head, message: {headline: \$title, body: \$body},"
    input+=" fileChanges: {additions: \$files}}"
    jaq -n --arg query "$mutation" --arg repo "$GITHUB_REPOSITORY" --arg branch "$BRANCH" \
      --arg head "$HEAD" --arg title "$TITLE" --rawfile body "$BODY" \
      --slurpfile files "$files" "{query: \$query, variables: {input: ${input}}}" \
      > "$files.json"
    gh api graphql --input "$files.json" --jq '.data.createCommitOnBranch.commit.oid'
