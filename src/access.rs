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
//!
//! The rules live here, apart from the proxy, because `idle::Limits` carries
//! them into `Router::bind` and the proxy names `idle`; checking a request
//! against them reads a request head, and that is in `proxy::access`.

use std::env;
use std::ffi::OsString;

/// Where the key is configured.
pub(crate) const KEY: &str = "MAESTRO_API_KEY";

/// Where the allowed origins are configured, separated by commas.
const ORIGINS: &str = "MAESTRO_ALLOWED_ORIGINS";

/// Who may use the router: open, unless a key or a list of origins is set.
#[derive(Debug, Clone, Default)]
pub struct Access {
    /// The key every request but a preflight must carry, when one is set.
    pub(crate) key: Option<String>,
    /// The browser origins that may call the router, when a list is set.
    pub(crate) origins: Option<Vec<String>>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
