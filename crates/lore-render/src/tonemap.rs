//! The last passes: the glow round bright things, then one pass that grades
//! the scene's high-dynamic-range light and fits it to the display.
//!
//! What they do is said by a [`Said`](crate::look::Said): see
//! [`crate::look`].

use std::io::Cursor;
use std::sync::Arc;

use ash::vk;

use crate::buffer::once;
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::look::{SETTINGS_BYTES, SIZE_BYTES, Said};
use crate::scene::Picture;
use crate::swapchain::COLOUR;
use crate::ui_pass::UiPass;

/// The shaders, compiled from `shaders/tonemap.wgsl` by the build script.
const SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tonemap.spv"));

/// The most sizes the glow is blurred at: from half the picture down to a
/// sixty-fourth of it.
const MOST_LEVELS: usize = 6;

/// What the glow is drawn in: light with no upper limit, as the scene is.
const GLOW_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

/// What the renderer is doing when it makes the glow's images.
const GLOW_DOING: &str = "making the images the glow round bright things is drawn in";

/// What happens to an image after the picture is drawn into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Then {
    /// It is shown in the window.
    Show,
    /// Its pixels are copied out.
    Copy,
}

/// An image to draw the finished picture into.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Canvas {
    pub(crate) image: vk::Image,
    /// A view of the whole image.
    pub(crate) view: vk::ImageView,
    /// The image's size in pixels.
    pub(crate) extent: vk::Extent2D,
}

/// How much brighter the picture is for an exposure of `stops`.
///
/// One stop doubles the light, as on a camera.
pub(crate) fn exposure_scale(stops: f32) -> f32 {
    stops.exp2()
}

/// Whether images of `format` apply the sRGB curve themselves when written.
pub(crate) fn stores_srgb(format: vk::Format) -> bool {
    matches!(
        format,
        vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB | vk::Format::A8B8G8R8_SRGB_PACK32
    )
}

/// The bindings of the set the tone mapping reads the picture through: one
/// sampled image. The game's screen effects make sets with the same
/// bindings, so that the last of them can be read here instead.
pub(crate) fn set_bindings() -> [vk::DescriptorSetLayoutBinding<'static>; 1] {
    [vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)]
}

/// The size of the largest glow for a picture of `extent`: half of it.
fn glow_extent(extent: vk::Extent2D) -> vk::Extent2D {
    vk::Extent2D {
        width: (extent.width / 2).max(1),
        height: (extent.height / 2).max(1),
    }
}

/// How many sizes the glow of a picture of `extent` is blurred at: each
/// half the one before, as many as keep the smallest at least 2 pixels
/// across, up to [`MOST_LEVELS`].
fn glow_levels(extent: vk::Extent2D) -> u32 {
    let base = glow_extent(extent);
    let narrowest = base.width.min(base.height);
    let mut levels = 1;
    while (levels as usize) < MOST_LEVELS && narrowest >> levels >= 2 {
        levels += 1;
    }
    levels
}

/// The size of the glow `level` (0 for the largest), as Vulkan sizes the
/// smaller copies of an image.
fn level_extent(base: vk::Extent2D, level: u32) -> vk::Extent2D {
    vk::Extent2D {
        width: (base.width >> level).max(1),
        height: (base.height >> level).max(1),
    }
}

/// The images the glow is drawn in: one image with a smaller copy for each
/// size, and a view of each copy by itself.
struct Glow {
    picture: Picture,
    levels: [vk::ImageView; MOST_LEVELS],
    /// How many sizes there are; 0 before the first picture.
    count: u32,
    /// The size of the largest.
    extent: vk::Extent2D,
    /// The size of the picture it is made for.
    for_extent: vk::Extent2D,
}

impl Glow {
    const fn null() -> Self {
        Self {
            picture: Picture::null(),
            levels: [vk::ImageView::null(); MOST_LEVELS],
            count: 0,
            extent: vk::Extent2D {
                width: 0,
                height: 0,
            },
            for_extent: vk::Extent2D {
                width: 0,
                height: 0,
            },
        }
    }

