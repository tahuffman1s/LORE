//! The sun's shadows: what the sun sees of the level, kept as four pictures
//! of how far away it is, each covering a wider square round the eye than
//! the one before (cascaded shadow maps).
//!
//! The squares are centred on the eye and not fitted to what the camera
//! sees, so turning the camera changes nothing, and each square moves in
//! steps of a sixteenth of its half-width rather than with every step the
//! player takes. A square is only drawn again when it has moved or the
//! level or the sun has changed: a player who stands still, or only looks
//! around, pays nothing for shadows but reading them.
//!
//! The steps are whole numbers of the picture's pixels, so a shadow's edge
//! does not crawl as the square moves.

use std::cell::RefCell;
use std::sync::Arc;

use ash::vk;

use crate::camera::Mat4;
use crate::error::RenderError;
use crate::gpu::Gpu;

/// How many squares there are.
pub(crate) const CASCADES: usize = 4;

/// How many pixels wide and high each square's picture is.
pub(crate) const SIZE: u32 = 2048;

/// Half the width of each square, in metres. Beyond the last nothing is
/// shadowed.
const REACH: [f32; CASCADES] = [8.0, 24.0, 72.0, 216.0];

/// How many pixels a square moves at a time: a sixteenth of its half-width.
const STEP_PIXELS: f32 = 64.0;

/// How wide the sun is: the width of a shadow's soft edge for each metre
/// between what casts it and what it falls on. The sun is half a degree
/// across (0.53°), which is 0.0093 m of soft edge a metre, and it is drawn
/// as it is: a pole's shadow is sharp at its foot and soft at its tip, as
/// in life.
pub(crate) const SUN_WIDTH: f32 = 0.0093;

/// The format of the pictures: how far from the sun, as one float.
pub(crate) const FORMAT: vk::Format = vk::Format::D32_SFLOAT;

/// One square: where it is and how the shader finds a point in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cascade {
    /// Takes a point in the world to the square: x and y from -1 to 1
    /// across it, and z from 0 at the level's sunward end to 1 at its far
    /// end.
    pub(crate) clip: Mat4,
    /// How many metres one pixel of the picture covers.
    pub(crate) texel: f32,
    /// How many metres the picture's depth runs over.
    pub(crate) depth: f32,
    /// The square's own sideways and upward directions in the world.
    across: [f32; 3],
    along: [f32; 3],
    /// Where its middle is, measured along those two.
    middle: [f32; 2],
    half: f32,
}

fn dot(one: [f32; 3], other: [f32; 3]) -> f32 {
    one[0] * other[0] + one[1] * other[1] + one[2] * other[2]
}

fn cross(one: [f32; 3], other: [f32; 3]) -> [f32; 3] {
    [
        one[1] * other[2] - one[2] * other[1],
        one[2] * other[0] - one[0] * other[2],
        one[0] * other[1] - one[1] * other[0],
    ]
}

fn unit(direction: [f32; 3]) -> [f32; 3] {
    let length = dot(direction, direction).sqrt();
    if length > 0.0 && length.is_finite() {
        direction.map(|part| part / length)
    } else {
        [0.0, 1.0, 0.0]
    }
}

/// How many pictures there are in all: the sun's, and one of the level
/// seen from straight above, which says what is overhead.
pub(crate) const PICTURES: usize = CASCADES + 1;

/// Half the width of the picture from above, in metres.
const OVERHEAD_REACH: f32 = 96.0;

/// The squares for an eye at `eye` under a sun found in the direction
/// `towards_sun`, in a level that lies between the corners `low` and `high`:
/// the sun's, nearest first, and then the one seen from straight above.
pub(crate) fn cascades(
    eye: [f32; 3],
    towards_sun: [f32; 3],
    bounds: [[f32; 3]; 2],
) -> [Cascade; PICTURES] {
    std::array::from_fn(|number| match REACH.get(number) {
        Some(half) => square(eye, towards_sun, bounds, *half),
        None => square(eye, [0.0, 1.0, 0.0], bounds, OVERHEAD_REACH),
    })
}

