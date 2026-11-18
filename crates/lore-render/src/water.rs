//! Water on the graphics card: the pictures its waves are read from, the
//! square of rings round the eye, and the shape of each body of still water.
//!
//! Three things are worked out before a frame's picture is drawn:
//!
//! - the wind's waves, added up into pictures of a square of water that
//!   repeat without a seam (`waves.rs`, `shaders/waves.wgsl`);
//! - the rings and wakes sent out by whoever is in the water, in a square
//!   that follows the eye (`shaders/ripples.wgsl`);
//! - which body of still water the eye is nearest, so that everything
//!   drawn can ask how deep the water is over it.
//!
//! Still water is drawn from the shape it took when it filled its hollow
//! ([`lore_assets::Water`]): a grid of points that is fine round the eye
//! and coarser the farther off, laid over the body and cut to its shore by
//! the picture of its depths.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ash::vk;
use lore_assets::{Scene, Water};

use crate::buffer::{Buffer, once};
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::scene::Picture;
use crate::sky;
use crate::swapchain::COLOUR;
use crate::waves::{EACH, LAYERS, Waves};

const WAVE_SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/waves.spv"));
const RIPPLE_SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ripples.spv"));

const DOING: &str = "making the water";

/// How many pixels each picture of the wind's waves is along a side.
const WAVE_SIDE: u32 = 256;
/// Four sixteen-bit floats: heights of millimetres beside slopes near one.
const WAVE_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
/// How far, as a share of its height, a wave carries the surface towards
/// its crest. At 1 the steepest crests would fold over.
const CARRIED: f32 = 0.85;

/// How many points the square of rings has along a side, and how far apart
/// they are in metres: a square about thirteen metres across.
pub(crate) const RING_SIDE: u32 = 512;
pub(crate) const RING_APART: f32 = 0.025;
/// How long one step of the rings is, in seconds. Two are always taken
/// together, so that the newest heights end up in the same picture.
const RING_STEP: f32 = 1.0 / 120.0;
/// The most pairs of steps taken in one frame: a frame that took longer
/// than this many sixtieths does not owe the rest.
const MOST_PAIRS: u32 = 3;
/// How fast rings die away, as a share each second.
const RINGS_DIE: f32 = 0.35;
/// How many seconds after the last thing stirred the water its rings are
/// too faint to see, and are no longer worked out.
const RINGS_GONE: f32 = 20.0;
/// How far each side of a point its neighbours count: the shader's `REACH`.
const RING_REACH: i32 = 6;
/// How many things can stir the water in one frame.
pub(crate) const MOST_STIRS: usize = 32;
/// The size in bytes of the ring shader's `Step`.
const STEP_BYTES: usize = 43 * 16 + 3 * 16 + MOST_STIRS * 32;

/// How many squares the grid water is drawn with has along a side, and
/// how far apart its points are, in metres, where it is finest. Each
/// coarser grid is twice as wide with points twice as far apart.
pub(crate) const GRID_SQUARES: u32 = 192;
pub(crate) const GRID_FINEST: f32 = 0.015;
/// The most grids one body of water is drawn with: the last reaches more
/// than ninety kilometres from the eye, so that the widest body there can
/// be (8192 points eight metres apart) is drawn whole from anywhere over
/// it. A body takes only as many as reach it, and draws only those of them
/// it is not wholly under a finer one of, so a pond costs no more for it.
const MOST_GRIDS: u32 = 17;

/// Something that stirs the water for one frame: a leg wading, a body
/// swimming, a stone or a drop going in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stir {
    /// Where it is: x and z, in metres.
    pub place: [f32; 2],
    /// How wide it is, in metres.
    pub wide: f32,
    /// How fast it pushes the surface up, in metres a second: below
    /// nothing for something going in, which pushes it down.
    pub lift: f32,
    /// How fast it moves along x and z, in metres a second. Water piles
    /// up ahead of what moves through it and is drawn down behind.
    pub moving: [f32; 2],
    /// How much of a body it is to the water it moves through: 0 for a
    /// drop, about 1 for a person.
    pub body: f32,
    /// How white it churns the water, from 0 to 1.
    pub churn: f32,
}

/// The rows of the shader's `View` that say what water there is: the body
/// the eye is nearest (three rows, the last number of the third how far
/// the waves can lift its surface at the most), each layer of waves
/// (four), the square of rings, and one spare.
pub(crate) type WaterRows = [[f32; 4]; 9];

/// A picture of several layers: all of them for a shader to read, and each
/// by itself to draw into.
struct Layered {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    layers: Vec<vk::ImageView>,
}

impl Layered {
    const fn null() -> Self {
        Self {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            layers: Vec::new(),
        }
    }

    /// Fills in `self` with a square picture of `layers` layers. What was
    /// made before a failure is left for [`destroy`](Self::destroy).
    fn create(&mut self, gpu: &Gpu, side: u32, layers: u32) -> Result<(), RenderError> {
        let device = gpu.device();
        let range = |layer: u32, count: u32| vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: layer,
            layer_count: count,
        };
        // SAFETY: the device is alive, and each object is created from
        // valid settings and stored in `self` straight away. The memory is
        // as large as the image asks for and of a kind it allows.
        unsafe {
            self.image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(WAVE_FORMAT)
                        .extent(vk::Extent3D {
                            width: side,
                            height: side,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(layers)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .usage(
                            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                        ),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let needs = device.get_image_memory_requirements(self.image);
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
            self.memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(needs.size)
                        .memory_type_index(kind),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            device
                .bind_image_memory(self.image, self.memory, 0)
                .map_err(RenderError::driver(DOING))?;
            self.view = device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(self.image)
                        .view_type(vk::ImageViewType::TYPE_2D_ARRAY)
                        .format(WAVE_FORMAT)
                        .subresource_range(range(0, layers)),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            for layer in 0..layers {
                let view = device
                    .create_image_view(
                        &vk::ImageViewCreateInfo::default()
                            .image(self.image)
                            .view_type(vk::ImageViewType::TYPE_2D)
                            .format(WAVE_FORMAT)
                            .subresource_range(range(layer, 1)),
                        None,
                    )
                    .map_err(RenderError::driver(DOING))?;
                self.layers.push(view);
            }
        }
        Ok(())
    }

    /// # Safety
    ///
    /// The picture must have been made on `gpu` (or be null), and the
    /// graphics card must have finished using it.
    unsafe fn destroy(&self, gpu: &Gpu) {
        let device = gpu.device();
        // SAFETY: the caller guarantees the handles are this device's and
        // idle; destroying a null handle is allowed.
        unsafe {
            for layer in &self.layers {
                device.destroy_image_view(*layer, None);
            }
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// A sixteen-bit float holding `value` as nearly as it can.
fn half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let fraction = bits & 0x007f_ffff;
    if !value.is_finite() || exponent >= 31 {
        // Too large to hold: the largest there is.
        return sign | 0x7bff;
    }
    if exponent <= 0 {
        // Too small for a whole exponent: a smaller fraction, or nothing.
        if exponent < -10 {
            return sign;
        }
        let whole = (fraction | 0x0080_0000) >> (1 - exponent);
        // Fits: at most eleven bits are left.
        #[allow(clippy::cast_possible_truncation)]
        return sign | ((whole + 0x1000) >> 13) as u16;
    }
    // Rounded to the nearest; rounding up into the exponent is right too.
    // Fits: five bits of exponent and ten of fraction.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let packed = (((exponent as u32) << 10) | (fraction >> 13)) as u16;
    let rounds_up = fraction & 0x1000 != 0;
    sign | (packed + u16::from(rounds_up))
}

/// A small picture of sixteen-bit floats holding `numbers`, `parts` to a
/// pixel, copied to the graphics card and ready for shaders to read.
fn picture_of(
    gpu: &Arc<Gpu>,
    [width, height]: [u32; 2],
    format: vk::Format,
    numbers: &[f32],
) -> Result<Picture, RenderError> {
    let mut picture = Picture::null();
    let made = (|| {
        let extent = vk::Extent2D { width, height };
        picture.create(
            gpu,
            extent,
            format,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            COLOUR,
            DOING,
        )?;
        let bytes: Vec<u8> = numbers
            .iter()
            .flat_map(|number| half(*number).to_ne_bytes())
            .collect();
        let staging = Buffer::staging(gpu, bytes.len() as u64, DOING)?;
        staging.fill(&bytes, DOING)?;
        let device = gpu.device();
        let image = picture.image;
        let barrier = || {
            vk::ImageMemoryBarrier2::default()
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(COLOUR)
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
                        .image_extent(extent.into())],
                );
                let to_read = [barrier()
                    .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(
                        vk::PipelineStageFlags2::FRAGMENT_SHADER
                            | vk::PipelineStageFlags2::VERTEX_SHADER,
                    )
                    .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            })
        }
    })();
    match made {
        Ok(()) => Ok(picture),
        Err(error) => {
            // SAFETY: nothing has used the picture but the commands above,
            // which were waited for.
            unsafe { picture.destroy(gpu) };
            Err(error)
        }
    }
}

