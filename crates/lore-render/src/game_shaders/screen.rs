//! The screen pass: a game's effects drawn one after another over the
//! finished picture, before it is tone mapped.
//!
//! Effect `i` of `n` reads the picture the one before made (the first reads
//! the scene's own) and draws into one of two images of ours, `A` and `F`,
//! chosen so that the last always draws into `F`, which the tone mapping then
//! reads in place of the scene.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ash::vk;

use super::book::Book;
use super::works::{Built, SCREEN_PUSH_BYTES, Shared};
use crate::camera::Camera;
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::scene::{Picture, SceneTarget};
use crate::swapchain::COLOUR;
use crate::view::SceneView;

/// What the renderer is doing while it makes the pictures.
const DOING: &str = "making the images the game's screen effects draw into";

/// The number the next pair of targets is given; 0 is none.
static NEXT_TARGETS: AtomicU64 = AtomicU64::new(1);

/// Which of the two images an effect draws into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Which {
    A,
    F,
}

/// Which image effect `index` of `count` draws into: the last one into `F`,
/// and each one before it into the other of the two than the one after it.
pub(super) fn draws_into(count: usize, index: usize) -> Which {
    if count.saturating_sub(1).saturating_sub(index) % 2 == 0 {
        Which::F
    } else {
        Which::A
    }
}

/// The two images effects draw into, made while any effect is live.
pub(crate) struct Targets {
    gpu: Arc<Gpu>,
    a: Picture,
    f: Picture,
    /// Never given to another pair: a set that reads them is pointed again
    /// when the pair is not the one it read, which a view's handle cannot
    /// tell, since the driver may give a new view the handle of one just freed.
    ids: [u64; 2],
    pub(super) extent: vk::Extent2D,
}

impl Targets {
    /// Makes both images `extent` pixels in size.
    pub(super) fn new(gpu: &Arc<Gpu>, extent: vk::Extent2D) -> Result<Self, RenderError> {
        let first = NEXT_TARGETS.fetch_add(2, Ordering::Relaxed);
        let mut targets = Self {
            gpu: Arc::clone(gpu),
            a: Picture::null(),
            f: Picture::null(),
            ids: [first, first + 1],
            extent,
        };
        // Dropping `targets` cleans up whatever was made.
        for picture in [&mut targets.a, &mut targets.f] {
            picture.create(
                gpu,
                extent,
                SceneTarget::FORMAT,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                COLOUR,
                DOING,
            )?;
        }
        Ok(targets)
    }

    fn picture(&self, which: Which) -> &Picture {
        match which {
            Which::A => &self.a,
            Which::F => &self.f,
        }
    }
}

impl Drop for Targets {
    fn drop(&mut self) {
        // SAFETY: the pictures were made on `self.gpu`, and the owner makes
        // sure the graphics card is done with them (they are retired a few
        // frames before they are dropped, or the device is idle).
        unsafe {
            self.a.destroy(&self.gpu);
            self.f.destroy(&self.gpu);
        }
    }
}

/// What the pass needs to know about the frame being recorded.
#[derive(Clone, Copy)]
pub(super) struct Frame<'a> {
    pub(super) commands: vk::CommandBuffer,
    /// Which frame in flight this is.
    pub(super) slot: usize,
    pub(super) view: &'a SceneView,
    pub(super) camera: &'a Camera,
    /// The shader clock, in seconds.
    pub(super) time: f32,
}

/// What a set points at, to tell when it must be pointed again.
type Stamp = (u64, u64);

/// One frame in flight's sets.
struct Sets {
    /// Read the scene's picture, then `A`, then `F`, each with the depth.
    sources: [vk::DescriptorSet; 3],
    /// What the tone mapping reads: `F`.
    tonemap: vk::DescriptorSet,
    pointed: [Stamp; 4],
}

/// The sets the effects read through, and the commands that draw them.
pub(super) struct ScreenPass {
    gpu: Arc<Gpu>,
    pool: vk::DescriptorPool,
    slots: Vec<Sets>,
}

