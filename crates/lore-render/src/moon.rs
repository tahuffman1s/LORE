//! The moon's face (#481): NASA's maps of its colour and its height, on the
//! graphics card for `night_sky` in `mesh.wgsl` to draw it with.
//!
//! The maps are from NASA's Scientific Visualization Studio "CGI Moon Kit"
//! (<https://svs.gsfc.nasa.gov/4720>), public domain; NASA asks for the
//! credit "NASA's Scientific Visualization Studio". The colour is the Lunar
//! Reconnaissance Orbiter's wide-angle camera, made Hapke-normalised and
//! balanced as an eye sees it (`lroc_color_poles_1k.jpg`); the height is
//! the orbiter's laser altimeter (`ldem_3_8bit.jpg`), about 20 km from
//! black to white. Both are 1024 by 512, longitude across with 0 in the
//! middle and east to the right, latitude down with north at the top.
//!
//! The Milky Way (#480) is read the same way and kept with them: NASA's
//! "Deep Star Maps 2020" (<https://svs.gsfc.nasa.gov/4851>, credit "NASA/
//! Goddard Space Flight Center Scientific Visualization Studio. Gaia DR2:
//! ESA/Gaia/DPAC"), the map without the stars brighter than about
//! magnitude 11.5, so the stars drawn one by one are not counted twice.
//! `data/sky/make_milky_way.sh` makes it: 4096 by 2048, right ascension
//! across with 0 in the middle and growing to the left, declination down
//! with north at the top, each pixel the fourth root of NASA's linear
//! light.

use std::sync::Arc;

use ash::vk;

use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::texture::{AnisotropicFiltering, Textures};

/// The colour of the moon's face, as NASA made it.
const COLOUR: &[u8] = include_bytes!("../data/moon/lroc_color_poles_1k.jpg");

/// The height of the moon's face, as NASA made it.
const HEIGHT: &[u8] = include_bytes!("../data/moon/ldem_3_8bit.jpg");

/// The Milky Way, as `data/sky/make_milky_way.sh` makes it from NASA's map.
const MILKY_WAY: &[u8] = include_bytes!("../data/sky/milky_way.jpg");

/// The moon's two maps and the Milky Way on the graphics card.
pub(crate) struct MoonMaps {
    /// Kept so the pictures live as long as the maps.
    _textures: Textures,
    /// The colour, read as colours.
    pub(crate) colour: vk::ImageView,
    /// The height, read as numbers.
    pub(crate) height: vk::ImageView,
    /// The Milky Way, read as numbers (the fourth root of its light).
    pub(crate) milky_way: vk::ImageView,
    /// Reads both smoothly and from their smaller copies, as small as the
    /// moon is drawn, so its face does not sparkle: round in longitude,
    /// not past the poles.
    pub(crate) sampler: vk::Sampler,
    gpu: Arc<Gpu>,
}

impl Drop for MoonMaps {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the maps, so the sampler is not in use.
        unsafe { self.gpu.device().destroy_sampler(self.sampler, None) };
    }
}

impl MoonMaps {
    /// Decodes the maps and copies them to the graphics card.
    pub(crate) fn new(gpu: &Arc<Gpu>) -> Result<Self, RenderError> {
        let mut textures = Textures::new(gpu, AnisotropicFiltering::default())?;
        // The maps are the engine's own and decode; were one broken, it
        // would be drawn as one plain colour, `otherwise`, rather than stop
        // the game: the moon white, the Milky Way none at all.
        let mut upload = |bytes: &[u8], colour: bool, otherwise: [u8; 4]| {
            let index = match lore_assets::decode_picture(bytes) {
                Ok(picture) => {
                    textures.upload(picture.width, picture.height, &picture.pixels, colour)
                }
                Err(_) => textures.upload(1, 1, &otherwise, colour),
            }?;
            Ok::<_, RenderError>(textures.view_at(index))
        };
        let colour = upload(COLOUR, true, [255; 4])?;
        let height = upload(HEIGHT, false, [255; 4])?;
        let milky_way = upload(MILKY_WAY, false, [0, 0, 0, 255])?;
        // SAFETY: the device is alive, and the settings live until the call
        // returns.
        let sampler = unsafe {
            gpu.device().create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::REPEAT)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .max_lod(vk::LOD_CLAMP_NONE),
                None,
            )
        }
        .map_err(RenderError::driver("making the reader of the moon's maps"))?;
        Ok(Self {
            _textures: textures,
            colour,
            height,
            milky_way,
            sampler,
            gpu: Arc::clone(gpu),
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// The mean of the colour map's light over the face the Earth sees, in
    /// linear light, each point counted by how much of the round face it
    /// fills (its latitude's cosine squared times its longitude's): what
    /// the shader's `MOON_NEAR_MEAN` shares the full moon's brightness out
    /// by.
    const NEAR_SIDE_MEAN: f32 = 0.224;

    /// Linear light from an sRGB byte.
    fn linear(byte: u8) -> f32 {
        let value = f32::from(byte) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    #[test]
    fn the_maps_decode_at_their_size_and_the_near_side_is_as_bright_as_said() {
        let colour = lore_assets::decode_picture(COLOUR).expect("the colour map");
        let height = lore_assets::decode_picture(HEIGHT).expect("the height map");
        assert_eq!((colour.width, colour.height), (1024, 512));
        assert_eq!((height.width, height.height), (1024, 512));
        let milky_way = lore_assets::decode_picture(MILKY_WAY).expect("the Milky Way");
        assert_eq!((milky_way.width, milky_way.height), (4096, 2048));
        // The near side: longitude -90 to 90, the middle half across, each
        // pixel counted by the part of the round face it fills.
        let mut sum = 0.0_f64;
        let mut weights = 0.0_f64;
        for row in 0..512_u32 {
            let latitude = (0.5 - (f64::from(row) + 0.5) / 512.0) * std::f64::consts::PI;
            for column in 256..768_u32 {
                let longitude = ((f64::from(column) + 0.5) / 1024.0 - 0.5) * std::f64::consts::TAU;
                let weight = latitude.cos().powi(2) * longitude.cos();
                let at = ((row * 1024 + column) * 4) as usize;
                let [r, g, b] = [0, 1, 2].map(|part| linear(colour.pixels[at + part]));
                sum += weight * f64::from(0.2126 * r + 0.7152 * g + 0.0722 * b);
                weights += weight;
            }
        }
        let mean = sum / weights;
        assert!((mean - f64::from(NEAR_SIDE_MEAN)).abs() < 0.02, "{mean}");
        let shader = include_str!("../shaders/mesh.wgsl");
        assert!(shader.contains(&format!("const MOON_NEAR_MEAN: f32 = {NEAR_SIDE_MEAN};")));
    }
}