/// What a body's set holds: the picture of its depths.
pub(crate) fn body_layout(gpu: &Gpu) -> Result<vk::DescriptorSetLayout, RenderError> {
    let bindings = [vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)];
    // SAFETY: the device is alive and the settings live until the call returns.
    unsafe {
        gpu.device().create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
            None,
        )
    }
    .map_err(RenderError::driver(DOING))
}

/// The number the next body of water is known by. None is 0, which stands
/// for "no water near".
static NEXT_BODY: AtomicU64 = AtomicU64::new(1);

/// One body of still water, ready to draw.
pub(crate) struct Body {
    /// A number no other body has had, by which a set that points at this
    /// one's picture is told from one that points at a picture long gone:
    /// the graphics card hands the same handle out again.
    pub(crate) id: u64,
    pub(crate) level: f32,
    pub(crate) origin: [f32; 2],
    pub(crate) cell: f32,
    pub(crate) size: [u32; 2],
    pub(crate) colour: [f32; 4],
    /// The water itself, to ask how deep it is at a place.
    water: Water,
    /// How deep it is at each point of its grid, as a picture.
    depths: Picture,
    pub(crate) set: vk::DescriptorSet,
}

impl Body {
    /// The picture of its depths.
    pub(crate) fn depths(&self) -> vk::ImageView {
        self.depths.view
    }

    /// The x and z of the corner of its grid with the most of both.
    fn far_corner(&self) -> [f32; 2] {
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let reach = [0, 1].map(|axis| (self.size[axis].max(1) - 1) as f32 * self.cell);
        [self.origin[0] + reach[0], self.origin[1] + reach[1]]
    }

    /// How far outside the body's grid `eye` is, over the ground.
    pub(crate) fn away(&self, eye: [f32; 3]) -> f32 {
        let far = self.far_corner();
        let out = [0, 1].map(|axis| {
            (self.origin[axis] - eye[axis * 2])
                .max(eye[axis * 2] - far[axis])
                .max(0.0)
        });
        out[0].hypot(out[1])
    }

    /// How many grids, each twice as wide as the one before, it takes to
    /// cover the whole body from `eye`.
    pub(crate) fn grids(&self, eye: [f32; 3]) -> u32 {
        let far = self.far_corner();
        let reach = [0, 1]
            .map(|axis| {
                (eye[axis * 2] - self.origin[axis])
                    .abs()
                    .max((eye[axis * 2] - far[axis]).abs())
            })
            .into_iter()
            .fold(0.0, f32::max);
        // The finest grid reaches half its width from near the eye, less
        // the two points it may be off by.
        #[allow(clippy::cast_precision_loss)]
        let mut covered = (GRID_SQUARES as f32 / 2.0 - 2.0) * GRID_FINEST;
        let mut grids = 1;
        while covered < reach && grids < MOST_GRIDS {
            covered *= 2.0;
            grids += 1;
        }
        grids
    }

    /// Whether any of the body is drawn by grid `grid` as laid round
    /// `eye`: whether the grid's square reaches the body at all, and is
    /// not wholly hidden by the next finer grid lying over its middle.
    pub(crate) fn drawn_by(&self, eye: [f32; 3], grid: u32) -> bool {
        let (apart, middle) = grid_laid(eye, grid);
        // The grid is a couple of hundred squares across.
        #[allow(clippy::cast_precision_loss)]
        let half = GRID_SQUARES as f32 / 2.0 * apart;
        let far = self.far_corner();
        // A little more than the body: its shore is kept past its edge.
        let (low, high) = (
            [self.origin[0] - self.cell, self.origin[1] - self.cell],
            [far[0] + self.cell, far[1] + self.cell],
        );
        let reaches = (0..2)
            .all(|axis| low[axis] <= middle[axis] + half && high[axis] >= middle[axis] - half);
        if !reaches || grid == 0 {
            return reaches;
        }
        // The finer grid's middle, which the shader is given, and how far
        // round it the shader leaves this grid out.
        let (_, finer) = grid_laid(eye, grid - 1);
        let covered = half / 2.0 - apart;
        let hidden = (0..2)
            .all(|axis| low[axis] >= finer[axis] - covered && high[axis] <= finer[axis] + covered);
        !hidden
    }

    /// The rows of the shader's `View` that describe this body; the last
    /// number is for how far the waves on it reach.
    fn rows(&self) -> [[f32; 4]; 3] {
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let size = self.size.map(|points| points as f32);
        [
            [self.level, self.cell, 1.0, self.colour[3]],
            [self.origin[0], self.origin[1], size[0], size[1]],
            [self.colour[0], self.colour[1], self.colour[2], 0.0],
        ]
    }
}

/// The bodies of still water of one scene.
pub(crate) struct Bodies {
    gpu: Arc<Gpu>,
    pub(crate) list: Vec<Body>,
    layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
}

