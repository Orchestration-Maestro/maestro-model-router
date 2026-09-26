//! What a llama.cpp client reads, which the `OpenAI` listing does not carry.
//!
//! Two replies that share one rule with the listing beside them in `reply`:
//! saying what *could* be served is never a reason to start serving it. A
//! client asking what models exist, or whether this server loads them on
//! demand, must not cause a load -- so neither of these reaches a slot for
//! anything but a read.
//!
//! Separate from `reply::listing` because the two answer different clients.
//! `/v1/models` is the `OpenAI` shape and carries only names. `/models` is
//! what a llama.cpp client reads in router mode, and carries each entry's
//! status, its source and the context it was configured with -- the fields
//! that decide whether that client will offer the model at all. The framing
//! they share is `reply::json`; what remains here is only the part that
//! differs, which is the whole reason there are two.

use std::collections::HashMap;
use std::io;
use std::net::TcpStream;

use crate::build::{COMMIT, VERSION};
use crate::catalog::Entry;

use super::super::head::AllowedRoom;
use super::super::refusal::{Cause, Refusal};
use super::super::reply;
use super::super::shared::Shared;

/// Every entry that can hold a conversation, with whether it is loaded, in
/// the shape a llama.cpp client reads in router mode.
///
/// A client's whole test for "is this a router" is that each element carries
/// a string `id` and a string `status.value`, so both are always present and
/// neither is ever null. The status vocabulary is the server's own: an entry
/// is `loaded` when a child is holding it and `unloaded` otherwise. This
/// router has no third state -- it does not sleep a model or download one --
/// and saying so plainly is better than inventing a word a client would have
/// to guess at.
///
/// Entries that generate nothing are left out. A client reading this surface
/// is choosing something to talk to, and its filter is `status`, `source` and
/// `failed` -- none of which can say "this is not a chat model" without lying
/// about one of them. An embedding server offered here is a selection that can
/// only fail. `/v1/models` still carries them, because a caller wanting an
/// embedding has to find it somewhere, and that surface is a catalogue rather
/// than a menu.
pub(super) fn catalogue(stream: &TcpStream, shared: &Shared, head_only: bool) -> io::Result<()> {
    let catalog = shared.catalog();
    let loaded = shared.slots.loaded(&catalog);
    let held: HashMap<String, Option<u64>> = shared.slots.memory(&catalog).into_iter().collect();

    let data: Vec<serde_json::Value> = catalog
        .entries
        .iter()
        .filter(|entry| entry.generates())
        .map(|entry| {
            let status = if loaded.contains(&entry.id) {
                "loaded"
            } else {
                "unloaded"
            };
            serde_json::json!({
                "id": entry.id,
                "object": "model",
                "owned_by": "model-router",
                // `failed` is stated rather than left out so a client reading
                // it finds a boolean. This router has no failed state to
                // report: a model that will not start is a refusal to the
                // request that asked for it, not a lasting mark on the entry.
                "status": { "value": status, "failed": false },
                // Every entry here is configured and waiting, which is what a
                // preset is. A client will not offer an *unloaded* model at
                // all unless it says so -- the three conditions are autoload,
                // not failed, and this -- so leaving it out hides exactly the
                // models the router exists to start on demand.
                "source": "preset",
                // The window the entry was configured with, so a client sizes
                // itself from the catalog rather than from its own default.
                "meta": { "n_ctx": entry.context_size },
                // What the entry can be sent. A client reads this and nothing
                // else before deciding whether an image may go in the request,
                // so an entry given a projector and not saying so is offered
                // as though it were text-only. The catalog is what knows --
                // it names the projector -- and reporting it here answers for
                // every client rather than for one that was configured by hand.
                "architecture": { "input_modalities": entry.accepts() },
                // What the entry was estimated to hold, and what it was
                // measured holding once it was loaded. Admission compares the
                // first against the budget; the second is what the card said
                // the child actually took. They are reported together because
                // an estimate only drifts visibly when both are in one place,
                // and until now the only place was a line on a stdout the
                // service sends to /dev/null. `held_mib` is null for an entry
                // that is not loaded, and for one whose card could not be
                // read -- neither of which is a measurement of nothing.
                "memory": {
                    "declared_mib": entry.memory_estimate_mib,
                    "held_mib": held.get(&entry.id).copied().flatten(),
                },
            })
        })
        .collect();

    reply::json(
        stream,
        &serde_json::json!({ "object": "list", "data": data }),
        head_only,
    )
}

