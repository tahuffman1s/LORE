//! Ambient occlusion: the soft shade in corners, under things and where
//! something stands on the ground (#104).
//!
//! Before the picture is drawn, what is solid is drawn once for its depth
//! alone. From that depth each pixel is asked how much of the light from
//! all round reaches it, the answers are smoothed, and the picture's own
//! shader dims its light from all round by them (`occlusion.wgsl` says how
//! they are found). The depth is then kept for the picture, which shades
//! only what is seen and nothing that something else hides.
//!
//! This file holds the player's setting, the pipelines, and the recording
//! of those passes; the mesh pipeline calls it between making its images
//! ready and drawing the picture.

use ash::vk;

use crate::bounce::Shared;
use crate::scene::{DEPTH, SceneTarget};
use crate::swapchain::COLOUR;

/// How finely the shade in corners and under things is worked out: a
/// player's setting.
///
/// Each pixel looks along a few lines across the picture for what stands
/// near it, a few steps along each. More of both gives smoother shade for
/// more of the graphics card's time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AmbientOcclusion {
    /// No shade is found: corners are as bright as open ground, as before.
    Off,
    /// Two lines, two steps each way along each.
    Low,
    /// Three lines, three steps each way.
    #[default]
    Medium,
    /// Four lines, four steps each way.
    High,
}

impl AmbientOcclusion {
    /// How many lines each pixel looks along, and how many steps it takes
    /// each way along each.
    #[must_use]
    pub fn looks(self) -> (u32, u32) {
        match self {
            Self::Off => (0, 0),
            Self::Low => (2, 2),
            Self::Medium => (3, 3),
            Self::High => (4, 4),
        }
    }
}

/// How strong the shade is drawn when a game has not said: as much as
/// really arrives.
pub(crate) const USUAL_STRENGTH: f32 = 1.0;
/// How far round a point is searched for what shades it, in metres, when a
/// game has not said.
pub(crate) const USUAL_REACH: f32 = 0.6;
/// The strongest a game may ask for.
pub(crate) const STRONGEST: f32 = 4.0;
/// The nearest and farthest a game may ask the search to reach, in metres.
pub(crate) const REACHES: (f32, f32) = (0.05, 4.0);

/// The shade as one frame draws it: the player's setting and what the game
/// has said of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shade {
    pub(crate) quality: AmbientOcclusion,
    /// 0 for none, 1 for as much as really arrives, more for darker.
    pub(crate) strength: f32,
    /// How far round a point is searched, in metres.
    pub(crate) reach: f32,
}

impl Default for Shade {
    /// None: pictures are drawn as they were before unless shade is asked
    /// for.
    fn default() -> Self {
        Self {
            quality: AmbientOcclusion::Off,
            strength: USUAL_STRENGTH,
            reach: USUAL_REACH,
        }
    }
}

impl Shade {
    /// The same with what a game says of it: how strong (0 for none, 1 for
    /// as much as really arrives, up to 4) and how far round each point is
    /// searched, in metres (0.05 to 4). A number that is not one leaves
    /// what was said before.
    pub(crate) fn said(self, strength: f32, reach: f32) -> Self {
        Self {
            quality: self.quality,
            strength: if strength.is_finite() {
                strength.clamp(0.0, STRONGEST)
            } else {
                self.strength
            },
            reach: if reach.is_finite() {
                reach.clamp(REACHES.0, REACHES.1)
            } else {
                self.reach
            },
        }
    }

    /// Whether any shade is found at all.
    pub(crate) fn on(&self) -> bool {
        self.quality != AmbientOcclusion::Off && self.strength > 0.0
    }

    /// What the shader's `view.lens` takes after the near distance: the
    /// strength (0 for no shade), the reach, and the lines and steps as
    /// sixteen times the one plus the other.
    pub(crate) fn numbers(&self) -> [f32; 3] {
        if !self.on() {
            return [0.0; 3];
        }
        let (lines, steps) = self.quality.looks();
        // Both are below sixteen.
        #[allow(clippy::cast_precision_loss)]
        let looks = (lines * 16 + steps) as f32;
        [
            self.strength.clamp(0.0, STRONGEST),
            self.reach.clamp(REACHES.0, REACHES.1),
            looks,
        ]
    }
}