impl Bodies {
    /// Copies the depths of each body of water in `scene` to the graphics card.
    pub(crate) fn new(gpu: &Arc<Gpu>, scene: &Scene) -> Result<Self, RenderError> {
        let mut bodies = Self {
            gpu: Arc::clone(gpu),
            list: Vec::with_capacity(scene.waters.len()),
            layout: vk::DescriptorSetLayout::null(),
            pool: vk::DescriptorPool::null(),
        };
        let sound: Vec<&Water> = scene
            .waters
            .iter()
            .filter(|water| {
                let [across, deep] = water.size;
                across >= 2
                    && deep >= 2
                    && across <= 8192
                    && deep <= 8192
                    && water.depths.len() == across as usize * deep as usize
                    && water.cell > 0.0
                    && water.level.is_finite()
            })
            .collect();
        if sound.is_empty() {
            return Ok(bodies);
        }
        let device = gpu.device();
        let count = u32::try_from(sound.len()).unwrap_or(u32::MAX);
        let sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(count)];
        bodies.layout = body_layout(gpu)?;
        // SAFETY: the device is alive, and the pool is stored straight away.
        bodies.pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(count)
                    .pool_sizes(&sizes),
                None,
            )
        }
        .map_err(RenderError::driver(DOING))?;
        for water in sound {
            let depths = picture_of(gpu, water.size, vk::Format::R16_SFLOAT, &water.depths)?;
            let layouts = [bodies.layout];
            // SAFETY: the pool has room for one set for each body, and the
            // picture written into the set was just made on this device.
            let set = unsafe {
                let set = device
                    .allocate_descriptor_sets(
                        &vk::DescriptorSetAllocateInfo::default()
                            .descriptor_pool(bodies.pool)
                            .set_layouts(&layouts),
                    )
                    .map(|sets| sets.first().copied().unwrap_or_default());
                if let Ok(set) = set {
                    let pictures = [vk::DescriptorImageInfo::default()
                        .image_view(depths.view)
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                    device.update_descriptor_sets(
                        &[vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(0)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(&pictures)],
                        &[],
                    );
                }
                set
            };
            // Kept before the set is looked at, so that it is freed if
            // there was none to be had.
            bodies.list.push(Body {
                id: NEXT_BODY.fetch_add(1, Ordering::Relaxed),
                level: water.level,
                origin: water.origin,
                cell: water.cell,
                size: water.size,
                colour: water.colour,
                water: water.clone(),
                depths,
                set: vk::DescriptorSet::null(),
            });
            let set = set.map_err(RenderError::driver(DOING))?;
            if let Some(body) = bodies.list.last_mut() {
                body.set = set;
            }
        }
        Ok(bodies)
    }

    /// The body `eye` is in or over, or else the one it is nearest to.
    /// (A pool inside the square a moat lies in is told from the moat by
    /// which of them there is water of at the eye.)
    pub(crate) fn nearest(&self, eye: [f32; 3], rise: f32) -> Option<&Body> {
        self.list
            .iter()
            .filter(|body| body.water.depth_at(eye[0], eye[2]) + rise > 0.0)
            .max_by(|one, other| one.level.total_cmp(&other.level))
            .or_else(|| {
                self.list
                    .iter()
                    .min_by(|one, other| one.away(eye).total_cmp(&other.away(eye)))
            })
    }
}

impl Drop for Bodies {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping a scene's meshes. Destroying the pool frees its sets.
        unsafe {
            let device = self.gpu.device();
            for body in &self.list {
                body.depths.destroy(&self.gpu);
            }
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.layout, None);
        }
    }
}

/// `number`, or nothing if it is not a number.
fn finite(number: f32) -> f32 {
    if number.is_finite() { number } else { 0.0 }
}

/// How grid `grid` is laid for an eye at `eye`: how far apart its points
/// are, and the x and z of its middle. Grid 0 is the finest, and each one
/// after it is twice as wide with its points twice as far apart. The
/// middle is the eye to the nearest two points, so that the grid moves by
/// whole squares and its points stay where they were.
///
/// The shader is given the middle of the next finer grid as this works it
/// out, and does not work it out again: the graphics card rounds halves
/// another way and divides less exactly, and a grid left out round a
/// middle one point off leaves a crack.
pub(crate) fn grid_laid(eye: [f32; 3], grid: u32) -> (f32, [f32; 2]) {
    // Seventeen grids at most: the power fits an f32 exactly.
    #[allow(clippy::cast_possible_wrap)]
    let apart = GRID_FINEST * 2.0_f32.powi(grid as i32);
    let middle = [0, 2].map(|axis| (finite(eye[axis]) / (2.0 * apart)).round() * 2.0 * apart);
    (apart, middle)
}

/// What one frame in progress needs of its own: the numbers the two
/// shaders read, and the sets that point at them.
struct Frame {
    waves: Buffer,
    wave_set: vk::DescriptorSet,
    step: Buffer,
    /// Reads the first picture of rings to draw the second, and the other
    /// way round.
    ring_sets: [vk::DescriptorSet; 2],
    /// The body of water whose depths those two sets were last pointed
    /// at; 0 for none.
    depths: Cell<u64>,
}

/// Everything about water that is the same for every scene.
pub(crate) struct WaterWorks {
    gpu: Arc<Gpu>,
    /// Where the surface is carried to, and how it slopes: a layer of each
    /// picture for each layer of waves.
    shift: Layered,
    slope: Layered,
    /// Smooths between a picture's points and starts again at its far side.
    pub(crate) repeating: vk::Sampler,
    /// Smooths between a picture's points and stops at its edge.
    pub(crate) stopping: vk::Sampler,
    /// The square of rings, now and as it was half a pair of steps ago.
    rings: [Picture; 2],
    /// A picture of one point that says "no water here".
    dry: Picture,
    wave_layout: vk::DescriptorSetLayout,
    wave_pipeline_layout: vk::PipelineLayout,
    wave_pipeline: vk::Pipeline,
    ring_layout: vk::DescriptorSetLayout,
    ring_pipeline_layout: vk::PipelineLayout,
    ring_pipeline: vk::Pipeline,
    pool: vk::DescriptorPool,
    frames: Vec<Frame>,
    /// The points of the grid still water is drawn with, and its triangles.
    grid: Option<(Buffer, Buffer)>,
    /// What the height of each neighbour counts for in the rings' next step.
    weights: Vec<f32>,
    /// The waves being drawn, and the wind and width of water they are for.
    waves: RefCell<(Waves, [f32; 4])>,
    /// Where the first point of the square of rings was when it was last
    /// worked out.
    corner: Cell<Option<[f32; 2]>>,
    /// How many seconds of rings have not been worked out yet.
    owed: Cell<f32>,
    /// How many seconds it is since anything stirred the water.
    quiet: Cell<f32>,
    /// What is stirring the water, each with how many seconds of its
    /// stirring the rings have still to be given. A frame may come with
    /// no step of the rings to give it in, or with several.
    pending: RefCell<Vec<(Stir, f32)>>,
    /// Whether the wind's waves have been added up at least once: with no
    /// water in a scene they are not added up again.
    waves_drawn: Cell<bool>,
}

/// How many numbers the grid's triangles are.
pub(crate) const GRID_INDICES: u32 = GRID_SQUARES * GRID_SQUARES * 6;

/// What the height of each neighbour counts for in the next height of a
/// point of the rings: `RING_REACH` each side, row by row.
///
/// It is the rule of waves on deep water turned into a table: how strongly
/// a surface that stands high at one point is pulled back there by the
/// water `r` points away, for waves down to about six points long
/// (Tessendorf's iWave, with the table worked out as an integral so that
/// rings run at the speed real ones do).
fn ring_weights() -> Vec<f32> {
    // How much shorter waves than about six points are left out: they
    // could not be drawn anyway.
    let soft = 0.5_f64;
    let across = 2 * RING_REACH + 1;
    let mut weights = Vec::with_capacity((across * across) as usize);
    for down in -RING_REACH..=RING_REACH {
        for along in -RING_REACH..=RING_REACH {
            let far = f64::from(along * along + down * down).sqrt();
            // The integral, over every length of wave, of how strongly
            // waves of that length pull, seen from `far` points away.
            let step = 0.002_f64;
            let mut sum = 0.0;
            let mut turns = step / 2.0;
            while turns < 12.0 {
                sum += turns * turns * (-soft * turns * turns).exp() * bessel(turns * far) * step;
                turns += step;
            }
            // Heights are single precision on the graphics card.
            #[allow(clippy::cast_possible_truncation)]
            weights.push((sum / std::f64::consts::TAU) as f32);
        }
    }
    // Water that is level everywhere is pulled nowhere: the table adds up
    // to nothing. Cut off at six points it does not: the pull of all the
    // water farther off than that, each bit of it small, is missing. It
    // is put back where it was lost, shared over the table's outer points
    // as it thins with distance. (Taken off the middle instead, it would
    // turn the shortest waves' pull into a push, and they would grow
    // without end.)
    let over: f32 = weights.iter().sum();
    let thin = |along: i32, down: i32| {
        let far = f64::from(along * along + down * down).sqrt();
        // Shares are single precision like the table.
        #[allow(clippy::cast_possible_truncation)]
        let share = if far >= 3.0 {
            (1.0 / (far * far * far)) as f32
        } else {
            0.0
        };
        share
    };
    let places = || {
        (-RING_REACH..=RING_REACH)
            .flat_map(|down| (-RING_REACH..=RING_REACH).map(move |along| (along, down)))
    };
    let whole: f32 = places().map(|(along, down)| thin(along, down)).sum();
    for (weight, (along, down)) in weights.iter_mut().zip(places()) {
        *weight -= over * thin(along, down) / whole;
    }
    weights
}

