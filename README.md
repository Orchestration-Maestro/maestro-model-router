<p align="center">
  <img src=".github/assets/maestro-model-router.jpg" alt="Maestro Model Router: load what's asked, free what's idle." width="100%" />
</p>

<h1 align="center">🔀 Maestro Model Router</h1>

<p align="center">
  One OpenAI-compatible endpoint for every local model: it loads what a request asks for, and frees what sits idle.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2024-CE422B?style=for-the-badge&amp;logo=rust&amp;logoColor=white" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Serves-llama.cpp-334155?style=for-the-badge" alt="Supervises llama.cpp's llama-server" />
  <img src="https://img.shields.io/badge/API-OpenAI--compatible-334155?style=for-the-badge" alt="OpenAI-compatible endpoints" />
  <img src="https://img.shields.io/badge/Tests-Linux%20%7C%20macOS%20%7C%20Windows-334155?style=for-the-badge&amp;logo=githubactions&amp;logoColor=white" alt="Every pull request tests Linux, macOS and Windows" />
</p>

<p align="center">
  <a href="https://github.com/Orchestration-Maestro/maestro-model-router/actions/workflows/ci.yml"><img src="https://github.com/Orchestration-Maestro/maestro-model-router/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI" /></a>
  <a href="https://scorecard.dev/viewer/?uri=github.com/Orchestration-Maestro/maestro-model-router"><img src="https://api.scorecard.dev/projects/github.com/Orchestration-Maestro/maestro-model-router/badge" alt="OpenSSF Scorecard" /></a>
  <a href="https://codecov.io/gh/Orchestration-Maestro/maestro-model-router"><img src="https://codecov.io/gh/Orchestration-Maestro/maestro-model-router/graph/badge.svg" alt="Codecov line coverage" /></a>
</p>

maestro-model-router is Orchestration-Maestro's model router, and its command
is `model-router`. It supervises llama.cpp server processes and exposes one
OpenAI-compatible endpoint per model plus a generic routing endpoint.

## ⚡ Quick start

