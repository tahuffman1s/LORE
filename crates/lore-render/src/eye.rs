//! The eye: the picture's exposure follows the light there is, as an eye
//! gets used to the dark and to bright light.
//!
//! The sky of each hour is drawn with an exposure of its own (see
//! [`per_lux`](crate::lamps::per_lux)), so how bright a view really is can
//! be worked out from how bright it is drawn: by day it is the light that
//! falls, at night the picture is opened as an eye opens. What the eye
//! adds is to bring each view to how bright a view of its real light
//! should look: a sunlit view as the engine draws noon, and darker views
//! darker still, down to six stops darker for a moonlit one, so that a
//! night stays a night (Unreal's "exposure compensation curve" does the
//! same). So a street between tall buildings, a day all cloud and a dusk,
//! which the sky alone draws darker than they look, open up; a street
//! under lamps at night closes down; a moonlit night is drawn as the sky
//! draws it.
//!
//! Each frame the scene, before it is tone mapped, is measured on the
//! graphics card into a grid of cells (`eye.wgsl`), each the mean of the
//! logarithm of how bright it is, and copied back. A frame or two later,
//! when it has arrived, the cells nearest the middle of the picture are
//! counted most and the darkest and brightest tenth of them are left out,
//! as Unreal's histogram exposure does, and the mean of the rest is how
//! bright the view is. The exposure moves towards what that wants, within
//! how far the eye may open and close, opening slowly and closing three
//! times as fast (light hurts at once; seeing in the dark takes longer).

use std::io::Cursor;
use std::sync::Arc;

use ash::vk;

use crate::buffer::Buffer;
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::scene::{Picture, SceneTarget};
use crate::swapchain::COLOUR;

/// The measuring shader, compiled from `shaders/eye.wgsl` by the build
/// script.
const SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/eye.spv"));

/// The grid the picture is measured in: cells across and up.
const ACROSS: u32 = 64;
const UP: u32 = 36;
const CELLS: usize = (ACROSS * UP) as usize;
/// One 32-bit float a cell.
const FORMAT: vk::Format = vk::Format::R32_SFLOAT;

/// How bright a view in daylight is drawn, as the logarithm (base 2) of its
/// light in the picture's units: what a view across open level ground and
/// its sky (the `hello` example) measures in the engine's own noon light,
/// so that noon in the open is drawn as the sky draws it. A narrow street
/// at noon measures nearly three stops below it.
const USUAL_VIEW: f32 = -1.7;

/// How much darker a dark view is drawn than a sunlit one: none down to
/// `BRIGHT_EV` (EV100 10 is an overcast day or a bright room), and from
/// there evenly more, to `NIGHT_DARKER` stops at `DARK_EV` (EV100 -6 is a
/// moonlit night) and below.
const BRIGHT_EV: f32 = 10.0;
const DARK_EV: f32 = -6.0;
const NIGHT_DARKER: f32 = 6.0;

/// How many stops darker a view whose real light is `ev` (EV100) is drawn
/// than a sunlit one.
fn darker(ev: f32) -> f32 {
    NIGHT_DARKER * ((BRIGHT_EV - ev) / (BRIGHT_EV - DARK_EV)).clamp(0.0, 1.0)
}

/// How the picture follows the light: how far the eye may open where it is
/// darker than open ground under the sky, how far it may close where it is
/// brighter, and how quickly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EyeAdaptation {
    /// How many stops brighter the picture may be drawn where it is dark,
    /// from 0 (never) to 16. Each stop is twice as bright.
    pub opens: f32,
    /// How many stops darker it may be drawn where it is bright, from 0
    /// (never) to 16.
    pub closes: f32,
    /// About how many seconds the eye takes to get used to the dark; it
    /// gets used to bright light three times as fast. 0 is at once.
    pub seconds: f32,
}

impl EyeAdaptation {
    /// An eye that does not adapt: the picture is drawn as the sky is.
    pub const FIXED: Self = Self {
        opens: 0.0,
        closes: 0.0,
        seconds: 0.0,
    };