/// What this server does, as the one field a client reads from it.
///
/// `models_autoload` is true and is not a setting: a request for a model that
/// is not running starts it, which is the whole point of the router. A client
/// that reads this decides not to ask for a load before a completion, and it
/// would be right.
pub(super) fn properties(stream: &TcpStream, head_only: bool) -> io::Result<()> {
    reply::json(
        stream,
        &serde_json::json!({
            "models_autoload": true,
            // Which build is answering, so an operator asks the running
            // router rather than trusting what was meant to be installed.
            "build": { "version": VERSION, "commit": COMMIT },
        }),
        head_only,
    )
}

/// Reads the catalog file again, and says what that changed.
///
/// The one path here that is not a question. It exists because a catalog is
/// edited far more often than a router is restarted, and a restart ends every
/// child that happens to be answering at the time -- so the choice was
/// between an edit that needs a restart and an edit that does not, and this
/// is the second one.
///
/// `superseded` is the field worth reading. A child already running keeps the
/// arguments it was started with, because there is no way to change them
/// without ending it and ending it is what this avoids. So an entry can be
/// listed here whose catalog and whose process disagree, and the disagreement
/// lasts until that entry is next loaded. Saying which entries those are is
/// the difference between a reload an operator can reason about and one that
/// quietly half-applied.
///
/// Never `HEAD`: the method set for this endpoint is `POST` alone, so a
/// caller that got here sent one.
pub(super) fn reload(stream: &TcpStream, shared: &Shared) -> io::Result<()> {
    let outcome = shared
        .reload()
        .map(|changed| {
            serde_json::json!({
                "object": "reload",
                "added": changed.added,
                "removed": changed.removed,
                "superseded": changed.superseded,
            })
        })
        // The caller's mistake rather than the router's: the file they asked
        // it to read is the thing that is wrong, and the router is still
        // serving perfectly well from the one it already had.
        .map_err(|reason| {
            Refusal::new(
                Cause::CatalogUnreadable,
                format!(
                    "the catalog was not reloaded and the one already serving \
                     is untouched: {reason}"
                ),
            )
        });
    answered(stream, outcome)
}

/// Starts the entry's child if it is not running, and replies once it is
/// ready.
///
/// Through the admission a request takes, so a load evicts what a request
/// would and is refused for want of room as a request would be, in the room
/// its caller allows. The reply waits for the model rather than for the
/// decision, unlike llama.cpp's own: a caller told `success` can ask the
/// model at once.
pub(super) fn load(
    stream: &TcpStream,
    shared: &Shared,
    entry: &Entry,
    room: AllowedRoom,
) -> io::Result<()> {
    let outcome = shared
        .child(entry, room)
        .map(|_| done())
        .map_err(Refusal::from);
    answered(stream, outcome)
}

/// Ends the entry's child unless something is reading from it.
///
/// A model answering a request is refused rather than ended: cutting off a
/// caller mid-answer is not what an operator freeing memory means. One that
/// is not running is already what was asked for.
pub(super) fn unload(stream: &TcpStream, shared: &Shared, entry: &Entry) -> io::Result<()> {
    let outcome = if shared.slots.let_go(&entry.id) {
        Ok(done())
    } else {
        Err(Refusal::new(
            Cause::ModelBusy,
            format!(
                "'{}' is answering a request; unload it once that ends",
                entry.id
            ),
        ))
    };
    answered(stream, outcome)
}

/// What llama.cpp answers a load or an unload with.
fn done() -> serde_json::Value {
    serde_json::json!({ "success": true })
}

/// The reply for what one of the router's own changes came to: what it did,
/// or why it did not.
fn answered(stream: &TcpStream, outcome: Result<serde_json::Value, Refusal>) -> io::Result<()> {
    match outcome {
        Ok(value) => reply::json(stream, &value, false),
        Err(refusal) => reply::refuse(stream, &refusal),
    }
}
