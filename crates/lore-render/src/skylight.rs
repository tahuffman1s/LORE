//! The light of the sky from every way (#127): what a rough or glossy
//! surface mirrors of it, and what a matte one gets from all round.
//!
//! The sky is drawn, clouds and all, into the six faces of a small cube
//! round the eye; each face is blurred for each of a few roughnesses into
//! the smaller sizes of a second cube, which a surface reads by its own
//! roughness; and the light a matte surface gets from it, facing any way,
//! is kept as nine numbers a colour (`skylight.wgsl` says how each is
//! made). Before, both were two flat colours: the sky's above and the
//! ground's below. Now a chrome bumper shows the clouds, rough iron the
//! glow round a low sun, and the shade under a sunset is warm on the side
//! the sun went down.
//!
//! The sky changes slowly, so it is drawn again a face a frame, round and
//! round, and the blurring and the numbers follow: each frame costs about
//! as much as drawing 10,000 pixels of the picture, wherever the eye is.
//! The whole cube is drawn at once when there is none yet, when the sky
//! jumps (a level begins, a script moves the hour), and in a picture held
//! still whenever anything it is drawn from has changed, so a picture held
//! still is the same however many times it is drawn. When nothing is drawn
//! behind the picture there is no sky to draw: the light is the two colours
//! the level says, as it was before.
//!
//! What is drawn is planned here, on the processor, and tested without a
//! graphics card.

use std::cell::RefCell;
use std::sync::Arc;

use ash::vk;

use crate::bounce::Shared;
use crate::effects::Air;
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::mesh::{Lighting, begin_drawing};
use crate::scene::Picture;
use crate::sky::Sky;

/// How many pixels wide each face of the cube is. Smooth things mirror the
/// world, and the sky in it, from the picture itself (`solid_mirrored`);
/// the cube is for what is too rough for that, which shows nothing finer
/// than this.
pub(crate) const SIDE: u32 = 64;
/// How many sizes the drawn cube has, down to one pixel a face: the
/// blurring reads the wide part of a highlight from the small ones.
const DRAWN_SIZES: u32 = 7;
/// How many sizes the blurred cube has: one for each roughness from 0 to 1
/// in quarters, the smallest four pixels a face. `SKYLIGHT_SIZES` in the
/// shader.
pub(crate) const BLURRED_SIZES: u32 = 5;
/// How many numbers the row holds: nine harmonics, then the brightness.
const NUMBERS: u32 = 10;
/// The parts the row is added up from: a column for each harmonic and two
/// for what a surface facing up gets, and a line for each line of pixels
/// across each face of the size they are read from (`SKYLIGHT_PARTS` in
/// the shader).
const PARTS: [u32; 2] = [11, 48];
/// The format of both cubes: light, with no upper limit.
const FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
/// The format of the row of numbers, which can be less than nothing.
const NUMBERS_FORMAT: vk::Format = vk::Format::R32G32B32A32_SFLOAT;
/// How many faces a cube has.
const FACES: u32 = 6;
/// Every face, one bit each.
const ALL_FACES: u8 = 0b11_1111;

/// How far the sun can have moved since a face was drawn, as the cosine of
/// the angle, before the sky is said to have jumped: three degrees, which
/// it takes a quarter of an hour of a real day to go, or a fraction of a
/// second of a game's.
const SUN_JUMPS: f32 = 0.998_63;
/// How much the share of the sky that is cloud, how far the night has
/// come, how stormy the sky is, or how hard it rains or snows at the eye
/// can have changed since a face was drawn before the sky has jumped.
const SHARE_JUMPS: f32 = 0.1;
/// How much the level's sky light or the fog can have grown or shrunk, as
/// a share of itself, before it has jumped.
const LIGHT_JUMPS: f32 = 0.25;
/// How far the eye can have gone, in metres: farther is a jump to another
/// place, under other weather.
const EYE_JUMPS: f32 = 100.0;

/// What the sky is drawn from. A picture held still draws it again only if
/// one of these but the eye has changed, or the eye has gone far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Heavens {
    pub(crate) lighting: Lighting,
    pub(crate) sky: Sky,
    pub(crate) weather: Air,
    /// The sun, the moon and the stars, as the shader's `heavens`.
    pub(crate) heavens: [[f32; 4]; 6],
    /// How many seconds the clouds have drifted.
    pub(crate) drift: f32,
    /// Where the eye is: the clouds and the weather are drawn round it.
    pub(crate) eye: [f32; 3],
}