/// One square, `half` metres to each side of the eye, seen from the
/// direction `towards`.
fn square(eye: [f32; 3], towards: [f32; 3], [low, high]: [[f32; 3]; 2], half: f32) -> Cascade {
    // The way the light travels.
    let away = unit(towards).map(|part| -part);
    // Any direction that is not the light's own will do to hang the square from.
    let lean = if away[1].abs() > 0.99 {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let across = unit(cross(lean, away));
    let along = cross(away, across);

    // How far along the light the level starts and ends, with room to spare
    // so that nothing sits exactly on either end.
    let mut nearest = f32::INFINITY;
    let mut farthest = f32::NEG_INFINITY;
    for corner in 0..8 {
        let point = [0, 1, 2].map(|axis| {
            if corner >> axis & 1 == 0 {
                low[axis]
            } else {
                high[axis]
            }
        });
        let far = dot(point, away);
        nearest = nearest.min(far);
        farthest = farthest.max(far);
    }
    if !(nearest.is_finite() && farthest.is_finite()) {
        (nearest, farthest) = (0.0, 1.0);
    }
    let (nearest, farthest) = (nearest - 1.0, farthest + 1.0);
    let depth = farthest - nearest;

    // The sizes are far below where f32 stops being exact.
    #[allow(clippy::cast_precision_loss)]
    let texel = 2.0 * half / SIZE as f32;
    let step = texel * STEP_PIXELS;
    let middle = [across, along].map(|axis| {
        let at = (dot(eye, axis) / step).round() * step;
        if at.is_finite() { at } else { 0.0 }
    });
    let column = |axis: usize| {
        [
            across[axis] / half,
            along[axis] / half,
            away[axis] / depth,
            0.0,
        ]
    };
    Cascade {
        clip: [
            column(0),
            column(1),
            column(2),
            [-middle[0] / half, -middle[1] / half, -nearest / depth, 1.0],
        ],
        texel,
        depth,
        across,
        along,
        middle,
        half,
    }
}

impl Cascade {
    /// Whether something `radius` metres across its middle at `middle` can
    /// cast a shadow into this square: whether the sun sees any of it there.
    pub(crate) fn holds(&self, middle: [f32; 3], radius: f32) -> bool {
        [self.across, self.along]
            .iter()
            .zip(self.middle)
            .all(|(axis, centre)| (dot(middle, *axis) - centre).abs() <= self.half + radius)
    }
}

/// Pictures of how far away things are, side by side in one image, that
/// are drawn into one at a time and read together: the sun's (see
/// [`ShadowMap`]) and the lamps' (see
/// [`LampAtlas`](crate::lamp_shadow::LampAtlas)).
pub(crate) struct DepthLayers {
    gpu: Arc<Gpu>,
    image: vk::Image,
    memory: vk::DeviceMemory,
    /// All of them, for the shader to read.
    pub(crate) view: vk::ImageView,
    /// Each one by itself, to draw into.
    layers: Vec<vk::ImageView>,
    /// How many pixels wide and high each picture is.
    pub(crate) size: u32,
}

impl DepthLayers {
    /// Makes `count` pictures `size` pixels square, ready to be read and
    /// holding "nothing near" until something is drawn into them.
    pub(crate) fn new(
        gpu: &Arc<Gpu>,
        size: u32,
        count: u32,
        doing: &'static str,
    ) -> Result<Self, RenderError> {
        let device = gpu.device();
        let mut made = Self {
            gpu: Arc::clone(gpu),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            layers: Vec::with_capacity(count as usize),
            size,
        };
        // SAFETY: the device is alive, and each object is created from valid
        // settings and stored in `made` straight away, which frees whatever
        // was made if a later step fails. The memory is as large as the
        // image asks for and of a kind it allows.
        unsafe {
            made.image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(FORMAT)
                        .extent(vk::Extent3D {
                            width: size,
                            height: size,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(count)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .usage(
                            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT
                                | vk::ImageUsageFlags::SAMPLED
                                | vk::ImageUsageFlags::TRANSFER_DST,
                        ),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            let needs = device.get_image_memory_requirements(made.image);
            let kind = gpu
                .memory_type(
                    needs.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )
                .or_else(|| {
                    gpu.memory_type(needs.memory_type_bits, vk::MemoryPropertyFlags::empty())
                })
                .ok_or(RenderError::Driver {
                    doing,
                    code: vk::Result::ERROR_FEATURE_NOT_PRESENT,
                })?;
            made.memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(needs.size)
                        .memory_type_index(kind),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            device
                .bind_image_memory(made.image, made.memory, 0)
                .map_err(RenderError::driver(doing))?;
            made.view = device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(made.image)
                        .view_type(vk::ImageViewType::TYPE_2D_ARRAY)
                        .format(FORMAT)
                        .subresource_range(range(0, count)),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            for layer in 0..count {
                let view = device
                    .create_image_view(
                        &vk::ImageViewCreateInfo::default()
                            .image(made.image)
                            .view_type(vk::ImageViewType::TYPE_2D)
                            .format(FORMAT)
                            .subresource_range(range(layer, 1)),
                        None,
                    )
                    .map_err(RenderError::driver(doing))?;
                made.layers.push(view);
            }
        }
        // A shader that only might read a picture still needs it readable:
        // every picture is cleared to "nothing near" and left ready to read.
        let image = made.image;
        // SAFETY: the image is this device's, new, and used by nothing else.
        unsafe {
            crate::buffer::once(gpu, doing, |commands| {
                let to_clear = [vk::ImageMemoryBarrier2::default()
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range(0, count))
                    .dst_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_clear),
                );
                device.cmd_clear_depth_stencil_image(
                    commands,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                    &[range(0, count)],
                );
                let to_read = [vk::ImageMemoryBarrier2::default()
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range(0, count))
                    .src_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            })?;
        }
        Ok(made)
    }

    /// How many pictures there are.
    pub(crate) fn count(&self) -> usize {
        self.layers.len()
    }

    /// Records the start of drawing into picture `layer`, cleared to "as far
    /// as can be".
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this device that is being
    /// recorded, outside a render pass, and the pictures must only be in
    /// use by earlier commands on the same queue.
    pub(crate) unsafe fn begin(&self, commands: vk::CommandBuffer, layer: usize) {
        let device = self.gpu.device();
        let Some(view) = self.layers.get(layer) else {
            return;
        };
        let extent = vk::Extent2D {
            width: self.size,
            height: self.size,
        };
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        };
        // What it held is thrown away, so it does not matter what state it
        // was left in.
        let to_draw = [self
            .barrier(layer)
            .src_stage_mask(
                vk::PipelineStageFlags2::FRAGMENT_SHADER
                    | vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                    | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
            )
            .src_access_mask(vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE)
            .dst_stage_mask(
                vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                    | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
            )
            .dst_access_mask(
                vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                    | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
            )
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)];
        let depth = vk::RenderingAttachmentInfo::default()
            .image_view(*view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            });
        let rendering = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .depth_attachment(&depth);
        // The size is far below where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.size as f32,
            height: self.size as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        // SAFETY: the caller guarantees the command buffer is recording and
        // the pictures are this device's; every struct lives until its call
        // returns.
        unsafe {
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_draw),
            );
            device.cmd_begin_rendering(commands, &rendering);
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
        }
    }

    /// Records the end of drawing into picture `layer`, which leaves it
    /// ready for shaders to read.
    ///
    /// # Safety
    ///
    /// As for [`begin`](Self::begin), after it, with the same `layer`.
    pub(crate) unsafe fn end(&self, commands: vk::CommandBuffer, layer: usize) {
        let device = self.gpu.device();
        let to_read = [self
            .barrier(layer)
            .src_stage_mask(
                vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                    | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
            )
            .src_access_mask(vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
            .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
            .old_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        // SAFETY: as for `begin`.
        unsafe {
            device.cmd_end_rendering(commands);
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_read),
            );
        }
    }

    /// A change of state for picture `layer` alone.
    fn barrier(&self, layer: usize) -> vk::ImageMemoryBarrier2<'static> {
        vk::ImageMemoryBarrier2::default()
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.image)
            .subresource_range(range(u32::try_from(layer).unwrap_or(0), 1))
    }
}

