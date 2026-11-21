//! The LORE renderer: Vulkan 1.3, drawing into one window.
//!
//! [`Renderer`] owns the connection to the graphics card and the images shown
//! in the window. Give it a scene read by `lore_assets` with
//! [`Renderer::set_scene`] and a [`Camera`] to see it from. The scene is drawn
//! in high dynamic range, smaller than the window if [`Upscaling`] says so,
//! made full size by AMD's FSR 3.1, and tone mapped for the display as the
//! last step.
//!
//! Building this crate needs no Vulkan SDK. The Vulkan library is found when
//! the game starts, and the shaders are compiled by the build script.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod bolt;
mod bounce;
mod buffer;
mod camera;
mod clouds;
mod effects;
mod error;
mod eye;
mod fluid;
mod fluid_pass;
mod font;
mod game_shaders;
mod gpu;
mod grid;
mod headless;
mod lamp_shadow;
mod lamps;
mod look;
mod material;
mod mesh;
mod moon;
mod occlusion;
#[cfg(test)]
mod offscreen;
mod overlay;
#[cfg(feature = "panels")]
mod panels;
mod renderer;
mod scene;
#[cfg(test)]
mod shading;
mod shadow;
mod sky;
mod skylight;
mod stars;
mod swapchain;
mod texture;
mod tonemap;
mod tracks;
mod ui_pass;
mod upscale;
mod view;
mod water;
mod waves;
mod weather;

pub use bolt::Lightning;
pub use bounce::Bounce;
pub use camera::Camera;
pub use effects::Splash;
pub use error::{RenderError, Version};
pub use eye::EyeAdaptation;
pub use fluid::{FluidDrop, FluidLook, MOST_FLUID_DROPS, MOST_FLUID_KINDS, PoolDrawn};
pub use font::Font;
pub use game_shaders::{
    GameShader, MOST_FLUID_SHADERS, MOST_SCREEN_EFFECTS, MOST_SURFACE_SHADERS, ShaderReport,
    ShaderUse, check_game_shader,
};
pub use gpu::{Gpu, validation_errors};
pub use headless::{FrameTime, Headless};
pub use lamp_shadow::LampShadows;
pub use lamps::{Beam, Lamp, MOST_LAMPS};
pub use look::{Look, LookShares, ToneMapping};
pub use lore_sky::{Heavens, SkyPlace, date_from_2000, days_from_2000};
pub use occlusion::AmbientOcclusion;
pub use overlay::HudImage;
#[cfg(feature = "panels")]
pub use panels::Panels;
pub use renderer::{RenderConfig, Renderer};
pub use texture::AnisotropicFiltering;
pub use tracks::Tread;
pub use upscale::Upscaling;
pub use water::Stir;
pub use weather::{Falling, GroundWater, Tornado, Weather};
