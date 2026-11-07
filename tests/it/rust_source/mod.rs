//! Rust source read as modules and paths, for the conventions checks.
//!
//! Only what the checks need and no more: files are cut into tokens, the
//! modules of a build target are found from its root file, and each module's
//! paths resolve to absolute ones. Imports, aliases and globs are followed;
//! macro-generated paths and trait method calls are not, which is the accepted
//! limit of reading source without compiling it.

mod files;
mod findings;
mod lexer;
mod offers;
mod parsed;
mod tree;

pub(crate) use files::{ROOTS, repository};
pub(crate) use findings::{Exceptions, Finding, unsettled};
pub(crate) use parsed::parse;
pub(crate) use tree::{Sources, Tree};
