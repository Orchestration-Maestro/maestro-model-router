//! Whether a request may be served, under the rules `access` holds.
//!
//! Apart from the rules themselves because checking reads a request head and
//! answers with a refusal, and both are the proxy's; the rules are read from
//! the environment and carried in `idle::Limits`, which names no part of the
//! proxy.

use super::head::Head;
use super::refusal::{Cause, Refusal};
use crate::access::{Access, KEY};

impl Access {
    /// Whether this request may be served, and the refusal when it may not.
    pub(super) fn admits(&self, head: &Head) -> Result<(), Refusal> {
        if let (Some(origins), Some(origin)) = (&self.origins, header(head, "origin"))
            && !origins.iter().any(|allowed| allowed == origin)
        {
            return Err(Refusal::new(
                Cause::OriginNotAllowed,
                format!("the origin '{origin}' is not one this router serves"),
            ));
        }
        if let Some(key) = &self.key {
            let carried = header(head, "authorization")
                .and_then(|value| value.strip_prefix("Bearer "))
                .map(str::trim);
            let matches = carried.is_some_and(|carried| same(carried, key));
            if head.method != "OPTIONS" && !matches {
                return Err(Refusal::new(
                    Cause::Unauthorized,
                    format!(
                        "this router requires a key: send 'Authorization: Bearer <key>' with \
                         the value of {KEY}"
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// One header's value, however its name was capitalised.
fn header<'a>(head: &'a Head, name: &str) -> Option<&'a str> {
    head.headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Whether two keys are equal, compared in time that depends on their length
/// alone, so how long a refusal takes says nothing about how much of a guess
/// was right.
fn same(carried: &str, key: &str) -> bool {
    carried.len() == key.len()
        && carried
            .bytes()
            .zip(key.bytes())
            .fold(0u8, |difference, (carried_byte, key_byte)| {
                difference | (carried_byte ^ key_byte)
            })
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_equal_only_when_every_byte_is() {
        assert!(same("s3cret", "s3cret"));
        assert!(!same("s3cres", "s3cret"), "one byte differs");
        assert!(!same("s3cre", "s3cret"), "a prefix is not the key");
        assert!(!same("s3crets", "s3cret"), "nor is a longer guess");
        assert!(!same("ab", "ba"), "nor the key's bytes in another order");
    }
}
