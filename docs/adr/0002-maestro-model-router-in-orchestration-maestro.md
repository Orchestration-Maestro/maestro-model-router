# ADR 0002: maestro-model-router in Orchestration-Maestro

- Status: Accepted
- Date: 2026-09-24

## Context

This repository was `maestrolabs-hq/maestro-llamacpp`. That organization no
longer exists, so the code had no remote, no CI and no copy anywhere but one
machine. Its successor, Orchestration-Maestro, runs every repository under one
set of rules: public visibility, signed commits on `main`, squash-merged pull
requests with conventional titles, CodeQL results, and for Rust repositories
the reusable CI of `rust-workflows`. Every repository there carries the
`maestro-` prefix.

The name `llamacpp` also stopped describing the program. It routes to any
server a catalog entry names, llama.cpp builds and speech runtimes alike.

## Decision

The repository, package and library are `maestro-model-router` and
`maestro_model_router`. The command keeps its name, `model-router`, because the
service unit and every caller already use it, and nothing is gained by making
them change.

History starts again with one signed import. The old history holds unsigned
commits that `main` would refuse, and re-signing every commit would change
every hash quoted elsewhere. It is kept outside the repository as a git bundle,
with the old `.git` directory beside it.

The repository adopts the organization's Rust CI template at `rust-workflows`
v1.2.1, and with it these changes to what the code promised:

- **Rust 1.98.1**, pinned in `rust-toolchain.toml` and declared as
  `rust-version`. The old declaration, 1.85, was false: `let` chains need 1.88,
  and the declared-version gate compiles with the version declared.
- **No `unsafe` in any target.** The tests that changed environment variables
  now read each rule through its `from_variable` form, or run the binary with
  `Command::env`.
- **Tests on Linux, macOS and Windows** on every pull request, because the
  router claims all three.
- **The no-machine-paths rule is a test**, `tests/it/machine_paths.rs`. It
  lived in the old organization's shared workflow, which went with it.
- **The full eviction sweep runs weekly** in its own workflow, as it did in the
  old heavy tier.

Mutation testing is off for the import pull request alone. Its diff is the
whole codebase, which outgrows the job; the `rust-workflows` documentation
says to switch the input off when that happens. The next pull request turns it
back on, and from then every change is mutated.

## Consequences

The code has a remote again, and every change passes the same gates as every
other Rust repository in the organization. Code that was never mutated before
the import is not mutated now; only what changes afterwards is.

Two settings have no API and wait for the owner: reported content open to all
users, and the social preview image.
