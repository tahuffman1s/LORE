//! What is made the first time a game wants a shader: the thread that
//! builds pipelines, the layouts they share, and what each kind of shader
//! becomes on the graphics card.
//!
//! Everything a pipeline is made from is made on the compile thread, which
//! never submits to the queue; the frame's own thread installs the result.

use std::sync::Arc;

use ash::vk;
use lore_shader::{Compiled, Hook, ParamBlock, ShaderError};

use super::book::Job;
use super::retire::Retiring;
use super::screen::{ScreenPass, Targets};
use super::surface::SurfaceTable;
use super::template::fault;
use super::worker::Worker;
use crate::buffer::Buffer;
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::mesh::{MeshPipeline, Surface, make_surface_pipeline};
use crate::scene::SceneTarget;
use crate::tonemap::set_bindings;

/// What the renderer is doing while it makes these.
const DOING: &str = "preparing the game's shaders";

/// The file errors name when the graphics driver fails to build something.
const THIS_FILE: &str = "crates/lore-render/src/game_shaders/works.rs";

/// The entry points of a screen effect's module.
const VERTEX: &std::ffi::CStr = c"lore_vertex";
const FRAGMENT: &std::ffi::CStr = c"lore_fragment";

/// The entry points of a surface shader's module: the mesh's vertex function,
/// and a fragment function for each kind of surface it draws.
const SURFACE_VERTEX: &std::ffi::CStr = c"vertex";
const SURFACE_FRAGMENTS: [(Surface, &std::ffi::CStr); 3] = [
    (Surface::Solid, c"lore_fragment"),
    (Surface::CutOut, c"lore_fragment_cutout"),
    (Surface::SeeThrough, c"lore_fragment_blend"),
];

/// The size in bytes of the clock a surface shader reads: a `vec4f`.
const CLOCK_BYTES: u64 = 16;

/// The size in bytes of what a screen effect is pushed with: `LoreScreen`.
pub(super) const SCREEN_PUSH_BYTES: u32 = 24;

/// What the thread that builds and the frame share: the pipeline cache and
/// the layouts every screen effect is made with. It is destroyed once the
/// last of its owners is gone, and the thread is one, so never while it builds.
pub(super) struct Shared {
    gpu: Arc<Gpu>,
    /// Used by the compile thread only.
    cache: vk::PipelineCache,
    /// Set 0 of a screen effect: the picture, the depth and a sampler.
    pub(super) sources: vk::DescriptorSetLayout,
    /// Set 1: the effect's values.
    values: vk::DescriptorSetLayout,
    /// A set the tone mapping can read the last effect's picture through.
    pub(super) tonemap: vk::DescriptorSetLayout,
    pub(super) layout: vk::PipelineLayout,
    sampler: vk::Sampler,
    /// Set 3 of a surface shader: its values, then the clock.
    surface_values: vk::DescriptorSetLayout,
    /// What a surface shader is drawn with: the mesh pipeline's three sets,
    /// then set 3, and the mesh pipeline's push constants.
    pub(super) surface_layout: vk::PipelineLayout,
    /// The shader clock for each frame in flight, as the frame writes it.
    pub(super) clocks: Vec<Buffer>,
}

