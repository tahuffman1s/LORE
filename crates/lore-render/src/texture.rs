//! A scene's pictures on the graphics card, each with smaller copies of
//! itself for when it is seen from far away, and the ways they are sampled.

use std::sync::Arc;

use ash::vk;
use lore_assets::{Filter, Image, Sampler, Scene, Wrap};

use crate::buffer::{Buffer, once};
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::scene::Picture;

/// Pictures that hold colours: the graphics card turns sRGB into light when
/// it reads one, and back when it makes the smaller copies.
const COLOUR_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
/// Pictures that hold plain numbers, such as normal maps and roughness.
const NUMBER_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

/// How sharp textures stay where they are seen at a slant (anisotropic
/// filtering): a player's setting.
///
/// A floor, a road or a wall seen along its length is squeezed on the
/// screen far more one way than the other, and read the plain way it blurs
/// a few metres from the eye. Each step up reads it with up to twice as
/// many samples along the squeezed way, which keeps it sharp twice as far.
/// It costs next to nothing on a graphics card of its own, and a little on
/// one built into a processor. A card that cannot do it draws as `Off`
/// does, and one that cannot do as much does the most it can.
///
/// Only textures that are smoothed in every way are read like this: one
/// whose file asks for sharp pixels keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnisotropicFiltering {
    /// Read the plain way: fastest, and blurred at a slant.
    Off,
    /// Up to two samples.
    X2,
    /// Up to four samples.
    X4,
    /// Up to eight samples.
    X8,
    /// Up to sixteen samples: sharp as far as textures are seen.
    #[default]
    X16,
}

impl AnisotropicFiltering {
    /// The most samples a texture seen at a slant is read with: 1 for
    /// `Off`, where it is read once as any other is.
    #[must_use]
    pub fn samples(self) -> u8 {
        match self {
            Self::Off => 1,
            Self::X2 => 2,
            Self::X4 => 4,
            Self::X8 => 8,
            Self::X16 => 16,
        }
    }

    /// What a sampler is told: the samples asked for, held to the `most`
    /// the graphics card takes, or `None` to read the plain way (the
    /// setting is off, or the card cannot do it at all).
    fn slant(self, most: Option<f32>) -> Option<f32> {
        if self == Self::Off {
            return None;
        }
        // A card that says it takes fewer than one still takes one.
        most.map(|most| f32::from(self.samples()).min(most.max(1.0)))
    }
}

const DOING: &str = "copying the level's textures to the graphics card";