impl Drop for DepthLayers {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the pictures, so they are not in use; destroying a null
        // handle is allowed.
        unsafe {
            let device = self.gpu.device();
            for view in &self.layers {
                device.destroy_image_view(*view, None);
            }
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// Pictures `layer` to `layer + count` of a depth image.
fn range(layer: u32, count: u32) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::DEPTH,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: layer,
        layer_count: count,
    }
}

/// Makes the reader that compares a depth with a picture's and smooths the
/// answer: a point is lit if it is no farther from the light than what the
/// picture holds. Outside the picture counts as what its edge holds.
pub(crate) fn comparing_sampler(gpu: &Gpu, doing: &'static str) -> Result<vk::Sampler, RenderError> {
    // Not every graphics card can smooth between the pixels of a depth
    // picture; one that cannot gets a harder edge.
    // SAFETY: the instance and physical device are alive.
    let can_smooth = unsafe {
        gpu.instance()
            .get_physical_device_format_properties(gpu.physical_device(), FORMAT)
    }
    .optimal_tiling_features
    .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR);
    let filter = if can_smooth {
        vk::Filter::LINEAR
    } else {
        vk::Filter::NEAREST
    };
    // SAFETY: the device is alive and the settings are valid.
    unsafe {
        gpu.device().create_sampler(
            &vk::SamplerCreateInfo::default()
                .mag_filter(filter)
                .min_filter(filter)
                .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .compare_enable(true)
                .compare_op(vk::CompareOp::LESS_OR_EQUAL),
            None,
        )
    }
    .map_err(RenderError::driver(doing))
}

