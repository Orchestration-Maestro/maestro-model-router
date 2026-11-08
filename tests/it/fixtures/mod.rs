//! Files a test writes for itself.
//!
//! `gguf` composes a model file's bytes. `scratch` is the directory a test
//! writes into, whatever it writes: these model files, and `support`'s models
//! root and search path.

mod gguf;
mod scratch;

pub(crate) use gguf::{Gguf, Value};
pub(crate) use scratch::Scratch;
