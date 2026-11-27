//! Draws the HUD and menus over the finished picture.
//!
//! This runs inside the tone mapping's pass, after the scene has been
//! upscaled and tone mapped, so text and boxes are drawn at the full size
//! of the window and are never blurred by upscaling or dimmed by exposure.
//! What to draw is worked out by [`Overlay`]; this file hands it to the
//! graphics card. Groups faded as one are drawn first, before that pass
//! begins, each into a [layer](crate::ui_layers::Layers) the size of the
//! screen, which the pass then lays over the picture.

use std::io::Cursor;
use std::sync::Arc;

use ash::vk;

use crate::buffer::{Buffer, once};
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::overlay::{
    ATLAS_HEIGHT, ATLAS_WIDTH, ImageAtlas, LayerPass, MOST_LAYERS, MOST_QUADS, Overlay, QUAD_BYTES,
};
use crate::scene::Picture;
use crate::swapchain::COLOUR;
use crate::tonemap::stores_srgb;
use crate::ui_layers::Layers;

/// The shaders, compiled from `shaders/ui.wgsl` by the build script.
const SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.spv"));

/// The size in bytes of the shader's `Screen`: two `f32`s and a `u32`,
/// rounded up to a multiple of eight as the shader lays it out.
const SCREEN_BYTES: usize = 16;

/// One byte a pixel: how much of it a letter covers.
const ATLAS_FORMAT: vk::Format = vk::Format::R8_UNORM;

/// The game's HUD images: four bytes a pixel, the colours as stored, with
/// the sRGB curve, as box and letter colours are handed to the shader.
const IMAGES_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

/// How many descriptor sets there are: the picture's, which reads every
/// layer, and one for drawing into each layer, which reads a blank picture
/// in that layer's place.
const SETS: usize = 1 + MOST_LAYERS as usize;

/// The first binding of the layers; they take one each from it on.
const FIRST_LAYER: u32 = 3;

/// What one frame in progress needs for itself.
struct Slot {
    /// The frame's boxes and letters.
    quads: Buffer,
    /// New rows of the atlas, on their way to the graphics card.
    rows: Buffer,
}

/// Draws an [`Overlay`] into images of one colour format.
pub(crate) struct UiPass {
    // Dropped before `drop` runs out: the buffers free themselves.
    slots: Vec<Slot>,
    gpu: Arc<Gpu>,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    /// The same, for drawing into a layer: it keeps how solid each pixel
    /// is, which the picture's own pipeline leaves alone.
    layer_pipeline: vk::Pipeline,
    /// The same with no blending, for clearing a group's part of a layer.
    clear_pipeline: vk::Pipeline,
    pool: vk::DescriptorPool,
    /// Point the shader at the atlas, the images and the layers: the first
    /// for the picture, then one for drawing into each layer.
    sets: [vk::DescriptorSet; SETS],
    sampler: vk::Sampler,
    atlas: Picture,
    /// The game's HUD images, packed; one clear pixel until there are any.
    images: Picture,
    /// What faded groups are drawn into; one clear pixel until a group needs
    /// them.
    layers: Layers,
    /// Whether the atlas on the graphics card has been given anything yet.
    filled: bool,
    /// Which slot the frame being recorded uses and how many quads it has.
    frame: Option<(usize, u32)>,
    /// The colour format of the images this can draw into.
    format: vk::Format,
}

