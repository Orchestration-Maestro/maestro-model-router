# Changelog

All notable changes are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries follow the conventional pull request titles, which a squash merge
records as the commit on `main`.

## [Unreleased]

- Moved from `maestro-llamacpp` to `Orchestration-Maestro/maestro-model-router`
  under the organization's Rust CI; see
  [ADR 0002](docs/adr/0002-maestro-model-router-in-orchestration-maestro.md).
- A caller that hangs up releases its model at once, also while the model is
  still silent: reading a long prompt, or finishing a reply it does not stream.
- Every generating entry caps an answer at 32768 tokens unless it says
  otherwise, so a repetition loop ends before the context is full.
- A child's output reaches the router's standard error, each line prefixed
  with its entry, and a child that dies while loading is refused with its
  last lines.
- `model-router --version`, the startup output and `/props` name the release
  and commit; `just deploy` installs only committed code, once nothing is in
  flight.
- `serve` waits up to 30 seconds for an address no interface holds yet.
- `MAESTRO_API_KEY` and `MAESTRO_ALLOWED_ORIGINS` narrow who may use the
  router. Both are off unless set.

Nothing released yet. The first tag will be `v0.1.0`.