/// The Bessel function of the first kind and order nothing: how a ring
/// spreading on water stands at each distance from its middle.
fn bessel(x: f64) -> f64 {
    let x = x.abs();
    if x < 8.0 {
        // Its series, which settles quickly this near nothing.
        let quarter = x * x / 4.0;
        let (mut term, mut sum) = (1.0, 1.0);
        for number in 1..40 {
            let number = f64::from(number);
            term *= -quarter / (number * number);
            sum += term;
        }
        sum
    } else {
        // Far out it is a cosine that dies away; the two corrections to
        // that are fitted (Abramowitz and Stegun 9.4.3).
        let z = 8.0 / x;
        let y = z * z;
        let size = 1.0
            + y * (-0.109_862_862_7e-2
                + y * (0.273_451_040_7e-4 + y * (-0.207_337_063_9e-5 + y * 0.209_388_721_1e-6)));
        let turn = -0.156_249_999_5e-1
            + y * (0.143_048_876_5e-3
                + y * (-0.691_114_765_1e-5 + y * (0.762_109_516_1e-6 - y * 0.934_935_152e-7)));
        let along = x - std::f64::consts::FRAC_PI_4;
        (std::f64::consts::FRAC_2_PI / x).sqrt() * (along.cos() * size - z * along.sin() * turn)
    }
}

