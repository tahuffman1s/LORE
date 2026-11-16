//! LORE: a free, open-source engine for first- and third-person 3D games.
//!
//! A game is usually a folder with a Luau script, `main.luau`, run through
//! [`ScriptGame`]; `lore new` creates one and `lore run` plays it. A game can also be written in Rust: implement [`Game`] and
//! hand it to [`run`], which opens a window, draws into it with Vulkan, reads
//! keyboard, mouse and gamepad through one action map, and calls the game at a
//! fixed tick rate.
//!
//! ```no_run
//! use lore::{Config, Game, InputFrame};
//!
//! struct MyGame;
//!
//! impl Game for MyGame {
//!     fn tick(&mut self, input: &InputFrame, step: f32) {
//!         // Advance the simulation by `step` seconds.
//!     }
//! }
//!
//! fn main() -> Result<(), lore::AppError> {
//!     lore::run(Config::default(), MyGame)
//! }
//! ```

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(feature = "window")]
mod app;
#[cfg(feature = "window")]
pub mod bench;
pub mod blend;
#[cfg(feature = "cli")]
#[doc(hidden)]
pub mod cli;
pub mod controls;
#[cfg(feature = "editor")]
pub mod editor;
mod error_overlay;
mod game;
#[cfg(feature = "window")]
mod gamepad;
pub mod level;
mod look;
pub mod map;
pub mod noise;
#[cfg(feature = "window")]
mod pace;
#[cfg(feature = "cli")]
pub mod package;
#[cfg(feature = "window")]
mod pause;
#[cfg(test)]
mod playback;
pub mod player;
pub mod press;
pub mod scaffold;
mod script_game;
mod script_hud;
pub mod settings;
mod shooting;
mod sound_output;
pub mod sounds;
#[cfg(feature = "window")]
mod standin;
#[cfg(all(test, feature = "window"))]
mod starter_level;
mod tick;
#[cfg(feature = "window")]
mod translate;
pub mod world;

pub use lore_assets as assets;
pub use lore_audio as audio;
pub use lore_input as input;
pub use lore_input::{ActionId, ActionMap, ActionSetId, InputFrame, MouseMode, Rumble};
#[cfg(feature = "window")]
pub use lore_render as render;

#[cfg(feature = "window")]
pub use app::{AppError, Config, run};
pub use game::{Frame, Game};
#[cfg(feature = "window")]
pub use lore_render::Upscaling;
pub use lore_script as script;
pub use lore_ui as ui;
pub use script_game::ScriptGame;
pub use tick::TickClock;
