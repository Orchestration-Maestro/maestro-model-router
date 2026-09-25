//! The request head: what the router reads of a request, and what it sends on.
//!
//! Pure translation, in `parsed`. Lines in, a parsed head out, and a rewritten
//! head back to bytes -- no sockets, no processes, no clock. That is what lets
//! every rule there be asserted directly rather than inferred from a running
//! relay. The one step that does touch a socket, reading the lines within
//! bounds, is `read`. Both are re-exported from here so a caller asks one
//! module for a head.
//!
//! This is the only part of a request the router understands. Everything after
//! the blank line is copied without being read, which is the decision the
//! whole slice rests on: what the router does not parse, it cannot buffer.

mod parsed;
mod read;

pub(super) use parsed::{AllowedRoom, Head, Length, ROOM, parse};
pub(super) use read::{read, timed_out};