impl WaterWorks {
    /// Makes the pictures and the shaders, for up to `frames` frames in
    /// progress at once.
    pub(crate) fn new(gpu: &Arc<Gpu>, frames: usize) -> Result<Self, RenderError> {
        let [along_x, along_z, speed] = sky::WIND;
        let fetch = 40.0;
        let mut works = Self {
            gpu: Arc::clone(gpu),
            shift: Layered::null(),
            slope: Layered::null(),
            repeating: vk::Sampler::null(),
            stopping: vk::Sampler::null(),
            rings: [Picture::null(), Picture::null()],
            dry: Picture::null(),
            wave_layout: vk::DescriptorSetLayout::null(),
            wave_pipeline_layout: vk::PipelineLayout::null(),
            wave_pipeline: vk::Pipeline::null(),
            ring_layout: vk::DescriptorSetLayout::null(),
            ring_pipeline_layout: vk::PipelineLayout::null(),
            ring_pipeline: vk::Pipeline::null(),
            pool: vk::DescriptorPool::null(),
            frames: Vec::with_capacity(frames.max(1)),
            grid: None,
            weights: ring_weights(),
            waves: RefCell::new((
                Waves::new([along_x, along_z], speed, fetch),
                [along_x, along_z, speed, fetch],
            )),
            corner: Cell::new(None),
            owed: Cell::new(0.0),
            quiet: Cell::new(RINGS_GONE),
            pending: RefCell::new(Vec::with_capacity(2 * MOST_STIRS)),
            waves_drawn: Cell::new(false),
        };
        // Dropping `works` cleans up whatever was created.
        let device = gpu.device();
        works.shift.create(gpu, WAVE_SIDE, LAYERS as u32)?;
        works.slope.create(gpu, WAVE_SIDE, LAYERS as u32)?;
        works.dry = picture_of(gpu, [1, 1], vk::Format::R16_SFLOAT, &[Water::DRY])?;
        for ring in &mut works.rings {
            ring.create(
                gpu,
                vk::Extent2D {
                    width: RING_SIDE,
                    height: RING_SIDE,
                },
                WAVE_FORMAT,
                vk::ImageUsageFlags::COLOR_ATTACHMENT
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_DST,
                COLOUR,
                DOING,
            )?;
        }
        works.still()?;
        let sampler = |mode: vk::SamplerAddressMode| {
            vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                .address_mode_u(mode)
                .address_mode_v(mode)
                .address_mode_w(mode)
        };
        let count = u32::try_from(frames.max(1)).unwrap_or(1);
        // SAFETY: the device is alive, and each object is created from
        // valid settings that live until the call returns, and stored in
        // `works` straight away.
        unsafe {
            works.repeating = device
                .create_sampler(&sampler(vk::SamplerAddressMode::REPEAT), None)
                .map_err(RenderError::driver(DOING))?;
            works.stopping = device
                .create_sampler(&sampler(vk::SamplerAddressMode::CLAMP_TO_EDGE), None)
                .map_err(RenderError::driver(DOING))?;
            let block = |binding: u32, kind: vk::DescriptorType| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(binding)
                    .descriptor_type(kind)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            };
            works.wave_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default()
                        .bindings(&[block(0, vk::DescriptorType::UNIFORM_BUFFER)]),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            works.ring_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                        block(0, vk::DescriptorType::UNIFORM_BUFFER),
                        block(1, vk::DescriptorType::SAMPLED_IMAGE),
                        block(2, vk::DescriptorType::SAMPLED_IMAGE),
                        block(3, vk::DescriptorType::SAMPLER),
                    ]),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let pushed = |size: u32| {
                [vk::PushConstantRange::default()
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                    .size(size)]
            };
            let layouts = [works.wave_layout];
            works.wave_pipeline_layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&layouts)
                        .push_constant_ranges(&pushed(20)),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let layouts = [works.ring_layout];
            works.ring_pipeline_layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&layouts)
                        .push_constant_ranges(&pushed(16)),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(count * 3),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(count * 4),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::SAMPLER)
                    .descriptor_count(count * 2),
            ];
            works.pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(count * 3)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
        }
        works.wave_pipeline = works.pipeline(WAVE_SHADER, works.wave_pipeline_layout, 2)?;
        works.ring_pipeline = works.pipeline(RIPPLE_SHADER, works.ring_pipeline_layout, 1)?;
        for _ in 0..count {
            works.add_frame()?;
        }
        works.grid = Some(grid(gpu)?);
        Ok(works)
    }

    /// The pictures every shader that draws the scene reads, in the order
    /// of their bindings: where the waves carry the surface, how they
    /// slope it, the rings, and (for when no still water is near) the
    /// picture that says "dry".
    pub(crate) fn pictures(&self) -> [vk::ImageView; 4] {
        [
            self.shift.view,
            self.slope.view,
            self.rings[0].view,
            self.dry.view,
        ]
    }

    /// The grid still water is drawn with: its points and its triangles.
    pub(crate) fn grid(&self) -> Option<(&Buffer, &Buffer)> {
        self.grid
            .as_ref()
            .map(|(points, triangles)| (points, triangles))
    }

    /// Makes the rings flat: no water has been stirred yet.
    fn still(&self) -> Result<(), RenderError> {
        let device = self.gpu.device();
        let images = [self.rings[0].image, self.rings[1].image];
        // SAFETY: the pictures are alive until `once` has waited for the
        // commands, and each is cleared in the layout the barrier before
        // left it in.
        unsafe {
            once(&self.gpu, DOING, |commands| {
                for image in images {
                    let barrier = || {
                        vk::ImageMemoryBarrier2::default()
                            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                            .image(image)
                            .subresource_range(COLOUR)
                    };
                    let to_clear = [barrier()
                        .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .old_layout(vk::ImageLayout::UNDEFINED)
                        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)];
                    device.cmd_pipeline_barrier2(
                        commands,
                        &vk::DependencyInfo::default().image_memory_barriers(&to_clear),
                    );
                    device.cmd_clear_color_image(
                        commands,
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearColorValue { float32: [0.0; 4] },
                        &[COLOUR],
                    );
                    let to_read = [barrier()
                        .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(
                            vk::PipelineStageFlags2::FRAGMENT_SHADER
                                | vk::PipelineStageFlags2::VERTEX_SHADER,
                        )
                        .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                    device.cmd_pipeline_barrier2(
                        commands,
                        &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                    );
                }
            })
        }
    }

    /// A pipeline that draws one triangle over the whole of `targets`
    /// pictures with the shader in `code`.
    fn pipeline(
        &self,
        code: &[u8],
        layout: vk::PipelineLayout,
        targets: usize,
    ) -> Result<vk::Pipeline, RenderError> {
        let device = self.gpu.device();
        let words = ash::util::read_spv(&mut std::io::Cursor::new(code)).map_err(|_| {
            // Only possible if the build script wrote a broken file.
            RenderError::Driver {
                doing: DOING,
                code: vk::Result::ERROR_INVALID_SHADER_NV,
            }
        })?;
        // SAFETY: `words` is SPIR-V produced and validated by naga at build time.
        let module = unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
        }
        .map_err(RenderError::driver(DOING))?;
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(module)
                .name(c"vertex"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(module)
                .name(c"fragment"),
        ];
        let no_points = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default();
        let attachments = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA); 2];
        let blend = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(&attachments[..targets.min(2)]);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let formats = [WAVE_FORMAT; 2];
        let mut rendering = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&formats[..targets.min(2)]);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&no_points)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .depth_stencil_state(&depth)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(layout)
            .push_next(&mut rendering);
        // SAFETY: `info` and everything it points to are alive for the
        // call; the module holds both entry points named in `stages`, and
        // is only needed while the pipeline is being created.
        let created = unsafe {
            let created = device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                std::slice::from_ref(&info),
                None,
            );
            device.destroy_shader_module(module, None);
            created
        };
        match created {
            Ok(pipelines) if !pipelines.is_empty() => Ok(pipelines[0]),
            other => Err(RenderError::driver(DOING)(
                other.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN),
            )),
        }
    }

    /// Makes what one more frame in progress needs.
    fn add_frame(&mut self) -> Result<(), RenderError> {
        let device = self.gpu.device();
        let waves = Buffer::writable(
            &self.gpu,
            (LAYERS * EACH * 16) as u64,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            DOING,
        )?;
        let step = Buffer::writable(
            &self.gpu,
            STEP_BYTES as u64,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            DOING,
        )?;
        let layouts = [self.wave_layout, self.ring_layout, self.ring_layout];
        // SAFETY: the pool was made with room for three sets a frame, and
        // everything written into them is alive and of this device.
        let sets = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.pool)
                    .set_layouts(&layouts),
            )
        }
        .map_err(RenderError::driver(DOING))?;
        let [wave_set, first, second] = sets[..] else {
            return Err(RenderError::Driver {
                doing: DOING,
                code: vk::Result::ERROR_UNKNOWN,
            });
        };
        let wave_block = [vk::DescriptorBufferInfo::default()
            .buffer(waves.handle)
            .range(vk::WHOLE_SIZE)];
        let step_block = [vk::DescriptorBufferInfo::default()
            .buffer(step.handle)
            .range(vk::WHOLE_SIZE)];
        let picture = |view: vk::ImageView| {
            [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
        };
        let read = [picture(self.rings[0].view), picture(self.rings[1].view)];
        let dry = picture(self.dry.view);
        let smooth = [vk::DescriptorImageInfo::default().sampler(self.stopping)];
        let mut writes = vec![
            vk::WriteDescriptorSet::default()
                .dst_set(wave_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&wave_block),
        ];
        for (set, before) in [first, second].into_iter().zip(&read) {
            writes.extend([
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&step_block),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(before),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&dry),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(3)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .image_info(&smooth),
            ]);
        }
        // SAFETY: the sets were just allocated and nothing uses them yet.
        unsafe { device.update_descriptor_sets(&writes, &[]) };
        self.frames.push(Frame {
            waves,
            wave_set,
            step,
            ring_sets: [first, second],
            depths: Cell::new(0),
        });
        Ok(())
    }

    /// Records working out the waves and the rings for a frame seen from
    /// `eye`, `seconds` into the scene and `elapsed` seconds after the
    /// frame before, and gives the rows of the shader's `View` that say
    /// what water there is.
    ///
    /// `near` is the still water the eye is in or nearest, `any` whether
    /// the scene has water at all, `stirs` whatever is stirring the water
    /// during this frame, and `wind` the way the wind blows along x and z
    /// and how fast, in metres a second.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this device that is being
    /// recorded, outside a render pass. `near` must stay alive until the
    /// commands have run. `frame` picks which of the frames in progress
    /// this is, and the commands last recorded with it must have finished.
    pub(crate) unsafe fn record(
        &self,
        commands: vk::CommandBuffer,
        frame: usize,
        eye: [f32; 3],
        (seconds, elapsed): (f64, f32),
        (near, any): (Option<&Body>, bool),
        (stirs, wind): (&[Stir], [f32; 3]),
    ) -> Result<WaterRows, RenderError> {
        let mut rows = [[0.0; 4]; 9];
        let Some(slot) = self.frames.get(frame % self.frames.len().max(1)) else {
            return Ok(rows);
        };
        let elapsed = if elapsed.is_finite() {
            elapsed.clamp(0.0, 0.25)
        } else {
            0.0
        };
        // The waves the wind raises on water as wide as what is near. They
        // are worked out again when the wind has turned by a few degrees
        // or changed by a tenth, not at every breath of it; the stillest
        // air still ruffles water a little.
        let [along_x, along_z, speed] = wind;
        let speed = if speed.is_finite() {
            speed.clamp(0.5, 60.0)
        } else {
            sky::WIND[2]
        };
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let fetch = near.map_or(40.0, |body| {
            (body.size[0].max(body.size[1]) as f32 * body.cell).clamp(10.0, 20_000.0)
        });
        let mut reach = 0.0;
        if let Ok(mut waves) = self.waves.try_borrow_mut() {
            let wanted = [along_x, along_z, speed, fetch];
            // A different water is a different width for the wind to work on.
            let turned = waves.1[0] * along_x + waves.1[1] * along_z < 0.995;
            let changed = (waves.1[2] / speed).ln().abs() > 0.1;
            if (waves.1[3] / fetch).ln().abs() > 0.2 || turned || changed {
                *waves = (Waves::new([along_x, along_z], speed, fetch), wanted);
            }
            reach = waves.0.reach();
            for (row, layer) in rows[3..7].iter_mut().zip(&waves.0.layers) {
                // A layer's waves fit between 8 and 32 times into its square.
                *row = [
                    layer.tile,
                    layer.steepness,
                    layer.tile / 32.0,
                    layer.tile / 8.0,
                ];
            }
            // With no water to draw them on, once is enough: the pictures
            // only have to hold something.
            if any || !self.waves_drawn.get() {
                let mut numbers = [[0.0_f32; 4]; LAYERS * EACH];
                waves.0.numbers(seconds, &mut numbers);
                let mut bytes = [0_u8; LAYERS * EACH * 16];
                for (slot, number) in bytes
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(numbers.iter().flatten())
                {
                    *slot = number.to_ne_bytes();
                }
                // SAFETY: the buffer was made writable and this long, and
                // the caller guarantees the commands that read it last
                // have finished; its other guarantees are the ones
                // recording the waves asks for.
                unsafe {
                    slot.waves.write(&bytes)?;
                    self.record_waves(commands, slot, &waves.0);
                }
                self.waves_drawn.set(true);
            }
        }

        // The rings. Their square follows the eye by whole points, and is
        // only moved when a step of them is taken.
        let side = RING_SIDE as f32 * RING_APART;
        let wanted =
            [0, 2].map(|axis| ((finite(eye[axis]) - side / 2.0) / RING_APART).round() * RING_APART);
        let before = self.corner.get().unwrap_or(wanted);
        // Steps are taken in pairs, as many as the time that has passed
        // holds: the same rings at any frame rate. Truncating is the point.
        let owed = self.owed.get() + elapsed;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pairs = ((owed / (2.0 * RING_STEP)) as u32).min(MOST_PAIRS);
        #[allow(clippy::cast_precision_loss)]
        let stepped = pairs as f32 * 2.0 * RING_STEP;
        self.owed.set(if pairs == MOST_PAIRS {
            0.0
        } else {
            owed - stepped
        });
        // Water nothing has stirred for a good while is flat, near enough:
        // there is nothing to work out until something stirs it again.
        let quiet = if stirs.is_empty() {
            (self.quiet.get() + elapsed).min(2.0 * RINGS_GONE)
        } else {
            0.0
        };
        self.quiet.set(quiet);
        if let Ok(mut pending) = self.pending.try_borrow_mut() {
            // Each thing stirs for as long as the frame it was said in.
            if elapsed > 0.0 {
                for stir in stirs {
                    if pending.len() < 2 * MOST_STIRS {
                        pending.push((*stir, elapsed));
                    }
                }
            }
            if pairs > 0 && quiet < RINGS_GONE {
                // What stirs in these steps, each for the share of them
                // it has left to stir for.
                let mut acting = [Stir::default(); MOST_STIRS];
                let mut count = 0;
                for (stir, left) in pending.iter_mut() {
                    if let Some(slot) = acting.get_mut(count) {
                        let share = (left.min(stepped) / stepped).clamp(0.0, 1.0);
                        *slot = Stir {
                            lift: stir.lift * share,
                            body: stir.body * share,
                            ..*stir
                        };
                        count += 1;
                    }
                    *left -= stepped;
                }
                pending.retain(|(_, left)| *left > 0.0);
                // SAFETY: the buffer was made writable and this long, and
                // the caller guarantees the commands that read it last
                // have finished; the same goes for the sets pointed at
                // new depths.
                unsafe {
                    slot.step
                        .write(&step_bytes(&self.weights, near, &acting[..count]))?;
                    self.point_at(slot, near);
                    self.record_rings(commands, slot, wanted, before, pairs);
                }
                self.corner.set(Some(wanted));
            } else if quiet >= RINGS_GONE {
                pending.clear();
            }
        }
        if let Some(body) = near {
            rows[..3].copy_from_slice(&body.rows());
        }
        rows[2][3] = reach;
        let corner = self.corner.get().unwrap_or(wanted);
        rows[7] = [corner[0], corner[1], RING_APART, RING_SIDE as f32];
        Ok(rows)
    }

    /// Points a frame's ring sets at the depths of the water `near`, or
    /// at "dry" with none, if they are not already.
    ///
    /// # Safety
    ///
    /// The commands last recorded with these sets must have finished.
    unsafe fn point_at(&self, slot: &Frame, near: Option<&Body>) {
        let id = near.map_or(0, |body| body.id);
        if slot.depths.get() == id {
            return;
        }
        let pictures = [vk::DescriptorImageInfo::default()
            .image_view(near.map_or(self.dry.view, Body::depths))
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let writes = slot.ring_sets.map(|set| {
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(&pictures)
        });
        // SAFETY: the caller guarantees the sets are not in use.
        unsafe { self.gpu.device().update_descriptor_sets(&writes, &[]) };
        slot.depths.set(id);
    }

    /// Records adding up each layer of waves into its pictures.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record).
    unsafe fn record_waves(&self, commands: vk::CommandBuffer, slot: &Frame, waves: &Waves) {
        let device = self.gpu.device();
        let readers =
            vk::PipelineStageFlags2::FRAGMENT_SHADER | vk::PipelineStageFlags2::VERTEX_SHADER;
        let whole = vk::ImageSubresourceRange {
            layer_count: LAYERS as u32,
            ..COLOUR
        };
        let images = [self.shift.image, self.slope.image];
        let to_draw = images.map(|image| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(readers)
                .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(whole)
        });
        let to_read = images.map(|image| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(readers)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(whole)
        });
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: vk::Extent2D {
                width: WAVE_SIDE,
                height: WAVE_SIDE,
            },
        };
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: WAVE_SIDE as f32,
            height: WAVE_SIDE as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        // SAFETY: the caller guarantees the command buffer is recording
        // outside a render pass; the pictures, pipeline and set are this
        // object's own and alive, and every struct lives until its call
        // returns.
        unsafe {
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_draw),
            );
            device.cmd_bind_pipeline(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.wave_pipeline,
            );
            device.cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.wave_pipeline_layout,
                0,
                &[slot.wave_set],
                &[],
            );
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            for (number, layer) in waves.layers.iter().enumerate() {
                let (Some(shift), Some(slope)) =
                    (self.shift.layers.get(number), self.slope.layers.get(number))
                else {
                    continue;
                };
                let attachments = [*shift, *slope].map(|view| {
                    vk::RenderingAttachmentInfo::default()
                        .image_view(view)
                        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                        .load_op(vk::AttachmentLoadOp::DONT_CARE)
                        .store_op(vk::AttachmentStoreOp::STORE)
                });
                let rendering = vk::RenderingInfo::default()
                    .render_area(area)
                    .layer_count(1)
                    .color_attachments(&attachments);
                // Counts are a few hundred at most.
                #[allow(clippy::cast_possible_truncation)]
                let (first, count) = ((number * EACH) as u32, layer.waves.len().min(EACH) as u32);
                let mut pushed = [0_u8; 20];
                pushed[0..4].copy_from_slice(&first.to_ne_bytes());
                pushed[4..8].copy_from_slice(&count.to_ne_bytes());
                pushed[8..12].copy_from_slice(&layer.tile.to_ne_bytes());
                pushed[12..16].copy_from_slice(&(WAVE_SIDE as f32).to_ne_bytes());
                pushed[16..20].copy_from_slice(&CARRIED.to_ne_bytes());
                device.cmd_begin_rendering(commands, &rendering);
                device.cmd_push_constants(
                    commands,
                    self.wave_pipeline_layout,
                    vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &pushed,
                );
                device.cmd_draw(commands, 3, 1, 0, 0);
                device.cmd_end_rendering(commands);
            }
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_read),
            );
        }
    }

    /// Records `pairs` pairs of steps of the rings. The first step carries
    /// the square over from where its corner was (`before`) to where it
    /// is (`corner`).
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record).
    unsafe fn record_rings(
        &self,
        commands: vk::CommandBuffer,
        slot: &Frame,
        corner: [f32; 2],
        before: [f32; 2],
        pairs: u32,
    ) {
        let device = self.gpu.device();
        let readers =
            vk::PipelineStageFlags2::FRAGMENT_SHADER | vk::PipelineStageFlags2::VERTEX_SHADER;
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: vk::Extent2D {
                width: RING_SIDE,
                height: RING_SIDE,
            },
        };
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: RING_SIDE as f32,
            height: RING_SIDE as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        // SAFETY: the caller guarantees the command buffer is recording
        // outside a render pass; the pictures, pipeline and sets are this
        // object's own and alive. Each step reads one picture, which the
        // step before left ready to read, and draws the whole of the other.
        unsafe {
            device.cmd_bind_pipeline(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.ring_pipeline,
            );
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            for step in 0..pairs * 2 {
                // Reads the first to draw the second, then the other way.
                let reads = (step % 2) as usize;
                let draws = 1 - reads;
                let image = self.rings[draws].image;
                let to_draw = [vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(readers)
                    .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                    .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(COLOUR)];
                let to_read = [vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                    .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                    .dst_stage_mask(readers)
                    .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                    .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(COLOUR)];
                let attachments = [vk::RenderingAttachmentInfo::default()
                    .image_view(self.rings[draws].view)
                    .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .store_op(vk::AttachmentStoreOp::STORE)];
                let rendering = vk::RenderingInfo::default()
                    .render_area(area)
                    .layer_count(1)
                    .color_attachments(&attachments);
                // Only the first step has the square to carry over.
                let from = if step == 0 { before } else { corner };
                let mut pushed = [0_u8; 16];
                for (slot, number) in pushed
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip([corner[0], corner[1], from[0], from[1]])
                {
                    *slot = number.to_ne_bytes();
                }
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_draw),
                );
                device.cmd_bind_descriptor_sets(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.ring_pipeline_layout,
                    0,
                    &[slot.ring_sets[reads]],
                    &[],
                );
                device.cmd_begin_rendering(commands, &rendering);
                device.cmd_push_constants(
                    commands,
                    self.ring_pipeline_layout,
                    vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &pushed,
                );
                device.cmd_draw(commands, 3, 1, 0, 0);
                device.cmd_end_rendering(commands);
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            }
        }
    }
}