impl ScreenPass {
    /// Makes the sets for `frames` frames in flight, pointing at nothing yet.
    pub(super) fn new(
        gpu: &Arc<Gpu>,
        shared: &Shared,
        frames: usize,
    ) -> Result<Self, RenderError> {
        let device = gpu.device();
        let count = u32::try_from(frames).unwrap_or(u32::MAX);
        let sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(count * 7),
            // Their samplers are fixed in the layout, but cost an entry anyway.
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLER)
                .descriptor_count(count * 3),
        ];
        let mut pass = Self {
            gpu: Arc::clone(gpu),
            pool: vk::DescriptorPool::null(),
            slots: Vec::with_capacity(frames),
        };
        // SAFETY: the device is alive, and each object is made from valid
        // settings that live until the call returns and is stored at once.
        unsafe {
            pass.pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(count * 4)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(RenderError::driver(DOING))?;
            let layouts = [
                shared.sources,
                shared.sources,
                shared.sources,
                shared.tonemap,
            ];
            for _ in 0..frames {
                let sets = device
                    .allocate_descriptor_sets(
                        &vk::DescriptorSetAllocateInfo::default()
                            .descriptor_pool(pass.pool)
                            .set_layouts(&layouts),
                    )
                    .map_err(RenderError::driver(DOING))?;
                let [one, two, three, tonemap] = sets[..] else {
                    return Err(RenderError::Driver {
                        doing: DOING,
                        code: vk::Result::ERROR_UNKNOWN,
                    });
                };
                pass.slots.push(Sets {
                    sources: [one, two, three],
                    tonemap,
                    pointed: [(0, 0); 4],
                });
            }
        }
        Ok(pass)
    }

    /// Forgets what the sets point at: the images they read are gone. For
    /// when the graphics card is idle.
    pub(super) fn forget(&mut self) {
        for sets in &mut self.slots {
            sets.pointed = [(0, 0); 4];
        }
    }

    /// Records every live effect in `book`, in order, and gives the set the
    /// tone mapping reads instead of the scene's; `None` if there is no effect.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this pass's device that is
    /// being recorded, outside a render pass, after `view`'s scene was drawn
    /// (and upscaled), which left its picture and depth ready to be read by
    /// fragment shaders. Slot `slot`'s earlier frame must have finished, so
    /// that its sets can be written. `targets` must be the size of `view`'s
    /// picture, and nothing may use the effects but frames on this queue.
    pub(super) unsafe fn record(
        &mut self,
        shared: &Shared,
        book: &Book<Built>,
        targets: &Targets,
        frame: &Frame<'_>,
    ) -> Option<vk::DescriptorSet> {
        let Frame {
            commands,
            slot,
            view,
            camera,
            time,
        } = *frame;
        let count = book.screens().count();
        if count == 0 {
            return None;
        }
        let device = self.gpu.device();
        let sets = self.slots.get_mut(slot)?;

        let (picture, depth) = (view.picture(), view.depth_view());
        let stamps = [
            (view.pictures_id(), 0),
            (targets.ids[0], view.pictures_id()),
            (targets.ids[1], view.pictures_id()),
            (targets.ids[1], 0),
        ];
        let reads = [picture, targets.a.view, targets.f.view];
        for (at, set) in sets.sources.iter().enumerate() {
            if sets.pointed[at] != stamps[at] {
                // SAFETY: the caller guarantees the slot's last frame has
                // finished, so the set is not in use; both views are live.
                unsafe { point(device, *set, reads[at], Some(depth)) };
                sets.pointed[at] = stamps[at];
            }
        }
        if sets.pointed[3] != stamps[3] {
            // SAFETY: as above.
            unsafe { point(device, sets.tonemap, targets.f.view, None) };
            sets.pointed[3] = stamps[3];
        }

        let extent = targets.extent;
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        };
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let (width, height) = (extent.width as f32, extent.height as f32);
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width,
            height,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let reach = (camera.fov_radians() / 2.0).tan();
        let push = [
            reach * width / height.max(1.0),
            reach,
            camera.near_metres(),
            time,
            width,
            height,
        ];
        let mut bytes = [0_u8; SCREEN_PUSH_BYTES as usize];
        for (chunk, number) in bytes.chunks_exact_mut(4).zip(push) {
            chunk.copy_from_slice(&number.to_ne_bytes());
        }

        let mut drawn = 0;
        for (index, entry) in book.screens().enumerate() {
            let Some(shader) = entry
                .live()
                .and_then(|live| live.built.screen())
                .filter(|shader| slot < shader.sets.len())
            else {
                continue;
            };
            let target = targets.picture(draws_into(count, index));
            let source = match index {
                0 => sets.sources[0],
                _ => match draws_into(count, index - 1) {
                    Which::A => sets.sources[1],
                    Which::F => sets.sources[2],
                },
            };
            let to_draw = [vk::ImageMemoryBarrier2::default()
                // The last frame's readers of this image must be done.
                .src_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(target.image)
                .subresource_range(COLOUR)];
            let to_read = [vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(target.image)
                .subresource_range(COLOUR)];
            // Every pixel is drawn, so there is nothing to clear.
            let attachments = [vk::RenderingAttachmentInfo::default()
                .image_view(target.view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::DONT_CARE)
                .store_op(vk::AttachmentStoreOp::STORE)];
            let rendering = vk::RenderingInfo::default()
                .render_area(area)
                .layer_count(1)
                .color_attachments(&attachments);
            // SAFETY: the caller guarantees the command buffer is recording
            // and everything is live; the sets were written for this slot,
            // and every struct passed lives until its call returns.
            unsafe {
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_draw),
                );
                device.cmd_begin_rendering(commands, &rendering);
                device.cmd_bind_pipeline(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    shader.pipeline,
                );
                device.cmd_bind_descriptor_sets(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    shared.layout,
                    0,
                    &[source, shader.sets[slot]],
                    &[],
                );
                device.cmd_push_constants(
                    commands,
                    shared.layout,
                    vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &bytes,
                );
                device.cmd_set_viewport(commands, 0, &[viewport]);
                device.cmd_set_scissor(commands, 0, &[area]);
                device.cmd_draw(commands, 3, 1, 0, 0);
                device.cmd_end_rendering(commands);
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            }
            drawn += 1;
        }
        // An effect that could not be drawn would leave `F` unwritten.
        (drawn == count).then_some(sets.tonemap)
    }
}