    /// The same, brought into range.
    #[must_use]
    pub fn sound(self) -> Self {
        let within = |stops: f32, most: f32| {
            if stops.is_finite() {
                stops.clamp(0.0, most)
            } else {
                0.0
            }
        };
        Self {
            opens: within(self.opens, 16.0),
            closes: within(self.closes, 16.0),
            seconds: within(self.seconds, 600.0),
        }
    }

    /// Whether the eye adapts at all.
    #[must_use]
    pub fn adapts(&self) -> bool {
        self.opens > 0.0 || self.closes > 0.0
    }
}

impl Default for EyeAdaptation {
    /// Eight stops either way: how dark each view is drawn is then the
    /// eye's own doing (sunlit views as noon, darker views darker, a
    /// moonlit night six stops darker), and the limits only stop the
    /// extremes. A second and a half to get used to the dark.
    fn default() -> Self {
        Self {
            opens: 8.0,
            closes: 8.0,
            seconds: 1.5,
        }
    }
}

/// What the eye is used to: the exposure it has come to, from what it has
/// seen.
pub(crate) struct Eye {
    pub(crate) adaptation: EyeAdaptation,
    /// How many stops brighter (or, below 0, darker) the picture is drawn
    /// than the sky alone would have it.
    stops: f32,
    /// The cells of the last measuring, and the same in order of brightness
    /// with how much each counts: kept so that measuring needs no new
    /// memory.
    cells: Vec<f32>,
    order: Vec<(f32, f32)>,
}

impl Eye {
    pub(crate) fn new(adaptation: EyeAdaptation) -> Self {
        Self {
            adaptation: adaptation.sound(),
            stops: 0.0,
            cells: vec![0.0; CELLS],
            order: Vec::with_capacity(CELLS),
        }
    }

    /// What the picture's light is multiplied by before it is tone mapped.
    pub(crate) fn multiplier(&self) -> f32 {
        if self.adaptation.adapts() {
            self.stops.exp2()
        } else {
            1.0
        }
    }

    /// Takes in a measuring of the picture (the bytes the graphics card
    /// copied back) drawn with a lux of light as bright as `per_lux`, and
    /// moves on `elapsed` seconds; `at_once` goes all the way, for a picture
    /// that must be the same each time it is drawn.
    pub(crate) fn see(&mut self, bytes: &[u8], per_lux: f32, elapsed: f32, at_once: bool) {
        if !self.adaptation.adapts() {
            self.stops = 0.0;
            return;
        }
        for (cell, word) in self.cells.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *cell = f32::from_ne_bytes(*word);
        }
        let Some(seen) = view_brightness(&self.cells, &mut self.order) else {
            return;
        };
        if !(per_lux.is_finite() && per_lux > 0.0) {
            return;
        }
        // How bright the view really is: its light in candela a square
        // metre (a white wall under `E` lux is `E / pi`), and as a camera
        // would meter it, EV100 = log2(L * 100 / 12.5).
        let ev = seen - per_lux.log2() + 3.0;
        let wanted = USUAL_VIEW - darker(ev) - seen;
        let wanted = wanted.clamp(-self.adaptation.closes, self.adaptation.opens);
        let seconds = if wanted > self.stops {
            self.adaptation.seconds
        } else {
            self.adaptation.seconds / 3.0
        };
        let share = if at_once || seconds <= 0.0 {
            1.0
        } else if elapsed.is_finite() {
            1.0 - (-elapsed.max(0.0) / seconds).exp()
        } else {
            0.0
        };
        self.stops += (wanted - self.stops) * share;
    }
}

