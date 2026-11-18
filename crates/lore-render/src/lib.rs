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
mod buffer;
mod camera;
mod clouds;
mod effects;
mod error;
mod fluid;
mod font;
mod gpu;
mod grid;
mod headless;
mod material;
mod mesh;
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
pub use camera::Camera;
pub use effects::Splash;
pub use error::{RenderError, Version};
pub use fluid::{FluidDrop, FluidLook, MOST_FLUID_DROPS, MOST_FLUID_KINDS};
pub use font::Font;
pub use gpu::{Gpu, validation_errors};
pub use headless::{FrameTime, Headless};
#[cfg(feature = "panels")]
pub use panels::Panels;
pub use renderer::{RenderConfig, Renderer};
pub use tracks::Tread;
pub use upscale::Upscaling;
pub use water::Stir;
pub use weather::{Falling, Tornado, Weather};