With `llama-server` from [llama.cpp](https://github.com/ggml-org/llama.cpp) on
the search path:

```sh
git clone https://github.com/Orchestration-Maestro/maestro-model-router
cd maestro-model-router
cargo install --locked --path . --bin model-router
model-router check catalog.toml   # every problem, each by its entry and field
model-router serve catalog.toml   # one endpoint per model, and one routed by the body
```

Models resolve under `MAESTRO_MODELS_ROOT`, or `models` in the home directory.
Then ask any model the catalog carries through the routed endpoint:

```sh
curl http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model": "gemma3", "messages": [{"role": "user", "content": "Hello"}]}'
```

## 🎯 Objectives

Give every local model one OpenAI-compatible endpoint, so a caller names a
model and never manages a server: the router loads what a request asks for,
keeps the resident models warm, and frees what sits idle. It runs on Linux,
macOS and Windows under the organization's gates. No tracked file names a
machine; the models root, the server binary and the memory budget are read
where the router runs.

## 🔄 How it works

<p align="center">
  <img src=".github/assets/how-it-works.svg" alt="A caller's OpenAI-compatible request goes through the router's route, admission and relay to the model's llama-server on a loopback port, and the reply streams back as it is generated. Admission fits a model that is not running under the memory budget and in what the device has free, unloading the coldest idle on-demand model or waiting in line for room; a model that is answering is never unloaded. Beside the requests, the catalog is read again on POST /reload, the reaper unloads a model unused for longer than the idle window, and /metrics reports what admission holds." width="100%" />
</p>

1. A caller sends an OpenAI-compatible request to an address the router
   listens on: to `/v1`, where the body names the model, or to
   `/models/<id>/v1`, where the path does.
2. The router reads the request head. With `MAESTRO_API_KEY` or
   `MAESTRO_ALLOWED_ORIGINS` set, it checks the key and the origin, then finds
   the catalog entry. A refusal, here or at admission, comes before anything is
   forwarded, in the error envelope an OpenAI client already parses.
3. Admission gives the request its model. A model already running takes it.
   One that is not running must fit twice: under the memory budget, which
   counts each model at its catalog estimate until it has been measured, and in
   what the device has free right now. To make room, admission unloads the
   coldest idle on-demand model, never one that is answering. When a busy model
   holds the only room, the request waits in line, a minute by default, and is
   refused with `503` if the room is still held then.
4. The model's `llama-server` starts on a loopback port and must answer within
   its startup budget. Resident models start with the router and are never
   evicted.
5. The relay forwards the request and copies the reply back byte for byte, so
   a stream reaches the caller as it is generated. A caller that hangs up
   releases its model at once.
6. Beside the requests, the reaper unloads an on-demand model unused for
   longer than the idle window, `POST /reload` reads an edited catalog without
   stopping anything, and `GET /metrics` reports what admission holds. A
   signalled router stops every child before it exits.

## 📍 Status

The six slices of the
[design](docs/superpowers/specs/2026-09-03-model-router-design.md#vertical-slices)
have shipped: the catalog, a supervised `llama-server`, the dedicated
endpoint, the generic endpoint with eviction, residency, and tests on Linux,
macOS and Windows under the organization's CI
([ADR 0002](docs/adr/0002-maestro-model-router-in-orchestration-maestro.md)).
Since then the router unloads a model left idle, waits in line for room rather
than refusing at once, bounds what one caller can hold, takes a key and a list
of browser origins, reloads its catalog, loads and unloads on request, and
reports at `/metrics`. What it leaves open is listed under
[what eviction never does](#what-eviction-does-and-what-it-never-does).

## ✅ Checking a catalog

```sh
model-router check catalog.toml
```

A usable catalog reports what it carries and exits zero:

```text
catalog.toml is valid: 4 models
```

An unusable one names every problem, each by the entry and the field it came
from, and exits non-zero. Every fault is listed rather than only the first, so
one run covers one round of edits:

```text
models.toml is not usable:
  entry 'alpha': field 'colour' is not recognised
  entry 'alpha': field 'path' is required
  entry 'alpha': field 'context_size' must be greater than zero, but is 0
  entry 'beta': field 'residency' must be one of resident or on-demand, but is 'sometimes'
```

`catalog.toml` holds the models this router serves. Every location in it is
relative to a models root supplied at run time, so the file describes a set of
models without naming the machine they sit on.

## 🚀 Launching one model

```sh
model-router launch catalog.toml gemma3
```

Starts that entry, waits until it answers, then stops it again:

```text
gemma3 is ready at http://127.0.0.1:41273 after 1.4 seconds
gemma3 stopped
```

This is deliberately not a long-running command: it proves a child starts and
answers, then ends it. What `serve` does when it is signalled is a separate
question, answered under [what eviction never does](#what-eviction-does-and-what-it-never-does).

A child that never becomes ready fails when its startup budget expires, and a
child that exits while loading fails with its status rather than waiting the
budget out. Every failure names the entry it came from.

### Where the models are

Catalog locations are relative, and resolve against a models root read at run
time:

| Source | Value |
| --- | --- |
| `MAESTRO_MODELS_ROOT` | used as given, when set and not empty |
| otherwise | `models` under the home directory |

### What fits in memory

| Source | Value |
| --- | --- |
| `MAESTRO_MEMORY_BUDGET_MIB` | the ceiling in mebibytes, when set and not empty |
| otherwise, on a machine with a device `nvidia-smi` can see | the device's total less a tenth, and never less than 1024 MiB less |
| otherwise, on a machine that reports its system memory | four fifths of it |
| otherwise | no budget, so nothing is ever unloaded to make room |

A budget is a fact about one machine's hardware, which is why it is read from
the environment or from the machine rather than written into a catalog: the
catalog describes a set of models without naming the machine they sit on. A
value that is not a whole number is refused rather than read as unset, because
the difference between those two is whether the ceiling is the one the operator
meant.

The router says which it found at startup, and when the machine set the
ceiling, what it read to set it:

```text
memory budget: 29347 MiB, derived from the device (32607 MiB total less a 3260 MiB margin)
```

The margin is there because a device is never empty when a model loads: the
display, the driver and whatever else the machine runs hold some of it. The
ceiling is not the only check, either. Before a model is started the device is
asked what it has free right now, which counts everything else on the machine,
and a loaded model is measured once it is ready so that what it turned out to
cost is what the budget counts from then on. Both are under
[what eviction does](#what-eviction-does-and-what-it-never-does).

### How long unused memory may be held

| Source | Value |
| --- | --- |
| `MAESTRO_IDLE_UNLOAD_SECONDS` | the window in whole seconds, when set and not empty |
| otherwise, or `0` | no window, so nothing is ever unloaded for sitting idle |

A budget is a ceiling on what may be held at once; this is independent of it,
and answers a different question: how long unused memory may be held. A
machine with no budget still wants its memory back: an on-demand model nothing
has asked for in longer than the window is unloaded, its endpoint stays up,
and the next request for it loads it again. A resident is never a candidate,
whatever the window.

A model is held for **at most one and a half windows plus one sweep**,
measured from when a request last *finished* rather than when it started.
Otherwise a generation longer than the window would be unloaded the instant it
ended. This is measured with a monotonic clock, so it does not advance while
the machine is suspended: a laptop that sleeps for eight hours wakes holding
whatever it was holding when it slept.

### How long a request waits for room

| Source | Value |
| --- | --- |
| `MAESTRO_ADMISSION_WAIT_SECONDS` | the wait in whole seconds, when set and not empty; `0` refuses at once |
| otherwise | a minute |

When the only room a model could take is held by a model something is still
reading from, the request waits for that reader to finish rather than being
refused, for as long as this allows. Waiting requests take the room in the
order they asked for it, and look again the moment a model is let go rather
than on a timer. A request whose model fits outright is not held behind them.
A reload while a request waits tells it to ask again, which a retry does under
the catalog now serving.

The server binary is located rather than bundled: `llama-server` is taken from
the search path, with the platform's executable suffix, so no tracked file
names one machine.

### What one caller can hold

A caller that sends none of its request, or reads none of its answer, for a
minute is let go: an idle connection is answered `408`, and one that stopped
reading releases the model it was holding. The router watches for a caller
that leaves, and this rule covers the one that stays and does nothing. At most
256 connections are answered at once; more wait in the operating system's
backlog until one ends.

### Who may use it

| Source | Value |
| --- | --- |
| `MAESTRO_API_KEY` | a key every request but a preflight carries as `Authorization: Bearer <key>`, when set and not empty |
| `MAESTRO_ALLOWED_ORIGINS` | the browser origins that may call the router, separated by commas, when set and not empty |
| otherwise | open to whoever reaches an address it listens on |

A request naming an origin not on the list is refused before anything else
happens; a caller that is no browser names none and is unaffected. The router
says at startup which rules it runs under, and never prints the key.

## 🔌 Serving

```sh
model-router serve catalog.toml
```

Binds the public port and stays up:

```text
serving on http://127.0.0.1:8080
  http://127.0.0.1:8080/models/<model>/v1/chat/completions
  http://127.0.0.1:8080/v1/chat/completions   (routed by the body's model)
  POST http://127.0.0.1:8080/reload                (re-reads the catalog)
access: no key is required (set MAESTRO_API_KEY to require one); any browser origin may call it (set MAESTRO_ALLOWED_ORIGINS to list them)
memory budget: 25000 MiB, so models are unloaded to make room
residents reserve 4096 MiB of 25000 MiB, leaving 20904 MiB for everything else
idle window: 3600 seconds, so an unused on-demand model is unloaded after that long
a streamed reply is passed through as it arrives
model-router 0.1.0 (3f2c9a1b7e40)
resident qwen3-4b loaded in 1.8 seconds
```

The last line before the residents is the build: the release, and the commit
`just deploy` stamped into it. `model-router --version` and `/props` say the
same, so a running router can be traced to its source. A child's own output
goes to the router's standard error, each line prefixed with its entry, so the
service's journal keeps what a model said; a child that dies while loading is
refused with its last lines.

The reservation line is there because a resident is memory the router promises
never to reclaim. A ceiling that covers the residents but not the largest model
beside them refuses that model permanently, so the budget has to cover the
resident **plus** the largest entry expected to run next to it. An operator
who learns that from a refusal under load learns it too late.

Residents load on a thread of their own, so the router answers while they load
rather than going silent until they finish. One that cannot load names itself
and the reason, and the router serves the rest of the catalog without it:

```text
resident qwen3-4b: entry 'qwen3-4b': no model file at '/…/Qwen3-4B-Q4_K_M.gguf'
  serving the rest of the catalog without it
```

Refusing to start at all would let one missing file deny service to every other
model, which is worse than the state it would be protecting against.

### Where it listens

Addresses may be given, separated by commas, and one router binds them all:

```sh
model-router serve catalog.toml 127.0.0.1:8080,192.168.140.1:8081
```

```text
serving on http://127.0.0.1:8080
serving on http://192.168.140.1:8081
  http://127.0.0.1:8080/models/<model>/v1/chat/completions
  http://127.0.0.1:8080/v1/chat/completions   (routed by the body's model)
```

One process, not one per address. Every listener hands its connections to the
same catalog, the same children and the same memory budget, so a model loaded
for a request that arrived on one address answers the next request on the
other without loading again. Two routers would load it twice into the same
card, each counting only its own half.

The paths are the same on every address, which is why they are printed once.

Each address names an interface. A wildcard, `0.0.0.0` or `::`, is
refused: it means every interface the machine has now and every one it gains
later, which is a reach nothing stated, and blanket-serving a network is the
security design this repository has not written. Naming `192.168.140.1` says
which network, and whether anything can reach it is then a matter of routes
and firewall rules rather than of what this router assumed.

Giving the lab-facing address its own port is a convention rather than a
requirement, since different addresses do not collide on the same port. A
distinct port still tells an operator reading `ss -tlnp`, a log line or a
firewall rule which surface a request arrived on without having to read the
address.

### Reloading the catalog

An edited catalog is served without a restart:

```sh
just reload                 # or: curl -X POST http://127.0.0.1:8080/reload
```

```json
{"object":"reload","added":["qwen3-8b"],"removed":[],"superseded":["gemma3"]}
```

Nothing running is stopped. An entry the file gained can be asked for at once,
and one it lost is refused from then on. `superseded` names the entries whose
child was already running when its definition changed or was removed: the
process keeps the arguments it was started with until it is next loaded, so
until then the catalog and the process disagree. A file that cannot be read or
does not parse is refused with `400` and `catalog_unreadable`, and the catalog
already serving is untouched. A request waiting for room when the catalog
changes is told to ask again.

Only `POST` reloads. A key set in `MAESTRO_API_KEY` is required for it as for
any other request, and `just reload` sends it.

### Loading and unloading on request

A model is loaded by the first request for it and let go when it sits idle,
when room is wanted, or when the router stops. Two endpoints do either now,
for warming a model before the request that would pay for its load, or for
giving memory back before the idle window would:

```sh
curl -X POST http://127.0.0.1:8080/models/load -d '{"model":"gemma3"}'
curl -X POST http://127.0.0.1:8080/models/unload -d '{"model":"gemma3"}'
```

Both answer `{"success":true}`. The paths and bodies are llama.cpp's own router
mode, so a llama.cpp client's load and unload work against this router too.

A load replies once the model is ready, not once the load has begun, so a
caller told `success` can ask the model at once. It goes through the same
admission as a request: it unloads what a request would to make room, and is
refused as a request would be when there is none. An unload of a model that
is answering a request is refused with `409` rather than cutting that caller
off, and one of a model that is not running has nothing to do and succeeds.

### Metrics

`GET /metrics` answers in the text format Prometheus scrapes, and starts
nothing:

| Gauge | Labels | Value |
| --- | --- | --- |
| `model_router_model_loaded` | `model` | `1` while a child holds the entry, else `0` |
| `model_router_model_declared_mib` | `model` | what the catalog estimates the entry holds |
| `model_router_model_held_mib` | `model` | what a loaded entry was measured holding; absent when nothing was measured |
| `model_router_requests_waiting` | | requests in line for room |
| `model_router_memory_budget_mib` | | the budget; absent when there is none |
| `model_router_build_info` | `version`, `commit` | `1`, naming the build that answers |

Each is read from the state admission decides with, so a graph shows what the
router believed it was holding when the machine ran short.

### Two ways to name a model

Each model is reached at its own endpoint, so a request needs no model field to
be routed:

```sh
curl --no-buffer http://127.0.0.1:8080/models/gemma3/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"hello"}],"stream":true}'
```

The generic endpoint takes the model from the request body instead, which is
what an existing OpenAI-compatible client already sends:

```sh
curl --no-buffer http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"gemma3","messages":[{"role":"user","content":"hello"}],"stream":true}'
```

The path is passed to the child unchanged, and `GET /v1/models` lists the
catalog's entries without starting anything. `HEAD /v1/models` answers with
the listing's headers and no body, and a query string on the listing is still
the listing, because a client library that pages its model list adds one.

Only `GET` and `POST` reach a child. The router answers a preflight
(`OPTIONS`) itself and never starts a model for one: `204`, an `Allow` header,
and permissive `Access-Control-Allow-*` headers, which are safe for the
loopback address and as safe as the network is for any other. With
`MAESTRO_ALLOWED_ORIGINS` set, a preflight from an origin not on the list is
refused with `403` instead. Any other method under a model
prefix is refused with `405` before a child is involved, because the only
thing forwarding it could achieve is a load that then answers `404`.

Reading the body costs the response nothing. The router parses the request to
learn which model answers; the reply is still copied byte for byte, and both
endpoints deliver a paced stream with the same timing.

A child starts on the first request for its entry and is kept while there is
room for it, so that request pays the startup cost and every later one finds
the child ready.

### What eviction does, and what it never does

With a budget set, a model that does not fit causes the coldest idle on-demand
model to be unloaded first. Two questions are asked before a model is started,
and both have to say yes. The budget is a ceiling on what the loaded models
cost, where each costs its catalog estimate until it has been measured and the
larger of the two afterwards. A model that turns out to hold four times its
estimate is counted at what it holds from the moment that is known, and the
operator reads it on the line the load prints:

```text
qwen3-06b: loading, estimated at 1024 MiB
qwen3-06b: ready in 5.4 s, measured 4.5 GiB resident and 0.7 GiB on the device (catalog said 1024 MiB)
```

Where the driver reports nothing per process, as on WSL, the device figure is
how far the device's free memory fell while the model loaded. Loads
are admitted one at a time, so nothing else the router starts moves it
meanwhile; anything else on the machine that allocates at that moment is
counted too, which errs toward counting a model high.

The device is the second question, asked at the moment of the decision for
what it has free right now. That counts everything on the machine, not only
what this router loaded, so a desktop that grew since the budget was set is
room the ledger still believes in and the device no longer has. The device can
ask for more to be unloaded than the budget would. When unloading every idle
model would still not make the room, it refuses, naming what was needed
against what was free. A model whose flags keep every layer off the device is
not held to the device's room; that is the one flag this router reads rather
than passes through. Such a model keeps its weights and its cache in host
memory, so holding it to the device's room would refuse it for memory it does
not use. Where the machine cannot be asked, the device question is not asked,
and the budget decides alone.

What neither question prevents is a model that costs more than both its
estimate and the room the device reported, in the seconds between the decision
and its load. That start fails, and what protects against it is that a failed
start names its entry.

Every load and every eviction is said out loud, so a swap is visible without
watching the process table:

```text
gemma3: unloading qwen38 to make room
gemma3: loading, estimated at 2048 MiB
```

A model is never unloaded while something is reading from it. Killing a child
mid-answer would truncate the stream, which a caller cannot tell apart from a
model that finished early. When the only model that could be unloaded is busy,
the request waits for it as long as `MAESTRO_ADMISSION_WAIT_SECONDS` allows,
and is refused if the room is still held then. That is checked at the moment a
model is taken out, not only when the decision is made: a request can arrive in
between, and emptying the slot then would leave a process running that the
budget no longer counts.

Nothing is unloaded for a start that could not have happened anyway. A model
file the models root does not carry is found before the decision, so a stale
path does not cost the operator a warm model as well as the one they asked
for. A start that fails only by being attempted, such as a startup budget
expiring or a model costing more than its estimate, cannot be prevented this
way, and the room is already gone when it does.

**A signalled router stops its children before it goes.** `serve` runs until
the process is asked to end, and that end is a signal: `SIGTERM`, which
`systemctl stop`, `kill` and a container stop all send, as well as `SIGINT`
and `SIGHUP` on Unix, and Ctrl-C, Ctrl-Break or a closing console on Windows.
On any of them the router ends every `llama-server` it started, says how many
it ended, and exits zero:

```text
stopping: ended 2 children
```

A second signal while that stop is still running ends the process at once,
with a failing status because the children were then not waited for. It is
the way out from a child that will not die: without it the second signal
would queue behind the first, and nothing short of `SIGKILL` could end the
router.

Two ends this cannot cover, and nothing inside a process can. Being killed
outright (`SIGKILL`, `taskkill /F`, an out-of-memory kill) reaches no
handler, so the children stay, and keep their memory. And a child that is
mid-answer when the signal arrives is held by that answer rather than by the
router: the router's claim on it is released, but the process exits before
the answer ends, and that child is left behind. Either way a restarted router
builds its table empty, counts nothing while the strays hold real memory, and
admits a full budget of models on top of them. Nothing in the process table
ties a stray server to the router that started it, so after such an end, look
for them:

```sh
pgrep -af llama-server
```

**Four smaller things are known and not addressed.** Recorded so the next
slice inherits them rather than discovering them:

- The loading thread keeps the router answering, but only what needs no child.
  A first request to another entry still waits on the resident's load, bounded
  by that entry's startup budget. The silence the thread exists to prevent
  moved from the listener to the first load rather than going away.
- A decision naming two entries takes them in turn and stops at the first that
  gained a reader, so an operator can lose a warm model *and* still be refused.
  The budget is never overcommitted by this: the router ends under-loaded
  rather than over, which is what makes it a cost rather than a defect.
- The tests read "this child stopped" from its port going quiet. Nothing stops
  a later child binding that same ephemeral port, which would fail while
  blaming the invariant rather than the coincidence.
- Taking a slot drops the child inside the slot's own guard, so the kill and
  the reaping run under it. A child that will not die holds that guard, and
  with it every admission.

**A stream is passed through as it arrives.** The router has no HTTP
dependency: it reads the request head (the request line and the headers),
rewrites it, and copies the response back without interpreting a byte of it. A
proxy that re-frames a response is a proxy that can buffer it; one that copies
bytes cannot, which makes token-by-token delivery a property of the design
rather than a setting to get right. The request is the exception, and only on
the generic endpoint: the model is inside the body, so the body is read. What
is forwarded is still the caller's own bytes.

A caller that hangs up closes the connection to the child, which is how
`llama-server` is told to stop generating. That works mid-answer, and also
while the model is still silent: reading a long prompt, or finishing a reply
it does not stream.
The router watches the caller as well as the child, so a caller that gives up
releases the model at once rather than when it next speaks. On Windows, where a
shutdown does not wake a blocked read, it is released once the child closes the
connection in turn, which `llama-server` checks for about once a second.

Every refusal happens before anything is forwarded, and is the JSON envelope
an OpenAI-compatible client already parses:

```json
{"error":{"message":"no model called 'nowhere'; this catalog carries: gemma3","type":"invalid_request_error","code":"model_not_found"}}
```

The status and the `code` are fixed by the cause, so a program switches on
those and never on prose; the `message` is for the reader and may be reworded.
`type` is `invalid_request_error` for a `4xx` and `server_error` for a `5xx`.
`Retry-After` is sent exactly when waiting changes the answer.

| Cause | Status | `code` |
| --- | --- | --- |
| the head cannot be read as a request | `400` | `malformed_request` |
| the head is larger than the router will read | `431` | `request_head_too_large` |
| `MAESTRO_API_KEY` is set and the request carries no key, or another | `401`, with `WWW-Authenticate: Bearer` | `invalid_api_key` |
| `MAESTRO_ALLOWED_ORIGINS` is set and does not list the request's origin | `403` | `origin_not_allowed` |
| the request head did not arrive within a minute | `408` | `request_timeout` |
| the path is no shape the router serves | `404` | `path_not_found` |
| the path or body names no entry the catalog carries | `404`, listing what it does carry | `model_not_found` |
| the method is none a model is asked anything with | `405`, with `Allow` | `method_not_allowed` |
| the request body announces chunked framing | `501`; send a body with a `Content-Length` | `chunked_body_not_implemented` |
| the `Content-Length` will not parse | `400`, quoting back what arrived | `malformed_content_length` |
| the generic endpoint is sent no declared body | `411`, naming the header it wanted | `content_length_required` |
| the body is larger than the router will read | `413`, naming both sizes | `body_too_large` |
| the body ends before its declared length | `400` | `body_incomplete` |
| the body is not JSON | `400` | `body_not_json` |
| the body names no model, on the generic endpoint | `400`, saying which endpoint needs none | `model_missing` |
| the child cannot be started | `502`, with the reason from the launcher | `child_unavailable` |
| the child misses its startup budget | `504`, naming the budget | `startup_timeout` |
| the room is held by a request that reached it first | `503`, with `Retry-After` | `room_contended` |
| nothing can be unloaded to make room | `503`, naming what is holding the memory | `insufficient_room` |
| an unload names a model that is answering a request | `409` | `model_busy` |

Once a response has begun there is no status left to send, so a failure after
that point closes the connection rather than pretending it can still answer.

## 🛠️ Local commands

```sh
just install    # the toolchain and the gate tools
just setup      # wire the local hooks
just check      # the quality commands rust-workflows runs in CI, run here
just serving    # what the router is holding, before interrupting it
just deploy     # build HEAD, install it, restart once nothing is in flight
```

`just` alone lists every recipe, with what it does.

## 📚 Documentation

- [Model router design](docs/superpowers/specs/2026-09-03-model-router-design.md):
  the purpose, the architecture, the catalog and the six slices it ships in
- Implementation plans:
  - [Bootstrap and catalog](docs/superpowers/plans/2026-09-03-bootstrap-and-catalog.md):
    the bootstrap and the first slice
  - [Process supervision](docs/superpowers/plans/2026-09-03-process-supervision.md):
    the second slice
  - [Dedicated endpoint proxy](docs/superpowers/plans/2026-09-03-dedicated-endpoint-proxy.md):
    the third slice, and the measurement behind its one hard decision
  - [Generic endpoint and eviction](docs/superpowers/plans/2026-09-03-generic-endpoint-and-eviction.md):
    the fourth slice, and the five design problems it had to settle first
  - [Residency](docs/superpowers/plans/2026-09-03-residency.md): the fifth
    slice
  - [Idle unload](docs/superpowers/plans/2026-09-04-idle-unload.md): the idle
    window and the reaper
  - [Estimates for entries that never touch the device](docs/superpowers/plans/2026-09-13-processor-pinned-estimates.md):
    what the estimate charges a model kept on the processor
  - [The router-mode surface says what it serves](docs/superpowers/plans/2026-09-13-router-mode-declares-what-it-serves.md):
    what a llama.cpp client in router mode reads from `/models`
  - [The small Qwens go to the device, and the retrieval pair get a rate](docs/superpowers/plans/2026-09-13-small-qwens-on-the-device.md):
    why `qwen3-4b` and `qwen3-06b` left the processor
- [ADR 0001](docs/adr/0001-one-crate-until-a-seam-is-real.md): why this is one
  crate
- [ADR 0002](docs/adr/0002-maestro-model-router-in-orchestration-maestro.md):
  the move to Orchestration-Maestro, the new name, and the standards it brought
- [Northstar](docs/standards/northstar.md),
  [engineering rules](docs/standards/engineering.md) and
  [security rules](docs/standards/security.md): how the organization's golden
  rules hold in this repository
- [Domain glossary](CONTEXT.md): what each name in the router means
- [Changelog](CHANGELOG.md): what each pull request changed
- [Agent instructions](AGENTS.md): how to work in this repository
- [Banner credits](.github/assets/CREDITS.md): how the banner artwork was made
- [LICENSE](LICENSE): MIT