/// What is found before smoothing: the share of light that arrives, and
/// the depth it was found at, which the smoothing tells surfaces apart by
/// to a hundredth: sixteen-bit floats round by a twentieth of that, which
/// over the two pixels the smoothing reaches is half of it.
pub(crate) const FOUND_FORMAT: vk::Format = vk::Format::R32G32_SFLOAT;
/// The smoothed share, as the picture reads it: 256 steps are finer than
/// an eye tells shade apart.
pub(crate) const FORMAT: vk::Format = vk::Format::R8_UNORM;

/// The pipelines that draw depth alone (solid, then cut-out), then the two
/// that find the shade and smooth it.
pub(crate) struct Pipelines {
    pub(crate) depth: [vk::Pipeline; 2],
    pub(crate) find: vk::Pipeline,
    pub(crate) smooth: vk::Pipeline,
}

impl Pipelines {
    pub(crate) const fn null() -> Self {
        Self {
            depth: [vk::Pipeline::null(); 2],
            find: vk::Pipeline::null(),
            smooth: vk::Pipeline::null(),
        }
    }

    /// Makes them from the mesh pipeline's own module, layout and states.
    /// What was made before a failure is left in `self` to be destroyed.
    ///
    /// # Safety
    ///
    /// `module` must be the compiled `mesh.wgsl` and `layout` the mesh
    /// pipeline's, both of `device`; `points` must be how a mesh's corner
    /// points are laid out.
    pub(crate) unsafe fn make(
        &mut self,
        device: &ash::Device,
        module: vk::ShaderModule,
        layout: vk::PipelineLayout,
        points: &vk::PipelineVertexInputStateCreateInfo<'_>,
        shared: &Shared<'_>,
    ) -> Result<(), vk::Result> {
        let create = |info: &vk::GraphicsPipelineCreateInfo<'_>| {
            // SAFETY: `info` and everything it points to are alive for the
            // call, and the caller guarantees the module and the layout.
            let created = unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(info),
                    None,
                )
            };
            match created {
                Ok(pipelines) if !pipelines.is_empty() => Ok(pipelines[0]),
                other => Err(other.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN)),
            }
        };
        // Depth alone: the same points, placed by the same function as the
        // picture's, so each lands exactly where the picture will draw it.
        // A solid surface needs no fragment shader at all.
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(module)
                .name(c"vertex"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(module)
                .name(c"fragment_depth_cutout"),
        ];
        let depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::GREATER);
        let no_colour = vk::PipelineColorBlendStateCreateInfo::default();
        for (cut_out, pipeline) in [false, true].into_iter().zip(&mut self.depth) {
            let stages = if cut_out { &stages[..] } else { &stages[..1] };
            let mut rendering = vk::PipelineRenderingCreateInfo::default()
                .depth_attachment_format(SceneTarget::DEPTH_FORMAT);
            *pipeline = create(
                &vk::GraphicsPipelineCreateInfo::default()
                    .stages(stages)
                    .vertex_input_state(points)
                    .input_assembly_state(shared.assembly)
                    .viewport_state(shared.viewport)
                    .rasterization_state(shared.raster)
                    .multisample_state(shared.multisample)
                    .depth_stencil_state(&depth)
                    .color_blend_state(&no_colour)
                    .dynamic_state(shared.dynamic)
                    .layout(layout)
                    .push_next(&mut rendering),
            )?;
        }
        // Finding and smoothing: one triangle over the whole picture each.
        let no_points = vk::PipelineVertexInputStateCreateInfo::default();
        let attachments = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        for (fragment, format, pipeline) in [
            (c"fragment_occlusion", FOUND_FORMAT, &mut self.find),
            (c"fragment_occlusion_smooth", FORMAT, &mut self.smooth),
        ] {
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vertex_air"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(fragment),
            ];
            let formats = [format];
            let mut rendering =
                vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
            *pipeline = create(
                &vk::GraphicsPipelineCreateInfo::default()
                    .stages(&stages)
                    .vertex_input_state(&no_points)
                    .input_assembly_state(shared.assembly)
                    .viewport_state(shared.viewport)
                    .rasterization_state(shared.raster)
                    .multisample_state(shared.multisample)
                    .color_blend_state(&blend)
                    .dynamic_state(shared.dynamic)
                    .layout(layout)
                    .push_next(&mut rendering),
            )?;
        }
        Ok(())
    }

    /// Frees them.
    ///
    /// # Safety
    ///
    /// They must be `device`'s (or null), and no longer in use.
    pub(crate) unsafe fn destroy(&self, device: &ash::Device) {
        // SAFETY: the caller's guarantee; destroying a null handle is allowed.
        unsafe {
            for pipeline in self.depth {
                device.destroy_pipeline(pipeline, None);
            }
            device.destroy_pipeline(self.find, None);
            device.destroy_pipeline(self.smooth, None);
        }
    }

    /// Records finding the shade from `target`'s depth, which holds what
    /// is solid and is in the state it is drawn into, and smoothing it
    /// into the picture the mesh shader reads. The depth is left as it
    /// was found, in the state it is drawn into.
    ///
    /// # Safety
    ///
    /// `commands` must be recording outside a render pass, with `set` the
    /// frame's view set of `layout`, already pointed at `target`'s
    /// pictures; all must be of `device` and stay alive until the commands
    /// have run.
    pub(crate) unsafe fn record(
        &self,
        device: &ash::Device,
        commands: vk::CommandBuffer,
        layout: vk::PipelineLayout,
        set: vk::DescriptorSet,
        target: &SceneTarget,
    ) {
        let depth_stages = vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
            | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS;
        let reading = vk::PipelineStageFlags2::FRAGMENT_SHADER;
        let depth_to_read = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(depth_stages)
            .src_access_mask(vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE)
            .dst_stage_mask(reading)
            .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
            .old_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(target.depth.image)
            .subresource_range(DEPTH);
        let depth_to_draw = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(reading)
            .src_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
            .dst_stage_mask(depth_stages)
            .dst_access_mask(
                vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                    | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
            )
            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(target.depth.image)
            .subresource_range(DEPTH);
        // Each is written whole, so what it held is not kept; the frame
        // before may still be reading it.
        let to_write = |image: vk::Image| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(reading)
                .src_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(COLOUR)
        };
        let to_read = |image: vk::Image| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(reading)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(COLOUR)
        };
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: target.extent,
        };
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: target.extent.width as f32,
            height: target.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let pass = |pipeline: vk::Pipeline, into: vk::ImageView| {
            let attachments = [vk::RenderingAttachmentInfo::default()
                .image_view(into)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::DONT_CARE)
                .store_op(vk::AttachmentStoreOp::STORE)];
            // SAFETY: the caller's guarantees; the picture drawn into was
            // made ready by the barrier before, and every pixel is written.
            unsafe {
                device.cmd_begin_rendering(
                    commands,
                    &vk::RenderingInfo::default()
                        .render_area(area)
                        .layer_count(1)
                        .color_attachments(&attachments),
                );
                device.cmd_set_viewport(commands, 0, &[viewport]);
                device.cmd_set_scissor(commands, 0, &[area]);
                device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, pipeline);
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
                device.cmd_draw(commands, 3, 1, 0, 0);
                device.cmd_end_rendering(commands);
            }
        };
        // SAFETY: the caller guarantees the command buffer, the target and
        // the states its pictures are in.
        unsafe {
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default()
                    .image_memory_barriers(&[depth_to_read, to_write(target.shade_found.image)]),
            );
            pass(self.find, target.shade_found.view);
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&[
                    to_read(target.shade_found.image),
                    to_write(target.shade.image),
                ]),
            );
            pass(self.smooth, target.shade.view);
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default()
                    .image_memory_barriers(&[to_read(target.shade.image), depth_to_draw]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_shade_is_asked_for_when_it_is_off_or_has_no_strength() {
        assert_eq!(Shade::default().numbers(), [0.0; 3]);
        let none = Shade {
            quality: AmbientOcclusion::High,
            strength: 0.0,
            reach: 1.0,
        };
        assert!(!none.on());
        assert_eq!(none.numbers(), [0.0; 3]);
    }

    #[test]
    fn the_shader_is_told_the_strength_the_reach_and_how_hard_to_look() {
        let shade = Shade {
            quality: AmbientOcclusion::Medium,
            strength: 1.5,
            reach: 0.8,
        };
        assert_eq!(shade.numbers(), [1.5, 0.8, 51.0]);
        // What a game asks for is kept within what the search can do.
        let wild = Shade {
            quality: AmbientOcclusion::Low,
            strength: 100.0,
            reach: 1000.0,
        };
        assert_eq!(wild.numbers(), [STRONGEST, REACHES.1, 34.0]);
    }

    #[test]
    fn more_looks_for_each_step_up() {
        let looks = [
            AmbientOcclusion::Low,
            AmbientOcclusion::Medium,
            AmbientOcclusion::High,
        ]
        .map(|quality| {
            let (lines, steps) = quality.looks();
            lines * steps
        });
        assert!(looks[0] < looks[1] && looks[1] < looks[2]);
        assert_eq!(AmbientOcclusion::Off.looks(), (0, 0));
    }
}
