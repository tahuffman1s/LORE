//! Plays animation clips on skeletons, the same on every computer.
//!
//! A model's [`Skeleton`] comes from its file; its [`Clip`]s are resampled
//! once, when loaded, to [`KEYS_PER_SECOND`] even keys, so playing one is a
//! lookup and a straight line between two keys, with no search and no curve
//! maths a tick. Clips find bones by name, so one set of clips plays on any
//! body with the same bone names (LORE's mannequins use Unreal's: `pelvis`,
//! `thigh_l`, `foot_r`...).
//!
//! An [`Animator`] holds what one character is playing: its moving about
//! (idle, walk, run, crouch, jump, fall, land, chosen from its speed and
//! footing), clips laid over that (a wave, a sword swing on the upper body),
//! and the fades between them. It is stepped on the game's fixed tick.
//!
//! Only adds, multiplies, divides and square roots are used, which every
//! processor rounds alike, so the same ticks give the same bones everywhere.
//! Nothing is allocated while playing: callers keep the buffers.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod animator;
mod clip;
pub mod math;
mod skeleton;
#[cfg(test)]
mod tests;

pub use animator::{
    Animator, Body, DEFAULT_FADE, Footing, Layer, MOST_PLAYING, Moving, Play, Playing, Strike,
    names,
};
pub use clip::{Binding, Channel, Clip, KEYS_PER_SECOND, Marker, Part};
pub use skeleton::{STANDARD_BONES, Skeleton};