    /// The images for a picture of `extent`, holding no light and ready to
    /// be read. On failure, what was made is left in `self` to destroy.
    fn create(&mut self, gpu: &Gpu, extent: vk::Extent2D) -> Result<(), RenderError> {
        let count = glow_levels(extent);
        let whole = vk::ImageSubresourceRange {
            level_count: count,
            ..COLOUR
        };
        self.extent = glow_extent(extent);
        self.for_extent = extent;
        self.picture.create(
            gpu,
            self.extent,
            GLOW_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_DST,
            whole,
            GLOW_DOING,
        )?;
        let device = gpu.device();
        for (level, view) in (0..count).zip(&mut self.levels) {
            // SAFETY: the image is alive and has this level; the view is
            // stored in `self` straight away.
            *view = unsafe {
                device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(self.picture.image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(GLOW_FORMAT)
                        .subresource_range(vk::ImageSubresourceRange {
                            base_mip_level: level,
                            level_count: 1,
                            ..COLOUR
                        }),
                    None,
                )
            }
            .map_err(RenderError::driver(GLOW_DOING))?;
        }
        self.count = count;
        let image = self.picture.image;
        let barrier = || {
            vk::ImageMemoryBarrier2::default()
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(whole)
        };
        // A frame with no glow still names the largest in the final pass,
        // so it must be readable, and hold nothing, from the start.
        // SAFETY: the image is alive until `once` has waited for the
        // commands, and each is used in the layout the barrier before left.
        unsafe {
            once(gpu, GLOW_DOING, |commands| {
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&[barrier()
                        .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .old_layout(vk::ImageLayout::UNDEFINED)
                        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)]),
                );
                device.cmd_clear_color_image(
                    commands,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue::default(),
                    &[whole],
                );
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&[barrier()
                        .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                        .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]),
                );
            })
        }
    }

    /// Frees the images and leaves `self` holding none.
    ///
    /// # Safety
    ///
    /// They must be `gpu`'s (or null), and no longer in use.
    unsafe fn destroy(&mut self, gpu: &Gpu) {
        // SAFETY: the caller's guarantee; destroying a null handle is allowed.
        unsafe {
            for view in self.levels {
                gpu.device().destroy_image_view(view, None);
            }
            self.picture.destroy(gpu);
        }
        *self = Self::null();
    }
}

/// Draws the scene, graded and tone mapped, into images of one colour
/// format, with the glow round bright things first.
pub(crate) struct TonemapPipeline {
    gpu: Arc<Gpu>,
    set_layout: vk::DescriptorSetLayout,
    /// The sampler and the glow, for every pass.
    glow_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    /// Halving the scene, halving a glow, and enlarging one into the one
    /// above.
    glow_pipelines: [vk::Pipeline; 3],
    sampler: vk::Sampler,
    pool: vk::DescriptorPool,
    /// Says which scene image the shader reads.
    set: vk::DescriptorSet,
    /// The sampler and the largest glow.
    glow_set: vk::DescriptorSet,
    /// Each size of glow, read as the scene is.
    level_sets: [vk::DescriptorSet; MOST_LEVELS],
    glow: Glow,
    /// The colour format of the images this can draw into.
    pub(crate) format: vk::Format,
}

/// What is drawn over the tone-mapped scene, in this order.
#[derive(Default)]
pub(crate) struct Over<'a> {
    /// The HUD and menus.
    pub(crate) ui: Option<&'a UiPass>,
    /// The editor's panels.
    #[cfg(feature = "panels")]
    pub(crate) panels: Option<&'a mut crate::panels::PanelHost>,
}

impl<'a> Over<'a> {
    /// The HUD and menus alone.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn ui(ui: Option<&'a UiPass>) -> Self {
        Self {
            ui,
            #[cfg(feature = "panels")]
            panels: None,
        }
    }
}

