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
//! `game.shaders` turns on a game's own shaders by name, a screen effect
//! over the finished picture or a change to the surface of one material, and
//! sets their values. Like lamps they only change what is drawn; whoever
//! draws the game reads them from [`ScriptHost::each_shader`] and notices a
//! change by [`ScriptHost::shader_revision`].
//!
//! The script API is described for editors in `types/lore.d.luau`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod api;
mod audio;
mod body_api;
mod builtin;
mod camera_api;
pub use body_api::{AnimationAsk, BUILT_IN_HELD, BUILT_IN_MODELS, CameraView, HeldSaid};
mod character_api;
mod clock_api;
mod dice_api;
mod difficulty_api;
mod error;
mod fluid_api;
mod fonts;
mod health_api;
mod host;
mod hud;
mod images;
mod keep;
mod lamp_api;
mod lighting_api;
mod limit;
mod listen_api;
mod look_api;
mod modules;
mod npc_api;
mod pool_api;
mod settings_api;
mod shader_api;
mod sides_api;
mod thing_api;
mod time_api;
mod tools_api;
mod typing_api;
mod ui;
mod walk_api;
mod weapon_api;
mod weather_api;

pub use audio::{AudioCommand, ScriptNoise, SoundId, SoundRequest, SoundTweak};
pub use camera_api::{CameraSaid, CameraTarget, MOST_PATH_POINTS, ShotSaid};
pub use character_api::CharacterId;
pub use clock_api::WorldClock;
pub use dice_api::{Dice, seed_new_games};
pub use difficulty_api::{DIFFICULTY_SETTING, DifficultyScales};
pub use error::{OpenError, ScriptError};
pub use fluid_api::{FluidAsk, FluidKind, FluidLook, FluidUnder, MOST_FLUID_ASKS, PoolTold};
pub use fonts::{
    FONTS_FOLDER, families_of as font_families_of, family_of as font_family_of, is_font_file,
    scan as scan_fonts,
};
pub use health_api::Deed;
pub use host::{ENTRY_SCRIPT, ScriptHost};
pub use hud::{HudPiece, HudShape, HudStyle};
pub use images::{HudImageInfo, IMAGES_FOLDER, png_size, scan as scan_images};
pub use lamp_api::{LampBeam, LampLight};
pub use lighting_api::{EyeSaid, OcclusionSaid};
pub use listen_api::BindingAsk;
pub use look_api::{LookSaid, ToneMappingSaid};
pub use lore_weapons as weapons;
pub use npc_api::{AwareAsk, DEFAULT_LOOK_DISTANCE, NPC_KIND, NpcAsk, NpcSenses, NpcTold};
pub use settings_api::{Setting, SettingKind, SettingValue};
pub use shader_api::{
    MOST_FLUID_SHADERS, MOST_SCREEN_EFFECTS, MOST_SURFACE_SHADERS, ShaderSaid, ShaderUse,
};
pub use thing_api::{DEFAULT_REACH, Placed, PlacedValue, ThingAsk, ThingMotion, Usable, UseSaid};
pub use tools_api::{
    Abilities, BindingChange, ControlsChange, FRAME_HISTORY, Motion, MoveSaid, Movement, Teleport,
    ViewTold,
};
pub use ui::ScreensChecked;
pub use walk_api::WalkingGround;
pub use weapon_api::{HeldWeapon, HoldSaid, NoiseTold, SoundName, WeaponHit, WeaponSounds};
pub use weather_api::{DEEPEST_SNOW, THICKEST_ICE, TornadoAsk, WeatherHere, WeatherSaid};

/// Type definitions for the script API, in the format the Luau language
/// server reads, so editors can autocomplete and check `game`.
pub const TYPE_DEFINITIONS: &str = include_str!("../types/lore.d.luau");
