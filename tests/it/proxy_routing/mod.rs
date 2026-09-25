//! Routing both endpoints, through the interface a caller holds.
//!
//! Everything the router does here is real: it binds a port, accepts a
//! connection, parses a head, starts a child, opens a socket to it and copies
//! bytes. The stub stands in for `llama-server` at the same seam slice 2 put
//! it, so the only thing avoided is a multi-gigabyte model and a graphics
//! card.
//!
//! The properties that are about time rather than content live in
//! `stream_timing.rs`. A failure here means routing; a failure there means the
//! relay.
//!
//! Split by what varies: where the model is named (the path or the body), how
//! the body arrives, and what becomes of the child behind a request.

mod body_framing;
mod child_lifecycle;
mod dedicated_endpoint;
mod generic_endpoint;
