//! Collision with a level, and the player's movement through it.
//!
//! A [`Level`] holds the triangles of a loaded scene. A [`Player`] is moved
//! through it one tick at a time by [`Player::step`]: walking, sprinting,
//! crouching and jumping, sliding along walls, climbing stairs and ramps,
//! and, where the level has still water, wading into it and swimming.
//!
//! ```no_run
//! use lore_physics::{Level, MoveInput, MoveSettings, Player};
//!
//! # fn main() -> Result<(), lore_assets::AssetError> {
//! let scene = lore_assets::Scene::load("level.glb")?;
//! let level = Level::from_scene(&scene);
//! let settings = MoveSettings::default();
//! let mut player = Player::at([0.0, 1.0, 0.0], &settings);
//!
//! // Once per tick, with what the player is holding down:
//! let input = MoveInput { forward: 1.0, sprint: true, ..MoveInput::default() };
//! player.step(&level, &settings, &input, 1.0 / 60.0);
//! # Ok(())
//! # }
//! ```
//!
//! Directions and sizes are the engine's: +Y is up, -Z is forward, +X is
//! right, and one unit is one metre.
//!
//! Nothing here reads a clock, a random number or anything else outside its
//! arguments: the same player, level, settings and inputs always give the
//! same result. A recording of a player's inputs therefore replays to the
//! same place, and a game can go back to an old [`Player`] (it is a small
//! `Copy` value) and run on from there.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod fly;
mod footing;
mod level;
mod medium;
mod player;
mod settings;
mod slide;
#[cfg(test)]
mod tests;
mod water;

pub use footing::Footing;
pub use level::{Level, PoolHere, RayHit, Stand};
pub use medium::Medium;
pub use player::Player;
pub use settings::{MoveInput, MoveMode, MoveSettings};
pub use water::{deeper, fill, hollow};