/// How bright a view is, as the logarithm (base 2) of its light: the mean
/// of its cells' after the darkest and brightest tenth are left out, the
/// middle of the picture counting twice as much as its corners. `None` if
/// it measured nothing that is a number.
fn view_brightness(cells: &[f32], order: &mut Vec<(f32, f32)>) -> Option<f32> {
    order.clear();
    // Cell numbers are far below where an f32 stops being exact.
    #[allow(clippy::cast_precision_loss)]
    let (across, up) = (ACROSS as f32, UP as f32);
    for (number, &value) in cells.iter().enumerate() {
        if !value.is_finite() {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (
            (number % ACROSS as usize) as f32,
            (number / ACROSS as usize) as f32,
        );
        let aside = (x + 0.5) / across * 2.0 - 1.0;
        let above = (y + 0.5) / up * 2.0 - 1.0;
        let weight = 1.0 - 0.25 * (aside * aside + above * above);
        order.push((value, weight));
    }
    order.sort_by(|one, other| one.0.total_cmp(&other.0));
    let whole: f32 = order.iter().map(|(_, weight)| weight).sum();
    if whole <= 0.0 {
        return None;
    }
    let (low, high) = (whole * 0.1, whole * 0.9);
    let (mut passed, mut sum, mut counted) = (0.0, 0.0, 0.0);
    for &(value, weight) in order.iter() {
        // The part of this cell's weight that is between the two tenths.
        let inside = (passed + weight).min(high) - passed.max(low);
        if inside > 0.0 {
            sum += value * inside;
            counted += inside;
        }
        passed += weight;
    }
    (counted > 0.0).then(|| sum / counted)
}

/// Measures the picture on the graphics card, for each frame that can be in
/// progress at once.
pub(crate) struct EyePass {
    gpu: Arc<Gpu>,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    pool: vk::DescriptorPool,
    /// What the scene is measured into.
    grid: Picture,
    /// For each frame: the set that says which picture is measured, that
    /// picture, the buffer the measuring is copied back into, and whether
    /// one was copied and not yet taken.
    frames: Vec<(
        vk::DescriptorSet,
        std::cell::Cell<vk::ImageView>,
        Buffer,
        std::cell::Cell<bool>,
    )>,
}

impl EyePass {
    /// Builds the pass, for up to `frames` frames in progress at once.
    pub(crate) fn new(gpu: &Arc<Gpu>, frames: usize) -> Result<Self, RenderError> {
        let doing = "preparing the shaders";
        let device = gpu.device();
        let frames_count = frames.max(1);
        let count = u32::try_from(frames_count).unwrap_or(1);
        let mut eye = Self {
            gpu: Arc::clone(gpu),
            set_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            pool: vk::DescriptorPool::null(),
            grid: Picture::null(),
            frames: Vec::with_capacity(frames_count),
        };
        // Dropping `eye` cleans up whatever was created; destroying a null
        // handle is allowed.
        eye.grid.create(
            gpu,
            vk::Extent2D {
                width: ACROSS,
                height: UP,
            },
            FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
            COLOUR,
            doing,
        )?;
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(count)];
        // SAFETY: the device is alive, and each object is created from valid
        // settings that live until the call returns, and stored in `eye`
        // straight away.
        unsafe {
            eye.set_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            let set_layouts = [eye.set_layout];
            eye.layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            eye.pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(count)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            for _ in 0..frames_count {
                let set = device
                    .allocate_descriptor_sets(
                        &vk::DescriptorSetAllocateInfo::default()
                            .descriptor_pool(eye.pool)
                            .set_layouts(&set_layouts),
                    )
                    .map_err(RenderError::driver(doing))?
                    .first()
                    .copied()
                    .unwrap_or_default();
                let buffer = Buffer::writable(
                    gpu,
                    (CELLS * 4) as u64,
                    vk::BufferUsageFlags::TRANSFER_DST,
                    doing,
                )?;
                eye.frames.push((
                    set,
                    std::cell::Cell::new(vk::ImageView::null()),
                    buffer,
                    std::cell::Cell::new(false),
                ));
            }
        }

        let code = ash::util::read_spv(&mut Cursor::new(SHADER)).map_err(|_| {
            // Only possible if the build script wrote a broken file.
            RenderError::Driver {
                doing,
                code: vk::Result::ERROR_INVALID_SHADER_NV,
            }
        })?;
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
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewports = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            // Small whole numbers: exact as floats.
            #[allow(clippy::cast_precision_loss)]
            width: ACROSS as f32,
            #[allow(clippy::cast_precision_loss)]
            height: UP as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        let scissors = [vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: vk::Extent2D {
                width: ACROSS,
                height: UP,
            },
        }];
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewports(&viewports)
            .scissors(&scissors);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let attachments = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::R)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let formats = [FORMAT];
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
            .layout(eye.layout)
            .push_next(&mut rendering);
        // SAFETY: `info` and everything it points to are alive for the call;
        // the shader module holds both entry points named in `stages`. The
        // module is only needed while the pipeline is being created.
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
            Ok(pipelines) if !pipelines.is_empty() => {
                eye.pipeline = pipelines[0];
                Ok(eye)
            }
            failed => {
                let code = failed.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN);
                Err(RenderError::driver(doing)(code))
            }
        }
    }

    /// Records measuring the colour of `target` and copying the measuring
    /// back for frame `frame`.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this pass's device that is
    /// being recorded, outside a render pass; `target`'s colour must have
    /// been drawn and left ready for fragment shaders to read, and stay
    /// alive until the commands have run. The commands last recorded for
    /// the same `frame` must have finished.
    pub(crate) unsafe fn record(
        &self,
        commands: vk::CommandBuffer,
        frame: usize,
        target: &SceneTarget,
    ) {
        let Some((set, bound, _, copied)) = self.frames.get(frame % self.frames.len().max(1))
        else {
            return;
        };
        let device = self.gpu.device();
        if bound.get() != target.colour.view {
            let pictures = [vk::DescriptorImageInfo::default()
                .image_view(target.colour.view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let writes = [vk::WriteDescriptorSet::default()
                .dst_set(*set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(&pictures)];
            // SAFETY: the caller guarantees the commands last recorded with
            // this frame's set have finished, so it is not in use.
            unsafe { device.update_descriptor_sets(&writes, &[]) };
            bound.set(target.colour.view);
        }
        let Some((_, _, buffer, _)) = self.frames.get(frame % self.frames.len().max(1)) else {
            return;
        };
        let to_draw = [vk::ImageMemoryBarrier2::default()
            // The copy back of an earlier frame read it.
            .src_stage_mask(vk::PipelineStageFlags2::COPY)
            .src_access_mask(vk::AccessFlags2::TRANSFER_READ)
            .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.grid.image)
            .subresource_range(COLOUR)];
        let to_copy = [vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::COPY)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.grid.image)
            .subresource_range(COLOUR)];
        let to_read = [vk::BufferMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::COPY)
            .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::HOST)
            .dst_access_mask(vk::AccessFlags2::HOST_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .buffer(buffer.handle)
            .size(vk::WHOLE_SIZE)];
        let attachments = [vk::RenderingAttachmentInfo::default()
            .image_view(self.grid.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE)];
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: vk::Extent2D {
                width: ACROSS,
                height: UP,
            },
        };
        let rendering = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .color_attachments(&attachments);
        let region = [vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(vk::Extent3D {
                width: ACROSS,
                height: UP,
                depth: 1,
            })];
        // SAFETY: the caller guarantees the command buffer is recording
        // outside a render pass and that the target's colour is ready to be
        // read; the grid and the buffer are this pass's own, and the
        // buffer is large enough for the whole grid.
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
                &[*set],
                &[],
            );
            device.cmd_draw(commands, 3, 1, 0, 0);
            device.cmd_end_rendering(commands);
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_copy),
            );
            device.cmd_copy_image_to_buffer(
                commands,
                self.grid.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer.handle,
                &region,
            );
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().buffer_memory_barriers(&to_read),
            );
        }
        copied.set(true);
    }

    /// The measuring copied back for frame `frame`, into `bytes`, if one
    /// was recorded since the last time this was asked; false if not.
    ///
    /// # Safety
    ///
    /// The commands last recorded for the same `frame` must have finished.
    pub(crate) unsafe fn take(
        &self,
        frame: usize,
        bytes: &mut Vec<u8>,
    ) -> Result<bool, RenderError> {
        let Some((_, _, buffer, copied)) = self.frames.get(frame % self.frames.len().max(1)) else {
            return Ok(false);
        };
        if !copied.replace(false) {
            return Ok(false);
        }
        bytes.resize(CELLS * 4, 0);
        // SAFETY: the buffer was made writable and is `CELLS * 4` long, and
        // the caller guarantees the commands that wrote it have finished.
        unsafe { buffer.read_into(bytes) }?;
        Ok(true)
    }
}