/// The ring shader's `Step` for water `near` stirred by `stirs`, with the
/// table of what each neighbour's height counts for.
fn step_bytes(weights: &[f32], near: Option<&Body>, stirs: &[Stir]) -> [u8; STEP_BYTES] {
    let mut bytes = [0_u8; STEP_BYTES];
    let (table, rest) = bytes.split_at_mut(43 * 16);
    for (slot, weight) in table.as_chunks_mut::<4>().0.iter_mut().zip(weights) {
        *slot = weight.to_ne_bytes();
    }
    let stirs = &stirs[..stirs.len().min(MOST_STIRS)];
    let (water, grid) = near.map_or(([0.0; 4], [0.0; 4]), |body| {
        let [water, grid, _] = body.rows();
        ([water[0], water[1], 1.0, 0.0], grid)
    });
    // A handful at most.
    #[allow(clippy::cast_precision_loss)]
    let about = [
        [RING_APART, RING_STEP, RINGS_DIE, stirs.len() as f32],
        water,
        grid,
    ];
    let sound = |number: f32| if number.is_finite() { number } else { 0.0 };
    let stirring = stirs.iter().flat_map(|stir| {
        [
            stir.place[0],
            stir.place[1],
            stir.wide,
            stir.lift,
            stir.moving[0],
            stir.moving[1],
            stir.body,
            stir.churn,
        ]
        .map(sound)
    });
    for (slot, number) in rest
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(about.into_iter().flatten().chain(stirring))
    {
        *slot = number.to_ne_bytes();
    }
    bytes
}