/// The sun's pictures on the graphics card, and what was last drawn into
/// each.
pub(crate) struct ShadowMap {
    gpu: Arc<Gpu>,
    pub(crate) pictures: DepthLayers,
    /// Compares a depth with the picture's and smooths the answer.
    pub(crate) sampler: vk::Sampler,
    /// The level and the square each picture was last drawn for.
    drawn: RefCell<[Option<(u64, Mat4)>; PICTURES]>,
}

impl ShadowMap {
    /// Makes the pictures, with nothing drawn in them yet.
    pub(crate) fn new(gpu: &Arc<Gpu>) -> Result<Self, RenderError> {
        let doing = "making room for the sun's shadows";
        // The count is a small constant.
        #[allow(clippy::cast_possible_truncation)]
        let pictures = DepthLayers::new(gpu, SIZE, PICTURES as u32, doing)?;
        let sampler = comparing_sampler(gpu, doing)?;
        Ok(Self {
            gpu: Arc::clone(gpu),
            pictures,
            sampler,
            drawn: RefCell::new([None; PICTURES]),
        })
    }

    /// The pictures that have to be drawn before `cascades` of the level
    /// numbered `level` can be read: those that hold something else, or
    /// nothing yet. They are counted as drawn from now on.
    pub(crate) fn take_stale(
        &self,
        level: u64,
        cascades: &[Cascade; PICTURES],
    ) -> [bool; PICTURES] {
        let mut stale = [true; PICTURES];
        let Ok(mut drawn) = self.drawn.try_borrow_mut() else {
            return stale;
        };
        for ((stale, drawn), cascade) in stale.iter_mut().zip(drawn.iter_mut()).zip(cascades) {
            let wanted = Some((level, cascade.clip));
            *stale = *drawn != wanted;
            *drawn = wanted;
        }
        stale
    }
}

