//! Draws the fluids a game pours about (`fluid.rs`), as one liquid: the
//! pictures it is worked out in, the pipelines that work it out, and the
//! passes that record them. What the shaders do is told in `fluid.wgsl`.
//!
//! A frame with no drops records nothing and asks for no memory here.

use std::cell::RefCell;
use std::ffi::CStr;
use std::sync::Arc;

use ash::vk;

use crate::buffer::Buffer;
use crate::error::RenderError;
use crate::fluid::{DROP_BYTES, FluidScene, LOOKS_BYTES, MOST_FLUID_DROPS};
use crate::gpu::Gpu;
use crate::scene::{DEPTH, Picture, SceneTarget};
use crate::swapchain::COLOUR;

/// What the renderer is doing when it makes the fluid's pictures.
const DOING: &str = "making the images fluids are drawn into";
/// What the renderer is doing when it makes the fluid's pipelines.
const WORKS: &str = "preparing the fluid shaders";

/// Which kind of fluid is nearest, plus one.
const KIND_FORMAT: vk::Format = vk::Format::R8_UINT;
/// What the fluid on a line of sight takes and scatters, and how much of
/// it there is: sixteen-bit floats can add up thousands of drops.
const SUMS_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
/// How far away the fluid is, smoothed, in metres.
const DISTANCE_FORMAT: vk::Format = vk::Format::R32_SFLOAT;

/// How many corners the box round one drop is drawn with.
const BOX_CORNERS: u32 = 36;

/// The pictures one drawing target keeps for fluid, made the first frame
/// that has any, all the size the scene is drawn at.
pub(crate) struct FluidPictures {
    gpu: Arc<Gpu>,
    /// The depth of the nearest fluid surface: the fluid's own depth
    /// buffer.
    near: Picture,
    /// The nearest drop's kind, plus one; 0 where there is no fluid.
    kind: Picture,
    /// The light the fluid takes away, and how many metres of it there are.
    amount: Picture,
    /// The light the fluid scatters, and how wet the ground at a rim is.
    light: Picture,
    /// Which way the surfaces under lying drops face, added up weighted by
    /// how much of each drop is over the pixel, and that weight: inside a
    /// puddle the liquid lies as flat as what it lies on.
    lie: Picture,
    /// How far away the fluid is, smoothed across and then down.
    across: Picture,
    down: Picture,
}

impl FluidPictures {
    /// Makes the pictures, `extent` pixels each.
    pub(crate) fn new(gpu: &Arc<Gpu>, extent: vk::Extent2D) -> Result<Self, RenderError> {
        let mut pictures = Self {
            gpu: Arc::clone(gpu),
            near: Picture::null(),
            kind: Picture::null(),
            amount: Picture::null(),
            light: Picture::null(),
            lie: Picture::null(),
            across: Picture::null(),
            down: Picture::null(),
        };
        // Dropping `pictures` cleans up whatever was created.
        let drawn_and_read = vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED;
        pictures.near.create(
            gpu,
            extent,
            SceneTarget::DEPTH_FORMAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            DEPTH,
            DOING,
        )?;
        for (picture, format) in [
            (&mut pictures.kind, KIND_FORMAT),
            (&mut pictures.amount, SUMS_FORMAT),
            (&mut pictures.light, SUMS_FORMAT),
            (&mut pictures.lie, SUMS_FORMAT),
            (&mut pictures.across, DISTANCE_FORMAT),
            (&mut pictures.down, DISTANCE_FORMAT),
        ] {
            picture.create(gpu, extent, format, drawn_and_read, COLOUR, DOING)?;
        }
        Ok(pictures)
    }
}

impl Drop for FluidPictures {
    fn drop(&mut self) {
        // SAFETY: the pictures were made on `self.gpu`, and the owner waits
        // for the graphics card to go idle before dropping the target
        // that holds them.
        unsafe {
            for picture in [
                &self.near,
                &self.kind,
                &self.amount,
                &self.light,
                &self.lie,
                &self.across,
                &self.down,
            ] {
                picture.destroy(&self.gpu);
            }
        }
    }
}

