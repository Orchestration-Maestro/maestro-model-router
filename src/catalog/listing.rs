//! Every model the router can serve, read from text alone.
//!
//! Split from the door above it when `catalog.rs` became `catalog/mod.rs`,
//! which may only declare. Reading the same text against a models root is
//! `resolve.rs`; this is the reading a machine holding none of the models can
//! still do.

use std::collections::BTreeSet;

use super::entry::{Entry, Residency};
use super::field;
use super::read;
use super::report::Report;

/// Where an entry's memory estimate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstimateSource {
    /// Written in the catalog, by the entry or by the defaults table.
    Declared,
    /// Worked out from the entry's files, because nothing declared it.
    Derived,
}

/// Every model the router can serve, and the settings they share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    /// The schema version this file was written against.
    pub version: u32,
    /// The entries, ordered by identifier so two reads agree.
    pub entries: Vec<Entry>,
    /// The entries whose estimate was derived from their files.
    ///
    /// Kept beside the entries rather than on them: an entry is what the
    /// router serves, and where a figure came from is a fact about one
    /// reading of it.
    pub(super) derived: BTreeSet<String>,
    /// The entries that came from the models root rather than the text.
    pub(super) discovered: BTreeSet<String>,
}

impl Catalog {
    /// What the entries held loaded reserve, in mebibytes.
    ///
    /// A catalog fact rather than a machine one: it is the sum of what the
    /// resident entries say they cost, with no ceiling anywhere near it. What
    /// that sum means for a particular machine is the budget's business.
    ///
    /// Widened to sum, because estimates that each fit in a `u32` need not sum
    /// into one, and an overflow here would report a reservation of almost
    /// nothing.
    #[must_use]
    pub fn resident_reservation_mib(&self) -> u64 {
        self.entries
            .iter()
            .filter(|entry| entry.residency == Residency::Resident)
            .map(|entry| u64::from(entry.memory_estimate_mib))
            .sum()
    }

    /// Reads a catalog, reporting everything wrong with it.
    ///
    /// # Errors
    ///
    /// Returns a [`Report`] naming every problem found. Text that is not TOML
    /// stops the parse and yields that one problem, because nothing further
    /// can be read; every other failure is collected, so one run of the tool
    /// surfaces one round of mistakes.
    pub fn parse(text: &str) -> Result<Self, Report> {
        let mut drafts = read::drafts(text)?;
        // Text alone cannot derive an estimate; with no root to read the
        // files from, an entry that declares none is refused.
        for id in &drafts.undeclared {
            drafts.problems.push(field::problem(
                &format!("entry '{id}'"),
                "memory_estimate_mib",
                "is required, and no default supplies it",
            ));
        }

        if drafts.problems.is_empty() {
            Ok(Self {
                version: drafts.version.unwrap_or_default(),
                entries: drafts.entries,
                derived: BTreeSet::new(),
                discovered: BTreeSet::new(),
            })
        } else {
            Err(Report {
                problems: drafts.problems,
            })
        }
    }

    /// The entry with this identifier, if the catalog carries one.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Where this entry's estimate came from, if the catalog carries it.
    #[must_use]
    pub fn estimate_source(&self, id: &str) -> Option<EstimateSource> {
        self.entry(id)?;
        Some(if self.derived.contains(id) {
            EstimateSource::Derived
        } else {
            EstimateSource::Declared
        })
    }

    /// Whether this entry was found under the models root rather than
    /// written in the catalog.
    #[must_use]
    pub fn is_discovered(&self, id: &str) -> bool {
        self.discovered.contains(id)
    }
}