/// How many sizes a picture is stored in: itself, then half as wide and
/// tall each time, down to a single pixel.
pub(crate) fn sizes(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// Whether the graphics card can make smaller copies of each kind of
/// picture itself: colours first, then numbers.
pub(crate) fn shrinks(gpu: &Gpu) -> [bool; 2] {
    let needed = vk::FormatFeatureFlags::BLIT_SRC
        | vk::FormatFeatureFlags::BLIT_DST
        | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR;
    [COLOUR_FORMAT, NUMBER_FORMAT].map(|format| {
        // SAFETY: the instance and the physical device are alive as long as `gpu` is.
        unsafe {
            gpu.instance()
                .get_physical_device_format_properties(gpu.physical_device(), format)
        }
        .optimal_tiling_features
        .contains(needed)
    })
}

/// The pictures and samplers of one scene.
pub(crate) struct Textures {
    gpu: Arc<Gpu>,
    pictures: Vec<Picture>,
    /// Which of `pictures` holds a scene image, read as colours or as numbers.
    made: Vec<((u32, bool), Option<usize>)>,
    samplers: Vec<(Sampler, vk::Sampler)>,
    /// The widest and tallest picture the graphics card takes.
    largest: u32,
    /// The most samples the graphics card reads a texture with at a
    /// slant, or `None` if it cannot do it.
    most_slant: Option<f32>,
    /// How sharp the player wants textures at a slant.
    filtering: AnisotropicFiltering,
    /// Whether the graphics card can make smaller copies of each kind of
    /// picture itself: colours first, then numbers.
    shrinks: [bool; 2],
}

impl Textures {
    /// Gets ready to hold pictures, read at a slant as `filtering` says.
    /// The first is a single white pixel, for materials to read where they
    /// have no texture of their own.
    pub(crate) fn new(
        gpu: &Arc<Gpu>,
        filtering: AnisotropicFiltering,
    ) -> Result<Self, RenderError> {
        let shrinks = shrinks(gpu);
        // SAFETY: the instance and the physical device are alive as long as `gpu` is.
        let limits = unsafe {
            gpu.instance()
                .get_physical_device_properties(gpu.physical_device())
        }
        .limits;
        let mut textures = Self {
            gpu: Arc::clone(gpu),
            pictures: Vec::new(),
            made: Vec::new(),
            samplers: Vec::new(),
            largest: limits.max_image_dimension2_d,
            most_slant: gpu.anisotropy().then_some(limits.max_sampler_anisotropy),
            filtering,
            shrinks,
        };
        textures.upload(1, 1, &[255; 4], false)?;
        Ok(textures)
    }

    /// The single white pixel.
    pub(crate) fn white(&self) -> vk::ImageView {
        self.pictures
            .first()
            .map_or(vk::ImageView::null(), |picture| picture.view)
    }

    /// The picture `image` of `scene` on the graphics card, copied there the
    /// first time it is asked for. `colour` says whether it holds colours
    /// (base colour, glow) or plain numbers (everything else).
    ///
    /// `None`, with a warning, if the scene has no such picture or it cannot
    /// be used; the material is then drawn without it.
    pub(crate) fn view(
        &mut self,
        scene: &Scene,
        image: u32,
        colour: bool,
    ) -> Result<Option<vk::ImageView>, RenderError> {
        let key = (image, colour);
        let index = if let Some((_, index)) = self.made.iter().find(|(made, _)| *made == key) {
            *index
        } else {
            let index = match scene.images.get(image as usize) {
                Some(picture) if self.usable(picture) => {
                    Some(self.upload(picture.width, picture.height, &picture.pixels, colour)?)
                }
                _ => None,
            };
            self.made.push((key, index));
            index
        };
        Ok(index
            .and_then(|index| self.pictures.get(index))
            .map(|picture| picture.view))
    }

    /// Whether `picture` can be copied to the graphics card; says why not.
    fn usable(&self, picture: &Image) -> bool {
        let name = picture.name.as_deref().unwrap_or("without a name");
        let bytes = u64::from(picture.width) * u64::from(picture.height) * 4;
        if picture.width == 0 || picture.height == 0 || picture.pixels.len() as u64 != bytes {
            log::warn!(
                "The image {name} is not drawn: it says it is {} by {} pixels but holds {} \
                 bytes instead of four for each pixel. If you built the scene in code, check \
                 the image's `width`, `height` and `pixels`; otherwise export the file again.",
                picture.width,
                picture.height,
                picture.pixels.len(),
            );
            return false;
        }
        if picture.width > self.largest || picture.height > self.largest {
            log::warn!(
                "The image {name} is not drawn: it is {} by {} pixels, and this graphics card \
                 takes at most {} each way. Make the image smaller in your 3D tool or an \
                 image editor and export the file again.",
                picture.width,
                picture.height,
                self.largest,
            );
            return false;
        }
        true
    }

    /// The picture at `index`, as [`upload`](Self::upload) said.
    pub(crate) fn view_at(&self, index: usize) -> vk::ImageView {
        self.pictures
            .get(index)
            .map_or(vk::ImageView::null(), |picture| picture.view)
    }

    /// Copies one picture to the graphics card, makes its smaller copies,
    /// and says where in `pictures` it is.
    pub(crate) fn upload(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
        colour: bool,
    ) -> Result<usize, RenderError> {
        let gpu = &self.gpu;
        let device = gpu.device();
        let (format, shrinks) = if colour {
            (COLOUR_FORMAT, self.shrinks[0])
        } else {
            (NUMBER_FORMAT, self.shrinks[1])
        };
        let levels = if shrinks { sizes(width, height) } else { 1 };
        let range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: levels,
            base_array_layer: 0,
            layer_count: 1,
        };
        let staging = Buffer::staging(gpu, pixels.len() as u64, DOING)?;
        staging.fill(pixels, DOING)?;

        // Kept in the list from the start, so that it is freed with the
        // rest whatever happens next.
        self.pictures.push(Picture::null());
        let index = self.pictures.len() - 1;
        let picture = &mut self.pictures[index];
        picture.create(
            gpu,
            vk::Extent2D { width, height },
            format,
            vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::TRANSFER_SRC,
            range,
            DOING,
        )?;
        let image = picture.image;

        let level = |level: u32| vk::ImageSubresourceRange {
            base_mip_level: level,
            level_count: 1,
            ..range
        };
        let layers = |level: u32| vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: level,
            base_array_layer: 0,
            layer_count: 1,
        };
        let barrier = || {
            vk::ImageMemoryBarrier2::default()
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
        };
        // The far corner of a size: half the one before, and never less than one pixel.
        let corner = |level: u32| vk::Offset3D {
            x: i32::try_from((width >> level).max(1)).unwrap_or(i32::MAX),
            y: i32::try_from((height >> level).max(1)).unwrap_or(i32::MAX),
            z: 1,
        };

        // SAFETY: the image and the staging buffer are alive until `once`
        // has waited for the commands. Each size is written before it is
        // read, in the layout the barrier before it left it in, and the
        // format was checked to allow smooth shrinking when there is more
        // than one size.
        unsafe {
            once(gpu, DOING, |commands| {
                let to_fill = [barrier()
                    .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .subresource_range(range)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_fill),
                );
                device.cmd_copy_buffer_to_image(
                    commands,
                    staging.handle,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy::default()
                        .image_subresource(layers(0))
                        .image_extent(vk::Extent3D {
                            width,
                            height,
                            depth: 1,
                        })],
                );
                for smaller in 1..levels {
                    // The size just written is read to make the next.
                    let to_read = [barrier()
                        .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .subresource_range(level(smaller - 1))];
                    device.cmd_pipeline_barrier2(
                        commands,
                        &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                    );
                    let regions = [vk::ImageBlit::default()
                        .src_subresource(layers(smaller - 1))
                        .src_offsets([vk::Offset3D::default(), corner(smaller - 1)])
                        .dst_subresource(layers(smaller))
                        .dst_offsets([vk::Offset3D::default(), corner(smaller)])];
                    device.cmd_blit_image(
                        commands,
                        image,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &regions,
                        vk::Filter::LINEAR,
                    );
                }
                // Every size but the last was read from; the last was only written.
                let read = barrier()
                    .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_READ)
                    .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .subresource_range(vk::ImageSubresourceRange {
                        level_count: levels - 1,
                        ..range
                    });
                let written = barrier()
                    .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .subresource_range(level(levels - 1));
                let to_show = [written, read];
                // With only one size there is nothing that was read from.
                let to_show = if levels > 1 {
                    &to_show[..]
                } else {
                    &to_show[..1]
                };
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(to_show),
                );
            })
        }?;
        Ok(index)
    }

    /// Where in the list the sampler is that reads a texture the way
    /// `wanted` says: it is made the first time it is asked for. The place
    /// stays the same when the player's setting remakes the samplers, so it
    /// is what to keep; [`sampler_at`](Self::sampler_at) gives the sampler.
    pub(crate) fn sampler(&mut self, wanted: &Sampler) -> Result<usize, RenderError> {
        if let Some(index) = self.samplers.iter().position(|(made, _)| made == wanted) {
            return Ok(index);
        }
        let sampler = self.make(wanted, self.filtering)?;
        self.samplers.push((*wanted, sampler));
        Ok(self.samplers.len() - 1)
    }

    /// The sampler at a place [`sampler`](Self::sampler) gave, as it is now.
    pub(crate) fn sampler_at(&self, index: usize) -> vk::Sampler {
        self.samplers
            .get(index)
            .map_or(vk::Sampler::null(), |(_, sampler)| *sampler)
    }

    /// How sharp textures at a slant are read now.
    pub(crate) fn filtering(&self) -> AnisotropicFiltering {
        self.filtering
    }

    /// Reads textures at a slant as `filtering` says from now on. `true`
    /// means every sampler was made again, so whatever holds one must ask
    /// for it again with [`sampler_at`](Self::sampler_at); `false` means
    /// they are as they were, because this card reads the same either way.
    /// If it fails, the samplers and the setting are as they were.
    ///
    /// # Safety
    ///
    /// Nothing still being drawn may use these samplers: the caller has
    /// waited for the graphics card to go idle. What holds a sampler that
    /// was made again must not be drawn with until it holds the new one.
    pub(crate) unsafe fn set_filtering(
        &mut self,
        filtering: AnisotropicFiltering,
    ) -> Result<bool, RenderError> {
        let before = std::mem::replace(&mut self.filtering, filtering);
        if before.slant(self.most_slant) == filtering.slant(self.most_slant) {
            return Ok(false);
        }
        let device = self.gpu.device();
        let mut made = Vec::with_capacity(self.samplers.len());
        for (wanted, _) in &self.samplers {
            match self.make(wanted, filtering) {
                Ok(sampler) => made.push(sampler),
                Err(error) => {
                    // SAFETY: these were made just now and nothing holds them.
                    unsafe {
                        for sampler in made {
                            device.destroy_sampler(sampler, None);
                        }
                    }
                    self.filtering = before;
                    return Err(error);
                }
            }
        }
        for ((_, sampler), new) in self.samplers.iter_mut().zip(made) {
            let old = std::mem::replace(sampler, new);
            // SAFETY: the caller says nothing being drawn uses it, and it
            // was made on this device.
            unsafe { device.destroy_sampler(old, None) };
        }
        Ok(true)
    }

    /// Makes the sampler that reads a texture the way `wanted` says, and at
    /// a slant as `filtering` says.
    fn make(
        &self,
        wanted: &Sampler,
        filtering: AnisotropicFiltering,
    ) -> Result<vk::Sampler, RenderError> {
        let filter = |filter| match filter {
            Filter::Nearest => vk::Filter::NEAREST,
            Filter::Linear => vk::Filter::LINEAR,
        };
        let wrap = |wrap| match wrap {
            Wrap::Repeat => vk::SamplerAddressMode::REPEAT,
            Wrap::MirroredRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
            Wrap::ClampToEdge => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        };
        let mut info = vk::SamplerCreateInfo::default()
            .mag_filter(filter(wanted.magnify))
            .min_filter(filter(wanted.minify))
            .mipmap_mode(match wanted.mipmaps {
                Some(Filter::Linear) => vk::SamplerMipmapMode::LINEAR,
                _ => vk::SamplerMipmapMode::NEAREST,
            })
            .address_mode_u(wrap(wanted.wrap_u))
            .address_mode_v(wrap(wanted.wrap_v))
            .address_mode_w(vk::SamplerAddressMode::REPEAT)
            // A quarter is how Vulkan spells "only ever the full-size picture".
            .max_lod(if wanted.mipmaps.is_some() {
                vk::LOD_CLAMP_NONE
            } else {
                0.25
            });
        // Only a texture that is smoothed in every way can be read at a
        // slant; one that asks for sharp pixels keeps them.
        let smooth = *wanted
            == Sampler {
                magnify: Filter::Linear,
                minify: Filter::Linear,
                mipmaps: Some(Filter::Linear),
                ..*wanted
            };
        if let (Some(samples), true) = (filtering.slant(self.most_slant), smooth) {
            info = info.anisotropy_enable(true).max_anisotropy(samples);
        }
        // SAFETY: the device is alive and the settings are valid: slanted
        // sampling is only asked for where the device has it switched on,
        // and for no more samples than it takes.
        unsafe { self.gpu.device().create_sampler(&info, None) }.map_err(RenderError::driver(DOING))
    }
}

