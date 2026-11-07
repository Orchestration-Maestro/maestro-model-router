//! The commit hooks run only tools this repository installs.
//!
//! `just setup` is the whole of a clone's preparation: it installs what
//! `mise.toml` pins and then installs the hooks. A hook that fetches a tool of
//! its own, or calls a recipe this justfile does not define, therefore fails on
//! every commit, and the failure arrives on the commit rather than on the
//! change. That is how a tool pinned at a release that was never published
//! broke every commit here, so the three files are read together and each
//! disagreement is refused.

use std::fs;
use std::path::Path;

use crate::common::repo_root;

/// The tools the shared continuous integration requires a caller to pin.
const REQUIRED_PINS: &[&str] = &["just", "prek"];

/// Hook repositories that fetch nothing: their tools come from the PATH.
const LOCAL_REPOS: &[&str] = &["builtin", "local"];

/// Read one file of the repository, naming it if it cannot be read.
fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The first word of every kept line, which is how a TOML table names a tool
/// and a justfile names a recipe.
fn leading_words<'a>(
    lines: impl Iterator<Item = &'a str>,
    keep: impl Fn(&str) -> bool,
) -> Vec<String> {
    lines
        .filter(|line| keep(line))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let word = line.split([' ', '=']).next().unwrap_or_default();
            word.trim_matches('"').trim_end_matches(':').to_owned()
        })
        .collect()
}

/// The names `mise.toml` pins under `[tools]`, the section `mise install`
/// reads.
fn pinned_tools(mise: &str) -> Vec<String> {
    let tools = mise
        .lines()
        .skip_while(|line| line.trim() != "[tools]")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['));
    leading_words(tools, |_| true)
}

/// The recipes a justfile defines: a name at the start of a line, then its
/// arguments and a colon.
fn recipes(justfile: &str) -> Vec<String> {
    leading_words(justfile.lines(), |line| !line.starts_with([' ', '\t']))
}

/// Every hook that installs a tool of its own, with the line that does it.
///
/// A hook runs a tool from the PATH, or it fetches one. Only the first keeps
/// one pin, in `mise.toml`, for the whole repository.
fn fetched_tools(hooks: &str) -> Vec<String> {
    let mut fetched = Vec::new();
    for (number, line) in hooks.lines().enumerate() {
        let line = line.trim();
        if line.starts_with("additional_dependencies") {
            fetched.push(format!("line {}: {}", number + 1, line));
        }
        if let Some(repo) = line.strip_prefix("- repo:")
            && !LOCAL_REPOS.contains(&repo.trim())
        {
            fetched.push(format!("line {}: fetches {repo}", number + 1));
        }
    }
    fetched
}

/// Every hook that runs a `just` recipe the justfile does not define.
fn undefined_recipes(hooks: &str, justfile: &str) -> Vec<String> {
    let defined = recipes(justfile);
    let mut missing = Vec::new();
    for (number, line) in hooks.lines().enumerate() {
        let Some(command) = line.trim().strip_prefix("entry:") else {
            continue;
        };
        let mut words = command.split_whitespace();
        if words.next() != Some("just") {
            continue;
        }
        // `just`'s own options and subcommands (`just --fmt`) name no recipe.
        if let Some(recipe) = words.find(|word| !word.starts_with('-'))
            && !defined.iter().any(|defined| defined == recipe)
        {
            missing.push(format!("line {}: just {recipe}", number + 1));
        }
    }
    missing
}

#[test]
fn hooks_run_only_tools_this_repository_installs() {
    let root = repo_root();
    let hooks = read(&root.join(".pre-commit-config.yaml"));
    let mise = read(&root.join("mise.toml"));
    let justfile = read(&root.join("justfile"));

    let pinned = pinned_tools(&mise);
    let unpinned: Vec<_> = REQUIRED_PINS
        .iter()
        .filter(|tool| !pinned.iter().any(|name| name == *tool))
        .map(|tool| format!("mise.toml pins no {tool}"))
        .collect();

    let mut refused = fetched_tools(&hooks);
    refused.extend(unpinned);
    refused.extend(undefined_recipes(&hooks, &justfile));
    assert!(
        refused.is_empty(),
        "A hook must run a tool `mise.toml` pins and a recipe this justfile \
         defines, or `just setup` cannot make a commit pass:\n  {}\n",
        refused.join("\n  ")
    );
}
