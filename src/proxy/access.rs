//! Who may use the router.
//!
//! Open by default: the router answers whoever reaches an address it was told
//! to listen on, which is right for loopback and as right as the network is
//! for a bridge. Two settings narrow it, each off until set.
//!
//! A key. Every request but a preflight carries it as
//! `Authorization: Bearer <key>`, which is what an OpenAI-compatible client
//! sends when it is given one. A preflight is exempt because a browser sends
//! none with it, and it starts nothing.
//!
//! A list of origins. A browser page names its origin on every request it
//! makes to another, and a router that approved every origin's preflight was
//! one any page open in a browser on the machine could drive -- start models
//! with, read answers from. With a list, a request naming an origin not on it
//! is refused before anything else happens. A caller that is no browser names
//! no origin and is unaffected.

use std::env;
use std::ffi::OsString;

use super::head::Head;
use super::refusal::{Cause, Refusal};

/// Where the key is configured.
const KEY: &str = "MAESTRO_API_KEY";

/// Where the allowed origins are configured, separated by commas.
const ORIGINS: &str = "MAESTRO_ALLOWED_ORIGINS";

/// Who may use the router: open, unless a key or a list of origins is set.
#[derive(Debug, Clone, Default)]
pub struct Access {
    key: Option<String>,
    origins: Option<Vec<String>>,
}

impl Access {
    /// The rules this machine's environment sets.
    #[must_use]
    pub fn configured() -> Self {
        Self::from_variables(env::var_os(KEY), env::var_os(ORIGINS))
    }

    /// The rules these values of the two variables describe.
    ///
    /// An empty key, and a list naming no origin, are unset rather than a lock
    /// with no way in: `export MAESTRO_API_KEY=` is a slip, not a decision to
    /// refuse everyone.
    #[must_use]
    pub fn from_variables(key: Option<OsString>, origins: Option<OsString>) -> Self {
        let key = key
            .map(|key| key.to_string_lossy().trim().to_owned())
            .filter(|key| !key.is_empty());
        let origins = origins
            .map(|list| {
                list.to_string_lossy()
                    .split(',')
                    .map(str::trim)
                    .filter(|origin| !origin.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|origins| !origins.is_empty());
        Self { key, origins }
    }

    /// What these rules are, as the line `serve` prints at startup. Never the
    /// key itself.
    #[must_use]
    pub fn described(&self) -> String {
        let key = if self.key.is_some() {
            format!("a key is required ({KEY})")
        } else {
            format!("no key is required (set {KEY} to require one)")
        };
        let origins = self.origins.as_ref().map_or_else(
            || format!("any browser origin may call it (set {ORIGINS} to list them)"),
            |origins| format!("browsers may call it from {}", origins.join(", ")),
        );
        format!("access: {key}; {origins}")
    }

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

    #[test]
    fn the_startup_line_names_the_rules_and_never_the_key() {
        let open = Access::default().described();
        assert!(open.contains("no key is required") && open.contains("any browser origin"));

        let guarded = Access::from_variables(
            Some("s3cret".into()),
            Some("http://a.example, http://b.example".into()),
        )
        .described();
        assert!(guarded.contains("a key is required"), "{guarded}");
        assert!(
            guarded.contains("http://a.example, http://b.example"),
            "{guarded}"
        );
        assert!(
            !guarded.contains("s3cret"),
            "the key is never printed: {guarded}"
        );
    }
}
