//! Answering one connection.
//!
//! Split from the module beside it when that file grew past the module-size
//! gate, along the seam the gate exposed: `router` carries the type a caller
//! holds, and this carries what one connection is answered with. What that
//! answer looks like on the wire is `reply`'s business; this decides which
//! answer a request has earned.
//!
//! Everything here happens before a byte of a child's response has been
//! forwarded, which is what makes a status still possible. Once the relay
//! starts, it does not come back here.

mod connection;
mod own;

pub(super) use connection::to;
