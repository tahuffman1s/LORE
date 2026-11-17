//! What clouds are shaped from: a block of noise that repeats without a
//! seam in every direction, worked out once and kept on the graphics card.
//!
//! The shader walks each line of sight through the layer of sky where
//! clouds are and asks this block, at each step, how thick the cloud is
//! there. The block holds two numbers at each of its points: a soft,
//! billowing one that says where clouds are, and a finer, cellular one that
//! eats into their edges. Both drift with the wind, the fine one faster, so
//! clouds move and change shape.

use std::sync::{Arc, OnceLock};

use ash::vk;

use crate::buffer::{Buffer, once};
use crate::error::RenderError;
use crate::gpu::Gpu;

/// How many points the block has along each side.
pub(crate) const SIDE: usize = 64;

const FORMAT: vk::Format = vk::Format::R8G8_UNORM;
const DOING: &str = "making the clouds";

/// A number from 0 to 1 for each point of a grid, with no pattern to it.
fn speckle(x: u32, y: u32, z: u32, seed: u32) -> f32 {
    let mut n = x
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(y.wrapping_mul(0x85EB_CA77))
        .wrapping_add(z.wrapping_mul(0xC2B2_AE3D))
        .wrapping_add(seed.wrapping_mul(0x27D4_EB2F));
    n ^= n >> 15;
    n = n.wrapping_mul(0x2C1B_3C6D);
    n ^= n >> 12;
    n = n.wrapping_mul(0x297A_2D39);
    n ^= n >> 15;
    // The top 24 bits are exact as an f32.
    #[allow(clippy::cast_precision_loss)]
    let share = (n >> 8) as f32 / 16_777_216.0;
    share
}

/// Smooth noise that repeats every `cells` squares: `at` is a place in the
/// block from 0 to 1 along each side.
fn smooth(at: [f32; 3], cells: u32, seed: u32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let scaled = at.map(|part| part * cells as f32);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let corner = scaled.map(|part| part.floor() as u32);
    let within = [0, 1, 2].map(|axis| {
        #[allow(clippy::cast_precision_loss)]
        let part = scaled[axis] - corner[axis] as f32;
        part * part * (3.0 - 2.0 * part)
    });
    let at_corner = |dx: u32, dy: u32, dz: u32| {
        speckle(
            (corner[0] + dx) % cells,
            (corner[1] + dy) % cells,
            (corner[2] + dz) % cells,
            seed,
        )
    };
    let along = |dy: u32, dz: u32| {
        at_corner(0, dy, dz) + (at_corner(1, dy, dz) - at_corner(0, dy, dz)) * within[0]
    };
    let across = |dz: u32| along(0, dz) + (along(1, dz) - along(0, dz)) * within[1];
    across(0) + (across(1) - across(0)) * within[2]
}

/// Cellular noise that repeats every `cells` squares: 1 at the middle of a
/// cell and falling to 0 half way to its neighbours, which gives the round,
/// packed shapes of cloud tops (Worley noise, turned over).
fn cells(at: [f32; 3], cells: u32, seed: u32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let scaled = at.map(|part| part * cells as f32);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let home = scaled.map(|part| part.floor() as i64);
    let mut nearest = f32::INFINITY;
    for dz in -1..=1_i64 {
        for dy in -1..=1_i64 {
            for dx in -1..=1_i64 {
                let cell = [home[0] + dx, home[1] + dy, home[2] + dz];
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let wrapped = cell.map(|part| part.rem_euclid(i64::from(cells)) as u32);
                let mut distance = 0.0;
                for axis in 0..3 {
                    // Each cell has one point somewhere inside it.
                    let inside = speckle(wrapped[0], wrapped[1], wrapped[2], seed + axis as u32);
                    #[allow(clippy::cast_precision_loss)]
                    let away = cell[axis] as f32 + inside - scaled[axis];
                    distance += away * away;
                }
                nearest = nearest.min(distance);
            }
        }
    }
    (1.0 - nearest.sqrt()).clamp(0.0, 1.0)
}

/// The block: for each point, the number that places the clouds and then
/// the one that eats their edges, as bytes.
pub(crate) fn noise() -> &'static [u8] {
    static NOISE: OnceLock<Vec<u8>> = OnceLock::new();
    NOISE.get_or_init(|| {
        let mut bytes = Vec::with_capacity(SIDE * SIDE * SIDE * 2);
        #[allow(clippy::cast_precision_loss)]
        let place = |number: usize| (number as f32 + 0.5) / SIDE as f32;
        for z in 0..SIDE {
            for y in 0..SIDE {
                for x in 0..SIDE {
                    let at = [place(x), place(y), place(z)];
                    // Large soft shapes, with smaller ones on them.
                    let soft = smooth(at, 4, 1) * 0.5
                        + smooth(at, 8, 2) * 0.27
                        + smooth(at, 16, 3) * 0.15
                        + smooth(at, 32, 4) * 0.08;
                    let packed = cells(at, 4, 10) * 0.6 + cells(at, 8, 20) * 0.4;
                    // Where the round cells are, the soft shape is fuller:
                    // clouds come out heaped, not smeared.
                    let shape = (soft * 0.7 + packed * 0.45 - 0.08).clamp(0.0, 1.0);
                    let fine = cells(at, 6, 30) * 0.55
                        + cells(at, 12, 40) * 0.3
                        + cells(at, 24, 50) * 0.15;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    bytes.extend([shape, fine].map(|part| (part * 255.0).round() as u8));
                }
            }
        }
        bytes
    })
}

