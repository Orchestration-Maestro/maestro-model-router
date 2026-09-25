//! How fast an entry answers, in the unit that entry answers in.
//!
//! A generative entry is measured in tokens a second, and the figure is the
//! server's own: `llama-server` reports `timings.predicted_per_second` on
//! every completion, measured across generation alone. Timing it from out here
//! would fold in the connection, the prompt evaluation and this process's
//! scheduling, and would disagree with every other figure anyone has for these
//! models.
//!
//! An embedding or reranking entry has no such number, because it generates
//! nothing: there is no prediction to time, and the server reports no rate for
//! a forward pass. Those are timed from this side, out of necessity rather
//! than preference, and the figure therefore includes the round trip. It is
//! honest about what it is -- a throughput a caller would see, not a model's
//! intrinsic speed -- and it is comparable between runs of this command, which
//! is what a catalog decision needs.
//!
//! The two are kept in one type so that a report can print a column without
//! knowing which kind of entry produced the row, and cannot print a passage
//! rate under a heading that says tokens.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use std::collections::BTreeMap;

use crate::catalog::Entry;

/// How long to wait for the measuring request before giving up.
///
/// Generous because a large model on a cold page cache is slow, and this is a
/// command someone runs deliberately rather than a request path.
const REPLY_TIMEOUT: Duration = Duration::from_secs(300);

/// How many tokens to generate when measuring a generative entry.
///
/// Enough that the rate is not dominated by the first token, short enough that
/// four large models are minutes rather than an afternoon.
const TOKENS: u32 = 128;

/// The prompt every generative entry is measured on.
///
/// Fixed so runs are comparable to each other. That makes them incomparable to
/// anyone else's benchmark, which is the trade this takes deliberately:
/// internal comparability is what decides a catalog number.
const PROMPT: &str = "Write a short paragraph explaining what a memory budget \
                      is and why a program might need one.";

/// How many passages an embedding or reranking entry is measured over.
///
/// One request carrying many, rather than many requests carrying one: a
/// reranker is asked for a whole candidate list in practice, and measuring it
/// a passage at a time would report the round trip rather than the model.
const PASSAGES: u32 = 32;

/// What one entry managed, in the unit it works in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Throughput {
    /// Tokens a second, as the server itself reported them.
    Generated(f64),
    /// Passages a second, timed from here because nothing reports it.
    Scored(f64),
}

impl Throughput {
    /// The figure and the unit it is in, for a report that prints both.
    #[must_use]
    pub fn parts(self) -> (f64, &'static str) {
        match self {
            Self::Generated(rate) => (rate, "tok/s"),
            Self::Scored(rate) => (rate, "seq/s"),
        }
    }
}

/// Which endpoint answers for this entry, and so how it can be timed.
///
/// Read from the flags the server itself keys on, for the same reason the
/// estimator reads them: the file describes a model, and only the flags say
/// which way of running it is in front of us. An entry started with
/// `embeddings` will refuse a completion, and one started with `reranking`
/// will refuse both.
///
/// This asks a different question from the estimator's `keeps_no_cache`, which
/// happens to read the same two flags. That one decides what an entry costs;
/// this one decides how to address it. They are separate because they would
/// diverge the moment a server grew a mode that kept no cache and still
/// generated.
fn answers(flags: &BTreeMap<String, String>) -> Answers {
    let set = |name: &str| {
        flags.get(name).is_some_and(|value| {
            !matches!(value.trim().to_ascii_lowercase().as_str(), "false" | "0")
        })
    };
    if set("reranking") || set("rerank") {
        Answers::Reranking
    } else if set("embeddings") || set("embedding") {
        Answers::Embeddings
    } else {
        Answers::Completions
    }
}

/// The endpoint that will answer, and the unit the answer comes back in.
enum Answers {
    Completions,
    Embeddings,
    Reranking,
}

