//! The router's integration tests, built as one crate.
//!
//! One crate rather than one per file: each file under `tests/` used to be a
//! crate of its own, linking the library and compiling the shared helpers
//! again, and every helper a crate did not use was dead code there. Folded,
//! the helpers are declared once below and the library is linked once.
//!
//! `support` drives real processes, `common` walks the repository for the
//! prose and size gates, and `fixtures` writes synthetic model files; each
//! test module names the one it needs through `crate::`.
//!
//! The crate is marked `#![cfg(test)]`, which changes nothing about what is
//! built: it only exists under `cargo test`. It tells Clippy what the compiler
//! already knows. A helper outside a `#[test]` function is test code too, and
//! the allowances `clippy.toml` makes for tests -- an `expect` whose message
//! is the failure report, a `panic!` that names the state it found -- are
//! written for exactly these.

#![cfg(test)]

mod common;
mod fixtures;
mod support;

mod api_replies;
mod caller_access;
mod caller_hangup;
mod catalog_entries;
mod catalog_reload;
mod child_supervision;
mod cli_commands;
mod connection_limit;
mod document_links;
mod duplication_allowlist;
mod english_only;
mod eviction_policy;
mod gguf_metadata;
mod idle_unload;
mod idle_window;
mod listen_addresses;
mod machine_paths;
mod memory_budget;
mod model_discovery;
mod models_root;
mod module_size;
mod operator_control;
mod prometheus_metrics;
mod proxy_routing;
mod request_queueing;
mod resident_entries;
mod router_mode;
mod runtime_selection;
mod signalled_shutdown;
mod stalled_callers;
mod stream_timing;
mod stub_server;