impl Heavens {
    /// Whether this is the same sky as `before`, wherever the eye is: a
    /// camera moved about while time is held still sees the same sky, to
    /// the cube's pixels, until it has gone far.
    fn same_sky(&self, before: &Self) -> bool {
        Self {
            eye: before.eye,
            ..*self
        } == *before
    }

    /// Whether the sky has changed too much since `before` to be drawn
    /// again a face at a time: the faces would disagree where they meet.
    fn jumped(&self, before: &Self) -> bool {
        let along = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let [sun, before_sun] = [self, before].map(|heavens| {
            let [x, y, z, _] = heavens.heavens[0];
            [x, y, z]
        });
        let grey = |[red, green, blue]: [f32; 3]| 0.2126 * red + 0.7152 * green + 0.0722 * blue;
        let grown = |now: f32, then: f32| (now - then).abs() > LIGHT_JUMPS * now.max(then);
        let share = |now: f32, then: f32| (now - then).abs() > SHARE_JUMPS;
        let gone = self
            .eye
            .iter()
            .zip(before.eye)
            .map(|(now, then)| (now - then) * (now - then));
        let (rows, before_rows) = (&self.weather.rows, &before.weather.rows);
        along(self.lighting.towards_sun, before.lighting.towards_sun) < SUN_JUMPS
            || along(sun, before_sun) < SUN_JUMPS
            || share(self.heavens[0][3], before.heavens[0][3])
            || share(self.weather.cover, before.weather.cover)
            // How stormy the sky is, how hard it rains and how hard it snows.
            || share(rows[1][0], before_rows[1][0])
            || share(rows[0][0], before_rows[0][0])
            || share(rows[1][3], before_rows[1][3])
            || grown(grey(self.lighting.sky), grey(before.lighting.sky))
            || grown(self.weather.fog, before.weather.fog)
            || gone.sum::<f32>() > EYE_JUMPS * EYE_JUMPS
    }
}

/// What is drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Work {
    /// The faces drawn again, one bit each: those faces are blurred again
    /// and the numbers worked out again.
    pub(crate) faces: u8,
    /// Whether the numbers are wiped, so that the shaders go back to the
    /// level's two colours: there is no longer a sky drawn.
    pub(crate) forget: bool,
}

/// Which faces are drawn when.
#[derive(Debug, Default)]
pub(crate) struct Turns {
    /// What the sky was drawn from when each face was last drawn: none
    /// since there was last no sky.
    drawn: [Option<Heavens>; 6],
    /// The face whose turn it is next.
    next: u8,
    /// What the sky was drawn from the frame before, and whether time was
    /// held still then.
    last: Option<Heavens>,
    was_still: bool,
}

impl Turns {
    /// What to draw for a frame whose sky is drawn from `now`, or that has
    /// none; `still` when time is held still.
    pub(crate) fn plan(&mut self, now: Option<&Heavens>, still: bool) -> Work {
        let Some(now) = now else {
            let forget = self.drawn.iter().any(Option::is_some);
            *self = Self::default();
            return Work { faces: 0, forget };
        };
        // A face drawn so long ago that the sky has jumped since, measured
        // against each face so that a change too slow to be a jump in one
        // frame cannot leave the faces apart either.
        let behind = self
            .drawn
            .iter()
            .any(|drawn| drawn.as_ref().is_none_or(|drawn| now.jumped(drawn)));
        // A picture held still is drawn from one sky, whatever the frames
        // before it did.
        let held =
            still && (!self.was_still || self.last.as_ref().is_none_or(|last| !now.same_sky(last)));
        let faces = if behind || held {
            ALL_FACES
        } else if still {
            0
        } else {
            let face = 1 << self.next;
            self.next = (self.next + 1) % 6;
            face
        };
        for (face, drawn) in self.drawn.iter_mut().enumerate() {
            if faces & (1 << face) != 0 {
                *drawn = Some(*now);
            }
        }
        self.last = Some(*now);
        self.was_still = still;
        Work {
            faces,
            forget: false,
        }
    }
}

/// A cube's picture: six faces, each with smaller sizes of itself, read
/// as one cube, and drawn into a face and a size at a time.
struct Cube {
    image: vk::Image,
    memory: vk::DeviceMemory,
    /// All of it, as a cube.
    view: vk::ImageView,
    /// Each face of each size, the faces of the largest first.
    faces: Vec<vk::ImageView>,
}

