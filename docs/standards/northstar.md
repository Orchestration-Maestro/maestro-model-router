# Northstar for `maestro-model-router`

> Automate the guardrails to deliver faster, with higher quality, and more
> securely.

`maestro-model-router` steers by the organization's
[Northstar](https://github.com/Orchestration-Maestro/.github/blob/main/golden-rules/northstar.md):
one KPI per pillar, each with its measurement. Unmeasured is written `not
measured`, never estimated; a value read by hand carries the date it was read.

## The point

Agents on one machine reach many local models through one endpoint each, without
anyone starting, stopping or fitting models into memory by hand: the router
loads what a request asks for within the memory it is given, and unloads what
sits idle.

## KPIs

| Pillar | KPI | Current | Target | Measured by |
| --- | --- | --- | --- | --- |
| Speed | Duration of the required Rust CI on a pull request, p95 | not measured | under 5 minutes | The run time of `rust / Required Rust CI` |
| Quality | Surviving mutants on a pull request's diff | 0 | 0 | rust-workflows' mutation testing on every pull request |
| Maintainability | Clippy warnings | 0 | 0 | `just check` and CI deny every warning |
| Security | Open code scanning alerts | not measured | 0 | CodeQL and Clippy SARIF in code scanning |
