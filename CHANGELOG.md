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
- A request whose model fits is no longer held behind one waiting for room.
  Waiting requests take the room in the order they asked, look again the
  moment a model is let go rather than every quarter second, and are told to
  ask again when the catalog is reloaded under them.
- A caller that sends none of its request, or reads none of its answer, for a
  minute is let go; at most 256 connections are answered at once, and the rest
  wait in the operating system's backlog.
- Relayed writes are sent as they are made, not held for the last one's
  acknowledgement.
- Where the driver reports nothing per process on the device, as under WSL, a
  model is counted at how far the device's free memory fell while it loaded.
- `just reload` re-reads the catalog through `POST /reload` rather than
  restarting the router, so nothing loaded is interrupted.
- `POST /models/load` and `POST /models/unload`, in llama.cpp's router shape,
  load a model before anything asks it a question and give its memory back
  before its idle window ends. A model answering a request is not unloaded.
- `GET /metrics` reports what is loaded, what each entry was estimated and
  measured holding, the line waiting for room, the budget and the build, in
  the text format Prometheus scrapes.
- A request carrying `X-Model-Router-Room: free` is loaded only into free
  room: one whose model would need another unloaded is refused with `503` and
  `insufficient_room` before anything is unloaded. Any other value is refused
  with `400` and `unknown_room`. A model loaded that way is a guest until it
  is unloaded: when a later request needs room, idle guests are unloaded
  before any other model.

Nothing released yet. The first tag will be `v0.1.0`.