impl UiPass {
    /// Builds the pass for drawing into images of `format`, for up to
    /// `frames` frames in progress at once.
    pub(crate) fn new(
        gpu: &Arc<Gpu>,
        format: vk::Format,
        frames: usize,
    ) -> Result<Self, RenderError> {
        let doing = "preparing to draw the HUD and menus";
        let device = gpu.device();
        let code = ash::util::read_spv(&mut Cursor::new(SHADER)).map_err(|_| {
            // Only possible if the build script wrote a broken file.
            RenderError::Driver {
                doing,
                code: vk::Result::ERROR_INVALID_SHADER_NV,
            }
        })?;
        let mut pass = Self {
            slots: Vec::with_capacity(frames.max(1)),
            gpu: Arc::clone(gpu),
            set_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            layer_pipeline: vk::Pipeline::null(),
            clear_pipeline: vk::Pipeline::null(),
            pool: vk::DescriptorPool::null(),
            sets: [vk::DescriptorSet::null(); SETS],
            sampler: vk::Sampler::null(),
            atlas: Picture::null(),
            images: Picture::null(),
            layers: Layers::null(),
            filled: false,
            frame: None,
            format,
        };
        // Dropping `pass` cleans up whatever was created; destroying a null handle is allowed.

        for _ in 0..frames.max(1) {
            pass.slots.push(Slot {
                quads: Buffer::writable(
                    gpu,
                    (MOST_QUADS * QUAD_BYTES) as u64,
                    vk::BufferUsageFlags::VERTEX_BUFFER,
                    doing,
                )?,
                rows: Buffer::writable(
                    gpu,
                    u64::from(ATLAS_WIDTH * ATLAS_HEIGHT),
                    vk::BufferUsageFlags::TRANSFER_SRC,
                    doing,
                )?,
            });
        }
        pass.atlas.create(
            gpu,
            vk::Extent2D {
                width: ATLAS_WIDTH,
                height: ATLAS_HEIGHT,
            },
            ATLAS_FORMAT,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            COLOUR,
            doing,
        )?;

        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        ];
        let layer_bindings: [vk::DescriptorSetLayoutBinding; MOST_LAYERS as usize] =
            std::array::from_fn(|index| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(FIRST_LAYER + index as u32)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            });
        let bindings: Vec<vk::DescriptorSetLayoutBinding> =
            bindings.into_iter().chain(layer_bindings).collect();
        let sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count((2 + MOST_LAYERS) * SETS as u32),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLER)
                .descriptor_count(SETS as u32),
        ];
        let ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .size(SCREEN_BYTES as u32)];

        // SAFETY: the device is alive, and each object is created from valid
        // settings that live until the call returns, and stored in `pass`
        // straight away. The set is pointed at the atlas and sampler made
        // just before, which live as long as it does.
        unsafe {
            pass.set_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            let set_layouts = [pass.set_layout];
            let all_layouts = [pass.set_layout; SETS];
            pass.layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&set_layouts)
                        .push_constant_ranges(&ranges),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            pass.pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(SETS as u32)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            let sets = device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(pass.pool)
                        .set_layouts(&all_layouts),
                )
                .map_err(RenderError::driver(doing))?;
            for (kept, set) in pass.sets.iter_mut().zip(sets) {
                *kept = set;
            }
            // Letters are drawn pixel for pixel, where smoothing changes
            // nothing; it only shows on text larger than the largest kept.
            pass.sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::LINEAR)
                        .min_filter(vk::Filter::LINEAR)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            let images = [vk::DescriptorImageInfo::default()
                .image_view(pass.atlas.view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let samplers = [vk::DescriptorImageInfo::default().sampler(pass.sampler)];
            for set in pass.sets {
                let writes = [
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&images),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .image_info(&samplers),
                ];
                device.update_descriptor_sets(&writes, &[]);
            }
        }
        // Until the game gives pictures, the shader reads one clear pixel,
        // and until a group is faded, a blank picture for every layer.
        pass.set_images(&ImageAtlas::default())?;
        pass.set_layers(
            vk::Extent2D {
                width: 0,
                height: 0,
            },
            0,
        )?;

        // SAFETY: `code` is SPIR-V produced and validated by naga at build time.
        let module = unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)
        }
        .map_err(RenderError::driver(doing))?;

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
        // One entry in the buffer for each box or letter, not for each corner.
        let vertex_bindings = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(QUAD_BYTES as u32)
            .input_rate(vk::VertexInputRate::INSTANCE)];
        // Sixteen groups of four numbers: see `QUAD_BYTES`.
        let attributes: [vk::VertexInputAttributeDescription; 16] = std::array::from_fn(|index| {
            vk::VertexInputAttributeDescription::default()
                .location(index as u32)
                .format(vk::Format::R32G32B32A32_SFLOAT)
                .offset(index as u32 * 16)
        });
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&vertex_bindings)
            .vertex_attribute_descriptions(&attributes);
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        // Each quad is laid over what is there by how solid it is. The
        // picture's own alpha is left alone: a window must stay solid.
        let attachments = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ZERO)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(
                vk::ColorComponentFlags::R
                    | vk::ColorComponentFlags::G
                    | vk::ColorComponentFlags::B,
            )];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        // Into a layer, which starts clear, alpha is kept too: the layer
        // then holds each colour multiplied by how solid it is, and how
        // solid the group is at each pixel.
        let layer_attachments = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let layer_blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&layer_attachments);
        // Clearing replaces what is there.
        let clear_attachments = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(false)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let clear_blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&clear_attachments);
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
            .layout(pass.layout)
            .push_next(&mut rendering);
        let mut layer_rendering =
            vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let layer_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&layer_blend)
            .dynamic_state(&dynamic)
            .layout(pass.layout)
            .push_next(&mut layer_rendering);
        let mut clear_rendering =
            vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let clear_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&clear_blend)
            .dynamic_state(&dynamic)
            .layout(pass.layout)
            .push_next(&mut clear_rendering);

        // SAFETY: `info`, `layer_info` and everything they point to are
        // alive for the call; the shader module holds both entry points
        // named in `stages`. The module is only needed while the pipelines
        // are being created.
        let created = unsafe {
            let created = device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                &[info, layer_info, clear_info],
                None,
            );
            device.destroy_shader_module(module, None);
            created
        };
        match created {
            Ok(pipelines) if pipelines.len() == 3 => {
                pass.pipeline = pipelines[0];
                pass.layer_pipeline = pipelines[1];
                pass.clear_pipeline = pipelines[2];
                Ok(pass)
            }
            failed => {
                let code = failed.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN);
                Err(RenderError::driver(doing)(code))
            }
        }
    }

    /// The colour format of the images this draws into.
    pub(crate) fn format(&self) -> vk::Format {
        self.format
    }

    /// Gives the shader `atlas`, the game's HUD images packed into one
    /// picture, in place of the one it had. Waits for the graphics card to
    /// finish what it was doing first: images change when a game starts or
    /// is saved, not every frame.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver reports a failure.
    pub(crate) fn set_images(&mut self, atlas: &ImageAtlas) -> Result<(), RenderError> {
        let doing = "copying the HUD's images to the graphics card";
        let gpu = Arc::clone(&self.gpu);
        let device = gpu.device();
        let clear = [0_u8; 4];
        let (size, pixels) = if atlas.size[0] > 0 && atlas.size[1] > 0 {
            (atlas.size, atlas.rgba.as_slice())
        } else {
            ([1, 1], clear.as_slice())
        };
        let mut picture = Picture::null();
        let made = (|| {
            picture.create(
                &gpu,
                vk::Extent2D {
                    width: size[0],
                    height: size[1],
                },
                IMAGES_FORMAT,
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                COLOUR,
                doing,
            )?;
            let staging = Buffer::staging(&gpu, pixels.len() as u64, doing)?;
            staging.fill(pixels, doing)?;
            let barrier = || {
                vk::ImageMemoryBarrier2::default()
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(picture.image)
                    .subresource_range(COLOUR)
            };
            let copy = [vk::BufferImageCopy::default()
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image_extent(vk::Extent3D {
                    width: size[0],
                    height: size[1],
                    depth: 1,
                })];
            // SAFETY: the picture and the staging buffer live until `once`
            // has waited for the commands; the picture is written in the
            // layout the first barrier leaves it in, and left for shaders.
            unsafe {
                once(&gpu, doing, |commands| {
                    let to_fill = [barrier()
                        .dst_stage_mask(vk::PipelineStageFlags2::COPY)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .old_layout(vk::ImageLayout::UNDEFINED)
                        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)];
                    let to_read = [barrier()
                        .src_stage_mask(vk::PipelineStageFlags2::COPY)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                        .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                    device.cmd_pipeline_barrier2(
                        commands,
                        &vk::DependencyInfo::default().image_memory_barriers(&to_fill),
                    );
                    device.cmd_copy_buffer_to_image(
                        commands,
                        staging.handle,
                        picture.image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &copy,
                    );
                    device.cmd_pipeline_barrier2(
                        commands,
                        &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                    );
                })?;
            }
            Ok::<(), RenderError>(())
        })();
        if let Err(error) = made {
            // SAFETY: nothing has used the new picture; destroying what was
            // made of it, or a null handle, is allowed.
            unsafe { picture.destroy(&gpu) };
            return Err(error);
        }
        // SAFETY: waiting for the device to go idle means no frame still
        // reads the old picture through the set, and the set is not bound in
        // any command buffer being recorded: the HUD binds it when it draws,
        // after this. The new picture is ready to be read.
        unsafe {
            device
                .device_wait_idle()
                .map_err(RenderError::driver(doing))?;
            let images = [vk::DescriptorImageInfo::default()
                .image_view(picture.view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            for set in self.sets {
                let writes = [vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&images)];
                device.update_descriptor_sets(&writes, &[]);
            }
            self.images.destroy(&gpu);
        }
        self.images = picture;
        Ok(())
    }

    /// Makes `count` layers `extent` in size in place of those there were,
    /// and points the shader at them. Waits for the graphics card to finish
    /// what it was doing first: this happens when the window changes size
    /// or more groups overlap than before, not every frame.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver reports a failure.
    fn set_layers(&mut self, extent: vk::Extent2D, count: u32) -> Result<(), RenderError> {
        let doing = "making room to draw faded groups of the HUD and menus";
        let gpu = Arc::clone(&self.gpu);
        let device = gpu.device();
        let layers = Layers::create(&gpu, extent, count, self.format)?;
        // SAFETY: waiting for the device to go idle means no frame still
        // reads the old layers through the set, and the set is not bound in
        // any command buffer being recorded: it is bound after this. The new
        // layers are in a layout the shader reads.
        unsafe {
            if let Err(code) = device.device_wait_idle() {
                let mut layers = layers;
                layers.destroy(&gpu);
                return Err(RenderError::driver(doing)(code));
            }
            // The picture's set reads every layer; the set for drawing into
            // a layer reads the blank picture in its place.
            for (which, set) in self.sets.into_iter().enumerate() {
                let drawing = which.checked_sub(1);
                for index in 0..MOST_LAYERS as usize {
                    let images = [vk::DescriptorImageInfo::default()
                        .image_view(layers.view(index, drawing))
                        .image_layout(vk::ImageLayout::GENERAL)];
                    let writes = [vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(FIRST_LAYER + index as u32)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&images)];
                    device.update_descriptor_sets(&writes, &[]);
                }
            }
            self.layers.destroy(&gpu);
        }
        self.layers = layers;
        Ok(())
    }

    /// Gets `overlay` ready to be drawn by the frame being recorded: its
    /// boxes and letters go to the graphics card, with any letters it has
    /// not seen before, and its faded groups are drawn into layers for a
    /// picture `extent` pixels in size.
    ///
    /// Call this before the pass that [`draw`](Self::draw) is recorded in
    /// begins.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver reports a failure.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this pass's device that is
    /// being recorded, outside a render pass. `frame` picks which of the
    /// pass's frames this is, and the commands last recorded with the same
    /// `frame` must have finished. The pass must stay alive until the
    /// commands have run, and the picture [`draw`](Self::draw) is then
    /// recorded over must be `extent` in size.
    pub(crate) unsafe fn prepare(
        &mut self,
        commands: vk::CommandBuffer,
        frame: usize,
        overlay: &mut Overlay,
        extent: vk::Extent2D,
    ) -> Result<(), RenderError> {
        self.frame = None;
        if let Some(images) = overlay.take_images() {
            self.set_images(images)?;
        }
        let count = overlay.count();
        let index = frame % self.slots.len().max(1);
        let Some(slot) = self.slots.get(index) else {
            return Ok(());
        };
        if count == 0 {
            // Nothing reads the atlas, so new letters can wait.
            return Ok(());
        }
        let (layered, _) = overlay.layers();
        // SAFETY: the buffer was made writable and long enough for the most
        // quads an overlay holds, those it draws and those of its groups
        // together, and the caller guarantees the commands that read it
        // last have finished.
        unsafe { slot.quads.write_parts(&[overlay.quads(), layered]) }?;

        let new = if self.filled {
            overlay.changed()
        } else {
            overlay.everything()
        };
        if let Some((rows, pixels)) = new {
            let device = self.gpu.device();
            // What is in the atlas already is kept, except the first time,
            // when there is nothing to keep.
            let (before, readers, reading) = if self.filled {
                (
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    vk::PipelineStageFlags2::FRAGMENT_SHADER,
                    vk::AccessFlags2::SHADER_SAMPLED_READ,
                )
            } else {
                (
                    vk::ImageLayout::UNDEFINED,
                    vk::PipelineStageFlags2::NONE,
                    vk::AccessFlags2::NONE,
                )
            };
            let to_fill = [vk::ImageMemoryBarrier2::default()
                .src_stage_mask(readers)
                .src_access_mask(reading)
                .dst_stage_mask(vk::PipelineStageFlags2::COPY)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .old_layout(before)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(self.atlas.image)
                .subresource_range(COLOUR)];
            let to_read = [vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::COPY)
                .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(self.atlas.image)
                .subresource_range(COLOUR)];
            // Whole rows, as wide as the atlas, so the buffer needs no gaps.
            let copy = [vk::BufferImageCopy::default()
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image_offset(vk::Offset3D {
                    x: 0,
                    y: i32::try_from(rows.start).unwrap_or(0),
                    z: 0,
                })
                .image_extent(vk::Extent3D {
                    width: ATLAS_WIDTH,
                    height: rows.end - rows.start,
                    depth: 1,
                })];
            // SAFETY: the rows buffer is writable and as large as the whole
            // atlas, of which `pixels` is a part, and the commands that read
            // it last have finished. The atlas is this pass's own, in the
            // layout the first barrier names: undefined until first filled,
            // and left for shaders to read ever after. Every struct passed
            // lives until its call returns.
            unsafe {
                slot.rows.write(pixels)?;
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_fill),
                );
                device.cmd_copy_buffer_to_image(
                    commands,
                    slot.rows.handle,
                    self.atlas.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &copy,
                );
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            }
            overlay.sent();
            self.filled = true;
        }
        let passes = overlay.layer_passes();
        if !passes.is_empty() {
            let needed = passes.iter().map(|pass| pass.layer + 1).max().unwrap_or(1);
            let room = extent.width > 0 && extent.height > 0;
            if room && (self.layers.extent != extent || self.layers.count() < needed) {
                let count = needed.max(self.layers.count()).min(MOST_LAYERS);
                self.set_layers(extent, count)?;
            }
            // SAFETY: the caller guarantees `commands` is recording outside
            // a render pass, and the frame's buffer holds the groups' quads
            // after the `count` it draws.
            unsafe { self.draw_layers(commands, index, passes, count, extent) };
        }
        self.frame = Some((index, count));
        Ok(())
    }

    /// Records drawing what [`prepare`](Self::prepare) got ready over a
    /// picture `extent` pixels in size.
    ///
    /// # Safety
    ///
    /// `commands` must be the command buffer `prepare` was last called
    /// with, inside a render pass whose one colour image is `extent` in
    /// size and of this pass's format.
    pub(crate) unsafe fn draw(&self, commands: vk::CommandBuffer, extent: vk::Extent2D) {
        let Some((index, count)) = self.frame else {
            return;
        };
        let Some(slot) = self.slots.get(index) else {
            return;
        };
        let device = self.gpu.device();
        let (area, viewport) = whole(extent);
        let screen = self.screen(extent);

        // SAFETY: the caller guarantees the command buffer is recording
        // inside a matching render pass; the pipeline, set and buffer are
        // this pass's own, and `prepare` filled the buffer with `count`
        // quads and left the atlas ready to be read.
        unsafe {
            device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            device.cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &self.sets[..1],
                &[],
            );
            device.cmd_push_constants(
                commands,
                self.layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                &screen,
            );
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            device.cmd_bind_vertex_buffers(commands, 0, &[slot.quads.handle], &[0]);
            device.cmd_draw(commands, 4, count, 0, 0);
        }
    }
}