/// Makes one pipeline that draws a triangle over all of an image of
/// `format` with the fragment function `fragment` of `module`.
///
/// # Safety
///
/// `module` must be the compiled `tonemap.wgsl` and `layout` a layout it
/// fits, both of `device`.
unsafe fn full_screen(
    device: &ash::Device,
    module: vk::ShaderModule,
    layout: vk::PipelineLayout,
    fragment: &std::ffi::CStr,
    format: vk::Format,
    adds: bool,
) -> Result<vk::Pipeline, vk::Result> {
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(module)
            .name(c"vertex"),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(module)
            .name(fragment),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    // The viewport and scissor are set while drawing, so a resized window
    // does not need a new pipeline.
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let mut attachment = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA);
    if adds {
        attachment = attachment
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ZERO)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE)
            .alpha_blend_op(vk::BlendOp::ADD);
    }
    let attachments = [attachment];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let formats = [format];
    let mut rendering =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic)
        .layout(layout)
        .push_next(&mut rendering);
    // SAFETY: `info` and everything it points to are alive for the call;
    // the caller guarantees the module holds both entry points and fits
    // the layout.
    let created = unsafe {
        device.create_graphics_pipelines(
            vk::PipelineCache::null(),
            std::slice::from_ref(&info),
            None,
        )
    };
    match created {
        Ok(pipelines) if !pipelines.is_empty() => Ok(pipelines[0]),
        failed => Err(failed.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN)),
    }
}

