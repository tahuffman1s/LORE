//! Luau scripting for LORE.
//!
//! A game is a folder with a `main.luau` in it. [`ScriptHost`] runs that file
//! in a sandbox, calls the functions it registers on the `game` table, loads
//! it again whenever it is saved, and turns script mistakes into messages
//! that say where the problem is and what to try.
//!
//! A game can be split over files: `require("./weapons")` runs `weapons.luau`,
//! the file beside the one that asks, once, and gives back what it returned. Only scripts
//! inside the game's folder can be required, and saving any of them loads
//! the game again.
//!
//! ```lua
//! -- main.luau
//! game.onStart(function()
//!     print("Hello from Luau!")
//! end)
//!
//! game.onTick(function(step)
//!     if game.input.justPressed("jump") then
//!         print("Jump!")
//!     end
//! end)
//! ```
//!
//! A script never counts time or reads a clock: `game.after` and
//! `game.every` call a function later, `game.tween` moves a number,
//! `game.time` is the seconds of game time, all counted in ticks, and
//! `game.onDraw` is called once per picture ([`ScriptHost::draw`]) to show
//! the game without being able to change it.
//!
//! `game.hud` puts text, bars and boxes on the screen. The script keeps
//! each piece and changes it; whoever draws the game reads them from
//! [`ScriptHost::each_hud_piece`].
//!
//! The script API is described for editors in `types/lore.d.luau`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod api;
mod audio;
mod character_api;
mod error;
mod host;
mod hud;
mod keep;
mod limit;
mod modules;
mod time_api;
mod weapon_api;
mod weather_api;

pub use audio::{AudioCommand, SoundId, SoundRequest};
pub use character_api::CharacterId;
pub use error::{OpenError, ScriptError};
pub use host::{ENTRY_SCRIPT, ScriptHost};
pub use hud::{HudPiece, HudShape};
pub use lore_weapons as weapons;
pub use weapon_api::{HeldWeapon, WeaponHit};
pub use weather_api::{TornadoAsk, WeatherHere, WeatherSaid};

/// Type definitions for the script API, in the format the Luau language
/// server reads, so editors can autocomplete and check `game`.
pub const TYPE_DEFINITIONS: &str = include_str!("../types/lore.d.luau");
