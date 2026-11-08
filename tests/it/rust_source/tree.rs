//! The modules of one build target, found from its root file and read
//! through the paths they name.

use std::collections::{BTreeMap, BTreeSet};

use crate::rust_source::parsed::{Parsed, parse};

/// Source texts keyed by their path from the repository root, written with `/`.
pub(crate) type Sources = BTreeMap<String, String>;

/// One module, found through a `mod name;` declaration.
#[derive(Debug)]
pub(crate) struct Module {
    /// The names from the crate root down to this module; empty for the root.
    pub(crate) path: Vec<String>,
    /// Its file, from the repository root.
    pub(crate) file: String,
    /// Whether the module is test code in its whole: under `tests/`, declared
    /// by a module that is, or declared under a `#[cfg(..)]` naming `test`.
    pub(crate) tests: bool,
    /// What the file declares and names.
    pub(crate) parsed: Parsed,
    /// What its code names, as absolute paths.
    pub(crate) reads: Reads,
}

/// What a module names, as absolute paths.
#[derive(Debug, Default)]
pub(crate) struct Reads {
    /// Every path that resolves inside the crate.
    pub(crate) paths: Vec<Vec<String>>,
    /// The targets of its glob imports.
    pub(crate) globs: Vec<Vec<String>>,
}

/// Every module reachable from one root file, the root first.
#[derive(Debug)]
pub(crate) struct Tree {
    /// The modules, the root first.
    pub(crate) modules: Vec<Module>,
}

impl Module {
    /// Whether the module is a door: a `mod.rs` that gathers its children.
    pub(crate) fn is_door(&self) -> bool {
        self.file.ends_with("/mod.rs")
    }
}

impl Tree {
    /// Read the modules below `root`, skipping a declared file that is absent.
    pub(crate) fn load(sources: &Sources, root: &str) -> Self {
        let mut modules = Vec::new();
        let mut pending = vec![(Vec::new(), root.to_owned(), root.starts_with("tests/"))];
        while let Some((path, file, tests)) = pending.pop() {
            let Some(text) = sources.get(&file) else {
                continue;
            };
            let parsed = parse(text);
            let directory = children_directory(&file, path.is_empty());
            let declared = parsed.children.iter().filter_map(|child| {
                let flat = format!("{directory}/{}.rs", child.name);
                let nested = format!("{directory}/{}/mod.rs", child.name);
                let found = [flat, nested]
                    .into_iter()
                    .find(|candidate| sources.contains_key(candidate))?;
                let mut child_path = path.clone();
                child_path.push(child.name.clone());
                Some((child_path, found, tests || child.tests))
            });
            pending.extend(declared);
            modules.push(Module {
                path,
                file,
                tests,
                parsed,
                reads: Reads::default(),
            });
        }
        modules.sort_by(|left, right| left.path.cmp(&right.path));
        let mut tree = Self { modules };
        for index in 0..tree.modules.len() {
            let reads = tree.modules.get(index).map(|module| tree.reads(module));
            if let (Some(reads), Some(module)) = (reads, tree.modules.get_mut(index)) {
                module.reads = reads;
            }
        }
        tree
    }

    /// The module at `path`.
    pub(crate) fn module(&self, path: &[String]) -> Option<&Module> {
        self.modules.iter().find(|module| module.path == path)
    }

    /// The deepest module that holds `path`, the item's own path included.
    pub(crate) fn landing(&self, path: &[String]) -> Option<&Module> {
        (0..=path.len())
            .rev()
            .find_map(|length| self.module(path.get(..length)?))
    }

    /// `segments`, written inside the module at `context`, as a path from the
    /// crate root; `None` when it names something outside the crate.
    pub(crate) fn absolute(&self, context: &[String], segments: &[String]) -> Option<Vec<String>> {
        let (first, mut rest) = segments.split_first()?;
        let mut path = match first.as_str() {
            "crate" => Vec::new(),
            "self" => context.to_vec(),
            "super" => {
                let mut parent = context.to_vec();
                parent.pop()?;
                while rest.first().is_some_and(|next| next == "super") {
                    parent.pop()?;
                    rest = rest.get(1..)?;
                }
                parent
            }
            name => {
                let mut child = context.to_vec();
                child.push(name.to_owned());
                self.module(&child)?;
                return Some([child, rest.to_vec()].concat());
            }
        };
        path.extend(rest.iter().cloned());
        Some(path)
    }

    /// The absolute paths the code of a module names, its trailing test
    /// module left out, and the targets of its glob imports.
    fn reads(&self, module: &Module) -> Reads {
        let code = || module.parsed.named.iter().filter(|named| !named.in_tests);
        let aliases: BTreeMap<&str, Vec<String>> = code()
            .filter(|named| !named.glob)
            .filter_map(|named| {
                let binding = named.binding.as_deref()?;
                Some((binding, self.absolute(&module.path, &named.segments)?))
            })
            .collect();
        let mut reads = Reads::default();
        for named in code() {
            let resolved = match named.segments.split_first() {
                Some((first, rest)) if !named.glob && aliases.contains_key(first.as_str()) => {
                    aliases
                        .get(first.as_str())
                        .map(|target| [target.clone(), rest.to_vec()].concat())
                }
                _ => self.absolute(&module.path, &named.segments),
            };
            match resolved {
                Some(path) if named.glob => reads.globs.push(path),
                Some(path) => reads.paths.push(path),
                None => {}
            }
        }
        reads
    }

    /// The files of every module in the tree.
    pub(crate) fn files(&self) -> BTreeSet<String> {
        self.modules
            .iter()
            .map(|module| module.file.clone())
            .collect()
    }
}

/// The directory holding the files of a module's children.
fn children_directory(file: &str, is_root: bool) -> String {
    if is_root || file.ends_with("/mod.rs") {
        file.rsplit_once('/')
            .map_or(String::new(), |(directory, _)| directory.to_owned())
    } else {
        file.trim_end_matches(".rs").to_owned()
    }
}
