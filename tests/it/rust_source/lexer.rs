//! Rust source cut into words and punctuation, with comments and literals
//! gone.
//!
//! The conventions checks read code, not prose: a `sleep` in a comment or a
//! string says nothing about the program. Blanking keeps every newline, so a
//! token's line is its line in the file.

use std::iter::{once, repeat_n};

/// One word (an identifier, keyword or number) or one punctuation character.
#[derive(Debug)]
pub(crate) struct Token {
    /// The word, or the single punctuation character.
    pub(crate) text: String,
    /// The line it stands on, counting from 1.
    pub(crate) line: usize,
}

/// The tokens of `source`, comments and literals skipped.
pub(crate) fn tokens(source: &str) -> Vec<Token> {
    let blank: Vec<char> = blanked(source).chars().collect();
    let mut found = Vec::new();
    let mut line = 1;
    let mut index = 0;
    while let Some(&letter) = blank.get(index) {
        if letter == '\n' {
            line += 1;
            index += 1;
        } else if is_word(letter) {
            let length = blank
                .iter()
                .skip(index)
                .take_while(|&&next| is_word(next))
                .count();
            let text = blank.iter().skip(index).take(length).collect();
            found.push(Token { text, line });
            index += length;
        } else {
            if !letter.is_whitespace() {
                found.push(Token {
                    text: letter.to_string(),
                    line,
                });
            }
            index += 1;
        }
    }
    found
}

/// Whether a character belongs to a word.
fn is_word(letter: char) -> bool {
    letter.is_alphanumeric() || letter == '_'
}

/// `source` with every comment and literal replaced by spaces, newlines kept.
fn blanked(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    while let Some(&letter) = chars.get(index) {
        let end = skipped_end(&chars, index).unwrap_or(index);
        if end == index {
            out.push(letter);
            index += 1;
        } else {
            out.extend(
                chars
                    .iter()
                    .skip(index)
                    .take(end - index)
                    .map(|&gone| if gone == '\n' { '\n' } else { ' ' }),
            );
            index = end;
        }
    }
    out
}

/// Where the comment or literal starting at `at` ends, if one starts there.
fn skipped_end(chars: &[char], at: usize) -> Option<usize> {
    let here = |offset: usize| chars.get(at + offset).copied();
    match here(0)? {
        '/' if here(1) == Some('/') => Some(line_end(chars, at)),
        '/' if here(1) == Some('*') => Some(block_end(chars, at)),
        '"' => Some(quoted_end(chars, at + 1, '"')),
        '\'' => char_literal_end(chars, at),
        'r' | 'b' if preceded_by_word(chars, at) => None,
        'r' => raw_end(chars, at),
        'b' if here(1) == Some('r') => raw_end(chars, at + 1),
        'b' if here(1) == Some('"') => Some(quoted_end(chars, at + 2, '"')),
        'b' if here(1) == Some('\'') => char_literal_end(chars, at + 1),
        _ => None,
    }
}

/// Whether the character before `at` continues a word, so a `r` or `b` at
/// `at` is the middle of an identifier rather than a literal prefix.
fn preceded_by_word(chars: &[char], at: usize) -> bool {
    at.checked_sub(1)
        .and_then(|before| chars.get(before))
        .is_some_and(|&letter| is_word(letter))
}

/// The end of a `//` comment: the newline that closes it, which stays.
fn line_end(chars: &[char], at: usize) -> usize {
    let rest = chars.iter().skip(at).take_while(|&&letter| letter != '\n');
    at + rest.count()
}

/// The end of a `/* */` comment, nested ones included.
fn block_end(chars: &[char], at: usize) -> usize {
    let mut depth = 0usize;
    let mut index = at;
    while index < chars.len() {
        let pair = (chars.get(index), chars.get(index + 1));
        match pair {
            (Some('/'), Some('*')) => {
                depth += 1;
                index += 2;
            }
            (Some('*'), Some('/')) => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    chars.len()
}

/// The end of a literal whose body starts at `from`, past its closing `quote`.
fn quoted_end(chars: &[char], from: usize, quote: char) -> usize {
    let mut index = from;
    while let Some(&letter) = chars.get(index) {
        index += 1;
        if letter == '\\' {
            index += 1;
        } else if letter == quote {
            return index;
        }
    }
    chars.len()
}

/// The end of the character literal at `at`, or `None` when the quote opens
/// a lifetime instead.
fn char_literal_end(chars: &[char], at: usize) -> Option<usize> {
    let first = chars.get(at + 1).copied()?;
    if first == '\\' {
        return Some(quoted_end(chars, at + 1, '\''));
    }
    (chars.get(at + 2) == Some(&'\'')).then_some(at + 3)
}

/// The end of the raw string whose `r` is at `at`, or `None` when no raw
/// string starts there.
fn raw_end(chars: &[char], at: usize) -> Option<usize> {
    let hashes = chars
        .iter()
        .skip(at + 1)
        .take_while(|&&letter| letter == '#')
        .count();
    if chars.get(at + 1 + hashes) != Some(&'"') {
        return None;
    }
    let close: Vec<char> = once('"').chain(repeat_n('#', hashes)).collect();
    let body = at + 2 + hashes;
    let length = chars
        .windows(close.len())
        .skip(body)
        .position(|window| window == close.as_slice())?;
    Some(body + length + close.len())
}