/// The pipelines and the room the fluid is drawn with.
pub(crate) struct FluidWorks {
    gpu: Arc<Gpu>,
    /// What the fluid's shaders read, apart from the view: the looks, and
    /// the pictures.
    set_layout: vk::DescriptorSetLayout,
    /// The mesh pipelines' layout with the fluid's set added after theirs,
    /// and the same push constants, so that binding that set leaves the
    /// others as they were.
    layout: vk::PipelineLayout,
    pool: vk::DescriptorPool,
    /// For each frame that can be in progress at once: the set that points
    /// the shaders at the looks and the pictures, the looks, and the drops.
    sets: Vec<vk::DescriptorSet>,
    looks: Vec<Buffer>,
    drops: Vec<Buffer>,
    /// The picture each frame's set was last pointed at: a window that
    /// changes size gets new ones.
    pointed: RefCell<Vec<vk::ImageView>>,
    /// Draws the nearest surface of each drop.
    front: vk::Pipeline,
    /// Adds up what each drop takes from the line of sight.
    amount: vk::Pipeline,
    /// Smooths the distance across, and then down.
    smooth: [vk::Pipeline; 2],
    /// Shades the liquid over the picture.
    shade: vk::Pipeline,
}

/// What a frame that has fluid draws: borrowed from the pipeline and the
/// target for as long as the frame is recorded.
pub(crate) struct FluidFrame<'a> {
    works: &'a FluidWorks,
    pictures: &'a FluidPictures,
    set: vk::DescriptorSet,
    drops: vk::Buffer,
    count: u32,
    extent: vk::Extent2D,
}

/// Everything `build` needs to make one of the fluid's pipelines.
struct Stage<'a> {
    vertex: &'a CStr,
    fragment: &'a CStr,
    input: &'a vk::PipelineVertexInputStateCreateInfo<'a>,
    colours: &'a [vk::Format],
    depth_format: vk::Format,
    depth: &'a vk::PipelineDepthStencilStateCreateInfo<'a>,
    blends: &'a [vk::PipelineColorBlendAttachmentState],
}