impl Shared {
    fn new(gpu: &Arc<Gpu>, frames: usize, mesh: &MeshPipeline) -> Result<Self, RenderError> {
        let device = gpu.device();
        let mut shared = Self {
            gpu: Arc::clone(gpu),
            cache: vk::PipelineCache::null(),
            sources: vk::DescriptorSetLayout::null(),
            values: vk::DescriptorSetLayout::null(),
            tonemap: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            sampler: vk::Sampler::null(),
            surface_values: vk::DescriptorSetLayout::null(),
            surface_layout: vk::PipelineLayout::null(),
            clocks: Vec::with_capacity(frames),
        };
        // Dropping `shared` cleans up whatever was made; destroying a null handle is allowed.
        // SAFETY: the device is alive, and each object is made from valid
        // settings that live until the call returns and is stored at once.
        unsafe {
            shared.cache = device
                .create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None)
                .map_err(RenderError::driver(DOING))?;
            shared.sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::LINEAR)
                        .min_filter(vk::Filter::LINEAR)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let samplers = [shared.sampler];
            let sources = [
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(2)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                    .immutable_samplers(&samplers),
            ];
            shared.sources = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&sources),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let values = [vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
            shared.values = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&values),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            shared.tonemap = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&set_bindings()),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let sets = [shared.sources, shared.values];
            let ranges = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .size(SCREEN_PUSH_BYTES)];
            shared.layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&sets)
                        .push_constant_ranges(&ranges),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let bindings = [0, 1].map(|binding| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(binding)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            });
            shared.surface_values = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let ([view, material, body], push) = mesh.surface_base();
            let sets = [view, material, body, shared.surface_values];
            let ranges = [push];
            shared.surface_layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&sets)
                        .push_constant_ranges(&ranges),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
        }
        for _ in 0..frames {
            shared.clocks.push(Buffer::writable(
                gpu,
                CLOCK_BYTES,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                DOING,
            )?);
        }
        Ok(shared)
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        // SAFETY: the owners wait for the graphics card to go idle before
        // dropping this, and the compile thread has finished with it.
        unsafe {
            let device = self.gpu.device();
            device.destroy_pipeline_layout(self.surface_layout, None);
            device.destroy_descriptor_set_layout(self.surface_values, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.tonemap, None);
            device.destroy_descriptor_set_layout(self.values, None);
            device.destroy_descriptor_set_layout(self.sources, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_pipeline_cache(self.cache, None);
        }
    }
}

/// What the frame draws a game's shader with, made by the compile thread.
pub(crate) enum Built {
    /// An effect over the finished picture.
    Screen(ScreenShader),
    /// A change to the surface of a level's material.
    Surface(SurfaceShader),
}

impl Built {
    /// The screen effect, if this is one.
    pub(super) fn screen(&self) -> Option<&ScreenShader> {
        match self {
            Self::Screen(shader) => Some(shader),
            Self::Surface(_) => None,
        }
    }

    /// The surface shader, if this is one.
    pub(super) fn surface(&self) -> Option<&SurfaceShader> {
        match self {
            Self::Surface(shader) => Some(shader),
            Self::Screen(_) => None,
        }
    }
}

/// A screen effect: its pipeline, and for each frame in flight its values
/// in a buffer and the set that points at it.
pub(crate) struct ScreenShader {
    gpu: Arc<Gpu>,
    pub(super) pipeline: vk::Pipeline,
    pool: vk::DescriptorPool,
    /// One for each frame in flight, written by the frame that uses it.
    pub(super) buffers: Vec<Buffer>,
    /// One for each frame in flight, written once here and never again.
    pub(super) sets: Vec<vk::DescriptorSet>,
}

impl Drop for ScreenShader {
    fn drop(&mut self) {
        // SAFETY: the owner makes sure the graphics card is done with this
        // (it is retired a few frames before it is dropped, or the device
        // is idle). Destroying the pool frees its sets.
        unsafe {
            let device = self.gpu.device();
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_descriptor_pool(self.pool, None);
        }
    }
}

/// Makes a screen effect from its compiled module.
fn make_screen(
    gpu: &Arc<Gpu>,
    shared: &Shared,
    frames: usize,
    compiled: &Compiled,
    block: &ParamBlock,
) -> Result<ScreenShader, RenderError> {
    let device = gpu.device();
    let mut shader = ScreenShader {
        gpu: Arc::clone(gpu),
        pipeline: vk::Pipeline::null(),
        pool: vk::DescriptorPool::null(),
        buffers: Vec::with_capacity(frames),
        sets: Vec::new(),
    };
    // Dropping `shader` cleans up whatever was made.
    // SAFETY: the SPIR-V was made and checked by `lore-shader`.
    let module = unsafe {
        device.create_shader_module(
            &vk::ShaderModuleCreateInfo::default().code(&compiled.words),
            None,
        )
    }
    .map_err(RenderError::driver(DOING))?;

    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(module)
            .name(VERTEX),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(module)
            .name(FRAGMENT),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    // The viewport and scissor are set while drawing, as for the tone mapping.
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let attachments = [vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let formats = [SceneTarget::FORMAT];
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
        .layout(shared.layout)
        .push_next(&mut rendering);
    // SAFETY: `info` and everything it points to are alive for the call; the
    // module holds both entry points named in `stages`, and is only needed
    // while the pipeline is made. Only this thread uses the cache.
    let created = unsafe {
        let created =
            device.create_graphics_pipelines(shared.cache, std::slice::from_ref(&info), None);
        device.destroy_shader_module(module, None);
        created
    };
    match created {
        Ok(pipelines) if !pipelines.is_empty() => shader.pipeline = pipelines[0],
        failed => {
            let code = failed.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN);
            return Err(RenderError::driver(DOING)(code));
        }
    }

    let size = block.size() as u64;
    for _ in 0..frames {
        shader.buffers.push(Buffer::writable(
            gpu,
            size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            DOING,
        )?);
    }
    let count = u32::try_from(frames).unwrap_or(u32::MAX);
    let sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(count)];
    let layouts = vec![shared.values; frames];
    // SAFETY: the device is alive; each object is made from valid settings
    // that live until the call returns. The sets are written here, before
    // anything can use them, and never again.
    unsafe {
        shader.pool = device
            .create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(count)
                    .pool_sizes(&sizes),
                None,
            )
            .map_err(RenderError::driver(DOING))?;
        shader.sets = device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(shader.pool)
                    .set_layouts(&layouts),
            )
            .map_err(RenderError::driver(DOING))?;
        for (set, buffer) in shader.sets.iter().zip(&shader.buffers) {
            let infos = [vk::DescriptorBufferInfo::default()
                .buffer(buffer.handle)
                .range(size)];
            let writes = [vk::WriteDescriptorSet::default()
                .dst_set(*set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&infos)];
            device.update_descriptor_sets(&writes, &[]);
        }
    }
    Ok(shader)
}

