//! Reading a split model's shard suffix, and naming its other shards.
//!
//! Split from the door above it when `estimate.rs` became `estimate/mod.rs`,
//! which may only declare. The estimate sums every shard of a split model, and
//! discovery names the entry after the first, so both read the suffix here.

use std::path::{Path, PathBuf};

/// One shard's place in a split model, read from a name such as
/// `model-00001-of-00004.gguf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::catalog) struct Shard<'a> {
    /// The name before the shard suffix, which names the model.
    pub stem: &'a str,
    pub index: u64,
    total: &'a str,
    extension: &'a str,
}

impl Shard<'_> {
    /// The path of another shard of the same model, beside this one.
    pub(super) fn sibling(&self, model: &Path, index: u64) -> PathBuf {
        let width = self.total.len();
        model.with_file_name(format!(
            "{}-{index:0width$}-of-{}.{}",
            self.stem, self.total, self.extension
        ))
    }
}

/// Reads a shard suffix off a file name, if it carries one.
pub(in crate::catalog) fn shard_of(name: &str) -> Option<Shard<'_>> {
    let (rest, extension) = name.rsplit_once('.')?;
    let (head, total) = rest.rsplit_once("-of-")?;
    let (stem, index) = head.rsplit_once('-')?;
    if total.is_empty() || !total.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if index.len() != total.len() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(Shard {
        stem,
        index: index.parse().ok()?,
        total,
        extension,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shard_suffix_is_read_and_its_siblings_named() {
        let shard = shard_of("Big-Model-00002-of-00004.gguf").expect("a shard");
        assert_eq!(shard.stem, "Big-Model");
        assert_eq!(shard.index, 2);
        assert_eq!(
            shard.sibling(Path::new("x/Big-Model-00002-of-00004.gguf"), 4),
            Path::new("x").join("Big-Model-00004-of-00004.gguf")
        );
        assert_eq!(shard_of("model.gguf"), None, "no suffix");
        assert_eq!(shard_of("model-1-of-x.gguf"), None, "not digits");
        assert_eq!(shard_of("model-1-of-04.gguf"), None, "widths differ");
    }
}
