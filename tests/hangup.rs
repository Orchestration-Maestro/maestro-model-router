//! A caller that hangs up releases the model it was waiting on.
//!
//! A model reading a long prompt says nothing, and a model generating a reply
//! that is not streamed says nothing until it is done. A relay that only
//! notices its caller is gone when it next has something to write kept such a
//! model busy -- unevictable, and generating for nobody -- for as long as the
//! silence lasted. The relay now watches the caller's side as well, so a
//! caller who gives up releases the model at once.
//!
//! Observed through eviction: with room for one model, a second model can be
//! loaded only once the first is no longer busy.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

mod support;
use support::{MODEL, ModelsRoot, get, post, queued, request, settled, status};

/// How long the first model stays silent. Far longer than the test waits, so
/// a model released only when it speaks is one the test never sees released.
const SILENCE_MS: u64 = 10_000;

/// Two models, each estimated at 512 MiB: under a 600 MiB budget only one
/// fits, so loading the second means unloading the first.
fn catalog() -> String {
    format!(
        "version = 1\n\
         \n\
         [defaults]\n\
         context_size = 4096\n\
         residency = \"on-demand\"\n\
         memory_estimate_mib = 512\n\
         startup_timeout_seconds = 30\n\
         \n\
         [models.thinking]\n\
         path = \"{MODEL}\"\n\
         [models.thinking.flags]\n\
         first-byte-after = \"{SILENCE_MS}\"\n\
         \n\
         [models.other]\n\
         path = \"{MODEL}\"\n"
    )
}

#[test]
fn a_caller_that_hangs_up_during_the_silence_releases_the_model() {
    let serving = queued(
        &catalog(),
        ModelsRoot::with(&[MODEL]),
        Some(600),
        Duration::ZERO,
    );

    let mut caller = TcpStream::connect(serving.address()).expect("the router accepts");
    caller
        .write_all(post("/models/thinking/v1/chat/completions", "{}").as_bytes())
        .expect("the request is sent");
    settled(&serving, "the thinking model to load", |serving| {
        serving.loaded().contains(&"thinking".to_owned())
    });

    // The model is still silent: nothing has arrived, so this is the window
    // in which only watching the caller can notice it leave.
    caller
        .set_read_timeout(Some(Duration::from_millis(300)))
        .expect("a read timeout");
    let mut byte = [0u8; 1];
    let heard = caller.read(&mut byte);
    assert!(
        matches!(&heard, Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)),
        "the model says nothing while it thinks; the caller heard {heard:?}"
    );
    drop(caller);

    let hung_up = Instant::now();
    let mut reply = String::new();
    while hung_up.elapsed() < Duration::from_secs(3) {
        reply = request(serving.address(), &get("/models/other/v1/echo"));
        if status(&reply) == Some(200) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        status(&reply),
        Some(200),
        "within three seconds of its caller hanging up, the silent model is \
         no longer busy, so the other model can take its room:\n{reply}"
    );
}