/// A surface shader: its pipelines, one for each kind of surface it draws,
/// and for each frame in flight its values in a buffer and the set (set 3)
/// that points at it and at that frame's clock.
pub(crate) struct SurfaceShader {
    gpu: Arc<Gpu>,
    /// For solid, cut-out and see-through surfaces, in that order.
    pub(super) pipelines: [vk::Pipeline; 3],
    /// Draws a cut-out surface for its depth alone, before the picture,
    /// with the holes this shader cuts.
    pub(super) depth: vk::Pipeline,
    pool: vk::DescriptorPool,
    /// One for each frame in flight, written by the frame that uses it.
    pub(super) buffers: Vec<Buffer>,
    /// One for each frame in flight, written once here and never again.
    pub(super) sets: Vec<vk::DescriptorSet>,
}

impl Drop for SurfaceShader {
    fn drop(&mut self) {
        // SAFETY: as for `ScreenShader`: the card is done with this.
        unsafe {
            let device = self.gpu.device();
            for pipeline in self.pipelines {
                device.destroy_pipeline(pipeline, None);
            }
            device.destroy_pipeline(self.depth, None);
            device.destroy_descriptor_pool(self.pool, None);
        }
    }
}

/// Makes a surface shader from its compiled module.
fn make_surface(
    gpu: &Arc<Gpu>,
    shared: &Shared,
    frames: usize,
    compiled: &Compiled,
    block: &ParamBlock,
) -> Result<SurfaceShader, RenderError> {
    let device = gpu.device();
    let mut shader = SurfaceShader {
        gpu: Arc::clone(gpu),
        pipelines: [vk::Pipeline::null(); 3],
        depth: vk::Pipeline::null(),
        pool: vk::DescriptorPool::null(),
        buffers: Vec::with_capacity(frames),
        sets: Vec::new(),
    };
    // Dropping `shader` cleans up whatever was made.
    // SAFETY: the SPIR-V was made and checked by `lore-shader`.
    let module = unsafe {
        device.create_shader_module(
            &vk::ShaderModuleCreateInfo::default().code(&compiled.words),
            None,
        )
    }
    .map_err(RenderError::driver(DOING))?;
    let mut failed = None;
    for ((surface, fragment), pipeline) in SURFACE_FRAGMENTS.into_iter().zip(&mut shader.pipelines)
    {
        // SAFETY: the module holds the entry points named, written for the
        // surface layout; only this thread uses the cache.
        let made = unsafe {
            make_surface_pipeline(
                device,
                shared.cache,
                shared.surface_layout,
                module,
                SURFACE_VERTEX,
                fragment,
                surface,
            )
        };
        match made {
            Ok(made) => *pipeline = made,
            Err(code) => {
                failed = Some(code);
                break;
            }
        }
    }
    if failed.is_none() {
        // SAFETY: as for the pipelines above.
        let made = unsafe {
            crate::occlusion::depth_pipeline(
                device,
                shared.cache,
                shared.surface_layout,
                module,
                SURFACE_VERTEX,
                Some(c"lore_fragment_depth_cutout"),
            )
        };
        match made {
            Ok(made) => shader.depth = made,
            Err(code) => failed = Some(code),
        }
    }
    // SAFETY: the module is only needed while pipelines are made.
    unsafe { device.destroy_shader_module(module, None) };
    if let Some(code) = failed {
        return Err(RenderError::driver(DOING)(code));
    }

    let size = block.size() as u64;
    for _ in 0..frames {
        shader.buffers.push(Buffer::writable(
            gpu,
            size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            DOING,
        )?);
    }
    let count = u32::try_from(frames).unwrap_or(u32::MAX);
    let sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(count.saturating_mul(2))];
    let layouts = vec![shared.surface_values; frames];
    // SAFETY: the device is alive; each object is made from valid settings
    // that live until the call returns. The sets are written here, before
    // anything can use them, and never again; the clocks live as long as
    // `shared`, which outlives every shader.
    unsafe {
        shader.pool = device
            .create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(count)
                    .pool_sizes(&sizes),
                None,
            )
            .map_err(RenderError::driver(DOING))?;
        shader.sets = device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(shader.pool)
                    .set_layouts(&layouts),
            )
            .map_err(RenderError::driver(DOING))?;
        for ((set, buffer), clock) in shader.sets.iter().zip(&shader.buffers).zip(&shared.clocks) {
            let values = [vk::DescriptorBufferInfo::default()
                .buffer(buffer.handle)
                .range(size)];
            let time = [vk::DescriptorBufferInfo::default()
                .buffer(clock.handle)
                .range(CLOCK_BYTES)];
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(*set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&values),
                vk::WriteDescriptorSet::default()
                    .dst_set(*set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&time),
            ];
            device.update_descriptor_sets(&writes, &[]);
        }
    }
    Ok(shader)
}