impl UiPass {
    /// The push constants for drawing into a picture `extent` in size.
    fn screen(&self, extent: vk::Extent2D) -> [u8; SCREEN_BYTES] {
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let (width, height) = (extent.width as f32, extent.height as f32);
        let mut screen = [0_u8; SCREEN_BYTES];
        screen[..4].copy_from_slice(&width.to_ne_bytes());
        screen[4..8].copy_from_slice(&height.to_ne_bytes());
        screen[8..12].copy_from_slice(&u32::from(stores_srgb(self.format)).to_ne_bytes());
        screen
    }

    /// Records the passes that draw faded groups into their layers, in
    /// order: a group inside another is drawn before the one it is in,
    /// which reads it. Each pass clears its groups' parts of its layer and
    /// draws them, over only the part of the layer they cover; between
    /// passes, and after the last, a barrier makes what was drawn ready to
    /// be read.
    ///
    /// # Safety
    ///
    /// `commands` must be recording, outside a render pass, and slot
    /// `index`'s buffer must hold the groups' quads from quad `first` on.
    /// The layers must be `extent` in size and as many as the passes use.
    unsafe fn draw_layers(
        &self,
        commands: vk::CommandBuffer,
        index: usize,
        passes: &[LayerPass],
        first: u32,
        extent: vk::Extent2D,
    ) {
        let Some(slot) = self.slots.get(index) else {
            return;
        };
        let device = self.gpu.device();
        let (_, viewport) = whole(extent);
        let screen = self.screen(extent);
        let barrier = [Layers::barrier()];
        let barrier = vk::DependencyInfo::default().memory_barriers(&barrier);
        // SAFETY: the caller guarantees the command buffer is recording
        // outside a render pass; the layers, pipelines, set and buffer are
        // this pass's own, the layers in the general layout, which they
        // are drawn into and read in. Each pass draws into one layer and
        // reads only the others, and every struct passed lives until its
        // call returns.
        unsafe {
            for pass in passes {
                let (Some(layer), Some(&set)) = (
                    self.layers.each.get(pass.layer as usize),
                    self.sets.get(pass.layer as usize + 1),
                ) else {
                    continue;
                };
                let view = layer.view;
                let Some(area) = on_screen(pass.area, extent) else {
                    continue;
                };
                device.cmd_pipeline_barrier2(commands, &barrier);
                let target = [vk::RenderingAttachmentInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::GENERAL)
                    .load_op(vk::AttachmentLoadOp::LOAD)
                    .store_op(vk::AttachmentStoreOp::STORE)];
                device.cmd_begin_rendering(
                    commands,
                    &vk::RenderingInfo::default()
                        .render_area(area)
                        .layer_count(1)
                        .color_attachments(&target),
                );
                device.cmd_bind_descriptor_sets(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.layout,
                    0,
                    &[set],
                    &[],
                );
                device.cmd_push_constants(
                    commands,
                    self.layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &screen,
                );
                device.cmd_set_viewport(commands, 0, &[viewport]);
                device.cmd_set_scissor(commands, 0, &[area]);
                device.cmd_bind_vertex_buffers(commands, 0, &[slot.quads.handle], &[0]);
                // The groups never overlap in a pass, so all are cleared
                // first and then all drawn.
                for (pipeline, quads) in [
                    (self.clear_pipeline, &pass.clears),
                    (self.layer_pipeline, &pass.quads),
                ] {
                    device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, pipeline);
                    device.cmd_draw(
                        commands,
                        4,
                        quads.end.saturating_sub(quads.start),
                        0,
                        first + quads.start,
                    );
                }
                device.cmd_end_rendering(commands);
            }
            device.cmd_pipeline_barrier2(commands, &barrier);
        }
    }
}

