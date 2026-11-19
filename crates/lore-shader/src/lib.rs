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
//! - [`params_of`] and [`ParamBlock`] read the `//! param` lines of a shader and
//!   turn them into a uniform block, and check the values a script gives.
//! - [`refuse_endless_loops`] turns away `loop`, `while` and a `for` with no
//!   condition, so a slip cannot freeze the picture.
//! - [`hooked_module`] puts the engine's template, the values and the
//!   maker's function (a [`Hook`]: `screen` or `surface`) together, after
//!   checking that the function is there and has the right shape.
//!
//! Nothing here touches Vulkan, so `lore check` and the engine's build script
//! can use it too.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
// A shader error is a few strings and is built once per mistake, never in a
// loop, so boxing it would only make every caller's code clumsier.
#![allow(clippy::result_large_err)]

mod compile;
mod error;
mod hook;
mod params;
mod scan;
mod source;
mod spirv;

pub use compile::{Bounds, Compiled, compile};
pub use error::{ShaderError, closest};
pub use hook::{Hook, hooked_module};
pub use params::{Given, Param, ParamBlock, ParamKind, ParamValue, params_of};
pub use scan::refuse_endless_loops;
pub use source::{Owner, Place, Source};

/// The most values (`//! param` lines) one shader can declare.
pub const MAX_VALUES: usize = 32;

/// The most text one game shader can hold, in bytes.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
