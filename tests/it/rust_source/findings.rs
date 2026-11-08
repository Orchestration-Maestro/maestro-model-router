//! What a convention check reports, and the one file that may excuse it.
//!
//! `maestro-quality.toml` lists each excused finding as an `[[exception]]`
//! with its rule, the file, the item and the reason it stays. A finding with
//! no exception fails, and an exception that excuses nothing fails too, so
//! the list cannot outlive the code it excused.

use std::fs;

use crate::common::repo_root;

/// One place a convention is broken.
#[derive(Debug)]
pub(crate) struct Finding {
    /// The rule's identifier, as `maestro-quality.toml` names it.
    pub(crate) rule: &'static str,
    /// The file, from the repository root, written with `/`.
    pub(crate) path: String,
    /// The line, counting from 1.
    pub(crate) line: usize,
    /// The function or item the finding is about, or empty.
    pub(crate) item: String,
    /// What is wrong and what to do about it.
    pub(crate) message: String,
}

/// One entry of `maestro-quality.toml`.
#[derive(Debug)]
struct Exception {
    /// The rule it excuses.
    rule: String,
    /// The file it excuses a finding in.
    path: String,
    /// The item it excuses.
    item: String,
}

/// The reviewed exceptions.
#[derive(Debug)]
pub(crate) struct Exceptions(Vec<Exception>);

impl Exceptions {
    /// Read the exceptions of a `maestro-quality.toml` text. Every entry
    /// needs a rule, a path, an item and a reason.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let table: toml::Table = text.parse().map_err(|fault| format!("not TOML: {fault}"))?;
        let Some(entries) = table.get("exception") else {
            return Ok(Self(Vec::new()));
        };
        let entries = entries
            .as_array()
            .ok_or("`exception` is not an array of tables")?;
        let mut found = Vec::new();
        for (number, entry) in entries.iter().enumerate() {
            let field = |name: &str| {
                entry
                    .get(name)
                    .and_then(toml::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| format!("exception {} has no {name}", number + 1))
            };
            field("reason")?;
            found.push(Exception {
                rule: field("rule")?,
                path: field("path")?,
                item: field("item")?,
            });
        }
        Ok(Self(found))
    }

    /// The exceptions recorded in the repository.
    pub(crate) fn repository() -> Self {
        let text = fs::read_to_string(repo_root().join("maestro-quality.toml"))
            .expect("maestro-quality.toml is readable");
        Self::parse(&text).expect("maestro-quality.toml lists well-formed exceptions")
    }

    /// Whether an exception excuses `finding`.
    fn excuses(&self, finding: &Finding) -> bool {
        self.0.iter().any(|exception| {
            exception.rule == finding.rule
                && exception.path == finding.path
                && exception.item == finding.item
        })
    }
}

/// The failures of `rule`: each finding no exception excuses, and each
/// exception for `rule` that excuses no finding.
pub(crate) fn unsettled(rule: &str, findings: &[Finding], exceptions: &Exceptions) -> Vec<String> {
    let mut failures: Vec<String> = findings
        .iter()
        .filter(|finding| !exceptions.excuses(finding))
        .map(|finding| {
            format!(
                "{} {}:{}: {}",
                finding.rule, finding.path, finding.line, finding.message
            )
        })
        .collect();
    for exception in exceptions.0.iter().filter(|entry| entry.rule == rule) {
        let used = findings
            .iter()
            .any(|finding| finding.path == exception.path && finding.item == exception.item);
        if !used {
            failures.push(format!(
                "{rule} {}: the exception for `{}` excuses nothing any more; remove it from \
                 maestro-quality.toml",
                exception.path, exception.item
            ));
        }
    }
    failures
}
