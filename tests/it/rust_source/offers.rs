//! What a door offers the rest of the crate, and who uses it.

use crate::rust_source::tree::{Module, Reads, Tree};

/// What a door offers beyond its parent.
#[derive(Debug)]
pub(crate) struct Offer {
    /// The name a caller writes after the door.
    pub(crate) name: String,
    /// The line of the door that offers it.
    pub(crate) line: usize,
    /// Its path through the door.
    through: Vec<String>,
    /// Its path where it is defined.
    defined: Vec<String>,
}

/// The modules that use an offer.
#[derive(Debug)]
pub(crate) struct Callers<'a> {
    /// Those outside the door, the crate root left out.
    pub(crate) outside: Vec<&'a Module>,
    /// Those inside it, besides the door and the module that defines the offer.
    pub(crate) inside: Vec<&'a Module>,
}

impl Tree {
    /// The doors of the tree: its `mod.rs` modules.
    pub(crate) fn doors(&self) -> impl Iterator<Item = &Module> {
        self.modules.iter().filter(|module| module.is_door())
    }

    /// The modules a door declares and the names it re-exports, each with a
    /// visibility that reaches past the door's parent.
    pub(crate) fn offers(&self, door: &Module) -> Vec<Offer> {
        let through = |name: &str| [door.path.clone(), vec![name.to_owned()]].concat();
        let mut found: Vec<Offer> = door
            .parsed
            .children
            .iter()
            .filter(|child| child.offered)
            .map(|child| Offer {
                name: child.name.clone(),
                line: child.line,
                through: through(&child.name),
                defined: through(&child.name),
            })
            .collect();
        for export in &door.parsed.named {
            let (true, false, Some(name)) = (export.offered, export.glob, &export.binding) else {
                continue;
            };
            if let Some(defined) = self.absolute(&door.path, &export.segments) {
                let line = door
                    .parsed
                    .tokens
                    .get(export.at)
                    .map_or(0, |token| token.line);
                found.push(Offer {
                    name: name.clone(),
                    line,
                    through: through(name),
                    defined,
                });
            }
        }
        found
    }

    /// The modules that use `offer`. The crate root, the door itself and the
    /// module that defines the offer, with everything below it, are left out.
    pub(crate) fn callers(&self, door: &Module, offer: &Offer) -> Callers<'_> {
        let defining = self
            .landing(&offer.defined)
            .map(|module| module.path.clone())
            .unwrap_or_default();
        let mut callers = Callers {
            outside: Vec::new(),
            inside: Vec::new(),
        };
        for module in &self.modules {
            let skipped = module.path.is_empty()
                || module.path == door.path
                || module.path.starts_with(&defining);
            if skipped || !uses(&module.reads, offer) {
                continue;
            }
            if module.path.starts_with(&door.path) {
                callers.inside.push(module);
            } else {
                callers.outside.push(module);
            }
        }
        callers
    }
}

/// Whether what a module reads names the offer, through the door or where it
/// is defined, or globs in the module that holds it.
fn uses(read: &Reads, offer: &Offer) -> bool {
    let held_by =
        |glob: &[String], path: &[String]| path.len() == glob.len() + 1 && path.starts_with(glob);
    read.paths
        .iter()
        .any(|path| path.starts_with(&offer.through) || path.starts_with(&offer.defined))
        || read
            .globs
            .iter()
            .any(|glob| held_by(glob, &offer.through) || held_by(glob, &offer.defined))
}