/// Measures the entry, in whichever unit it works in.
///
/// Returns `None` rather than an error: a rate that could not be read is worth
/// less than the memory reading beside it, and losing both would be worse.
pub(super) fn of(endpoint: SocketAddr, model: &Entry) -> Option<Throughput> {
    match answers(&model.flags) {
        Answers::Completions => generated(endpoint, &model.id),
        Answers::Embeddings => scored(endpoint, "/v1/embeddings", &embeddings(&model.id)),
        Answers::Reranking => scored(endpoint, "/v1/rerank", &reranking(&model.id)),
    }
}

/// Asks the server to generate, and reads the rate it reports.
fn generated(endpoint: SocketAddr, id: &str) -> Option<Throughput> {
    let body = serde_json::json!({
        "model": id,
        "messages": [{ "role": "user", "content": PROMPT }],
        "max_tokens": TOKENS,
        "stream": false,
        // Deterministic, so that a rate is not quietly measured against a
        // different amount of work each run.
        "temperature": 0.0,
    })
    .to_string();

    let reply = ask(endpoint, "/v1/chat/completions", &body).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(body_of(&reply)?).ok()?;
    let rate = parsed
        .get("timings")?
        .get("predicted_per_second")?
        .as_f64()?;
    Some(Throughput::Generated(rate))
}

/// Times one request carrying every passage, and divides.
///
/// The reply is parsed before the clock is read from, so that a server which
/// answers an error quickly is not recorded as a fast one.
fn scored(endpoint: SocketAddr, path: &str, body: &str) -> Option<Throughput> {
    let started = Instant::now();
    let reply = ask(endpoint, path, body).ok()?;
    let elapsed = started.elapsed();
    let parsed: serde_json::Value = serde_json::from_str(body_of(&reply)?).ok()?;
    if parsed.get("error").is_some() {
        return None;
    }
    let seconds = elapsed.as_secs_f64();
    (seconds > 0.0).then(|| Throughput::Scored(f64::from(PASSAGES) / seconds))
}

/// A fixed batch, so that two runs measure the same work.
fn passages() -> Vec<String> {
    (0..PASSAGES)
        .map(|index| format!("Passage {index}. {PROMPT}"))
        .collect()
}

fn embeddings(id: &str) -> String {
    serde_json::json!({ "model": id, "input": passages() }).to_string()
}

fn reranking(id: &str) -> String {
    serde_json::json!({
        "model": id,
        "query": "what is a memory budget",
        "documents": passages(),
    })
    .to_string()
}

/// One HTTP round trip, hand-written for the same reason the router's is.
fn ask(endpoint: SocketAddr, path: &str, body: &str) -> io::Result<String> {
    let mut stream = TcpStream::connect(endpoint)?;
    stream.set_read_timeout(Some(REPLY_TIMEOUT))?;

    write!(
        stream,
        "POST {path} HTTP/1.1\r\n\
         Host: {endpoint}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )?;
    stream.flush()?;

    // `Connection: close` means end-of-file is the end of the reply, so the
    // length header does not have to be honoured to know when to stop.
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply)
}

