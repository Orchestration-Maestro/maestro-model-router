//! Shared by the gates. Kept here so they do not each carry their own copy of
//! the walk -- which the duplication gate would rightly flag.

mod repository;

pub(crate) use repository::{has_extension, repo_root, sources};
