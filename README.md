# maestro-model-router

[![CI](https://github.com/Orchestration-Maestro/maestro-model-router/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Orchestration-Maestro/maestro-model-router/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/Orchestration-Maestro/maestro-model-router/badge)](https://scorecard.dev/viewer/?uri=github.com/Orchestration-Maestro/maestro-model-router)

maestro-model-router is Orchestration-Maestro's model router, and its command
is `model-router`. It supervises llama.cpp server processes and exposes one
OpenAI-compatible endpoint per model plus a generic routing endpoint.

Status: the fifth slice. The router reads and validates a catalog, takes one
entry from it as far as a running `llama-server` on a loopback port, and serves
both a dedicated endpoint per model and a generic endpoint that routes by the
model a request body names, relaying a streamed reply to the caller as it
arrives. It holds models within a configured memory budget, unloading an idle
one to make room for another, and holds the resident entries loaded from the
moment it starts serving.

## Checking a catalog

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

## Launching one model

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
and answers a different question -- how long unused memory may be held. A
machine with no budget still wants its memory back: an on-demand model nothing
has asked for in longer than the window is unloaded, its endpoint stays up,
and the next request for it loads it again. A resident is never a candidate,
whatever the window.

A model is held for **at most one and a half windows plus one sweep**,
measured from when a request last *finished* rather than when it started --
otherwise a generation longer than the window would be unloaded the instant it
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
reading releases the model it was holding -- the router watches for a caller
that leaves, and this is the one that stays and does nothing. At most 256
connections are answered at once; more wait in the operating system's backlog
until one ends.

### Who may use it

| Source | Value |
| --- | --- |
| `MAESTRO_API_KEY` | a key every request but a preflight carries as `Authorization: Bearer <key>`, when set and not empty |
| `MAESTRO_ALLOWED_ORIGINS` | the browser origins that may call the router, separated by commas, when set and not empty |
| otherwise | open to whoever reaches an address it listens on |

A request naming an origin not on the list is refused before anything else
happens; a caller that is no browser names none and is unaffected. The router
says at startup which rules it runs under, and never prints the key.

## Serving

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
resident **plus** the largest entry expected to run next to it -- and an
operator who learns that from a refusal under load learns it too late.

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

Each address names an interface. A wildcard -- `0.0.0.0` or `::` -- is
refused: it means every interface the machine has now and every one it gains
later, which is a reach nothing stated, and blanket-serving a network is the
security design this repository has not written. Naming `192.168.140.1` says
which network, and whether anything can reach it is then a matter of routes
and firewall rules rather than of what this router assumed.

Giving the lab-facing address its own port is a convention rather than a
requirement -- different addresses do not collide on the same port -- but a
distinct port tells an operator reading `ss -tlnp`, a log line or a firewall
rule which surface a request arrived on without having to read the address.

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

Only `GET` and `POST` reach a child. A preflight (`OPTIONS`) is answered by
the router itself -- `204`, an `Allow` header, and permissive
`Access-Control-Allow-*` headers, which are safe for the loopback address and
as safe as the network is for any other -- and never starts a model. With
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
larger of the two afterwards -- so a model that turns out to hold four times
its estimate is counted at what it holds from the moment that is known, and
the operator reads it on the line the load prints:

```text
qwen3-06b: loading, estimated at 1024 MiB
qwen3-06b: ready in 5.4 s, measured 4.5 GiB resident and 0.7 GiB on the device (catalog said 1024 MiB)
```

Where the driver reports nothing per process -- WSL's does not -- the device
figure is how far the device's free memory fell while the model loaded. Loads
are admitted one at a time, so nothing else the router starts moves it
meanwhile; anything else on the machine that allocates at that moment is
counted too, which errs toward counting a model high.

The device is the second question, asked at the moment of the decision for
what it has free right now. That counts everything on the machine, not only
what this router loaded, so a desktop that grew since the budget was set is
room the ledger still believes in and the device no longer has. The device can
ask for more to be unloaded than the budget would, and refuses -- naming what
was needed against what was free -- when unloading every idle model would
still not make the room. A model whose flags keep every layer off the device
is not held to the device's room, which is the one flag this router reads
rather than passes through: the resident entry in the shipped catalog lives on
the processor beside a large model that fills the device, and holding it to
the device's room would refuse the arrangement it was measured in. Where the
machine cannot be asked, the device question is not asked, and the budget
decides alone.

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
and is refused if the room is still held then. That is checked at the moment a model is taken
out, not only when the decision is made: a request can arrive in between, and
emptying the slot then would leave a process running that the budget no longer
counts.

Nothing is unloaded for a start that could not have happened anyway. A model
file the models root does not carry is found before the decision, so a stale
path does not cost the operator a warm model as well as the one they asked
for. A start that fails only by being attempted -- a startup budget expiring,
a model costing more than its estimate -- cannot be prevented this way, and
the room is already gone when it does.

**A signalled router stops its children before it goes.** `serve` runs until
the process is asked to end, and that end is a signal: `SIGTERM` -- what
`systemctl stop`, `kill` and a container stop all send -- as well as `SIGINT`
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
outright -- `SIGKILL`, `taskkill /F`, an out-of-memory kill -- reaches no
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
  The budget is never overcommitted by this -- the router ends under-loaded
  rather than over -- which is what makes it a cost rather than a defect.
- The tests read "this child stopped" from its port going quiet. Nothing stops
  a later child binding that same ephemeral port, which would fail while
  blaming the invariant rather than the coincidence.
- Taking a slot drops the child inside the slot's own guard, so the kill and
  the reaping run under it. A child that will not die holds that guard, and
  with it every admission.

**A stream is passed through as it arrives.** The router has no HTTP
dependency: it reads the request head -- the request line and the headers --
rewrites it, and copies the response back without interpreting a byte of it. A
proxy that re-frames a response is a proxy that can buffer it; one that copies
bytes cannot, which makes token-by-token delivery a property of the design
rather than a setting to get right. The request is the exception, and only on
the generic endpoint: the model is inside the body, so the body is read. What
is forwarded is still the caller's own bytes.

A caller that hangs up closes the connection to the child, which is how
`llama-server` is told to stop generating -- mid-answer, and while the model is
still silent: reading a long prompt, or finishing a reply it does not stream.
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

Once a response has begun there is no status left to send, so a failure after
that point closes the connection rather than pretending it can still answer.

## Documents

- [Model router design](docs/superpowers/specs/2026-09-03-model-router-design.md)
  -- the architecture, the catalog, and the six slices it ships in.
- [Bootstrap and catalog plan](docs/superpowers/plans/2026-09-03-bootstrap-and-catalog.md)
  -- this bootstrap, and the first slice.
- [Process supervision plan](docs/superpowers/plans/2026-09-03-process-supervision.md)
  -- the second slice.
- [Dedicated endpoint proxy plan](docs/superpowers/plans/2026-09-03-dedicated-endpoint-proxy.md)
  -- the third slice, and the measurement behind its one hard decision.
- [Generic endpoint and eviction plan](docs/superpowers/plans/2026-09-03-generic-endpoint-and-eviction.md)
  -- the fourth slice, and the five design problems it had to settle first.
- [ADR 0001](docs/adr/0001-one-crate-until-a-seam-is-real.md) -- why this is
  one crate.
- [ADR 0002](docs/adr/0002-maestro-model-router-in-orchestration-maestro.md)
  -- the move to Orchestration-Maestro, the new name, and the standards it
  brought.
- [AGENTS.md](AGENTS.md) -- how to work in this repository.

## Local commands

```sh
just install    # the toolchain and the gate tools
just setup      # wire the local hooks
just check      # the quality commands rust-workflows runs in CI, run here
just deploy     # build HEAD, install it, restart once nothing is in flight
```