/// Whatever followed the blank line.
fn body_of(reply: &str) -> Option<&str> {
    reply.split_once("\r\n\r\n").map(|(_, body)| body)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread;

    use super::*;
    use crate::catalog::Catalog;

    /// An entry carrying these flags, as a catalog would give it.
    fn entry(flags: &str) -> Entry {
        let text = format!(
            "version = 1\n[models.alpha]\npath = \"a.gguf\"\ncontext_size = 4096\n\
             memory_estimate_mib = 512\n[models.alpha.flags]\n{flags}"
        );
        let catalog = Catalog::parse(&text).expect("a valid entry");
        catalog.entries.into_iter().next().expect("one entry")
    }

    /// A server that answers one request with `body`, and sends back the
    /// request line and the body it was sent.
    ///
    /// Received with a deadline rather than joined, so that a measurement
    /// which never asks fails its test instead of hanging the run.
    fn answering(body: &'static str) -> (SocketAddr, Receiver<(String, String)>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let endpoint = listener.local_addr().expect("a bound address");
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the measuring request");
            let request = request_of(&stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("the reply");
            sender.send(request)
        });
        (endpoint, receiver)
    }

    /// The request line of one request, and the body its length declared.
    fn request_of(stream: &TcpStream) -> (String, String) {
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).expect("a request line");
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("a header");
            if line.trim().is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                length = value.trim().parse().expect("a length");
            }
        }
        let mut sent = vec![0; length];
        reader.read_exact(&mut sent).expect("the declared body");
        let sent = String::from_utf8(sent).expect("a text body");
        (request_line.trim_end().to_owned(), sent)
    }

    /// What the server was asked, or a failure after five seconds.
    fn asked(server: &Receiver<(String, String)>) -> (String, String) {
        server
            .recv_timeout(Duration::from_secs(5))
            .expect("the measurement asked the server within five seconds")
    }

    #[test]
    fn a_generative_entry_is_rated_by_what_the_server_reports() {
        let (endpoint, server) = answering(r#"{"timings":{"predicted_per_second":42.5}}"#);

        let rate = of(endpoint, &entry(""));
        let (request_line, _) = asked(&server);

        assert_eq!(rate, Some(Throughput::Generated(42.5)));
        assert_eq!(request_line, "POST /v1/chat/completions HTTP/1.1");
    }

    #[test]
    fn a_scoring_entry_is_rated_by_its_passages_over_the_round_trip() {
        for (flag, path, field) in [
            ("embeddings", "/v1/embeddings", "input"),
            ("reranking", "/v1/rerank", "documents"),
        ] {
            let (endpoint, server) = answering(r#"{"data":[]}"#);

            let started = Instant::now();
            let rate = of(endpoint, &entry(&format!("{flag} = \"true\"\n")));
            let bound = started.elapsed().as_secs_f64();
            let (request_line, sent) = asked(&server);

            let Some(Throughput::Scored(rate)) = rate else {
                panic!("{flag}: a passage rate, not {rate:?}");
            };
            let seconds = f64::from(PASSAGES) / rate;
            assert!(
                seconds > 0.0 && seconds <= bound,
                "{flag}: {PASSAGES} passages at {rate}/s took {seconds}s, \
                 inside a call that took {bound}s"
            );
            assert_eq!(request_line, format!("POST {path} HTTP/1.1"));
            let sent: serde_json::Value = serde_json::from_str(&sent).expect("a JSON body");
            let passages = sent[field].as_array().expect("the passages");
            assert_eq!(passages.len(), 32, "{flag}: the fixed batch");
            assert!(
                passages[31]
                    .as_str()
                    .is_some_and(|passage| passage.starts_with("Passage 31. ")),
                "{flag}: each passage numbered: {sent}"
            );
        }
    }

    fn flags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn an_entry_is_addressed_at_the_endpoint_its_flags_imply() {
        assert!(
            matches!(answers(&flags(&[])), Answers::Completions),
            "an entry that asks for neither mode generates"
        );
        assert!(
            matches!(
                answers(&flags(&[("embeddings", "true")])),
                Answers::Embeddings
            ),
            "a server told to embed will refuse a completion"
        );
        assert!(
            matches!(
                answers(&flags(&[("reranking", "true")])),
                Answers::Reranking
            ),
            "and one told to rerank refuses both"
        );
        assert!(
            matches!(
                answers(&flags(&[("embeddings", "false")])),
                Answers::Completions
            ),
            "read as the flags are elsewhere: an explicit false is not the mode"
        );
    }

    #[test]
    fn a_throughput_carries_the_unit_it_was_measured_in() {
        assert_eq!(Throughput::Generated(12.5).parts(), (12.5, "tok/s"));
        assert_eq!(
            Throughput::Scored(12.5).parts(),
            (12.5, "seq/s"),
            "a passage rate must not be printable under a heading that says \
             tokens -- they are different work and differ by orders of \
             magnitude"
        );
    }
}
