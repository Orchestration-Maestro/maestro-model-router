//! What one Rust file declares and names: its child modules, its imports and
//! the paths its code walks.

use crate::common::test_start;
use crate::rust_source::lexer::{Token, tokens};

/// A `mod name;` declaration.
#[derive(Debug)]
pub(crate) struct Child {
    /// The module's name.
    pub(crate) name: String,
    /// Whether code outside the declaring module's parent can name it.
    pub(crate) offered: bool,
    /// The line of the declaration.
    pub(crate) line: usize,
}

/// A path written in the file, relative to the module it stands in.
#[derive(Debug)]
pub(crate) struct Named {
    /// The path's segments; an import's trailing `self` is already gone.
    pub(crate) segments: Vec<String>,
    /// The name an import binds, when the path is an import.
    pub(crate) binding: Option<String>,
    /// Whether the import ends in `*`.
    pub(crate) glob: bool,
    /// Whether an import is visible outside its module's parent.
    pub(crate) offered: bool,
    /// The index of the path's first token.
    pub(crate) at: usize,
    /// Whether the path stands in the file's trailing test code.
    pub(crate) in_tests: bool,
}

/// One file, read.
#[derive(Debug)]
pub(crate) struct Parsed {
    /// Every token of the file.
    pub(crate) tokens: Vec<Token>,
    /// The modules it declares.
    pub(crate) children: Vec<Child>,
    /// Imports first, then every other path of two segments or more.
    pub(crate) named: Vec<Named>,
}

/// Read `source`.
pub(crate) fn parse(source: &str) -> Parsed {
    let tokens = tokens(source);
    let test_line = test_start(source).map(|index| index + 1);
    let mut parsed = Parsed {
        children: Vec::new(),
        named: Vec::new(),
        tokens: Vec::new(),
    };
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        let in_tests = test_line.is_some_and(|line| token.line >= line);
        index = match token.text.as_str() {
            "use" => import(&tokens, index, in_tests, &mut parsed.named),
            "mod" => {
                declaration(&tokens, index, &mut parsed.children);
                index + 1
            }
            _ => path(&tokens, index, in_tests, &mut parsed.named),
        };
    }
    parsed.tokens = tokens;
    parsed
}

/// Record `mod name;` at `at`.
fn declaration(tokens: &[Token], at: usize, children: &mut Vec<Child>) {
    if let (Some(name), Some(end)) = (tokens.get(at + 1), tokens.get(at + 2))
        && end.text == ";"
    {
        children.push(Child {
            name: name.text.clone(),
            offered: offered_before(tokens, at),
            line: name.line,
        });
    }
}

/// Whether the visibility before the item at `at` reaches beyond the parent:
/// a bare `pub` or `pub(crate)`, not `pub(super)`, `pub(self)` or `pub(in ..)`.
fn offered_before(tokens: &[Token], at: usize) -> bool {
    let text = |back: usize| {
        at.checked_sub(back)
            .and_then(|index| tokens.get(index))
            .map(|token| token.text.as_str())
    };
    match text(1) {
        Some("pub") => true,
        Some(")") => text(2) == Some("crate") && text(3) == Some("(") && text(4) == Some("pub"),
        _ => false,
    }
}

/// Record the path at `at` when it has two segments or more, and return the
/// index after it.
fn path(tokens: &[Token], at: usize, in_tests: bool, named: &mut Vec<Named>) -> usize {
    let mut segments = Vec::new();
    let mut index = at;
    while let Some(word) = tokens.get(index).filter(|token| is_word(token)) {
        segments.push(word.text.clone());
        if separator_at(tokens, index + 1) {
            index += 3;
        } else {
            index += 1;
            break;
        }
    }
    let continues = at
        .checked_sub(2)
        .is_some_and(|before| separator_at(tokens, before));
    if segments.len() > 1 && !continues {
        named.push(Named {
            segments,
            binding: None,
            glob: false,
            offered: false,
            at,
            in_tests,
        });
    }
    index.max(at + 1)
}

