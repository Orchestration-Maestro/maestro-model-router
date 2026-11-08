//! The commit hooks add no tool of their own and call only defined recipes.
//!
//! `just setup` is the whole of a clone's preparation: it installs what
//! `mise.toml` pins and then installs the hooks. A hook that fetches a tool of
//! its own, or calls a recipe this justfile does not define, therefore fails on
//! every commit, and the failure arrives on the commit rather than on the
//! change. That is how a tool pinned at a release that was never published
//! broke every commit here, so the files are read together and each
//! disagreement is refused.
//!
//! The same reading keeps the pins installable and verified: continuous
//! integration installs every pin `mise.toml` declares on each host the caller
//! asks it to build for, so a pin without a download for one of those hosts
//! fails that leg, and a download whose contents go unrecorded is not a pin.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::common::repo_root;

/// The tools the shared continuous integration requires a caller to pin.
const REQUIRED_PINS: &[&str] = &["just", "prek"];

/// The host continuous integration always runs, which the caller never names
/// because every caller has it.
const LINUX_HOST: &str = "linux-x64";

/// The host `mise.lock` records a declared target's build on. The shared
/// workflow names one runner per target, and Apple's two targets share the
/// arm64 runner it runs them on.
const HOST_FOR_TARGET: &[(&str, &str)] = &[
    ("aarch64-apple-darwin", "macos-arm64"),
    ("x86_64-apple-darwin", "macos-arm64"),
    ("aarch64-unknown-linux-gnu", "linux-arm64"),
    ("x86_64-unknown-linux-gnu", LINUX_HOST),
    ("x86_64-pc-windows-msvc", "windows-x64"),
];

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

/// A file of the repository, parsed as TOML, naming it if it is not.
fn table(path: &Path) -> toml::Table {
    let text = read(path);
    toml::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The names `mise.toml` pins under `[tools]`, the section `mise install`
/// reads, beside the operating systems each pin claims: `None` when the pin
/// names none and so claims every host.
fn pins(mise: &toml::Table) -> BTreeMap<String, Option<BTreeSet<String>>> {
    let mut pins = BTreeMap::new();
    let tools = mise.get("tools").and_then(toml::Value::as_table);
    for (name, pin) in tools.into_iter().flatten() {
        let claimed = pin
            .as_table()
            .and_then(|options| options.get("os"))
            .and_then(toml::Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            });
        pins.insert(name.clone(), claimed);
    }
    pins
}

/// One download `mise.lock` records.
struct LockedDownload {
    /// The tool the download installs.
    tool: String,
    /// The platform it runs on, as the lock names it.
    platform: String,
    /// Whether the lock says what the download contains.
    checksummed: bool,
}

/// Every download `mise.lock` records.
fn locked_downloads(lock: &toml::Table) -> Vec<LockedDownload> {
    let mut downloads = Vec::new();
    let tools = lock.get("tools").and_then(toml::Value::as_table);
    for (tool, pins) in tools.into_iter().flatten() {
        for pin in pins.as_array().into_iter().flatten() {
            let entries = pin.as_table().into_iter().flatten();
            let platformed = entries.filter_map(|(key, entry)| {
                let platform = key.strip_prefix("platforms.")?;
                Some((platform.to_owned(), entry.get("checksum").is_some()))
            });
            downloads.extend(platformed.map(|(platform, checksummed)| LockedDownload {
                tool: tool.clone(),
                platform,
                checksummed,
            }));
        }
    }
    downloads
}

/// The hosts continuous integration builds for: the Linux runner every caller
/// has, and the runner named for each target this repository declares.
fn build_hosts(ci: &toml::Table) -> BTreeSet<String> {
    let mut hosts = BTreeSet::from([LINUX_HOST.to_owned()]);
    let targets = ci.get("check_targets").and_then(toml::Value::as_array);
    for target in targets
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
    {
        match HOST_FOR_TARGET.iter().find(|(name, _)| *name == target) {
            Some((_, host)) => {
                hosts.insert((*host).to_owned());
            }
            None => panic!("check_targets names {target}, which has no runner recorded here"),
        }
    }
    hosts
}

/// The operating system a locked platform runs on, which is what a pin's `os`
/// filter names.
fn operating_system(platform: &str) -> &str {
    platform.split('-').next().unwrap_or(platform)
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
fn hooks_add_no_tool_and_call_only_defined_recipes() {
    let root = repo_root();
    let hooks = read(&root.join(".pre-commit-config.yaml"));
    let justfile = read(&root.join("justfile"));

    let pinned: Vec<String> = pins(&table(&root.join("mise.toml"))).into_keys().collect();
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

#[test]
fn every_pin_installs_on_every_host_this_repository_builds_for() {
    let root = repo_root();
    let ci = table(&root.join(".github/ci.toml"));
    let hosts = build_hosts(&ci);
    let pinned = pins(&table(&root.join("mise.toml")));
    let locked = locked_downloads(&table(&root.join("mise.lock")));

    let mut refused = Vec::new();
    for (tool, claimed) in &pinned {
        for host in &hosts {
            let claims_host = claimed
                .as_ref()
                .is_none_or(|systems| systems.contains(operating_system(host)));
            let has_download = locked
                .iter()
                .any(|download| download.tool == *tool && download.platform == *host);
            if claims_host && !has_download {
                refused.push(format!(
                    "mise.toml pins {tool} but mise.lock records no {host} download: \
                     scope the pin with `os = [...]` or pin a release that ships one"
                ));
            }
        }
    }
    assert!(
        refused.is_empty(),
        "Continuous integration installs these pins on every host it builds \
         for, so each one needs a download there:\n  {}\n",
        refused.join("\n  ")
    );
}

#[test]
fn every_locked_download_records_its_contents() {
    let root = repo_root();
    let locked = locked_downloads(&table(&root.join("mise.lock")));

    let refused: Vec<_> = locked
        .iter()
        .filter(|download| !download.checksummed)
        .map(|download| {
            let tool = &download.tool;
            let platform = &download.platform;
            format!("mise.lock records no checksum for {tool} on {platform}")
        })
        .collect();
    assert!(
        refused.is_empty(),
        "A download whose contents go unrecorded is not a pin, because an \
         artifact replaced in place would install unnoticed:\n  {}\n",
        refused.join("\n  ")
    );
}
