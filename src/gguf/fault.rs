//! Why a file could not be read as GGUF metadata.
//!
//! Its own module because both the value reader and the metadata reader
//! raise it, and neither should have to name the other to do so.

use std::error::Error;
use std::fmt;

/// Why a file could not be read as GGUF metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault(pub(super) String);

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for Fault {}