/// The whole of a picture `extent` in size, as an area and a viewport.
fn whole(extent: vk::Extent2D) -> (vk::Rect2D, vk::Viewport) {
    // Sizes are far below the point where f32 stops being exact.
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (extent.width as f32, extent.height as f32);
    (
        vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        },
        vk::Viewport {
            x: 0.0,
            y: 0.0,
            width,
            height,
            min_depth: 0.0,
            max_depth: 1.0,
        },
    )
}

/// The part of `area`, left, top, right and bottom in pixels, that is on a
/// picture `extent` in size; `None` if none of it is.
fn on_screen(area: [i32; 4], extent: vk::Extent2D) -> Option<vk::Rect2D> {
    let width = i32::try_from(extent.width).unwrap_or(i32::MAX);
    let height = i32::try_from(extent.height).unwrap_or(i32::MAX);
    let left = area[0].clamp(0, width);
    let top = area[1].clamp(0, height);
    let right = area[2].clamp(0, width);
    let bottom = area[3].clamp(0, height);
    (right > left && bottom > top).then(|| vk::Rect2D {
        offset: vk::Offset2D { x: left, y: top },
        extent: vk::Extent2D {
            width: u32::try_from(right - left).unwrap_or(0),
            height: u32::try_from(bottom - top).unwrap_or(0),
        },
    })
}

impl Drop for UiPass {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the pass, so nothing here is in use. Destroying the pool
        // frees its set.
        unsafe {
            let device = self.gpu.device();
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline(self.layer_pipeline, None);
            device.destroy_pipeline(self.clear_pipeline, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            device.destroy_sampler(self.sampler, None);
            self.atlas.destroy(&self.gpu);
            self.images.destroy(&self.gpu);
            self.layers.destroy(&self.gpu);
        }
    }
}
