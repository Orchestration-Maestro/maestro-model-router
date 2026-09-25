//! Which child is loaded for which entry, and what has to go to make room.
//!
//! One type, holding the part of serving that is about memory rather than
//! about HTTP. A caller asks for the child that serves an entry and gets one,
//! or gets told why not; whether that meant finding a running process,
//! starting one, or ending somebody else's first is this module's business
//! and nothing else's.
//!
//! The policy itself is not here. `admission` decides what may be loaded from
//! a handful of values and no machine at all; this acts on that decision,
//! which is the half that kills processes.
//!
//! A door: the type is defined in `table`, beside the rules of the table it
//! holds, and each child adds the methods of one concern to it. A child names
//! a sibling by the module that defines it, never through this file.

mod admit;
mod lease;
mod queue;
mod room;
mod start;
mod sweep;
mod table;
mod view;

pub(super) use admit::Asked;
pub(in crate::proxy) use lease::Lease;
pub(super) use table::Slots;