impl Cube {
    const fn null() -> Self {
        Self {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            faces: Vec::new(),
        }
    }

    /// The part of the cube that is `count` sizes from `first`, of every
    /// face.
    const fn sizes(first: u32, count: u32) -> vk::ImageSubresourceRange {
        vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: first,
            level_count: count,
            base_array_layer: 0,
            layer_count: FACES,
        }
    }

    /// One face of one size.
    const fn face(size: u32, face: u32) -> vk::ImageSubresourceRange {
        vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: size,
            level_count: 1,
            base_array_layer: face,
            layer_count: 1,
        }
    }

    /// Fills in `self` with a cube of `sizes` sizes, with a view of each
    /// face of the first `drawn` of them. On failure, what was made so far
    /// is left in `self` for [`destroy`](Self::destroy).
    fn create(
        &mut self,
        gpu: &Gpu,
        sizes: u32,
        drawn: u32,
        usage: vk::ImageUsageFlags,
        doing: &'static str,
    ) -> Result<(), RenderError> {
        let device = gpu.device();
        // SAFETY: the device is alive, and each object is created from valid
        // settings and stored in `self` straight away. The memory is as
        // large as the image asks for and of a kind it allows.
        unsafe {
            self.image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .flags(vk::ImageCreateFlags::CUBE_COMPATIBLE)
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(FORMAT)
                        .extent(vk::Extent3D {
                            width: SIDE,
                            height: SIDE,
                            depth: 1,
                        })
                        .mip_levels(sizes)
                        .array_layers(FACES)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .usage(usage),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
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
                    doing,
                    code: vk::Result::ERROR_FEATURE_NOT_PRESENT,
                })?;
            self.memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(needs.size)
                        .memory_type_index(kind),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            device
                .bind_image_memory(self.image, self.memory, 0)
                .map_err(RenderError::driver(doing))?;
            self.view = device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(self.image)
                        .view_type(vk::ImageViewType::CUBE)
                        .format(FORMAT)
                        .subresource_range(Self::sizes(0, sizes)),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            for size in 0..drawn {
                for face in 0..FACES {
                    let view = device
                        .create_image_view(
                            &vk::ImageViewCreateInfo::default()
                                .image(self.image)
                                .view_type(vk::ImageViewType::TYPE_2D)
                                .format(FORMAT)
                                .subresource_range(Self::face(size, face)),
                            None,
                        )
                        .map_err(RenderError::driver(doing))?;
                    self.faces.push(view);
                }
            }
        }
        Ok(())
    }

    /// The view of `face` of `size`.
    fn face_view(&self, size: u32, face: u32) -> vk::ImageView {
        self.faces
            .get((size * FACES + face) as usize)
            .copied()
            .unwrap_or_default()
    }

    /// Frees the cube.
    ///
    /// # Safety
    ///
    /// It must have been made on `gpu` (or be null), and the graphics card
    /// must have finished using it.
    unsafe fn destroy(&self, gpu: &Gpu) {
        let device = gpu.device();
        // SAFETY: the caller guarantees the handles are this device's and
        // idle; destroying a null handle is allowed.
        unsafe {
            device.destroy_image_view(self.view, None);
            for view in &self.faces {
                device.destroy_image_view(*view, None);
            }
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// The light of the sky's pictures and pipelines on the graphics card, and
/// the turns its faces are drawn in.
pub(crate) struct Skylight {
    gpu: Arc<Gpu>,
    /// The sky as drawn, and as blurred: read by shaders except while a
    /// face of them is drawn.
    drawn: Cube,
    blurred: Cube,
    /// The row of numbers, and the parts it is added up from, likewise.
    numbers: Picture,
    parts: Picture,
    sampler: vk::Sampler,
    /// Draw a face of the sky, blur a face of one size, and work out the
    /// parts and the numbers: null until the mesh pipeline makes them, and
    /// where the card could not, and then the light is the level's two
    /// colours.
    capture: vk::Pipeline,
    blur: vk::Pipeline,
    part: vk::Pipeline,
    harmonics: vk::Pipeline,
    pub(crate) turns: RefCell<Turns>,
}

/// A change of layout of the part `range` of `image`.
fn change(
    image: vk::Image,
    range: vk::ImageSubresourceRange,
    old: vk::ImageLayout,
    new: vk::ImageLayout,
) -> vk::ImageMemoryBarrier2<'static> {
    vk::ImageMemoryBarrier2::default()
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range)
        .old_layout(old)
        .new_layout(new)
}

