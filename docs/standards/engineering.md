# Engineering rules in `maestro-model-router`

`maestro-model-router` follows the organization's [engineering
rules](https://github.com/Orchestration-Maestro/.github/blob/864d85597a833864cd8506c3925830503b3c2163/golden-rules/engineering.md).
This page is its rule map (C-001): for every rule, what holds it here, or why it
does not apply. A row may name a stricter local rule; none weakens one.

Its rows come from the golden rules of `.github@864d855`; the page is kept
current by review, and the organization's daily drift check reports a rule that
is not mapped here.

## Rule map

| Rule | Held here by |
| --- | --- |
| FND-001 Think before coding | Review: the pull request states its assumptions, the alternatives weighed and what stays unclear |
| FND-002 Simplicity first | Review: the pull request names the requirement each change serves; nothing speculative lands |
| FND-003 Surgical changes | Review: every changed line traces to the pull request's goal |
| FND-004 Goal-driven execution | Review: the pull request's Verification section holds the check that means done, and its output |
| P-001 YAGNI | Review: a reviewer names the principle a change breaks |
| P-002 KISS | Review: a reviewer names the principle a change breaks |
| P-003 DRY | Review: a reviewer names the principle a change breaks |
| P-004 WET | Review: a reviewer names the principle a change breaks |
| P-005 Rule of three | Review: a reviewer names the principle a change breaks |
| P-006 Chesterton's fence | Review: a reviewer names the principle a change breaks |
| P-007 Boy Scout rule | Review: a reviewer names the principle a change breaks |
| P-008 Least astonishment | Review: a reviewer names the principle a change breaks |
| P-009 Single responsibility | Review: a reviewer names the principle a change breaks |
| P-010 Composition over inheritance | Review: a reviewer names the principle a change breaks |
| P-011 Fail fast | Code and test: `model-router check` validates the catalog before anything loads; a request for a model the catalog lacks is refused |
| P-012 Make illegal states unrepresentable | Review: a reviewer names the principle a change breaks |
| P-013 Parse, don't validate | Code: the catalog and each request parse into typed values before use |
| P-014 Principle of least privilege | CI: each job holds only its scope (`sarif` alone writes security events, `coverage` alone gets an OIDC token) |
| P-015 Separation of concerns | Review: a reviewer names the principle a change breaks |
| P-016 Zero, one or many | Review: a reviewer names the principle a change breaks |
| P-017 Premature optimisation | Review: a reviewer names the principle a change breaks |
| P-018 Broken windows | Gate: Clippy with `-D warnings`, `todo`, `dbg_macro` and `unsafe_code` denied, rustdoc warnings and missing docs as errors, 90% line coverage |
| ENF-001 No machine-named paths | Review: paths are derived at run time (AGENTS.md); no automated path check yet |
| ENF-002 Every claimed platform is tested | CI: every pull request compiles and lints for macOS and Windows beside the Linux test run (`check_targets` in `.github/ci.toml`), each of those jobs installs the pins `mise.toml` declares for its host (`test_tools`), the landing run tests macOS natively, and the weekly eviction sweep runs all three (`matrix.runner` in `.github/workflows/eviction-sweep.yml`) |
| ENF-003 English only | Review: prose and identifiers are English |
| ENF-004 Conventional commits | Organization: the `commits-are-conventional` ruleset refuses any other title on the default branch |
| ENF-005 Failing test first | Review: the new test is seen failing first; `just mutants` mutates the whole crate and a surviving mutant fails it |
| ENF-006 Never weaken a gate | Organization: required checks and code scanning block every merge; a gate changes only in its own reviewed pull request |
| ENF-007 Pull requests only | Organization: the `default-branch-discipline` ruleset (pull request, signed commits, code scanning) and `floor-no-destruction`, with no bypass actor |
| ENF-008 Tiered checks | Commit hooks: a conventional title, a secret scan, the file-format checks, and `just pre-commit` formatting and linting the crate; `just check` runs formatting, Clippy, the documentation and the tests in CI's order; CI adds the other platforms, the coverage floor, advisories and the audits |
| ENF-009 Allowlists that cannot rot | Tests: the duplication allowlist refuses unrecorded and stale pairs; a failed `similarity-rs` run fails with its exit status and stderr before any pairs are read. Each `.cargo/mutants.toml` exclusion names its mutant and the reason, removed when the reason stops being true |
| ENF-010 Configuration is the authority | Organization: its settings as code in `.github/org/`, checked weekly by `org-drift.yml` |
| ENF-011 Instructions grant nothing | Organization: authority lives in rulesets, workflow `permissions:` and access control; no instruction file grants any |
| ENF-012 Pinned inputs | Gate: `Cargo.lock` with `--locked`, actions pinned by SHA, tools pinned in `mise.toml`, checksummed in `mise.lock` and installed by `just setup` |
| ENF-013 No secret in history | Organization: secret scanning with push protection and validity checks (`maestrolabs-baseline`); CI: gitleaks |
| ENF-014 Multi-factor authentication | Organization: two-factor authentication is required of every member and outside collaborator |
| C-001 Map every rule | These pages, kept current by review; the daily drift check reports a row not mapped yet |
| C-004 Detect drift | Organization: the daily drift check opens a `Drift:` issue for this repository |
| C-005 Keep the evidence | GitHub: pull requests, CI runs with their reports, and drift issues |
| C-006 Controlled exceptions | Review: an exception is recorded in the pull request that makes it, with its scope, rationale and expiry |