/// The block on the graphics card, and what reads it.
pub(crate) struct CloudVolume {
    gpu: Arc<Gpu>,
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub(crate) view: vk::ImageView,
    /// Smooths between the block's points and starts again at its far side.
    pub(crate) sampler: vk::Sampler,
}

impl CloudVolume {
    /// Works the block out, the first time, and copies it to the graphics
    /// card.
    pub(crate) fn new(gpu: &Arc<Gpu>) -> Result<Self, RenderError> {
        let device = gpu.device();
        let mut volume = Self {
            gpu: Arc::clone(gpu),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            sampler: vk::Sampler::null(),
        };
        let bytes = noise();
        let staging = Buffer::staging(gpu, bytes.len() as u64, DOING)?;
        staging.fill(bytes, DOING)?;
        let side = SIDE as u32;
        let extent = vk::Extent3D {
            width: side,
            height: side,
            depth: side,
        };
        let range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        // SAFETY: the device is alive, and each object is created from valid
        // settings and stored in `volume` straight away, which frees
        // whatever was made if a later step fails. The memory is as large
        // as the image asks for and of a kind it allows.
        unsafe {
            volume.image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_3D)
                        .format(FORMAT)
                        .extent(extent)
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let needs = device.get_image_memory_requirements(volume.image);
            let kind = gpu
                .memory_type(
                    needs.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )
                .or_else(|| {
                    gpu.memory_type(needs.memory_type_bits, vk::MemoryPropertyFlags::empty())
                })
                .ok_or(RenderError::Driver {
                    doing: DOING,
                    code: vk::Result::ERROR_FEATURE_NOT_PRESENT,
                })?;
            volume.memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(needs.size)
                        .memory_type_index(kind),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            device
                .bind_image_memory(volume.image, volume.memory, 0)
                .map_err(RenderError::driver(DOING))?;
            volume.view = device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(volume.image)
                        .view_type(vk::ImageViewType::TYPE_3D)
                        .format(FORMAT)
                        .subresource_range(range),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            volume.sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::LINEAR)
                        .min_filter(vk::Filter::LINEAR)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::REPEAT)
                        .address_mode_v(vk::SamplerAddressMode::REPEAT)
                        .address_mode_w(vk::SamplerAddressMode::REPEAT),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
        }
        let image = volume.image;
        let barrier = || {
            vk::ImageMemoryBarrier2::default()
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(range)
        };
        // SAFETY: the image and the staging buffer are alive until `once`
        // has waited for the commands, and the image is written in the
        // layout the barrier before left it in.
        unsafe {
            once(gpu, DOING, |commands| {
                let to_fill = [barrier()
                    .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)];
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
                        .image_subresource(vk::ImageSubresourceLayers {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            mip_level: 0,
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                        .image_extent(extent)],
                );
                let to_read = [barrier()
                    .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            })
        }?;
        Ok(volume)
    }
}

impl Drop for CloudVolume {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the volume; destroying a null handle is allowed.
        unsafe {
            let device = self.gpu.device();
            device.destroy_sampler(self.sampler, None);
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_repeats_without_a_seam_and_uses_its_whole_range() {
        // Just inside one face and just inside the opposite one are
        // neighbours when the block repeats: nearly the same.
        for at in [[0.3, 0.6, 0.001], [0.001, 0.2, 0.7], [0.5, 0.999, 0.1]] {
            let mut wrapped = at;
            for part in &mut wrapped {
                if *part < 0.01 {
                    *part = 0.999;
                } else if *part > 0.99 {
                    *part = 0.001;
                }
            }
            for (one, other) in [
                (smooth(at, 8, 2), smooth(wrapped, 8, 2)),
                (cells(at, 6, 30), cells(wrapped, 6, 30)),
            ] {
                assert!((one - other).abs() < 0.03, "{one} against {other}");
            }
        }
        let bytes = noise();
        assert_eq!(bytes.len(), SIDE * SIDE * SIDE * 2);
        for channel in 0..2 {
            let values = bytes.iter().skip(channel).step_by(2);
            let (low, high) = values.fold((255, 0), |(low, high), value| {
                (low.min(*value), high.max(*value))
            });
            assert!(low < 60 && high > 200, "{low} to {high}");
        }
    }
}