/// The whole of the row of numbers.
const fn row() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

impl Skylight {
    /// Makes the pictures, empty and ready to be read: the shaders take an
    /// empty row of numbers for no sky. The pipelines are made with the
    /// rest of the mesh pipeline's.
    pub(crate) fn new(gpu: &Arc<Gpu>) -> Result<Self, RenderError> {
        let doing = "making room for the light of the sky";
        let mut skylight = Self {
            gpu: Arc::clone(gpu),
            drawn: Cube::null(),
            blurred: Cube::null(),
            numbers: Picture::null(),
            parts: Picture::null(),
            sampler: vk::Sampler::null(),
            capture: vk::Pipeline::null(),
            blur: vk::Pipeline::null(),
            part: vk::Pipeline::null(),
            harmonics: vk::Pipeline::null(),
            turns: RefCell::new(Turns::default()),
        };
        let written = vk::ImageUsageFlags::COLOR_ATTACHMENT
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_DST;
        skylight.drawn.create(
            gpu,
            DRAWN_SIZES,
            1,
            written | vk::ImageUsageFlags::TRANSFER_SRC,
            doing,
        )?;
        skylight
            .blurred
            .create(gpu, BLURRED_SIZES, BLURRED_SIZES, written, doing)?;
        skylight.numbers.create(
            gpu,
            vk::Extent2D {
                width: NUMBERS,
                height: 1,
            },
            NUMBERS_FORMAT,
            written,
            row(),
            doing,
        )?;
        skylight.parts.create(
            gpu,
            vk::Extent2D {
                width: PARTS[0],
                height: PARTS[1],
            },
            NUMBERS_FORMAT,
            written,
            row(),
            doing,
        )?;
        let device = gpu.device();
        // SAFETY: the device is alive and the settings are valid; the
        // sampler is stored straight away.
        skylight.sampler = unsafe {
            device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .max_lod(vk::LOD_CLAMP_NONE),
                None,
            )
        }
        .map_err(RenderError::driver(doing))?;
        let images = [
            (skylight.drawn.image, Cube::sizes(0, DRAWN_SIZES)),
            (skylight.blurred.image, Cube::sizes(0, BLURRED_SIZES)),
            (skylight.numbers.image, row()),
            (skylight.parts.image, row()),
        ];
        // SAFETY: the pictures are this device's, new, and used by nothing.
        unsafe {
            crate::buffer::once(gpu, doing, |commands| {
                let to_clear = images.map(|(image, range)| {
                    change(
                        image,
                        range,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    )
                    .dst_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                });
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_clear),
                );
                for (image, range) in images {
                    device.cmd_clear_color_image(
                        commands,
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearColorValue { float32: [0.0; 4] },
                        &[range],
                    );
                }
                let to_read = images.map(|(image, range)| {
                    change(
                        image,
                        range,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    )
                    .src_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                });
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            })?;
        }
        Ok(skylight)
    }

    /// Whether the sky can be drawn into the cube on this card: the
    /// pipelines were made.
    pub(crate) fn works(&self) -> bool {
        [self.capture, self.blur, self.part, self.harmonics]
            .iter()
            .all(|pipeline| *pipeline != vk::Pipeline::null())
    }

    /// Makes the four pipelines from the mesh pipeline's shader module and
    /// layout, and the states its own pipelines share.
    ///
    /// # Safety
    ///
    /// `module` and `layout` must be live and of this device, and hold the
    /// entry points and sets `skylight.wgsl` names.
    pub(crate) unsafe fn make_pipelines(
        &mut self,
        module: vk::ShaderModule,
        layout: vk::PipelineLayout,
        shared: &Shared<'_>,
    ) -> Result<(), vk::Result> {
        let device = self.gpu.device();
        let make = |fragment: &'static std::ffi::CStr, format: vk::Format| {
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vertex_skylight"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(fragment),
            ];
            let formats = [format];
            let mut rendering =
                vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
            let attachments = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA)];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
            let no_points = vk::PipelineVertexInputStateCreateInfo::default();
            let no_depth = vk::PipelineDepthStencilStateCreateInfo::default();
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&no_points)
                .input_assembly_state(shared.assembly)
                .viewport_state(shared.viewport)
                .rasterization_state(shared.raster)
                .multisample_state(shared.multisample)
                .depth_stencil_state(&no_depth)
                .color_blend_state(&blend)
                .dynamic_state(shared.dynamic)
                .layout(layout)
                .push_next(&mut rendering);
            // SAFETY: the caller guarantees the module and layout; `info`
            // and what it points to live for the call.
            unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&info),
                    None,
                )
            }
            .map_err(|(_, code)| code)
            .and_then(|made| made.first().copied().ok_or(vk::Result::ERROR_UNKNOWN))
        };
        self.capture = make(c"fragment_skylight_capture", FORMAT)?;
        self.blur = make(c"fragment_skylight_blur", FORMAT)?;
        self.part = make(c"fragment_skylight_parts", NUMBERS_FORMAT)?;
        self.harmonics = make(c"fragment_skylight_harmonics", NUMBERS_FORMAT)?;
        Ok(())
    }

    /// The pictures and sampler a frame's set points at: bindings 32 to 36
    /// of the mesh set.
    pub(crate) fn infos(&self) -> SkylightInfos {
        let picture = |view: vk::ImageView| {
            [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
        };
        SkylightInfos {
            drawn: picture(self.drawn.view),
            blurred: picture(self.blurred.view),
            numbers: picture(self.numbers.view),
            sampler: [vk::DescriptorImageInfo::default().sampler(self.sampler)],
            parts: picture(self.parts.view),
        }
    }

    /// Records what `work` asks: the faces drawn, the smaller sizes made,
    /// those faces blurred and the numbers worked out; or the numbers wiped.
    ///
    /// # Safety
    ///
    /// `commands` must be recording outside a render pass, with `set` this
    /// frame's mesh set, already written, and `layout` the mesh pipeline's.
    pub(crate) unsafe fn record(
        &self,
        commands: vk::CommandBuffer,
        set: vk::DescriptorSet,
        layout: vk::PipelineLayout,
        work: Work,
    ) {
        let device = self.gpu.device();
        let barrier = |barriers: &[vk::ImageMemoryBarrier2<'_>]| {
            // SAFETY: the caller guarantees the command buffer is recording
            // outside a render pass; the images are this pipeline's own.
            unsafe {
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(barriers),
                );
            }
        };
        let readable = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        let drawing = vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;
        let read = vk::PipelineStageFlags2::FRAGMENT_SHADER;
        let output = vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT;
        let transfer = vk::PipelineStageFlags2::ALL_TRANSFER;
        if work.forget {
            barrier(&[change(
                self.numbers.image,
                row(),
                readable,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            )
            .src_stage_mask(read)
            .dst_stage_mask(vk::PipelineStageFlags2::CLEAR)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)]);
            // SAFETY: as above.
            unsafe {
                device.cmd_clear_color_image(
                    commands,
                    self.numbers.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue { float32: [0.0; 4] },
                    &[row()],
                );
            }
            barrier(&[change(
                self.numbers.image,
                row(),
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                readable,
            )
            .src_stage_mask(vk::PipelineStageFlags2::CLEAR)
            .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            .dst_stage_mask(read)
            .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)]);
        }
        if work.faces == 0 || !self.works() {
            return;
        }
        // The faces drawn this frame: the first `count`.
        let mut drawn_now = [0_u32; FACES as usize];
        let mut count = 0;
        for face in 0..FACES {
            if work.faces & (1 << face) != 0 {
                drawn_now[count] = face;
                count += 1;
            }
        }
        let faces = &drawn_now[..count];
        // SAFETY: as above; every struct lives until its call returns.
        unsafe {
            device.cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                layout,
                0,
                &[set],
                &[],
            );
            device.cmd_set_cull_mode(commands, vk::CullModeFlags::NONE);
            device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
        }
        let attachment = |view: vk::ImageView| {
            [vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(drawing)
                .load_op(vk::AttachmentLoadOp::DONT_CARE)
                .store_op(vk::AttachmentStoreOp::STORE)]
        };

        // The faces whose turn it is, drawn afresh.
        let to_draw = drawn_now.map(|face| {
            change(
                self.drawn.image,
                Cube::face(0, face),
                vk::ImageLayout::UNDEFINED,
                drawing,
            )
            .src_stage_mask(read)
            .dst_stage_mask(output)
            .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
        });
        barrier(&to_draw[..count]);
        for &face in faces {
            // SAFETY: as above; the view is of the face just made ready.
            unsafe {
                begin_drawing(
                    device,
                    commands,
                    SIDE,
                    SIDE,
                    &attachment(self.drawn.face_view(0, face)),
                );
                device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.capture);
                device.cmd_draw(commands, 3, 1, 0, face);
                device.cmd_end_rendering(commands);
            }
        }

        // The smaller sizes, each made from the one before. The faces not
        // drawn this frame are made again with the rest, which costs less
        // than telling them apart.
        let to_shrink: [_; FACES as usize + 1] = std::array::from_fn(|at| {
            let Ok(face) = u32::try_from(at) else {
                return vk::ImageMemoryBarrier2::default();
            };
            if face == FACES {
                return change(
                    self.drawn.image,
                    Cube::sizes(1, DRAWN_SIZES - 1),
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                )
                .src_stage_mask(read)
                .dst_stage_mask(transfer)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE);
            }
            let just_drawn = faces.contains(&face);
            change(
                self.drawn.image,
                Cube::face(0, face),
                if just_drawn { drawing } else { readable },
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            )
            .src_stage_mask(if just_drawn { output } else { read })
            .src_access_mask(if just_drawn {
                vk::AccessFlags2::COLOR_ATTACHMENT_WRITE
            } else {
                vk::AccessFlags2::NONE
            })
            .dst_stage_mask(transfer)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
        });
        barrier(&to_shrink);
        let corner = |size: u32| {
            let side = i32::try_from((SIDE >> size).max(1)).unwrap_or(1);
            [
                vk::Offset3D::default(),
                vk::Offset3D {
                    x: side,
                    y: side,
                    z: 1,
                },
            ]
        };
        let layers = |size: u32| vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: size,
            base_array_layer: 0,
            layer_count: FACES,
        };
        for size in 1..DRAWN_SIZES {
            let regions = [vk::ImageBlit::default()
                .src_subresource(layers(size - 1))
                .src_offsets(corner(size - 1))
                .dst_subresource(layers(size))
                .dst_offsets(corner(size))];
            // SAFETY: as above; the two sizes are in the layouts named.
            unsafe {
                device.cmd_blit_image(
                    commands,
                    self.drawn.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    self.drawn.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &regions,
                    vk::Filter::LINEAR,
                );
            }
            barrier(&[change(
                self.drawn.image,
                Cube::sizes(size, 1),
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            )
            .src_stage_mask(transfer)
            .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            .dst_stage_mask(transfer)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)]);
        }
        barrier(&[change(
            self.drawn.image,
            Cube::sizes(0, DRAWN_SIZES),
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            readable,
        )
        .src_stage_mask(transfer)
        .src_access_mask(vk::AccessFlags2::TRANSFER_READ)
        .dst_stage_mask(read)
        .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)]);

        // The same faces blurred, every size of them.
        let blurring = |old: vk::ImageLayout, new: vk::ImageLayout| {
            drawn_now.map(|face| {
                let range = vk::ImageSubresourceRange {
                    level_count: BLURRED_SIZES,
                    ..Cube::face(0, face)
                };
                change(self.blurred.image, range, old, new)
            })
        };
        let to_blur = blurring(vk::ImageLayout::UNDEFINED, drawing).map(|barrier| {
            barrier
                .src_stage_mask(read)
                .dst_stage_mask(output)
                .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
        });
        barrier(&to_blur[..count]);
        for &face in faces {
            for size in 0..BLURRED_SIZES {
                let side = SIDE >> size;
                // SAFETY: as above; the view is of a face just made ready.
                unsafe {
                    begin_drawing(
                        device,
                        commands,
                        side,
                        side,
                        &attachment(self.blurred.face_view(size, face)),
                    );
                    device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.blur);
                    device.cmd_draw(commands, 3, 1, 0, size * FACES + face);
                    device.cmd_end_rendering(commands);
                }
            }
        }
        let blurred = blurring(drawing, readable).map(|barrier| {
            barrier
                .src_stage_mask(output)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(read)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
        });
        barrier(&blurred[..count]);

        // The parts, from the whole sky as it now is, and the numbers from
        // them.
        barrier(&[
            change(self.parts.image, row(), vk::ImageLayout::UNDEFINED, drawing)
                .src_stage_mask(read)
                .dst_stage_mask(output)
                .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE),
        ]);
        // SAFETY: as above.
        unsafe {
            begin_drawing(
                device,
                commands,
                PARTS[0],
                PARTS[1],
                &attachment(self.parts.view),
            );
            device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.part);
            device.cmd_draw(commands, 3, 1, 0, 0);
            device.cmd_end_rendering(commands);
        }
        barrier(&[change(self.parts.image, row(), drawing, readable)
            .src_stage_mask(output)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .dst_stage_mask(read)
            .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)]);
        barrier(&[change(
            self.numbers.image,
            row(),
            vk::ImageLayout::UNDEFINED,
            drawing,
        )
        .src_stage_mask(read)
        .dst_stage_mask(output)
        .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)]);
        // SAFETY: as above.
        unsafe {
            begin_drawing(device, commands, NUMBERS, 1, &attachment(self.numbers.view));
            device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.harmonics);
            device.cmd_draw(commands, 3, 1, 0, 0);
            device.cmd_end_rendering(commands);
        }
        barrier(&[change(self.numbers.image, row(), drawing, readable)
            .src_stage_mask(output)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .dst_stage_mask(read)
            .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)]);
    }
}

