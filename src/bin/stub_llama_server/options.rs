//! What the stub was asked to do, read from its command line.

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use crate::reply::Pacing;

/// What the stub was asked to do. Every other argument is ignored on purpose:
/// the same invocation that drives `llama-server` has to drive this.
pub(crate) struct Options {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) ready_after: Duration,
    pub(crate) exit_after: Option<Duration>,
    pub(crate) exit_code: u8,
    /// Which entry this stub was started as, taken from `--alias`.
    ///
    /// Every child is this same binary, so a test that asserts a request
    /// reached the right one needs the reply to say which one answered.
    pub(crate) alias: String,
    pub(crate) pacing: Pacing,
    /// Names a marker file. On the run that finds it absent, this stub exits
    /// before binding at all; on a later run that finds it present, it
    /// behaves normally. Stands in for `launch::server`'s `free_port` losing
    /// its own release-then-rebind race: the port this stub was given is
    /// never touched, so nothing outside the process can ever connect to it.
    pub(crate) never_bind_marker: Option<PathBuf>,
    /// The same failure, on every run rather than only the first, for
    /// proving a retry is bounded rather than unbounded.
    pub(crate) never_bind: bool,
}

/// Reads the known arguments and steps over the rest.
///
/// A value is only consumed for a flag this stub knows, so a bare flag it
/// does not know costs nothing and a valued one has its value skipped as an
/// unknown flag in turn.
pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut host = "127.0.0.1".to_owned();
    let mut port = 0u16;
    let mut ready_after = Duration::ZERO;
    let mut exit_after = None;
    let mut exit_code = 0u8;
    let mut alias = String::new();
    let mut first_byte_after = Duration::ZERO;
    let mut events = 3usize;
    let mut gap = Duration::ZERO;
    let mut die_after = None;
    let mut hangup_marker = None;
    let mut never_bind_marker = None;
    let mut never_bind = false;

    let mut args = args;
    while let Some(argument) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--host" => host = value()?,
            "--port" => port = number(&value()?, "--port")?,
            "--ready-after" => {
                ready_after = Duration::from_millis(number(&value()?, "--ready-after")?);
            }
            "--exit-after" => {
                exit_after = Some(Duration::from_millis(number(&value()?, "--exit-after")?));
            }
            "--exit-code" => exit_code = number(&value()?, "--exit-code")?,
            "--alias" => alias = value()?,
            "--first-byte-after" => {
                first_byte_after = Duration::from_millis(number(&value()?, "--first-byte-after")?);
            }
            "--stream-events" => events = number(&value()?, "--stream-events")?,
            "--stream-gap" => gap = Duration::from_millis(number(&value()?, "--stream-gap")?),
            "--die-after-events" => die_after = Some(number(&value()?, "--die-after-events")?),
            "--hangup-marker" => hangup_marker = Some(PathBuf::from(value()?)),
            "--never-bind-marker" => never_bind_marker = Some(PathBuf::from(value()?)),
            "--never-bind" => never_bind = true,
            _ => {}
        }
    }
    Ok(Options {
        host,
        port,
        ready_after,
        exit_after,
        exit_code,
        alias,
        pacing: Pacing {
            first_byte_after,
            events,
            gap,
            die_after,
            hangup_marker,
        },
        never_bind_marker,
        never_bind,
    })
}

/// A flag's value as a number, or a complaint naming the flag.
fn number<T: FromStr>(value: &str, flag: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} takes a number, not '{value}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Options {
        parse(arguments.iter().map(|argument| (*argument).to_owned())).expect("parses")
    }

    #[test]
    fn every_flag_the_stub_knows_sets_what_it_names() {
        // Each value differs from its default, so a flag that was stepped
        // over as unknown leaves a field this test sees unchanged.
        let options = parsed(&[
            "--host",
            "0.0.0.0",
            "--port",
            "8123",
            "--exit-after",
            "250",
            "--never-bind-marker",
            "/tmp/a-marker",
        ]);

        assert_eq!(options.host, "0.0.0.0");
        assert_eq!(options.port, 8123);
        assert_eq!(options.exit_after, Some(Duration::from_millis(250)));
        assert_eq!(
            options.never_bind_marker,
            Some(PathBuf::from("/tmp/a-marker"))
        );
    }
}
