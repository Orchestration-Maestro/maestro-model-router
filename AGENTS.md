# Working in maestro-model-router

The model router agents work under.

Written for an agent, and true for a person.

## Before anything else

Read `README.md` for what this is, the
[model router design](docs/superpowers/specs/2026-09-03-model-router-design.md)
for where it is going, and `docs/adr/` for what has already been decided. A
decision recorded there was made with reasons. Reopen it with new evidence,
not with a preference.

The organization's golden rules bind here too. This repository's map of them
says what holds each one here: [engineering](docs/standards/engineering.md),
[security](docs/standards/security.md) and the
[Northstar](docs/standards/northstar.md), with one KPI per pillar.

## What this repository is

A router that supervises `llama-server` child processes and serves one
OpenAI-compatible endpoint per model. Two consequences shape almost every
change here:

**It owns processes it did not write.** A child can hang, exit, or be killed
by the operating system. Treat every spawn, health check and shutdown as a
thing that fails, and say what happens when it does.

**It must behave the same on Windows, macOS and Linux.** Process semantics,
path separators and signal handling all differ across them. Derive paths and
resolve behaviour at run time rather than assuming the platform you are on.
CI keeps the claim honest: every pull request compiles and lints for macOS
and Windows beside the Linux run that tests (`check_targets` in
`.github/ci.toml`), and the merge that lands them runs the tests on macOS
too. The eviction sweep runs the whole matrix on all three weekly. That is
the check that catches this.

---

## The four rules

From [Andrej Karpathy's observations](https://x.com/karpathy/status/2015883857489522876)
on how coding agents actually fail. They bias toward caution over speed; on a
one-line change, use judgement.

### 1. Think before coding

State your assumptions **before** implementing. Where more than one reading of
the request exists, present them rather than silently picking one. If a simpler
approach exists, say so and push back. If something is unclear, stop and name
what is confusing.

The failure this prevents: confidently building the wrong thing, fast.

### 2. Simplicity first

Write the least code that solves the problem. No speculative abstraction, no
configuration hook for a single caller, no error handling for a state that
cannot occur. Three similar lines beat a premature abstraction.

The failure this prevents: a framework where a function was wanted.

### 3. Surgical changes

Touch what the task requires and nothing else. A bug fix is not an invitation
to reformat the file, rename the variables, or upgrade the dependency. Drive-by
changes hide the real diff and make a revert dangerous.

The failure this prevents: a two-line fix arriving as a two-hundred-line diff.

### 4. Goal-driven execution

Define what "done" looks like before starting, in terms someone else could
check. Then run that check and report what it printed. "Tests pass" is a claim;
the output is evidence.

The failure this prevents: declaring success without ever verifying it.

---

## Rules that are enforced, not suggested

**Derive every path at run time.** The home directory from the environment,
the repository root from the working directory, model files from a configured
models root. `tests/it/machine_paths.rs` refuses a home directory, a drive
letter or a user profile, and deliberately allows `/usr`, `/opt`, `/etc`,
`/var` and `/tmp`, which name a platform rather than a machine. In a test, use
a synthetic root such as `/somewhere`.

**No `unsafe`, the tests included.** `Cargo.toml` forbids it for every target.
A test that wants a different environment variable reads the rule through its
`from_variable` form, or runs the binary with `Command::env`; changing the
process environment is `unsafe` in Rust 2024.

**English only.** Prose and identifiers. `tests/it/english_only.rs` scans for
Latin diacritics.

**Conventional commits.** `feat:`, `fix:`, `docs:`, `refactor:`, `test:`,
`ci:`, `build:`, `chore:`, `perf:`, `revert:`. An organisation ruleset refuses
anything else on `main`, where a squash merge records the pull request title,
and the changelog is written from those titles.

**Write the failing test first, and watch it fail.** Record what it printed. A
test that passes the moment it is written has proved nothing: it may be
testing the implementation that was just written rather than the behaviour
that was wanted.

**Report a blocked check rather than weakening it.** If a gate blocks
something correct, say so in the pull request. A gate that is quietly bypassed
is worse than no gate, because the repository still looks guarded.

## The shape of a change

`main` takes changes only through a pull request. Direct pushes are refused by
the platform, for the maintainer too. Install rustup and `mise` first, as
[`README.md`](README.md) shows under its local commands.

```text
just setup            # once per clone: the pinned tools and the commit hooks
git switch -c <topic>
just check            # exactly what CI runs, run here
git push -u origin <topic>
gh pr create --fill
gh pr merge --squash --delete-branch
```

## What the gates will tell you

Locally, `just setup` installs the hooks and every tool they run, pinned in
`mise.toml`. The hooks format and lint a commit, check its message,
and look for a secret in it; the hand-maintained file-format checks beside them
cover YAML, TOML, shell, Markdown and the workflows. `just check` runs
formatting, Clippy with warnings denied on every target, the documentation and
the tests, in the order continuous integration runs them.

In CI, the shared Rust workflow blocks the merge: formatting, Clippy, tests on
Linux with native tests on the macOS leg, a Windows compile-and-lint leg,
strict rustdoc, the coverage floor this repository declares in
`.github/ci.toml`, advisories, spelling, licences, a secret scan, the workflow
audits and the declared Rust version. The full eviction sweep runs weekly and
reports.

`tests/it/duplication_allowlist.rs` uses an allowlist rather than a threshold.
Adding an entry is expected; adding one without its reason is not, and a
second test fails when an entry stops being true, so the list cannot rot into
excuses.

## Things that will surprise you

**One crate, deliberately.** A seam is only real when something varies across
it. See [ADR 0001](docs/adr/0001-one-crate-until-a-seam-is-real.md) for the
condition that reopens this: a second consumer of a module, not a feeling that
a file is getting long.

**Renaming a CI job renames a required context.** The ruleset naming the old
one blocks every pull request until it is updated. They change together.

**The `llama-server` flag surface moves between releases.** The catalog
carries flags it does not interpret, so a change upstream is a catalog edit
rather than a code change.