impl FluidWorks {
    /// Builds the pipelines from `module`, the mesh shaders, for up to
    /// `frames` frames in progress at once. `mesh_sets` are the layouts of
    /// the sets the mesh pipelines use, and `push` their push constants.
    pub(crate) fn new(
        gpu: &Arc<Gpu>,
        frames: usize,
        module: vk::ShaderModule,
        mesh_sets: [vk::DescriptorSetLayout; 3],
        push: &[vk::PushConstantRange],
    ) -> Result<Self, RenderError> {
        let device = gpu.device();
        let frames = frames.max(1);
        let count = u32::try_from(frames).unwrap_or(1);
        let mut works = Self {
            gpu: Arc::clone(gpu),
            set_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pool: vk::DescriptorPool::null(),
            sets: Vec::with_capacity(frames),
            looks: Vec::with_capacity(frames),
            drops: Vec::with_capacity(frames),
            pointed: RefCell::new(vec![vk::ImageView::null(); frames]),
            front: vk::Pipeline::null(),
            amount: vk::Pipeline::null(),
            smooth: [vk::Pipeline::null(); 2],
            shade: vk::Pipeline::null(),
        };
        // Dropping `works` cleans up whatever was created; destroying a
        // null handle is allowed.
        let picture_binding = |binding: u32| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(binding)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
        };
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            picture_binding(1),
            picture_binding(2),
            picture_binding(3),
            picture_binding(4),
            picture_binding(5),
            picture_binding(6),
            picture_binding(7),
        ];
        let sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(count),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(count * 7),
        ];
        for _ in 0..frames {
            works.looks.push(Buffer::writable(
                gpu,
                LOOKS_BYTES as u64,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                WORKS,
            )?);
            works.drops.push(Buffer::writable(
                gpu,
                (MOST_FLUID_DROPS * DROP_BYTES) as u64,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                WORKS,
            )?);
        }
        // SAFETY: the device is alive, and each object is created from valid
        // settings that live until the call returns, and stored in `works`
        // straight away.
        unsafe {
            works.set_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .map_err(RenderError::driver(WORKS))?;
            let layouts = [mesh_sets[0], mesh_sets[1], mesh_sets[2], works.set_layout];
            works.layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&layouts)
                        .push_constant_ranges(push),
                    None,
                )
                .map_err(RenderError::driver(WORKS))?;
            works.pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(count)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(RenderError::driver(WORKS))?;
            for looks in &works.looks {
                let own = [works.set_layout];
                let set = device
                    .allocate_descriptor_sets(
                        &vk::DescriptorSetAllocateInfo::default()
                            .descriptor_pool(works.pool)
                            .set_layouts(&own),
                    )
                    .map_err(RenderError::driver(WORKS))?
                    .first()
                    .copied()
                    .unwrap_or_default();
                let buffers = [vk::DescriptorBufferInfo::default()
                    .buffer(looks.handle)
                    .range(LOOKS_BYTES as u64)];
                device.update_descriptor_sets(
                    &[vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .buffer_info(&buffers)],
                    &[],
                );
                works.sets.push(set);
            }
        }
        works.build_pipelines(module)?;
        Ok(works)
    }

    /// Makes the five pipelines. Each draws with a dynamic viewport and
    /// scissor, so that a resized window needs no new ones.
    fn build_pipelines(&mut self, module: vk::ShaderModule) -> Result<(), RenderError> {
        let none = vk::PipelineColorBlendAttachmentState::default();
        let everything = none.color_write_mask(vk::ColorComponentFlags::RGBA);
        let nothing = none.color_write_mask(vk::ColorComponentFlags::empty());
        let added = everything
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE)
            .alpha_blend_op(vk::BlendOp::ADD);
        let no_depth = vk::PipelineDepthStencilStateCreateInfo::default();

        // The drops: one instance each, three rows of four numbers.
        let bindings = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(DROP_BYTES as u32)
            .input_rate(vk::VertexInputRate::INSTANCE)];
        let attributes = [0_u32, 1, 2].map(|row| {
            vk::VertexInputAttributeDescription::default()
                .location(row)
                .format(vk::Format::R32G32B32A32_SFLOAT)
                .offset(row * 16)
        });
        let per_drop = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&bindings)
            .vertex_attribute_descriptions(&attributes);
        let no_points = vk::PipelineVertexInputStateCreateInfo::default();

        // Pass 1, over the same pictures: the nearest surface wins, and
        // only it says which kind it is and how far away it is...
        let nearest = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::GREATER);
        let drawn = [KIND_FORMAT, SUMS_FORMAT, SUMS_FORMAT, SUMS_FORMAT];
        let front = self.pipeline(
            module,
            &Stage {
                vertex: c"vertex_fluid",
                fragment: c"fragment_fluid_front",
                input: &per_drop,
                colours: &drawn,
                depth_format: SceneTarget::DEPTH_FORMAT,
                depth: &nearest,
                blends: &[everything, nothing, nothing, nothing],
            },
        )?;
        self.front = front;
        // ...and every drop adds what it takes and scatters.
        self.amount = self.pipeline(
            module,
            &Stage {
                vertex: c"vertex_fluid",
                fragment: c"fragment_fluid_amount",
                input: &per_drop,
                colours: &drawn,
                depth_format: SceneTarget::DEPTH_FORMAT,
                depth: &no_depth,
                blends: &[nothing, added, added, added],
            },
        )?;
        // Pass 2: one number a pixel, over the whole picture.
        for (pipeline, fragment) in self
            .smooth
            .iter_mut()
            .zip([c"fragment_fluid_across", c"fragment_fluid_down"])
        {
            let made = Self::build(
                self.gpu.device(),
                self.layout,
                module,
                &Stage {
                    vertex: c"vertex_fluid_screen",
                    fragment,
                    input: &no_points,
                    colours: &[DISTANCE_FORMAT],
                    depth_format: vk::Format::UNDEFINED,
                    depth: &no_depth,
                    blends: &[everything],
                },
            )
            .map_err(RenderError::driver(WORKS))?;
            *pipeline = made;
        }
        // Pass 3, in the scene's own pass: every pixel it draws replaces
        // what was there, and says how deep it is.
        let replaces = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::ALWAYS);
        self.shade = self.pipeline(
            module,
            &Stage {
                vertex: c"vertex_fluid_screen",
                fragment: c"fragment_fluid_shade",
                input: &no_points,
                colours: &[SceneTarget::FORMAT, SceneTarget::MOTION_FORMAT],
                depth_format: SceneTarget::DEPTH_FORMAT,
                depth: &replaces,
                blends: &[everything, everything],
            },
        )?;
        Ok(())
    }

    fn pipeline(
        &self,
        module: vk::ShaderModule,
        stage: &Stage<'_>,
    ) -> Result<vk::Pipeline, RenderError> {
        Self::build(self.gpu.device(), self.layout, module, stage)
            .map_err(RenderError::driver(WORKS))
    }

    /// Makes one pipeline from `stage` with the dynamic state they all share.
    fn build(
        device: &ash::Device,
        layout: vk::PipelineLayout,
        module: vk::ShaderModule,
        stage: &Stage<'_>,
    ) -> Result<vk::Pipeline, vk::Result> {
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(module)
                .name(stage.vertex),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(module)
                .name(stage.fragment),
        ];
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let dynamic_states = [
            vk::DynamicState::VIEWPORT,
            vk::DynamicState::SCISSOR,
            vk::DynamicState::CULL_MODE,
            vk::DynamicState::FRONT_FACE,
        ];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(stage.blends);
        let mut rendering = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(stage.colours)
            .depth_attachment_format(stage.depth_format);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(stage.input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .depth_stencil_state(stage.depth)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(layout)
            .push_next(&mut rendering);
        // SAFETY: `info` and everything it points to are alive for the
        // call; the shader module holds both entry points named in
        // `stages`.
        let created = unsafe {
            device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                std::slice::from_ref(&info),
                None,
            )
        };
        match created {
            Ok(pipelines) => pipelines.first().copied().ok_or(vk::Result::ERROR_UNKNOWN),
            Err((_, code)) => Err(code),
        }
    }

    /// Hands this frame's drops and looks to the graphics card and points
    /// the frame's set at `target`'s fluid pictures, making them the first
    /// time. `None` if `scene` has no drops: then nothing is recorded.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver refuses to make the pictures or to take
    /// the drops.
    ///
    /// # Safety
    ///
    /// The commands last recorded with the same `frame` must have finished,
    /// and `target` must belong to the same device.
    pub(crate) unsafe fn prepare<'a>(
        &'a self,
        frame: usize,
        scene: &FluidScene,
        target: &'a SceneTarget,
    ) -> Result<Option<FluidFrame<'a>>, RenderError> {
        let bytes = scene.drops();
        let count = (bytes.len() / DROP_BYTES).min(MOST_FLUID_DROPS);
        if count == 0 {
            return Ok(None);
        }
        let slot = frame % self.sets.len().max(1);
        let (Some(set), Some(looks), Some(drops)) = (
            self.sets.get(slot).copied(),
            self.looks.get(slot),
            self.drops.get(slot),
        ) else {
            return Ok(None);
        };
        if target.fluid.get().is_none() {
            let made = FluidPictures::new(&self.gpu, target.extent)?;
            // Nothing else fills it in: it is only ever made here.
            let _ = target.fluid.set(made);
        }
        let Some(pictures) = target.fluid.get() else {
            return Ok(None);
        };
        // SAFETY: the buffers were made writable and long enough for every
        // drop there can be and for every look, and the caller guarantees
        // the commands that read them last have finished.
        unsafe {
            drops.write(&bytes[..count * DROP_BYTES])?;
            looks.write(scene.looks())?;
        }
        if let Ok(mut pointed) = self.pointed.try_borrow_mut()
            && let Some(bound) = pointed.get_mut(slot)
            && *bound != pictures.near.view
        {
            let views = [
                pictures.near.view,
                pictures.kind.view,
                pictures.amount.view,
                pictures.light.view,
                pictures.across.view,
                pictures.down.view,
                pictures.lie.view,
            ];
            let infos = views.map(|view| {
                [vk::DescriptorImageInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
            });
            let writes: [vk::WriteDescriptorSet; 7] = std::array::from_fn(|picture| {
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(1 + picture as u32)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&infos[picture])
            });
            // SAFETY: the caller guarantees the commands last recorded with
            // this frame's set have finished, so it is not in use.
            unsafe { self.gpu.device().update_descriptor_sets(&writes, &[]) };
            *bound = pictures.near.view;
        }
        // At most `MOST_FLUID_DROPS`, which is far below `u32::MAX`.
        #[allow(clippy::cast_possible_truncation)]
        let count = count as u32;
        Ok(Some(FluidFrame {
            works: self,
            pictures,
            set,
            drops: drops.handle,
            count,
            extent: target.extent,
        }))
    }
}