impl Drop for EyePass {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the pass, so it is not in use. Destroying the pool frees
        // its sets.
        unsafe {
            let device = self.gpu.device();
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            self.grid.destroy(&self.gpu);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes of a measuring where every cell is `stops`.
    fn even(stops: f32) -> Vec<u8> {
        (0..CELLS).flat_map(|_| stops.to_ne_bytes()).collect()
    }

    /// How bright a lux is drawn at noon.
    const NOON: f32 = crate::lamps::DAYLIGHT_PER_LUX;

    #[test]
    fn a_sunlit_view_is_drawn_as_the_sky_draws_noon() {
        let mut eye = Eye::new(EyeAdaptation::default());
        eye.see(&even(USUAL_VIEW), NOON, 1.0, true);
        assert!(
            (eye.multiplier() - 1.0).abs() < 1e-5,
            "{}",
            eye.multiplier()
        );
    }

    #[test]
    fn the_eye_opens_in_the_dark_and_closes_in_bright_light_within_its_limits() {
        let mut eye = Eye::new(EyeAdaptation::default());
        // A sunlit view one stop darker than usual: one stop brighter.
        eye.see(&even(USUAL_VIEW - 1.0), NOON, 1.0, true);
        assert!(
            (eye.multiplier() - 2.0).abs() < 1e-3,
            "{}",
            eye.multiplier()
        );
        // Far darker: no more than it may open.
        eye.see(&even(USUAL_VIEW - 30.0), NOON * 1e9, 1.0, true);
        assert!((eye.multiplier() - 8.0_f32.exp2()).abs() < 1e-1);
        // Far brighter: no more than it may close.
        eye.see(&even(USUAL_VIEW + 12.0), NOON, 1.0, true);
        assert!((eye.multiplier() - (-8.0_f32).exp2()).abs() < 1e-5);
    }

    #[test]
    fn a_moonlit_view_stays_as_dark_as_the_night_sky_draws_it() {
        // At night a lux is drawn about 2.4 times as bright as white; a
        // moonlit street really sends about 0.002 candela a square metre,
        // EV100 -6: it is drawn six stops darker than a sunlit view.
        let night = 2.4;
        let seen = (0.002_f32 * night).log2();
        let mut eye = Eye::new(EyeAdaptation::default());
        eye.see(&even(seen), night, 1.0, true);
        let stops = eye.multiplier().log2();
        assert!((stops - (USUAL_VIEW - 6.0 - seen)).abs() < 0.05, "{stops}");
        assert!(
            stops.abs() < 1.5,
            "a moonlit street is drawn much as the sky draws it: {stops}"
        );
        // Lamplight thirty times as bright closes it.
        eye.see(&even(seen + 5.0), night, 1.0, true);
        assert!(eye.multiplier().log2() < stops - 2.0);
    }

    #[test]
    fn it_gets_used_to_the_dark_slowly_and_to_bright_light_faster() {
        let mut opening = Eye::new(EyeAdaptation::default());
        opening.see(&even(USUAL_VIEW - 2.0), NOON, 0.5, false);
        let mut closing = Eye::new(EyeAdaptation::default());
        closing.see(&even(USUAL_VIEW + 2.0), NOON, 0.5, false);
        let opened = opening.multiplier().log2() / 2.0;
        let closed = -closing.multiplier().log2() / 2.0;
        assert!(opened > 0.1 && opened < 0.5, "{opened}");
        assert!(closed > opened * 1.5, "closed {closed}, opened {opened}");
    }

    #[test]
    fn an_eye_that_does_not_adapt_draws_the_sky_as_it_is() {
        let mut eye = Eye::new(EyeAdaptation::FIXED);
        eye.see(&even(-12.0), NOON, 1.0, true);
        assert_eq!(eye.multiplier(), 1.0);
    }

    #[test]
    fn the_darkest_and_brightest_tenth_are_left_out() {
        let mut cells = vec![0.0; CELLS];
        // A lamp seen straight on fills a twentieth of the cells.
        for cell in cells.iter_mut().take(CELLS / 20) {
            *cell = 12.0;
        }
        let mut order = Vec::new();
        let seen = view_brightness(&cells, &mut order).unwrap();
        assert!(seen.abs() < 1e-5, "{seen}");
    }
}
