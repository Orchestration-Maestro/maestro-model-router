# Copilot instructions for maestro-model-router

## Start here

maestro-model-router is Orchestration-Maestro's model router, and its command is
`model-router`. It supervises llama.cpp server processes and exposes one
OpenAI-compatible endpoint per model plus a generic routing endpoint.

Paths below are relative to this repository. Before editing, read
[AGENTS.md](../AGENTS.md) for the rules that bind every change,
[CONTEXT.md](../CONTEXT.md) for the words it uses and
[CONTRIBUTING.md](https://github.com/Orchestration-Maestro/.github/blob/main/CONTRIBUTING.md)
for how a change is proposed. The organization's [golden
rules](https://github.com/Orchestration-Maestro/.github/blob/main/golden-rules/engineering.md)
come first: nothing in a specification, a plan or this repository weakens them.

For quality, engineering or security changes, read
[northstar.md](../docs/standards/northstar.md),
[engineering.md](../docs/standards/engineering.md) and
[security.md](../docs/standards/security.md): this repository's map of the
organization's golden rules.

Keep changes scoped to the request, and read historical plans and specifications
as records, not as instructions to start new work.

## Repository tree

Every tracked file, with what it is for. `rust-gate guide` writes this tree at
every commit and keeps each explanation already here, so improve an explanation
in place.

```text
.                                                                  # Repository root
├── .cargo/                                                        # Cargo settings for this workspace
│   └── mutants.toml                                               # What cargo mutants leaves out when rust-workflows mutates a change
├── .github/                                                       # GitHub metadata, templates and workflows
│   ├── assets/                                                    # Images and other assets
│   │   ├── CREDITS.md                                             # Banner credits
│   │   ├── how-it-works.svg                                       # A caller's OpenAI-compatible request goes through the router's route, admission and relay to the model's llama-server on a loopback port
│   │   └── maestro-model-router.jpg                               # Maestro Model Router: load what's asked, free what's idle
│   ├── workflows/                                                 # GitHub Actions workflows
│   │   ├── dependabot-auto-merge.yml                              # Dependabot auto-merge
│   │   ├── eviction-sweep.yml                                     # Eviction sweep
│   │   └── scorecard.yml                                          # OpenSSF Scorecard
│   ├── CODEOWNERS                                                 # Who reviews each path
│   ├── copilot-instructions.md                                    # This guide, written by rust-gate guide at every commit
│   └── dependabot.yml                                             # The organization merges only conventional titles: "ci(deps): bump ..."
├── docs/                                                          # Documentation
│   ├── adr/                                                       # Architecture decision records
│   │   ├── 0001-one-crate-until-a-seam-is-real.md                 # ADR 0001: One crate until a seam is real
│   │   └── 0002-maestro-model-router-in-orchestration-maestro.md  # ADR 0002: maestro-model-router in Orchestration-Maestro
│   ├── standards/                                                 # Standards
│   │   ├── engineering.md                                         # Engineering rules in maestro-model-router
│   │   ├── northstar.md                                           # Northstar for maestro-model-router
│   │   └── security.md                                            # Security rules in maestro-model-router
│   └── superpowers/                                               # Superpowers
│       ├── plans/                                                 # Implementation plans
│       │   ├── 2026-09-03-bootstrap-and-catalog.md                # Bootstrap and catalog implementation plan
│       │   ├── 2026-09-03-dedicated-endpoint-proxy.md             # Dedicated endpoint proxy implementation plan
│       │   ├── 2026-09-03-generic-endpoint-and-eviction.md        # Generic endpoint and eviction implementation plan
│       │   ├── 2026-09-03-process-supervision.md                  # Process supervision implementation plan
│       │   ├── 2026-09-03-residency.md                            # Residency implementation plan
│       │   ├── 2026-09-04-idle-unload.md                          # Idle unload implementation plan
│       │   ├── 2026-09-13-processor-pinned-estimates.md           # Estimates for entries that never touch the device
│       │   ├── 2026-09-13-router-mode-declares-what-it-serves.md  # The router-mode surface says what it serves
│       │   └── 2026-09-13-small-qwens-on-the-device.md            # The small Qwens go to the device, and the retrieval pair get a rate
│       └── specs/                                                 # Specifications, one directory per slice
│           └── 2026-09-03-model-router-design.md                  # Model router design
├── src/                                                           # The crate's sources
│   ├── admission/                                                 # Deciding what may be loaded, and what must be unloaded first
│   │   ├── budget.rs                                              # Where the budget comes from: the environment, the machine, or a test
│   │   ├── decision.rs                                            # The decision itself: whether a wanted entry fits, and what goes first
│   │   ├── mod.rs                                                 # Deciding what may be loaded, and what must be unloaded first
│   │   ├── room.rs                                                # What there is to fit into, and the words for when there is not
│   │   └── subject.rs                                             # What a decision is about: the models held, and the one being asked for
│   ├── bench/                                                     # What an entry actually costs, and how fast it actually runs
│   │   ├── measure.rs                                             # Measuring one entry: start it, read it, stop it
│   │   ├── mod.rs                                                 # What an entry actually costs, and how fast it actually runs
│   │   ├── rate.rs                                                # How fast an entry answers, in the unit that entry answers in
│   │   └── report.rs                                              # Running the measurements, and saying what they mean
│   ├── bin/                                                       # Binaries, one file per executable
│   │   └── stub_llama_server/                                     # Stub llama server
│   │       ├── main.rs                                            # A stand-in for llama-server, so continuous integration can supervise a
│   │       ├── options.rs                                         # What the stub was asked to do, read from its command line
│   │       ├── reply.rs                                           # Answering one request
│   │       └── serve.rs                                           # Running the stub as it was asked to: exiting before the bind, serving
│   ├── catalog/                                                   # The catalog: which models this router can serve, and how each is launched
│   │   ├── discover/                                              # Every model file under the root that the catalog does not name
│   │   │   ├── mod.rs                                             # Every model file under the root that the catalog does not name
│   │   │   ├── name.rs                                            # What a file's name says: whether it is a model on its own, and what the
│   │   │   └── walk.rs                                            # Walking the root for model files, and turning each into an entry
│   │   ├── estimate/                                              # What loading an entry is expected to cost, worked out from its files
│   │   │   ├── cache.rs                                           # What the key-value cache costs at a given context
│   │   │   ├── derived.rs                                         # Summing the four terms into one figure, from the files an entry names
│   │   │   ├── mod.rs                                             # What loading an entry is expected to cost, worked out from its files
│   │   │   ├── served.rs                                          # What the flags say about the way an entry is served
│   │   │   └── shard.rs                                           # Reading a split model's shard suffix, and naming its other shards
│   │   ├── capability.rs                                          # What an entry can be asked for, as against what it costs to hold
│   │   ├── entry.rs                                               # One model the router can serve, and whether it is held loaded
│   │   ├── field.rs                                               # Reading one field, and naming it when it is wrong
│   │   ├── listing.rs                                             # Every model the router can serve, read from text alone
│   │   ├── mod.rs                                                 # The catalog: which models this router can serve, and how each is launched
│   │   ├── path.rs                                                # The path type the catalog is built from
│   │   ├── read.rs                                                # The shape of a catalog: its version, its defaults, and its entries
│   │   ├── report.rs                                              # Everything wrong with one catalog, and how it reads when said
│   │   └── resolve.rs                                             # Reading a catalog against the models root it will be served from
│   ├── gguf/                                                      # What a model file says about itself
│   │   ├── bytes.rs                                               # The format's primitives: fixed-width integers, strings, and stepping over
│   │   ├── fault.rs                                               # Why a file could not be read as GGUF metadata
│   │   ├── metadata.rs                                            # The metadata a model file carries, read into the keys the router keeps
│   │   └── mod.rs                                                 # What a model file says about itself
│   ├── launch/                                                    # Turning one catalog entry into a running server, and stopping it again
│   │   ├── binary.rs                                              # Finding the server binary, and the named builds beside it
│   │   ├── child.rs                                               # One running child, and whether it is still there
│   │   ├── failure.rs                                             # Why a server could not be located, started, or resolved
│   │   ├── invocation.rs                                          # The command line one catalog entry becomes
│   │   ├── mod.rs                                                 # Turning one catalog entry into a running server, and stopping it again
│   │   ├── output.rs                                              # What a child says, passed on and the last of it kept
│   │   ├── probe.rs                                               # Asking a child whether it is ready to answer
│   │   ├── root.rs                                                # Where catalog locations resolve against
│   │   ├── search.rs                                              # Finding a program on the search path, as the operating system would
│   │   └── server.rs                                              # Finding the server binary, and taking one entry as far as a ready child
│   ├── memory/                                                    # What the machine says about its memory, asked at run time
│   │   ├── command.rs                                             # Running the platform's own tools, bounded, and handing back what they
│   │   ├── figures.rs                                             # The figures the machine reports: device memory as a whole, and what one
│   │   ├── mod.rs                                                 # What the machine says about its memory, asked at run time
│   │   ├── parse.rs                                               # Reading numbers out of what the platform's own tools print
│   │   └── probe.rs                                               # Where the machine's own figures come from: a test's fixed numbers, or the
│   ├── proxy/                                                     # Serving every model in the catalog, and relaying each one to a child
│   │   ├── answer/                                                # Answering one connection
│   │   │   ├── connection.rs                                      # What one connection is answered with, decided from its head
│   │   │   ├── mod.rs                                             # Answering one connection
│   │   │   └── own.rs                                             # What a llama.cpp client reads, which the OpenAI listing does not carry
│   │   ├── head/                                                  # The request head: what the router reads of a request, and what it sends on
│   │   │   ├── mod.rs                                             # The request head: what the router reads of a request, and what it sends on
│   │   │   ├── parsed.rs                                          # A request head as the router reads it, and as it sends it on
│   │   │   └── read.rs                                            # Reading a request head off a connection, within bounds
│   │   ├── slots/                                                 # Which child is loaded for which entry, and what has to go to make room
│   │   │   ├── admit.rs                                           # Handing a request its child: the one running, or one started once there
│   │   │   ├── lease.rs                                           # A child handed to a request, and the bell it rings when it is let go
│   │   │   ├── mod.rs                                             # Which child is loaded for which entry, and what has to go to make room
│   │   │   ├── queue.rs                                           # The line of requests waiting for room, kept under the admission lock
│   │   │   ├── room.rs                                            # Making room for a model, and waiting in turn when the room is held
│   │   │   ├── start.rs                                           # Starting one entry's child, measuring what it holds, and saying both
│   │   │   ├── sweep.rs                                           # What the reaper acts through: naming what has gone idle, and stamping what
│   │   │   ├── table.rs                                           # The table itself: which entries have a slot, and how one is reached
│   │   │   └── view.rs                                            # The read-only projections of what is loaded, moved here when slots.rs
│   │   ├── access.rs                                              # Whether a request may be served, under the rules access holds
│   │   ├── body.rs                                                # The request body, and the model it names
│   │   ├── endpoint.rs                                            # Which endpoint a path addressed
│   │   ├── listen.rs                                              # Which addresses this router answers on, and who accepts on each
│   │   ├── loaded.rs                                              # What a loaded child is, and how "somebody is reading from it" is known
│   │   ├── metrics.rs                                             # What the router holds, in the text format Prometheus scrapes
│   │   ├── mod.rs                                                 # Serving every model in the catalog, and relaying each one to a child
│   │   ├── permits.rs                                             # How many connections are answered at once
│   │   ├── reaper.rs                                              # The thread that unloads what the idle window has expired
│   │   ├── refusal.rs                                             # Why a request was refused, and what a client is told about it
│   │   ├── relay.rs                                               # Copying bytes between the caller's connection and the child's
│   │   ├── reply.rs                                               # What the router says in its own voice
│   │   ├── residents.rs                                           # Loading the entries the catalog holds loaded
│   │   ├── router.rs                                              # The type a caller holds: the public listeners, and what serving them shares
│   │   └── shared.rs                                              # What Shared itself answers, apart from either connection handling in
│   ├── access.rs                                                  # Who may use the router
│   ├── arguments.rs                                               # The argument table: which command a command line names, and how what it
│   ├── build.rs                                                   # Which source a binary was built from
│   ├── check.rs                                                   # The check command: reading a catalog and saying whether it is usable
│   ├── idle.rs                                                    # How long an on-demand model may sit unused, and what that expires
│   ├── launching.rs                                               # The launch command, and what every command that starts a child needs
│   ├── lib.rs                                                     # The router's interior, exposed so the tests can link against it
│   ├── main.rs                                                    # The model-router binary: check a catalog, measure its entries, launch
│   ├── queue.rs                                                   # How long a request waits for room before it is refused
│   ├── serving.rs                                                 # The serve command: every entry in a catalog, served until the process is
│   ├── startup.rs                                                 # What the router says about memory before it serves anything
│   └── voice.rs                                                   # Where the router's own lines for its operator go
├── supply-chain/                                                  # cargo-vet audits, configuration and imports
│   ├── audits.toml                                                # cargo-vet audits file
│   ├── config.toml                                                # cargo-vet config file
│   └── imports.lock                                               # The audits cargo-vet imports, locked
├── tests/                                                         # Integration tests
│   └── it/                                                        # It
│       ├── catalog_entries/                                       # The model catalog: the shape of an entry, and what an entry is estimated
│       │   ├── entry_schema.rs                                    # Schema gate for the model catalog
│       │   ├── memory_estimates.rs                                # What the catalog charges an entry, when it declares no estimate or one
│       │   ├── mod.rs                                             # The model catalog: the shape of an entry, and what an entry is estimated
│       │   └── window_estimates.rs                                # What a model whose layers attend only to a window is charged for its
│       ├── common/                                                # Shared by the gates
│       │   ├── mod.rs                                             # Shared by the gates
│       │   └── repository.rs                                      # The repository's files, walked once for every gate that reads them
│       ├── fixtures/                                              # Model files a test writes for itself
│       │   ├── catalog.toml                                       # Golden fixture: the four entries the design specification names, in the shape the parser must accept
│       │   ├── gguf.rs                                            # Synthetic GGUF files, written in the format's own layout
│       │   ├── mod.rs                                             # Model files a test writes for itself
│       │   └── scratch.rs                                         # A scratch directory for a test that writes model files with bytes in them
│       ├── proxy_routing/                                         # Routing both endpoints, through the interface a caller holds
│       │   ├── body_framing.rs                                    # How a request body arrives: the length it declares, the leave it asks
│       │   ├── child_lifecycle.rs                                 # What a caller sees of the child behind its request: one that cannot
│       │   ├── dedicated_endpoint.rs                              # The dedicated endpoint, /models/<id>/..., which names its model in the
│       │   ├── generic_endpoint.rs                                # The generic endpoint, /v1/..., which names its model in the body, and
│       │   └── mod.rs                                             # Routing both endpoints, through the interface a caller holds
│       ├── support/                                               # Shared by the tests that drive real processes
│       │   ├── http.rs                                            # Raw HTTP, written and read by hand
│       │   ├── mod.rs                                             # Shared by the tests that drive real processes
│       │   ├── models.rs                                          # What a router is given to serve: a models root and a catalog naming it
│       │   ├── poll.rs                                            # Waiting on a condition rather than on time
│       │   ├── router.rs                                          # A router serving on a thread of its own, and the ways a test starts one
│       │   ├── spawned.rs                                         # The router binary as a process, for the tests that need what only a
│       │   └── stub.rs                                            # The stub server's binary, which cargo builds beside the router's
│       ├── api_replies.rs                                         # What the router says in its own voice, and how a client is meant to read
│       ├── caller_access.rs                                       # Who may use the router
│       ├── caller_hangup.rs                                       # A caller that hangs up releases the model it was waiting on
│       ├── catalog_reload.rs                                      # Re-reading the catalog without ending the process
│       ├── child_supervision.rs                                   # Supervising one server child, through the interface a caller holds
│       ├── cli_commands.rs                                        # The commands a person types, run the way the binary is run
│       ├── connection_limit.rs                                    # How many callers are answered at once
│       ├── document_links.rs                                      # Link gate
│       ├── duplication_allowlist.rs                               # Copy-paste detection, via similarity-rs (APTED tree edit distance, so it
│       ├── english_only.rs                                        # English-only gate
│       ├── eviction_policy.rs                                     # What the router unloads to make room, and what it refuses to touch
│       ├── gguf_metadata.rs                                       # What the router reads out of a model file
│       ├── idle_unload.rs                                         # Idle unloading, driven through the router rather than through the policy
│       ├── idle_window.rs                                         # The idle window, read from its variable
│       ├── listen_addresses.rs                                    # Where the router listens, and what it refuses to listen on
│       ├── machine_paths.rs                                       # No-machine-paths gate
│       ├── main.rs                                                # The router's integration tests, built as one crate
│       ├── memory_budget.rs                                       # The memory budget, read from its variable
│       ├── model_discovery.rs                                     # Every model file under the models root is servable
│       ├── models_root.rs                                         # Where catalog locations resolve against
│       ├── module_size.rs                                         # Size limits
│       ├── operator_control.rs                                    # Loading and unloading a model on an operator's say-so
│       ├── prometheus_metrics.rs                                  # What the router is holding, in the text format Prometheus scrapes
│       ├── request_queueing.rs                                    # Waiting for room rather than refusing it
│       ├── resident_entries.rs                                    # Residency, driven through the router rather than through the policy
│       ├── router_mode.rs                                         # The llama.cpp router-mode surface, which is what a llama.cpp client speaks
│       ├── runtime_selection.rs                                   # Which build of the server an entry is served from
│       ├── signalled_shutdown.rs                                  # A signalled router ends its children before it goes
│       ├── stalled_callers.rs                                     # A caller that makes no progress is given up on
│       ├── stream_timing.rs                                       # The three properties of the relay that are about time rather than content
│       └── stub_server.rs                                         # The stub server, tested on its own
├── .editorconfig                                                  # Editor settings every repository in this organisation shares
├── .gitattributes                                                 # Repository-wide file handling
├── .gitignore                                                     # Build output, caches and machine-local state
├── .pre-commit-config.yaml                                        # Hook configuration, run by prek (https://github.com/j178/prek) -- the Rust implementation of the pre-commit protocol
├── AGENTS.md                                                      # Rules for coding agents: what to read, what never to weaken, how to verify
├── CHANGELOG.md                                                   # All notable changes are recorded here
├── CONTEXT.md                                                     # The words this repository uses, and the ones it avoids
├── Cargo.lock                                                     # Exact dependency versions, committed so every build resolves the same
├── Cargo.toml                                                     # Crate manifest: Router that supervises llama.cpp server processes and serves one endpoint per model
├── LICENSE                                                        # The licence this repository is distributed under
├── README.md                                                      # maestro-model-router is Orchestration-Maestro's model router, and its command is model-router
├── catalog.toml                                                   # The models this router serves
├── justfile                                                       # Optional convenience task runner (https://just.systems)
├── maestro-quality.toml                                           # The organization's quality rules as this repository shapes them: the inputs its CI caller passes
├── rust-toolchain.toml                                            # The pinned Rust toolchain
└── typos.toml                                                     # The words this repository means, from [typos] words in maestro-quality.toml; rendered by rust-gate sync
```

## Change and verification procedure

1. Read the rules in AGENTS.md that cover the files you change, and keep every
   gate intact: never weaken one to pass.
2. Add an executable regression check for a change in behaviour.
3. The commit hook `rust-gate guide` rewrites this guide when a file is added,
   moved or removed; commit it with the change. The organization's daily drift
   check reports a guide left stale.
4. Run `just check`, and report the commands you actually ran.
5. Commits are signed, with a conventional title; the default branch takes only
   squash-merged pull requests.