/// The grid still water is drawn with: a square of `GRID_SQUARES` squares
/// a side with its middle at nothing, each point two floats (how many
/// squares along x and along z it is from the middle), and its triangles.
fn grid(gpu: &Arc<Gpu>) -> Result<(Buffer, Buffer), RenderError> {
    let across = GRID_SQUARES + 1;
    let mut points = Vec::with_capacity((across * across * 8) as usize);
    // The grid is a couple of hundred points across.
    #[allow(clippy::cast_precision_loss)]
    let half = GRID_SQUARES as f32 / 2.0;
    for row in 0..across {
        for column in 0..across {
            #[allow(clippy::cast_precision_loss)]
            let at = [column as f32 - half, row as f32 - half];
            points.extend(at.iter().flat_map(|part| part.to_ne_bytes()));
        }
    }
    let triangles: Vec<u8> = grid_triangles()
        .iter()
        .flat_map(|point| point.to_ne_bytes())
        .collect();
    Ok((
        Buffer::holding(gpu, &points, vk::BufferUsageFlags::VERTEX_BUFFER, DOING)?,
        Buffer::holding(gpu, &triangles, vk::BufferUsageFlags::INDEX_BUFFER, DOING)?,
    ))
}

/// The triangles of the grid still water is drawn with, as numbers of its
/// points, a square at a time from the outermost ring of squares round
/// its middle in to the innermost, each ring walked round.
///
/// Water writes no depth that the rest of itself is tested against, so its
/// own triangles are drawn over each other in the order they come. The
/// grid's middle is where the eye is, and along every line of sight from
/// there what is farther off is in an outer ring: drawn first, so that a
/// near crest is drawn over the far side of the trough behind it and not
/// the other way round. Neighbours in a ring share two points, as
/// neighbours in a row do, so no point is worked out more often.
fn grid_triangles() -> Vec<u32> {
    let across = GRID_SQUARES + 1;
    let half = GRID_SQUARES / 2;
    let mut triangles = Vec::with_capacity(GRID_INDICES as usize);
    let mut square = |row: u32, column: u32| {
        let near = row * across + column;
        let far = near + across;
        // Seen from above, anticlockwise.
        triangles.extend([near, far, near + 1, near + 1, far, far + 1]);
    };
    for ring in (0..half).rev() {
        let (low, high) = (half - 1 - ring, half + ring);
        for column in low..=high {
            square(low, column);
        }
        for row in low + 1..=high {
            square(row, high);
        }
        for column in (low..high).rev() {
            square(high, column);
        }
        for row in (low + 1..high).rev() {
            square(row, low);
        }
    }
    triangles
}