impl Drop for ScreenPass {
    fn drop(&mut self) {
        // SAFETY: the owner makes sure the graphics card is done with the
        // sets. Destroying the pool frees them.
        unsafe { self.gpu.device().destroy_descriptor_pool(self.pool, None) };
    }
}

/// Points binding 0 of `set` at `colour`, and binding 1 at `depth` if given.
///
/// # Safety
///
/// The views must be live and ready to be read by shaders whenever the set
/// is used, and the set must not be in use.
unsafe fn point(
    device: &ash::Device,
    set: vk::DescriptorSet,
    colour: vk::ImageView,
    depth: Option<vk::ImageView>,
) {
    let image = |view| {
        vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
    };
    let colour = [image(colour)];
    let used = if depth.is_some() { 2 } else { 1 };
    let depth = [image(depth.unwrap_or_default())];
    let writes = [
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(&colour),
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(1)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(&depth),
    ];
    // SAFETY: the caller guarantees the set is idle and the views are live.
    unsafe { device.update_descriptor_sets(&writes[..used], &[]) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_effect_draws_into_f_and_each_before_it_into_the_other() {
        use Which::{A, F};
        let picks = |count| (0..count).map(|i| draws_into(count, i)).collect::<Vec<_>>();
        assert_eq!(picks(1), [F]);
        assert_eq!(picks(2), [A, F]);
        assert_eq!(picks(3), [F, A, F]);
        assert_eq!(picks(4), [A, F, A, F]);
        assert_eq!(picks(8).last(), Some(&F));
    }
}
