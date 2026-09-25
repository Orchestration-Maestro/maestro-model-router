//! Every model file under the root that the catalog does not name.
//!
//! The catalog names the models an operator has thought about; the root holds
//! every model they have downloaded. Until this module the difference was a
//! file that could not be invoked without an edit and a restart, and the
//! decision to close that gap is recorded in
//! `docs/adr/0002-discover-models-beside-the-catalog.md`.
//!
//! The rules, all of them about file names, because that is what a file on
//! disk offers before it is opened:
//!
//! - a file is a model when its extension is `gguf`, in any case;
//! - a file the catalog already names, as weights, draft or projector, is the
//!   catalog's and not found again;
//! - a projector, named `mmproj` by every tool that writes one, is not a
//!   model on its own;
//! - a draft for speculative decoding is not either. Nothing inside such a
//!   file says so -- it calls itself a model -- so the name has to, and the
//!   convention is an `mtp` segment such as `mtp-` or `FastMTP`;
//! - of a model split into shards, `-00001-of-0000N` names the entry and the
//!   rest are its weights;
//! - every directory is walked, `.cache` included: a download cache is where
//!   the tool that fetched a model put it, and the shipped catalog already
//!   points into one.
//!
//! An entry is on-demand, takes the defaults table, and gets a derived
//! estimate. Its identifier is the file's stem, lowercased, with every run of
//! anything but letters and digits collapsed to one hyphen. When that is a
//! name the catalog, the proxy or an earlier file already has, the parent
//! directory's name is put in front, and the note says so.

mod name;
mod walk;

pub(super) use walk::under;