impl Drop for WaterWorks {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping this. Destroying a null handle is allowed, and
        // destroying the pool frees its sets.
        unsafe {
            let device = self.gpu.device();
            device.destroy_pipeline(self.wave_pipeline, None);
            device.destroy_pipeline(self.ring_pipeline, None);
            device.destroy_pipeline_layout(self.wave_pipeline_layout, None);
            device.destroy_pipeline_layout(self.ring_pipeline_layout, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.wave_layout, None);
            device.destroy_descriptor_set_layout(self.ring_layout, None);
            device.destroy_sampler(self.repeating, None);
            device.destroy_sampler(self.stopping, None);
            self.shift.destroy(&self.gpu);
            self.slope.destroy(&self.gpu);
            for ring in &self.rings {
                ring.destroy(&self.gpu);
            }
            self.dry.destroy(&self.gpu);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_is_packed_into_sixteen_bits_and_comes_back() {
        fn unpacked(bits: u16) -> f32 {
            let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
            let exponent = i32::from((bits >> 10) & 0x1f);
            let fraction = f32::from(bits & 0x3ff) / 1024.0;
            if exponent == 0 {
                sign * fraction * 2.0_f32.powi(-14)
            } else {
                sign * (1.0 + fraction) * 2.0_f32.powi(exponent - 15)
            }
        }
        for value in [
            0.0, 1.0, -2.0, 0.5, 0.001, -0.0005, 3.75, 123.5, 1000.0, 1e-6,
        ] {
            let back = unpacked(half(value));
            assert!(
                (back - value).abs() <= value.abs() * 0.001 + 1e-7,
                "{value} came back as {back}"
            );
        }
        assert_eq!(half(f32::INFINITY), 0x7bff);
        assert_eq!(unpacked(half(1e9)), 65504.0);
    }

    #[test]
    fn level_water_pulls_nowhere_and_a_bump_is_pulled_down() {
        let weights = ring_weights();
        assert_eq!(weights.len(), 169);
        let sum: f32 = weights.iter().sum();
        assert!(sum.abs() < 1e-5, "{sum}");
        // The point itself counts most, and for a pull back to level.
        let middle = weights[84];
        assert!(middle > 0.0 && weights.iter().all(|weight| *weight <= middle));
        // No wave of any length is pushed where it should be pulled: that
        // would make it grow without end.
        for across in 0..24 {
            for down in 0..24 {
                let turns = [across, down].map(|step| std::f32::consts::PI * step as f32 / 23.0);
                let pull: f32 = weights
                    .iter()
                    .enumerate()
                    .map(|(at, weight)| {
                        let (x, z) = ((at % 13) as f32 - 6.0, (at / 13) as f32 - 6.0);
                        weight * (turns[0] * x + turns[1] * z).cos()
                    })
                    .sum();
                assert!(
                    pull > -1e-4,
                    "waves turning {turns:?} a point are pushed: {pull}"
                );
            }
        }
        // The same in every direction.
        assert!((weights[84 + 3] - weights[84 - 3]).abs() < 1e-6);
        assert!((weights[84 + 3] - weights[84 + 3 * 13]).abs() < 1e-6);
    }

    #[test]
    fn a_ring_on_water_stands_as_bessel_says() {
        assert!((bessel(0.0) - 1.0).abs() < 1e-12);
        // Its first two places of nothing, and a value between.
        assert!(bessel(2.404_825_557_695_773).abs() < 1e-9);
        assert!(bessel(5.520_078_110_286_311).abs() < 1e-9);
        assert!((bessel(1.0) - 0.765_197_686_557_966_6).abs() < 1e-9);
        assert!((bessel(10.0) - -0.245_935_764_451_348_3).abs() < 2e-3);
        assert!((bessel(30.0) - -0.086_367_983_581_040_2).abs() < 2e-3);
    }

    #[test]
    fn what_stirs_the_water_fills_the_shader_block_exactly() {
        let number = |bytes: &[u8], at: usize| {
            f32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let stir = Stir {
            place: [3.0, -4.0],
            wide: 0.3,
            lift: -1.5,
            moving: [1.0, 0.5],
            body: 1.0,
            churn: 0.25,
        };
        let bytes = step_bytes(&ring_weights(), None, &[stir, stir]);
        assert_eq!(bytes.len(), 1760);
        // The grid: how far apart, how long a step, and how many stir.
        assert_eq!(number(&bytes, 688), RING_APART);
        assert_eq!(number(&bytes, 700), 2.0);
        // No still water near.
        assert_eq!(number(&bytes, 712), 0.0);
        // The first thing stirring, and the second straight after.
        assert_eq!(number(&bytes, 736), 3.0);
        assert_eq!(number(&bytes, 748), -1.5);
        assert_eq!(number(&bytes, 764), 0.25);
        assert_eq!(number(&bytes, 768), 3.0);
    }

    /// A body of water `size` points `cell` metres apart from `origin`,
    /// with nothing on the graphics card.
    fn body(origin: [f32; 2], cell: f32, size: [u32; 2]) -> Body {
        Body {
            id: 0,
            level: 0.0,
            origin,
            cell,
            size,
            colour: [0.0; 4],
            water: Water {
                name: None,
                sources: Vec::new(),
                level: 0.0,
                origin,
                cell,
                size,
                depths: Vec::new(),
                colour: [0.0; 4],
            },
            depths: Picture::null(),
            set: vk::DescriptorSet::null(),
        }
    }

    #[test]
    fn the_widest_lake_is_drawn_whole_and_a_far_pond_by_one_grid() {
        // The widest body there can be, seen from one corner of it.
        let sea = body([0.0, 0.0], 8.0, [8192, 8192]);
        let eye = [1.0, 2.0, 1.0];
        let grids = sea.grids(eye);
        let (apart, middle) = grid_laid(eye, grids - 1);
        #[allow(clippy::cast_precision_loss)]
        let reach = GRID_SQUARES as f32 / 2.0 * apart;
        let far = sea.far_corner();
        assert!(
            middle[0] + reach >= far[0] && middle[1] + reach >= far[1],
            "{grids} grids reach {reach} m, the far shore is {far:?}"
        );
        // A pond two kilometres off, past where eleven grids reached, is
        // drawn, and by no more than two grids.
        let pond = body([2000.0, 40.0], 0.25, [120, 120]);
        let drawn = (0..pond.grids(eye))
            .filter(|grid| pond.drawn_by(eye, *grid))
            .count();
        assert!((1..=2).contains(&drawn), "drawn by {drawn} grids");
        // A pond round the eye takes no more grids than it did.
        let near = body([-10.0, -10.0], 0.1, [200, 200]);
        assert!(near.grids(eye) <= 6, "{}", near.grids(eye));
    }

    #[test]
    fn a_coarser_grid_is_left_out_round_the_middle_the_finer_one_is_laid_at() {
        // Eyes exactly half way between two points of a grid, where
        // rounding one way or the other matters.
        for grid in 1..MOST_GRIDS {
            let (apart, _) = grid_laid([0.0; 3], grid);
            let eye = [apart * 2.5, 1.0, -apart * 3.5];
            let (finer_apart, finer) = grid_laid(eye, grid - 1);
            assert_eq!(finer_apart * 2.0, apart);
            // The finer grid's middle is on a point of this one, within a
            // point of the eye.
            for (axis, at) in [(0, eye[0]), (1, eye[2])] {
                let points = finer[axis] / apart;
                assert!((points - points.round()).abs() < 1e-3, "{points}");
                assert!((finer[axis] - at).abs() <= apart * 0.5 + 1e-3);
            }
        }
    }

    #[test]
    fn the_grid_is_drawn_from_its_outer_ring_in() {
        let triangles = grid_triangles();
        assert_eq!(triangles.len(), GRID_INDICES as usize);
        let across = GRID_SQUARES + 1;
        let half = GRID_SQUARES / 2;
        // Each square once, by the point at its corner nearest nothing.
        let mut seen = vec![false; (GRID_SQUARES * GRID_SQUARES) as usize];
        let mut ring_before = u32::MAX;
        let mut corner_before: Option<u32> = None;
        for square in triangles.as_chunks::<6>().0 {
            let (row, column) = (square[0] / across, square[0] % across);
            let at = (row * GRID_SQUARES + column) as usize;
            assert!(!seen[at], "square {row}, {column} twice");
            seen[at] = true;
            // How many rings out from the middle it is: never more than
            // the square before.
            let ring = [row, column]
                .map(|n| if n < half { half - 1 - n } else { n - half })
                .into_iter()
                .max()
                .unwrap_or(0);
            assert!(ring <= ring_before, "ring {ring} after {ring_before}");
            // And in a ring, beside the square before, sharing two points.
            if ring == ring_before
                && let Some(before) = corner_before
            {
                let (was_row, was_column) = (before / across, before % across);
                assert_eq!(
                    was_row.abs_diff(row) + was_column.abs_diff(column),
                    1,
                    "{row}, {column} after {was_row}, {was_column}"
                );
            }
            ring_before = ring;
            corner_before = Some(square[0]);
        }
        assert!(seen.iter().all(|square| *square));
    }
}