/// What [`Skylight::infos`] gives, kept alive while the writes that point
/// at it are made.
pub(crate) struct SkylightInfos {
    drawn: [vk::DescriptorImageInfo; 1],
    blurred: [vk::DescriptorImageInfo; 1],
    numbers: [vk::DescriptorImageInfo; 1],
    sampler: [vk::DescriptorImageInfo; 1],
    parts: [vk::DescriptorImageInfo; 1],
}

impl SkylightInfos {
    /// The writes that point the set `set` at these.
    pub(crate) fn writes(&self, set: vk::DescriptorSet) -> [vk::WriteDescriptorSet<'_>; 5] {
        let write = |binding: u32, kind: vk::DescriptorType, info| {
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding)
                .descriptor_type(kind)
                .image_info(info)
        };
        [
            write(32, vk::DescriptorType::SAMPLED_IMAGE, &self.drawn),
            write(33, vk::DescriptorType::SAMPLED_IMAGE, &self.blurred),
            write(34, vk::DescriptorType::SAMPLED_IMAGE, &self.numbers),
            write(35, vk::DescriptorType::SAMPLER, &self.sampler),
            write(36, vk::DescriptorType::SAMPLED_IMAGE, &self.parts),
        ]
    }
}

impl Drop for Skylight {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping these, so none is in use; null handles are allowed.
        unsafe {
            let device = self.gpu.device();
            for pipeline in [self.capture, self.blur, self.part, self.harmonics] {
                device.destroy_pipeline(pipeline, None);
            }
            device.destroy_sampler(self.sampler, None);
            self.drawn.destroy(&self.gpu);
            self.blurred.destroy(&self.gpu);
            self.numbers.destroy(&self.gpu);
            self.parts.destroy(&self.gpu);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_noon() -> Heavens {
        let lighting = Lighting::default();
        let [x, y, z] = lighting.towards_sun;
        Heavens {
            lighting,
            sky: Sky::of(&lighting),
            weather: Air::default(),
            heavens: [
                [x, y, z, 0.0],
                [0.0; 4],
                [0.0, 1.0, 0.0, 0.0],
                [1.0; 4],
                [0.0; 4],
                [1.0, 0.0, 0.0, 0.0],
            ],
            drift: 0.0,
            eye: [0.0; 3],
        }
    }

    /// A while later: the clouds have drifted and the sun has moved a
    /// little, far less than a jump.
    fn later(heavens: &Heavens, seconds: f32) -> Heavens {
        let mut later = *heavens;
        later.drift += seconds;
        let [x, y, z] = later.lighting.towards_sun;
        let turned = [x + 0.001 * seconds, y, z];
        let long = turned.iter().map(|part| part * part).sum::<f32>().sqrt();
        later.lighting.towards_sun = turned.map(|part| part / long);
        later
    }

    #[test]
    fn the_whole_sky_is_drawn_first_then_a_face_a_frame_round_and_round() {
        let mut turns = Turns::default();
        let mut now = clear_noon();
        assert_eq!(turns.plan(Some(&now), false).faces, ALL_FACES);
        let mut drawn = Vec::new();
        for _ in 0..12 {
            now = later(&now, 1.0 / 60.0);
            drawn.push(turns.plan(Some(&now), false).faces);
        }
        assert_eq!(
            drawn,
            [1, 2, 4, 8, 16, 32, 1, 2, 4, 8, 16, 32],
            "each face in turn, one a frame"
        );
    }

    #[test]
    fn a_sky_that_jumps_is_drawn_whole_at_once() {
        let mut turns = Turns::default();
        let now = clear_noon();
        turns.plan(Some(&now), false);
        turns.plan(Some(&later(&now, 0.1)), false);
        // A script sets the hour: the sun is somewhere else at once.
        let mut evening = later(&now, 0.2);
        evening.lighting.towards_sun = [0.9, 0.1, 0.0];
        evening.heavens[0] = [0.9, 0.1, 0.0, 0.0];
        assert_eq!(turns.plan(Some(&evening), false).faces, ALL_FACES);
        // And clouds that come over in a moment are drawn at once too.
        let mut overcast = later(&evening, 0.1);
        overcast.weather.cover = 0.9;
        assert_eq!(turns.plan(Some(&overcast), false).faces, ALL_FACES);
    }

    #[test]
    fn a_picture_held_still_draws_the_sky_only_when_it_changes() {
        let mut turns = Turns::default();
        let now = clear_noon();
        assert_eq!(turns.plan(Some(&now), true).faces, ALL_FACES);
        assert_eq!(turns.plan(Some(&now), true).faces, 0, "nothing has changed");
        // Even a change too small to be a jump is drawn whole, so the
        // same view always gives the same picture.
        let mut wetter = now;
        wetter.weather.rows[0][0] = 0.01;
        assert_eq!(turns.plan(Some(&wetter), true).faces, ALL_FACES);
        assert_eq!(turns.plan(Some(&wetter), true).faces, 0);
    }

    #[test]
    fn a_change_too_slow_for_one_frame_still_redraws_the_whole_sky_before_faces_drift_apart() {
        let mut turns = Turns::default();
        let mut now = clear_noon();
        turns.plan(Some(&now), false);
        // The cloud cover grows by 0.04 a frame: never a tenth in one frame,
        // but a face drawn three frames ago is a tenth behind.
        let mut whole = Vec::new();
        for _ in 0..6 {
            now = later(&now, 1.0 / 60.0);
            now.weather.cover += 0.04;
            whole.push(turns.plan(Some(&now), false).faces == ALL_FACES);
        }
        assert!(whole.iter().any(|&whole| whole), "{whole:?}");
    }

    #[test]
    fn rain_beginning_or_the_eye_going_far_redraws_the_whole_sky() {
        let mut turns = Turns::default();
        let now = clear_noon();
        turns.plan(Some(&now), false);
        let mut raining = later(&now, 0.1);
        raining.weather.rows[0][0] = 0.6;
        assert_eq!(turns.plan(Some(&raining), false).faces, ALL_FACES);
        let mut elsewhere = later(&raining, 0.1);
        elsewhere.eye = [500.0, 0.0, 0.0];
        assert_eq!(turns.plan(Some(&elsewhere), false).faces, ALL_FACES);
    }

    #[test]
    fn the_first_picture_held_still_after_time_passed_draws_the_whole_sky() {
        let mut turns = Turns::default();
        let mut now = clear_noon();
        for _ in 0..4 {
            now = later(&now, 1.0 / 60.0);
            turns.plan(Some(&now), false);
        }
        // The same sky as the frame before, but the faces were drawn at
        // four different moments.
        assert_eq!(turns.plan(Some(&now), true).faces, ALL_FACES);
        assert_eq!(turns.plan(Some(&now), true).faces, 0);
        // A camera moved a little while held still sees the same sky; one
        // moved far is drawn from where it is.
        let mut moved = now;
        moved.eye = [3.0, 1.0, 0.0];
        assert_eq!(turns.plan(Some(&moved), true).faces, 0);
        moved.eye = [300.0, 1.0, 0.0];
        assert_eq!(turns.plan(Some(&moved), true).faces, ALL_FACES);
    }

    #[test]
    fn with_no_sky_the_numbers_are_wiped_once_and_drawn_whole_after() {
        let mut turns = Turns::default();
        let now = clear_noon();
        assert_eq!(
            turns.plan(None, false),
            Work::default(),
            "nothing drawn yet, nothing to wipe"
        );
        turns.plan(Some(&now), false);
        assert_eq!(
            turns.plan(None, false),
            Work {
                faces: 0,
                forget: true
            }
        );
        assert_eq!(turns.plan(None, false), Work::default());
        assert_eq!(turns.plan(Some(&now), false).faces, ALL_FACES);
    }
}