/// What the compile thread does after a job's module compiled.
fn make(
    gpu: &Arc<Gpu>,
    shared: &Shared,
    frames: usize,
    job: &Job,
    compiled: &Compiled,
    block: &ParamBlock,
) -> Result<Built, ShaderError> {
    match job.hook {
        Hook::Screen => make_screen(gpu, shared, frames, compiled, block)
            .map(Built::Screen)
            .map_err(|error| fault(THIS_FILE, error.to_string())),
        Hook::Surface => make_surface(gpu, shared, frames, compiled, block)
            .map(Built::Surface)
            .map_err(|error| fault(THIS_FILE, error.to_string())),
    }
}

/// Everything that exists once a game has wanted a shader.
pub(super) struct Works {
    // The thread is joined first, then the rest is dropped.
    pub(super) worker: Worker<Built>,
    /// What the thread built with that the frame needs too.
    pub(super) shared: Arc<Shared>,
    /// What was replaced or removed, until the card is done with it.
    pub(super) retiring: Retiring<Built>,
    /// The same for the screen pass's pictures.
    pub(super) old_targets: Retiring<Targets>,
    /// The pictures effects draw into, while any effect is live.
    pub(super) targets: Option<Targets>,
    pub(super) pass: ScreenPass,
    /// Which surface shader draws which of the level's materials.
    pub(super) surfaces: SurfaceTable,
}

impl Works {
    /// Starts the compile thread and makes what the frame needs.
    pub(super) fn new(
        gpu: &Arc<Gpu>,
        frames: usize,
        mesh: &MeshPipeline,
    ) -> Result<Self, RenderError> {
        let shared = Arc::new(Shared::new(gpu, frames, mesh)?);
        let pass = ScreenPass::new(gpu, &shared, frames)?;
        let surface_layout = shared.surface_layout;
        let (theirs, their_gpu) = (Arc::clone(&shared), Arc::clone(gpu));
        let worker = Worker::new(move |job, compiled, block| {
            make(&their_gpu, &theirs, frames, job, compiled, block)
        })
        .map_err(|_| RenderError::Driver {
            doing: DOING,
            code: vk::Result::ERROR_INITIALIZATION_FAILED,
        })?;
        let delay = frames as u64 + 1;
        Ok(Self {
            worker,
            shared,
            retiring: Retiring::new(delay),
            old_targets: Retiring::new(delay),
            targets: None,
            pass,
            surfaces: SurfaceTable::new(surface_layout),
        })
    }
}