/// Whether a token is a word rather than punctuation.
fn is_word(token: &Token) -> bool {
    token
        .text
        .chars()
        .next()
        .is_some_and(|first| first.is_alphanumeric() || first == '_')
}

/// Whether `::` stands at `at`.
fn separator_at(tokens: &[Token], at: usize) -> bool {
    matches!(
        (tokens.get(at), tokens.get(at + 1)),
        (Some(first), Some(second)) if first.text == ":" && second.text == ":"
    )
}

/// Record the imports of the `use` at `at` and return the index after its `;`.
fn import(tokens: &[Token], at: usize, in_tests: bool, named: &mut Vec<Named>) -> usize {
    let offered = offered_before(tokens, at);
    let mut index = at + 1;
    if separator_at(tokens, index) {
        index += 2;
    }
    let mut found = Vec::new();
    tree(tokens, &mut index, &[], &mut found);
    for (segments, binding, glob) in found {
        named.push(Named {
            segments,
            binding,
            glob,
            offered,
            at,
            in_tests,
        });
    }
    let end = tokens
        .iter()
        .skip(index)
        .position(|token| token.text == ";")
        .map_or(tokens.len(), |distance| index + distance + 1);
    end.max(at + 1)
}

/// One import: its segments, the name it binds and whether it is a glob.
type Leaf = (Vec<String>, Option<String>, bool);

/// Expand the use tree at `index` below `prefix` into its leaves.
fn tree(tokens: &[Token], index: &mut usize, prefix: &[String], found: &mut Vec<Leaf>) {
    let mut segments = prefix.to_vec();
    while let Some(token) = tokens.get(*index) {
        match token.text.as_str() {
            "{" => {
                *index += 1;
                group(tokens, index, &segments, found);
                return;
            }
            "*" => {
                *index += 1;
                found.push((segments, None, true));
                return;
            }
            _ if is_word(token) => {
                segments.push(token.text.clone());
                *index += 1;
                if separator_at(tokens, *index) {
                    *index += 2;
                } else {
                    found.push(leaf(tokens, index, segments));
                    return;
                }
            }
            _ => return,
        }
    }
}

/// Expand the comma-separated trees of a `{ .. }` group.
fn group(tokens: &[Token], index: &mut usize, segments: &[String], found: &mut Vec<Leaf>) {
    while let Some(token) = tokens.get(*index) {
        match token.text.as_str() {
            "}" => {
                *index += 1;
                return;
            }
            "," => *index += 1,
            _ => {
                let before = *index;
                tree(tokens, index, segments, found);
                if *index == before {
                    *index += 1;
                }
            }
        }
    }
}

/// The leaf ending at `index`, following an `as` rename and dropping `self`.
fn leaf(tokens: &[Token], index: &mut usize, mut segments: Vec<String>) -> Leaf {
    let mut binding = segments.last().cloned();
    if tokens.get(*index).is_some_and(|token| token.text == "as")
        && let Some(name) = tokens.get(*index + 1)
    {
        binding = Some(name.text.clone());
        *index += 2;
    }
    if segments.last().is_some_and(|last| last == "self") && segments.len() > 1 {
        segments.pop();
        binding = binding
            .filter(|name| name != "self")
            .or(segments.last().cloned());
    }
    (segments, binding, false)
}

impl Parsed {
    /// The name of the innermost function whose body holds the token at `at`.
    pub(crate) fn function_at(&self, at: usize) -> Option<&str> {
        let mut bodies: Vec<Option<&str>> = Vec::new();
        let mut signature = None;
        for (index, token) in self.tokens.iter().enumerate().take(at) {
            match token.text.as_str() {
                "fn" => {
                    let name = self.tokens.get(index + 1).filter(|next| is_word(next));
                    signature = name.map(|next| next.text.as_str()).or(signature);
                }
                "{" => bodies.push(signature.take()),
                "}" => {
                    bodies.pop();
                }
                ";" => signature = None,
                _ => {}
            }
        }
        bodies.into_iter().rev().flatten().next()
    }
}