impl TonemapPipeline {
    /// Builds the pipeline for drawing into images of `format`.
    pub(crate) fn new(gpu: &Arc<Gpu>, format: vk::Format) -> Result<Self, RenderError> {
        let doing = "preparing the shaders";
        let device = gpu.device();
        let code = ash::util::read_spv(&mut Cursor::new(SHADER)).map_err(|_| {
            // Only possible if the build script wrote a broken file.
            RenderError::Driver {
                doing,
                code: vk::Result::ERROR_INVALID_SHADER_NV,
            }
        })?;
        let mut tonemap = Self {
            gpu: Arc::clone(gpu),
            set_layout: vk::DescriptorSetLayout::null(),
            glow_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            glow_pipelines: [vk::Pipeline::null(); 3],
            sampler: vk::Sampler::null(),
            pool: vk::DescriptorPool::null(),
            set: vk::DescriptorSet::null(),
            glow_set: vk::DescriptorSet::null(),
            level_sets: [vk::DescriptorSet::null(); MOST_LEVELS],
            glow: Glow::null(),
            format,
        };
        // Dropping `tonemap` cleans up whatever was created; destroying a null handle is allowed.

        let bindings = set_bindings();
        let glow_bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        ];
        let sets = 2 + MOST_LEVELS as u32;
        let sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(sets),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLER)
                .descriptor_count(1),
        ];
        let ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            .size(SETTINGS_BYTES as u32)];

        // SAFETY: the device is alive, and each object is created from valid
        // settings that live until the call returns, and stored in `tonemap`
        // straight away.
        unsafe {
            tonemap.set_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            tonemap.glow_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&glow_bindings),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            let set_layouts = [tonemap.set_layout, tonemap.glow_layout];
            tonemap.layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&set_layouts)
                        .push_constant_ranges(&ranges),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            tonemap.sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::LINEAR)
                        .min_filter(vk::Filter::LINEAR)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            tonemap.pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(sets)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            let mut layouts = vec![tonemap.set_layout; 1 + MOST_LEVELS];
            layouts.push(tonemap.glow_layout);
            let allocated = device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(tonemap.pool)
                        .set_layouts(&layouts),
                )
                .map_err(RenderError::driver(doing))?;
            tonemap.set = allocated.first().copied().unwrap_or_default();
            for (set, allocated) in tonemap.level_sets.iter_mut().zip(allocated.iter().skip(1)) {
                *set = *allocated;
            }
            tonemap.glow_set = allocated.last().copied().unwrap_or_default();
            let samplers = [vk::DescriptorImageInfo::default().sampler(tonemap.sampler)];
            device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::default()
                    .dst_set(tonemap.glow_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .image_info(&samplers)],
                &[],
            );
        }

        // SAFETY: `code` is SPIR-V produced and validated by naga at build time.
        let module = unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)
        }
        .map_err(RenderError::driver(doing))?;
        // SAFETY: the module is `tonemap.wgsl`'s and the layout made for it;
        // the module is only needed while the pipelines are being created.
        let made = unsafe {
            let made = (|| {
                tonemap.pipeline =
                    full_screen(device, module, tonemap.layout, c"fragment", format, false)?;
                for ((fragment, adds), pipeline) in [
                    (c"fragment_glow_first", false),
                    (c"fragment_glow_halve", false),
                    (c"fragment_glow_enlarge", true),
                ]
                .into_iter()
                .zip(&mut tonemap.glow_pipelines)
                {
                    *pipeline =
                        full_screen(device, module, tonemap.layout, fragment, GLOW_FORMAT, adds)?;
                }
                Ok(())
            })();
            device.destroy_shader_module(module, None);
            made
        };
        made.map_err(RenderError::driver(doing))?;
        Ok(tonemap)
    }

    /// Makes later frames read the scene from `view`, a picture of
    /// `extent`, and makes the glow's images for that size if they are not
    /// already.
    ///
    /// # Errors
    ///
    /// Fails if the glow's images cannot be made.
    ///
    /// # Safety
    ///
    /// `view` must be a live view of a [`SceneTarget`](crate::scene::SceneTarget)
    /// on this pipeline's device, and the graphics card must not be running
    /// any commands recorded by [`record`](Self::record).
    pub(crate) unsafe fn read_from(
        &mut self,
        view: vk::ImageView,
        extent: vk::Extent2D,
    ) -> Result<(), RenderError> {
        let device = self.gpu.device();
        if self.glow.count == 0 || self.glow.for_extent != extent {
            // SAFETY: the caller guarantees nothing recorded is running.
            unsafe { self.glow.destroy(&self.gpu) };
            if let Err(error) = self.glow.create(&self.gpu, extent) {
                // SAFETY: as above; what was made is not in use.
                unsafe { self.glow.destroy(&self.gpu) };
                return Err(error);
            }
        }
        let image = |view: vk::ImageView| {
            [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
        };
        let scene = image(view);
        let largest = image(self.glow.levels[0]);
        let levels: Vec<_> = self.glow.levels[..self.glow.count as usize]
            .iter()
            .map(|level| image(*level))
            .collect();
        let mut writes = vec![
            vk::WriteDescriptorSet::default()
                .dst_set(self.set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(&scene),
            vk::WriteDescriptorSet::default()
                .dst_set(self.glow_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(&largest),
        ];
        for (set, level) in self.level_sets.iter().zip(&levels) {
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(*set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(level),
            );
        }
        // SAFETY: the sets belong to this pipeline, the caller guarantees the
        // view is live and that the sets are not in use; the glow's views
        // are this pipeline's own.
        unsafe { device.update_descriptor_sets(&writes, &[]) };
        Ok(())
    }

    /// Records drawing the glow: the picture `source` reads halved again
    /// and again, then each size enlarged and added to the one above, which
    /// leaves the sum of them all in the largest, ready to be read.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), outside a render pass.
    unsafe fn record_glow(
        &self,
        commands: vk::CommandBuffer,
        source: vk::DescriptorSet,
        said: &Said,
    ) {
        let device = self.gpu.device();
        let count = self.glow.count;
        let range = |first: u32, levels: u32| vk::ImageSubresourceRange {
            base_mip_level: first,
            level_count: levels,
            ..COLOUR
        };
        let barrier = |levels: vk::ImageSubresourceRange| {
            vk::ImageMemoryBarrier2::default()
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(self.glow.picture.image)
                .subresource_range(levels)
        };
        let reading = vk::PipelineStageFlags2::FRAGMENT_SHADER;
        let drawing = vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT;
        // Each is drawn whole before it is read, so what it held is not
        // kept; the last frame may still be reading it.
        let to_draw_all = barrier(range(0, count))
            .src_stage_mask(reading)
            .src_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
            .dst_stage_mask(drawing)
            .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        let to_read = |level: u32| {
            barrier(range(level, 1))
                .src_stage_mask(drawing)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(reading)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        };
        // Back to be added to, keeping what it holds.
        let to_add = |level: u32| {
            barrier(range(level, 1))
                .src_stage_mask(reading)
                .src_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .dst_stage_mask(drawing)
                .dst_access_mask(
                    vk::AccessFlags2::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                )
                .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        };
        // Worked out once; each pass changes only the size it draws into.
        let settings = said.bytes(count, [0, 0], false);
        let pass = |pipeline: vk::Pipeline, level: u32, from: vk::DescriptorSet, keep: bool| {
            let extent = level_extent(self.glow.extent, level);
            let area = vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent,
            };
            // Sizes are far below the point where f32 stops being exact.
            #[allow(clippy::cast_precision_loss)]
            let viewport = vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: extent.width as f32,
                height: extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            };
            let attachments = [vk::RenderingAttachmentInfo::default()
                .image_view(self.glow.levels[level as usize])
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(if keep {
                    vk::AttachmentLoadOp::LOAD
                } else {
                    vk::AttachmentLoadOp::DONT_CARE
                })
                .store_op(vk::AttachmentStoreOp::STORE)];
            let mut settings = settings;
            // Sizes are far below the point where f32 stops being exact.
            #[allow(clippy::cast_precision_loss)]
            let size = [extent.width as f32, extent.height as f32];
            for (bytes, value) in settings[SIZE_BYTES]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(size)
            {
                *bytes = value.to_ne_bytes();
            }
            // SAFETY: the caller's guarantees; the level drawn into was made
            // ready by the barrier before, and the set read is ready to read.
            unsafe {
                device.cmd_begin_rendering(
                    commands,
                    &vk::RenderingInfo::default()
                        .render_area(area)
                        .layer_count(1)
                        .color_attachments(&attachments),
                );
                device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, pipeline);
                device.cmd_bind_descriptor_sets(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.layout,
                    0,
                    &[from, self.glow_set],
                    &[],
                );
                device.cmd_push_constants(
                    commands,
                    self.layout,
                    vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &settings,
                );
                device.cmd_set_viewport(commands, 0, &[viewport]);
                device.cmd_set_scissor(commands, 0, &[area]);
                device.cmd_draw(commands, 3, 1, 0, 0);
                device.cmd_end_rendering(commands);
            }
        };
        let [first, halve, enlarge] = self.glow_pipelines;
        // SAFETY: the caller's guarantees; each level is in the layout the
        // barrier before it left it in.
        unsafe {
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&[to_draw_all]),
            );
            for level in 0..count {
                let (pipeline, from) = if level == 0 {
                    (first, source)
                } else {
                    (halve, self.level_sets[level as usize - 1])
                };
                pass(pipeline, level, from, false);
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&[to_read(level)]),
                );
            }
            for level in (0..count.saturating_sub(1)).rev() {
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&[to_add(level)]),
                );
                pass(enlarge, level, self.level_sets[level as usize + 1], true);
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&[to_read(level)]),
                );
            }
        }
    }

    /// Records drawing the graded, tone-mapped scene over the whole of
    /// `canvas`, with the glow drawn first if `said` asks for any.
    ///
    /// The image's previous contents are discarded. What `over` holds is
    /// drawn on top: the HUD and menus it was prepared with, then the
    /// editor's panels.
    ///
    /// # Errors
    ///
    /// Fails if the editor's panels cannot be drawn. The commands are left
    /// complete, but the frame should not be shown.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this pipeline's device that is
    /// being recorded, outside a render pass. `canvas` must be a live colour
    /// image of this pipeline's format with its whole-image view and its
    /// size, and the image must not be in use by other work on the graphics
    /// card. The scene given to [`read_from`](Self::read_from) must be
    /// alive, of the size given there, and ready to be read by shaders when
    /// these commands run. `read` is a set made with [`set_bindings`] whose
    /// image is ready to be read by shaders when these commands run, of the
    /// same size, or `None` for the scene given to
    /// [`read_from`](Self::read_from). The picture of the other need not be
    /// ready. `over.ui` must have been made for this pipeline's format, and
    /// [`prepare`](UiPass::prepare)d with `commands` for this frame; the
    /// panels must have been prepared for this pipeline's format since they
    /// were last set.
    pub(crate) unsafe fn record(
        &self,
        commands: vk::CommandBuffer,
        canvas: Canvas,
        said: &Said,
        over: Over<'_>,
        read: Option<vk::DescriptorSet>,
        then: Then,
    ) -> Result<(), RenderError> {
        let device = self.gpu.device();
        let source = read.unwrap_or(self.set);
        let glows = said.bloom() > 0.0 && self.glow.count > 0;
        if glows {
            // SAFETY: the caller's guarantees, which are this function's.
            unsafe { self.record_glow(commands, source, said) };
        }
        let mut said = *said;
        if !glows {
            said.look.bloom = 0.0;
        }
        let Canvas {
            image,
            view,
            extent,
        } = canvas;
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        };

        let to_draw = [vk::ImageMemoryBarrier2::default()
            // Waiting at this stage lines up with the wait on the image
            // becoming available when the commands are submitted.
            .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(COLOUR)];

        let (stage, access, layout) = match then {
            Then::Show => (
                vk::PipelineStageFlags2::NONE,
                vk::AccessFlags2::NONE,
                vk::ImageLayout::PRESENT_SRC_KHR,
            ),
            Then::Copy => (
                vk::PipelineStageFlags2::COPY,
                vk::AccessFlags2::TRANSFER_READ,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            ),
        };
        let to_finish = [vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .dst_stage_mask(stage)
            .dst_access_mask(access)
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(layout)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(COLOUR)];

        // Every pixel is drawn, so there is nothing to clear.
        let attachments = [vk::RenderingAttachmentInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE)];
        let rendering = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .color_attachments(&attachments);
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: extent.width as f32,
            height: extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let settings = said.bytes(
            self.glow.count,
            [extent.width, extent.height],
            !stores_srgb(self.format),
        );

        // SAFETY: the caller guarantees the command buffer is recording and
        // the images and views are live and match this pipeline; every struct
        // passed lives until its call returns. The largest glow is ready to
        // be read: drawn just now, or left so since it was made.
        unsafe {
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_draw),
            );
            device.cmd_begin_rendering(commands, &rendering);
            device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            device.cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &[source, self.glow_set],
                &[],
            );
            device.cmd_push_constants(
                commands,
                self.layout,
                vk::ShaderStageFlags::FRAGMENT,
                0,
                &settings,
            );
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            device.cmd_draw(commands, 3, 1, 0, 0);
            if let Some(ui) = over.ui {
                ui.draw(commands, extent);
            }
            #[cfg(feature = "panels")]
            let panels = match over.panels {
                Some(panels) => panels.draw(commands, extent),
                None => Ok(()),
            };
            #[cfg(not(feature = "panels"))]
            let panels = Ok(());
            // The pass is ended even if the panels failed, so the commands
            // are whole whatever the caller does with them.
            device.cmd_end_rendering(commands);
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_finish),
            );
            panels
        }
    }
}

