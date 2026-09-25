//! Model files a test writes for itself.
//!
//! Shared by every test module that needs a model file with something inside
//! it: `gguf` composes the bytes, `scratch` is where they are written.

mod gguf;
mod scratch;

pub(crate) use gguf::{Gguf, Value};
pub(crate) use scratch::Scratch;