impl Drop for ShadowMap {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the map, so the sampler is not in use.
        unsafe { self.gpu.device().destroy_sampler(self.sampler, None) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL: [[f32; 3]; 2] = [[-50.0, 0.0, -50.0], [50.0, 20.0, 50.0]];

    /// Where `cascade` puts a point of the world.
    fn placed(cascade: &Cascade, point: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|row| {
            cascade.clip[0][row] * point[0]
                + cascade.clip[1][row] * point[1]
                + cascade.clip[2][row] * point[2]
                + cascade.clip[3][row]
        })
    }

    #[test]
    fn each_square_holds_the_eye_and_the_whole_depth_of_the_level() {
        let eye = [3.3, 1.7, -12.9];
        let sun = unit([0.4, 0.77, 0.5]);
        for (cascade, half) in cascades(eye, sun, LEVEL).iter().zip(REACH) {
            let [x, y, z] = placed(cascade, eye);
            // Never more than half a step from the middle.
            assert!(x.abs() <= 1.0 / 32.0 + 1e-4 && y.abs() <= 1.0 / 32.0 + 1e-4);
            assert!((0.0..=1.0).contains(&z));
            for corner in [LEVEL[0], LEVEL[1], [-50.0, 20.0, 50.0]] {
                let depth = placed(cascade, corner)[2];
                assert!((0.0..=1.0).contains(&depth), "{depth}");
            }
            // Nearer the sun is a smaller depth.
            let nearer = [0, 1, 2].map(|axis| eye[axis] + sun[axis]);
            assert!(placed(cascade, nearer)[2] < z);
            assert!((cascade.texel * SIZE as f32 - 2.0 * half).abs() < 1e-3);
        }
    }

    #[test]
    fn a_square_stays_put_until_the_eye_has_gone_a_step_and_then_moves_whole_pixels() {
        let sun = unit([0.4, 0.77, 0.5]);
        let first = cascades([0.0, 1.7, 0.0], sun, LEVEL);
        // A few centimetres, and looking round, change nothing.
        assert_eq!(cascades([0.05, 1.7, -0.03], sun, LEVEL), first);
        // Far enough to move the nearest square, which moves by whole pixels.
        let moved = cascades([3.0, 1.7, 0.0], sun, LEVEL);
        assert_ne!(moved[0], first[0]);
        for axis in 0..2 {
            let pixels = (moved[0].middle[axis] - first[0].middle[axis]) / first[0].texel;
            assert!((pixels - pixels.round()).abs() < 1e-3, "{pixels}");
        }
        // The widest takes a step of 13.5 metres and has not moved.
        assert_eq!(moved[3], first[3]);
    }

    #[test]
    fn only_what_the_sun_sees_in_a_square_is_drawn_into_it() {
        let squares = cascades([0.0, 1.7, 0.0], [0.0, 1.0, 0.0], LEVEL);
        assert!(squares[0].holds([5.0, 0.0, 5.0], 1.0));
        // Just outside, but big enough to reach in.
        assert!(squares[0].holds([10.0, 0.0, 0.0], 3.0));
        assert!(!squares[0].holds([30.0, 0.0, 0.0], 3.0));
        assert!(squares[2].holds([30.0, 0.0, 0.0], 3.0));
        // High above still counts: it is between the sun and the square.
        assert!(squares[0].holds([0.0, 500.0, 0.0], 1.0));
    }

    #[test]
    fn a_sun_straight_overhead_and_a_level_with_nothing_in_it_still_work() {
        let empty = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
        for cascade in cascades([0.0; 3], [0.0, 1.0, 0.0], empty) {
            assert!(cascade.clip.iter().flatten().all(|part| part.is_finite()));
        }
        for cascade in cascades([f32::NAN; 3], [0.0, 0.0, 0.0], LEVEL) {
            assert!(cascade.clip.iter().flatten().all(|part| part.is_finite()));
        }
    }
}