/// The barrier that begins one image's change of layout, with no queue
/// change.
fn barrier(image: vk::Image, range: vk::ImageSubresourceRange) -> vk::ImageMemoryBarrier2<'static> {
    vk::ImageMemoryBarrier2::default()
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range)
}

/// The barriers of one step, as a command takes them.
fn dependency<'a>(barriers: &'a [vk::ImageMemoryBarrier2<'static>]) -> vk::DependencyInfo<'a> {
    vk::DependencyInfo::default().image_memory_barriers(barriers)
}

impl FluidFrame<'_> {
    /// Records the fluid's own passes: the drops, then the two smoothings.
    /// They leave every fluid picture ready for the shade to read.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of the pipeline's device that is
    /// being recorded, outside a render pass; `view_set` is the frame's set
    /// for the mesh shaders, whose `behind_depth` must be ready to read.
    pub(crate) unsafe fn record_passes(
        &self,
        commands: vk::CommandBuffer,
        view_set: vk::DescriptorSet,
    ) {
        let works = self.works;
        let device = works.gpu.device();
        let pictures = self.pictures;
        let depth_stages = vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
            | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS;
        let drawing = vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT;
        let reading = vk::PipelineStageFlags2::FRAGMENT_SHADER;
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: self.extent,
        };
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.extent.width as f32,
            height: self.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };

        // Every picture starts the frame empty: the last frame's shade may
        // still be reading them.
        let to_draw = |image: vk::Image| {
            barrier(image, COLOUR)
                .src_stage_mask(reading)
                .dst_stage_mask(drawing)
                .dst_access_mask(
                    vk::AccessFlags2::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                )
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        };
        let start = [
            to_draw(pictures.kind.image),
            to_draw(pictures.amount.image),
            to_draw(pictures.light.image),
            to_draw(pictures.lie.image),
            to_draw(pictures.across.image),
            to_draw(pictures.down.image),
            barrier(pictures.near.image, DEPTH)
                .src_stage_mask(reading)
                .dst_stage_mask(depth_stages)
                .dst_access_mask(
                    vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                )
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL),
        ];
        // What was drawn is read from here on.
        let colour_to_read = |image: vk::Image| {
            barrier(image, COLOUR)
                .src_stage_mask(drawing)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(reading)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        };
        let after_drops = [
            colour_to_read(pictures.kind.image),
            colour_to_read(pictures.amount.image),
            colour_to_read(pictures.light.image),
            colour_to_read(pictures.lie.image),
            barrier(pictures.near.image, DEPTH)
                .src_stage_mask(depth_stages)
                .src_access_mask(vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE)
                .dst_stage_mask(reading)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
        ];
        let after_across = [colour_to_read(pictures.across.image)];
        let after_down = [colour_to_read(pictures.down.image)];

        let clear = |view: vk::ImageView, value: vk::ClearColorValue| {
            vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue { color: value })
        };
        let zero = vk::ClearColorValue { float32: [0.0; 4] };
        let no_kind = vk::ClearColorValue { uint32: [0; 4] };
        let drawn = [
            clear(pictures.kind.view, no_kind),
            clear(pictures.amount.view, zero),
            clear(pictures.light.view, zero),
            clear(pictures.lie.view, zero),
        ];
        // Nothing drawn is as far as can be.
        let depth = vk::RenderingAttachmentInfo::default()
            .image_view(pictures.near.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: SceneTarget::FARTHEST,
                    stencil: 0,
                },
            });
        let first_pass = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .color_attachments(&drawn)
            .depth_attachment(&depth);
        let across = [clear(pictures.across.view, zero)];
        let second_pass = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .color_attachments(&across);
        let down = [clear(pictures.down.view, zero)];
        let third_pass = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .color_attachments(&down);

        // SAFETY: the caller guarantees the command buffer is recording
        // outside a render pass; every image is in the layout each pass
        // needs before the pass begins, as the barriers say, and every
        // struct passed lives until its call returns. The draws only name
        // the drops written for this frame.
        unsafe {
            device.cmd_pipeline_barrier2(commands, &dependency(&start));

            // The drops. The shaders here read the looks and the picture of
            // the solid world, and none of the fluid's own pictures.
            device.cmd_begin_rendering(commands, &first_pass);
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            self.bind_sets(commands, view_set);
            // Only the far sides of each box are drawn, so that each pixel
            // is worked out once, even with the eye inside a box.
            device.cmd_set_cull_mode(commands, vk::CullModeFlags::FRONT);
            device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
            device.cmd_bind_vertex_buffers(commands, 0, &[self.drops], &[0]);
            for pipeline in [works.front, works.amount] {
                device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, pipeline);
                device.cmd_draw(commands, BOX_CORNERS, self.count, 0, 0);
            }
            device.cmd_end_rendering(commands);
            device.cmd_pipeline_barrier2(commands, &dependency(&after_drops));

            // Smoothing across: reads the nearest surface's depth.
            device.cmd_begin_rendering(commands, &second_pass);
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            self.bind_sets(commands, view_set);
            self.draw_screen(commands, works.smooth[0]);
            device.cmd_end_rendering(commands);
            device.cmd_pipeline_barrier2(commands, &dependency(&after_across));

            // And down: reads what that made.
            device.cmd_begin_rendering(commands, &third_pass);
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            self.bind_sets(commands, view_set);
            self.draw_screen(commands, works.smooth[1]);
            device.cmd_end_rendering(commands);
            device.cmd_pipeline_barrier2(commands, &dependency(&after_down));
        }
    }

    /// Records shading the liquid over the picture, inside the scene's
    /// render pass, whose first set is already bound.
    ///
    /// # Safety
    ///
    /// As for [`record_passes`](Self::record_passes), inside the scene pass
    /// begun after it, with the mesh shaders' set bound as set 0.
    pub(crate) unsafe fn shade(&self, commands: vk::CommandBuffer) {
        let device = self.works.gpu.device();
        // SAFETY: the caller guarantees the command buffer is recording
        // inside the scene pass; the set was pointed at pictures that
        // `record_passes` left ready to read.
        unsafe {
            device.cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.works.layout,
                3,
                &[self.set],
                &[],
            );
            self.draw_screen(commands, self.works.shade);
        }
    }

    /// Binds the mesh shaders' set and the fluid's.
    ///
    /// # Safety
    ///
    /// As for [`record_passes`](Self::record_passes), inside a render pass.
    unsafe fn bind_sets(&self, commands: vk::CommandBuffer, view_set: vk::DescriptorSet) {
        // SAFETY: the caller guarantees the command buffer is recording;
        // both sets are alive and fit the layout.
        unsafe {
            self.works.gpu.device().cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.works.layout,
                0,
                &[view_set],
                &[],
            );
            self.works.gpu.device().cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.works.layout,
                3,
                &[self.set],
                &[],
            );
        }
    }

    /// Draws `pipeline` over the whole picture: one triangle, with nothing
    /// culled.
    ///
    /// # Safety
    ///
    /// As for [`record_passes`](Self::record_passes), inside a render pass.
    unsafe fn draw_screen(&self, commands: vk::CommandBuffer, pipeline: vk::Pipeline) {
        let device = self.works.gpu.device();
        // SAFETY: the caller guarantees the command buffer is recording
        // inside a render pass the pipeline fits.
        unsafe {
            device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, pipeline);
            device.cmd_set_cull_mode(commands, vk::CullModeFlags::NONE);
            device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
            device.cmd_draw(commands, 3, 1, 0, 0);
        }
    }
}

impl Drop for FluidWorks {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the pipeline, so nothing here is in use. Destroying the
        // pool frees its sets.
        unsafe {
            let device = self.gpu.device();
            for pipeline in [self.front, self.amount, self.shade]
                .into_iter()
                .chain(self.smooth)
            {
                device.destroy_pipeline(pipeline, None);
            }
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
        }
    }
}
