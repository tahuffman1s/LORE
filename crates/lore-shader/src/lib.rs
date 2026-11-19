//! Game shaders for LORE, on the CPU.
//!
//! A game brings its own WGSL and changes it while the game runs. This crate
//! holds everything about a shader that needs no graphics card:
//!
//! - [`Source`] stitches a module together from pieces (the engine's text and
//!   the game's) and remembers which file and line every line came from.
//! - [`compile`] checks a module with naga and writes SPIR-V.
//! - [`ShaderError`] says what is wrong in the words of the person who has to
//!   fix it, at the file and line they wrote, with a hint where one is known.
//!
//! Nothing here touches Vulkan, so `lore check` and the engine's build script
//! can use it too.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
// A shader error is a few strings and is built once per mistake, never in a
// loop, so boxing it would only make every caller's code clumsier.
#![allow(clippy::result_large_err)]

mod compile;
mod error;
mod source;

pub use compile::{Bounds, Compiled, compile};
pub use error::ShaderError;
pub use source::{Owner, Place, Source};

/// The most values (`//! param` lines) one shader can declare.
pub const MAX_VALUES: usize = 32;

/// The most text one game shader can hold, in bytes.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
