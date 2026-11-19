//! A game's own shaders: screen effects over the finished picture and surface
//! shaders that change what a material looks like.
//!
//! This part of the module needs no graphics card: the templates a game's text
//! is put into ([`template`]), the list of what the game wants against what is
//! drawn ([`book`]), the thread that compiles ([`worker`]) and the things kept
//! until the card is done with them ([`retire`]).

// A shader error is a few strings, built once per mistake and never in a loop,
// so boxing it would only make every caller clumsier (as in `lore-shader`).
#![allow(clippy::result_large_err)]

// Used by the parts that draw, which come after these; tests use them already.
#[cfg_attr(not(test), allow(dead_code))]
mod book;
#[cfg_attr(not(test), allow(dead_code))]
mod retire;
mod template;
#[cfg_attr(not(test), allow(dead_code))]
mod worker;

pub use template::check_game_shader;

/// How many screen effects can be on at once.
pub const MOST_SCREEN_EFFECTS: usize = 8;

/// How many surface shaders can be on at once.
pub const MOST_SURFACE_SHADERS: usize = 8;

/// What a game shader is used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderUse<'a> {
    /// Drawn over the whole finished picture: `fn screen(in: ScreenIn) -> vec3f`.
    Screen,
    /// Changes the surface of every material of the level named `material`:
    /// `fn surface(in: SurfaceIn) -> Surface`.
    Surface {
        /// The name of the material in the level.
        material: &'a str,
    },
}

/// One shader the game wants drawn, as the script last gave it.
#[derive(Clone, Copy, Debug)]
pub struct GameShader<'a> {
    /// The game's name for it, such as `heat`. Reports carry it.
    pub name: &'a str,
    /// The file as errors show it, such as `shaders/heat.wgsl`.
    pub file: &'a str,
    /// The WGSL. It is read only when `revision` changed.
    pub text: &'a str,
    /// What the shader is for.
    pub used_as: ShaderUse<'a>,
    /// The values the script set, already checked against the shader's
    /// `//! param` lines.
    pub values: &'a [(String, lore_shader::ParamValue)],
    /// Raised by the script side whenever the text changes.
    pub revision: u64,
}

/// What became of a shader the game asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderReport {
    /// The game's name for the shader.
    pub name: String,
    /// The file as errors show it.
    pub file: String,
    /// The material a surface shader is for; `None` for a screen effect.
    pub material: Option<String>,
    /// The revision of the text this is about.
    pub revision: u64,
    /// `Ok`: this revision is drawn from now on. `Err`: what is wrong; its
    /// `note` says what is drawn instead.
    pub outcome: Result<(), lore_shader::ShaderError>,
}