impl Drop for TonemapPipeline {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the pipeline, so it is not in use. Destroying the pool
        // frees its sets.
        unsafe {
            self.glow.destroy(&self.gpu);
            let device = self.gpu.device();
            device.destroy_pipeline(self.pipeline, None);
            for pipeline in self.glow_pipelines {
                device.destroy_pipeline(pipeline, None);
            }
            device.destroy_sampler(self.sampler, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            device.destroy_descriptor_set_layout(self.glow_layout, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_stop_doubles_the_light() {
        assert!((exposure_scale(0.0) - 1.0).abs() < 1e-6);
        assert!((exposure_scale(1.0) - 2.0).abs() < 1e-6);
        assert!((exposure_scale(-2.0) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn only_srgb_images_are_left_to_encode_themselves() {
        assert!(stores_srgb(vk::Format::B8G8R8A8_SRGB));
        assert!(!stores_srgb(vk::Format::R8G8B8A8_UNORM));
        assert!(!stores_srgb(vk::Format::A2B10G10R10_UNORM_PACK32));
    }

    #[test]
    fn the_glow_is_blurred_at_six_sizes_on_a_large_screen_and_fewer_on_a_small_one() {
        let size = |width, height| vk::Extent2D { width, height };
        assert_eq!(glow_levels(size(1920, 1080)), 6);
        assert_eq!(glow_levels(size(128, 72)), 5);
        assert_eq!(glow_levels(size(4, 4)), 1);
        assert_eq!(glow_levels(size(1, 1)), 1);
        let base = glow_extent(size(1920, 1080));
        assert_eq!(base, size(960, 540));
        assert_eq!(level_extent(base, 5), size(30, 16));
    }
}
