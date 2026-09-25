//! Everything wrong with one catalog, and how it reads when said.
//!
//! Split from the door above it when `catalog.rs` became `catalog/mod.rs`,
//! which may only declare. Why a report names every problem rather than the
//! first is said there, because it is part of the interface.

use std::error::Error;
use std::fmt;

/// Everything wrong with one catalog, gathered in a single pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The problems, in the order they were found.
    pub(super) problems: Vec<String>,
}

impl Report {
    /// A report carrying one problem, for failures that stop the parse.
    pub(crate) fn single(problem: String) -> Self {
        Self {
            problems: vec![problem],
        }
    }

    /// The problems, in the order they were found.
    #[must_use]
    pub fn problems(&self) -> &[String] {
        &self.problems
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, problem) in self.problems.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{problem}")?;
        }
        Ok(())
    }
}

impl Error for Report {}
