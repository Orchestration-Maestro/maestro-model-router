//! The catalog: which models this router can serve, and how each is launched.
//!
//! This module began as the whole of slice 1, and its interface is still
//! small: parse text, get a `Catalog` back or a report naming everything
//! wrong with it; or read the same text against a models root, and get back
//! the catalog with every estimate settled and every model file the root
//! carries beside it. How TOML is walked, how an entry inherits the defaults
//! table, how a location is refused, how an estimate is derived and how a
//! file becomes an entry are implementation and stay inside.
//!
//! Two properties are part of the interface rather than the implementation,
//! because a caller cannot use the module correctly without knowing them.
//!
//! A report names every problem, not the first. A catalog with five mistakes
//! is fixed in one pass rather than five, which is the difference between a
//! tool people run and one they work around.
//!
//! Every problem names the entry it came from and the field that caused it.
//! An error reading "invalid catalog" sends the reader back to the file to
//! guess, which is the failure this design exists to avoid.

mod capability;
mod discover;
mod entry;
mod estimate;
mod field;
mod listing;
mod path;
mod read;
mod report;
mod resolve;

pub use entry::{Entry, Residency};
pub use listing::{Catalog, EstimateSource};
pub use path::RelativePath;
pub use report::Report;
pub use resolve::Reading;