impl Drop for Textures {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping a scene's textures, so nothing is using them, and all of
        // them were made on this device.
        unsafe {
            for (_, sampler) in &self.samplers {
                self.gpu.device().destroy_sampler(*sampler, None);
            }
            for picture in &self.pictures {
                picture.destroy(&self.gpu);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_stored_in_every_size_down_to_one_pixel() {
        assert_eq!(sizes(1, 1), 1);
        assert_eq!(sizes(2, 2), 2);
        assert_eq!(sizes(256, 256), 9);
        // The longer side decides, and a size that is not a power of two rounds down.
        assert_eq!(sizes(1024, 4), 11);
        assert_eq!(sizes(5, 3), 3);
        // Nothing is still one size, not none.
        assert_eq!(sizes(0, 0), 1);
    }

    #[test]
    fn a_sampler_asks_for_what_the_player_chose_and_no_more_than_the_card_takes() {
        use AnisotropicFiltering::{Off, X2, X4, X8, X16};

        // Each step doubles the samples, and sixteen is what a new player has.
        assert_eq!(
            [Off, X2, X4, X8, X16].map(AnisotropicFiltering::samples),
            [1, 2, 4, 8, 16]
        );
        assert_eq!(AnisotropicFiltering::default(), X16);
        // A card that takes sixteen gives each what it asks for.
        assert_eq!(X16.slant(Some(16.0)), Some(16.0));
        assert_eq!(X4.slant(Some(16.0)), Some(4.0));
        // One that takes fewer gives the most it can.
        assert_eq!(X16.slant(Some(8.0)), Some(8.0));
        assert_eq!(X2.slant(Some(8.0)), Some(2.0));
        assert_eq!(X8.slant(Some(0.0)), Some(1.0));
        // Off, or a card that cannot do it, reads the plain way.
        assert_eq!(Off.slant(Some(16.0)), None);
        assert_eq!(X16.slant(None), None);
    }
}
