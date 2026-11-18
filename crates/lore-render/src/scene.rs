//! The images the scene is drawn into before it is tone mapped for the display.

use std::cell::{Cell, OnceCell};
use std::sync::Arc;

use ash::vk;

use crate::buffer::once;
use crate::error::RenderError;
use crate::fluid_pass::FluidPictures;
use crate::gpu::Gpu;
use crate::swapchain::COLOUR;

/// What the renderer is doing when it makes a [`SceneTarget`]'s images.
pub(crate) const DOING: &str = "making the images the scene is drawn into";

/// The whole of a depth image.
pub(crate) const DEPTH: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::DEPTH,
    ..COLOUR
};

/// One image on the graphics card, with its memory and a view of all of it.
pub(crate) struct Picture {
    pub(crate) image: vk::Image,
    memory: vk::DeviceMemory,
    pub(crate) view: vk::ImageView,
}

impl Picture {
    /// A picture that holds nothing; destroying it does nothing.
    pub(crate) const fn null() -> Self {
        Self {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
        }
    }

    /// Fills in `self` with an image of `extent` pixels, with as many
    /// smaller copies of itself as `range` has levels.
    ///
    /// On failure, what was created so far is left in `self` for
    /// [`destroy`](Self::destroy) to clean up, and the error says the
    /// renderer was `doing` this.
    pub(crate) fn create(
        &mut self,
        gpu: &Gpu,
        extent: vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        range: vk::ImageSubresourceRange,
        doing: &'static str,
    ) -> Result<(), RenderError> {
        self.create_shaped(gpu, extent.into(), false, format, usage, range, doing)
    }

    /// The same for a block of pixels `extent` deep as well, which a
    /// shader reads as one 3D picture.
    pub(crate) fn create_block(
        &mut self,
        gpu: &Gpu,
        extent: vk::Extent3D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        range: vk::ImageSubresourceRange,
        doing: &'static str,
    ) -> Result<(), RenderError> {
        self.create_shaped(gpu, extent, true, format, usage, range, doing)
    }

    #[allow(clippy::too_many_arguments)]
    fn create_shaped(
        &mut self,
        gpu: &Gpu,
        extent: vk::Extent3D,
        block: bool,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        range: vk::ImageSubresourceRange,
        doing: &'static str,
    ) -> Result<(), RenderError> {
        let device = gpu.device();
        let (image_type, view_type) = if block {
            (vk::ImageType::TYPE_3D, vk::ImageViewType::TYPE_3D)
        } else {
            (vk::ImageType::TYPE_2D, vk::ImageViewType::TYPE_2D)
        };
        // SAFETY: the device is alive, and each object is created from valid
        // settings and stored in `self` straight away. The memory is as
        // large as the image asks for and of a kind it allows.
        unsafe {
            self.image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(image_type)
                        .format(format)
                        .extent(extent)
                        .mip_levels(range.level_count)
                        .array_layers(1)
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
                        .view_type(view_type)
                        .format(format)
                        .subresource_range(range),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
        }
        Ok(())
    }

    /// Frees the picture.
    ///
    /// # Safety
    ///
    /// The picture must have been made on `gpu` (or be null), and the
    /// graphics card must have finished using it.
    pub(crate) unsafe fn destroy(&self, gpu: &Gpu) {
        let device = gpu.device();
        // SAFETY: the caller guarantees the handles are this device's and
        // idle; destroying a null handle is allowed.
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// What the scene is drawn into: a high-dynamic-range colour image, a depth
/// image and how far each pixel moved since the last frame, all the size the
/// scene is drawn at.
///
/// Light is stored as it is, with no upper limit, so the sun can be thousands
/// of times brighter than a shadow; [`TonemapPipeline`](crate::tonemap::TonemapPipeline)
/// turns it into colours a display can show.
pub(crate) struct SceneTarget {
    gpu: Arc<Gpu>,
    pub(crate) colour: Picture,
    pub(crate) depth: Picture,
    pub(crate) motion: Picture,
    /// A copy of the colour and of the depth as they were before anything
    /// see-through was drawn, for water to look through: what is under the
    /// surface, and how far under. Only filled in a frame that has water.
    pub(crate) behind: Picture,
    pub(crate) behind_depth: Picture,
    /// Whether those copies were filled in the frame last drawn: in the
    /// next, smooth things may mirror what they hold.
    pub(crate) kept: Cell<bool>,
    /// What fluids are worked out in: made the first frame that has any,
    /// and kept for as long as the target is.
    pub(crate) fluid: OnceCell<FluidPictures>,
    pub(crate) extent: vk::Extent2D,
}

impl SceneTarget {
    /// Sixteen-bit floats for red, green and blue.
    ///
    /// Every Vulkan 1.3 graphics card can draw into and read this format. The
    /// smaller `B10G11R11` has too few steps for upscaling and anti-aliasing
    /// to work on without banding.
    pub(crate) const FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

    /// A 32-bit float for how far away each pixel is: 1 at the camera's near
    /// distance, falling to 0 infinitely far away.
    ///
    /// Every desktop graphics card can draw depth into this format. Floats
    /// are most precise near 0, which this ordering spends on the far
    /// distance, where a shooter needs it.
    pub(crate) const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

    /// Two sixteen-bit floats for how far a pixel moved across and down the
    /// picture since the last frame: where it was minus where it is, as a
    /// fraction of the picture, so the same numbers hold at any size.
    ///
    /// Sixteen bits tell apart moves of a thousandth of a pixel, which the
    /// upscaler needs to keep a slowly turning view sharp.
    pub(crate) const MOTION_FORMAT: vk::Format = vk::Format::R16G16_SFLOAT;

    /// The depth of a pixel nothing was drawn on: infinitely far away.
    pub(crate) const FARTHEST: f32 = 0.0;

    /// Makes a target of `extent` pixels.
    pub(crate) fn new(gpu: &Arc<Gpu>, extent: vk::Extent2D) -> Result<Self, RenderError> {
        let mut target = Self {
            gpu: Arc::clone(gpu),
            colour: Picture::null(),
            depth: Picture::null(),
            motion: Picture::null(),
            behind: Picture::null(),
            behind_depth: Picture::null(),
            kept: Cell::new(false),
            fluid: OnceCell::new(),
            extent,
        };
        // Dropping `target` cleans up whatever was created.
        target.colour.create(
            gpu,
            extent,
            Self::FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC,
            COLOUR,
            DOING,
        )?;
        // Sampled as well, because upscaling and later effects read the depth.
        target.depth.create(
            gpu,
            extent,
            Self::DEPTH_FORMAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC,
            DEPTH,
            DOING,
        )?;
        target.behind.create(
            gpu,
            extent,
            Self::FORMAT,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            COLOUR,
            DOING,
        )?;
        target.behind_depth.create(
            gpu,
            extent,
            Self::DEPTH_FORMAT,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            DEPTH,
            DOING,
        )?;
        // Every surface that could mirror the world is drawn with the
        // copies at hand, in frames before any has been filled too: they
        // are made ready to be read from the start, holding nothing yet.
        let ready = [
            (target.behind.image, COLOUR),
            (target.behind_depth.image, DEPTH),
        ]
        .map(|(image, range)| {
            vk::ImageMemoryBarrier2::default()
                .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(range)
        });
        // SAFETY: the images were made on `gpu` just now and nothing else
        // uses them yet; `once` waits until the commands have run.
        unsafe {
            once(gpu, DOING, |commands| {
                gpu.device().cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&ready),
                );
            })
        }?;
        // Copied out by the tests, which check the numbers.
        target.motion.create(
            gpu,
            extent,
            Self::MOTION_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC,
            COLOUR,
            DOING,
        )?;
        Ok(target)
    }
}

impl Drop for SceneTarget {
    fn drop(&mut self) {
        // SAFETY: the pictures were made on `self.gpu`, and the owner waits
        // for the graphics card to go idle before dropping the target.
        unsafe {
            self.colour.destroy(&self.gpu);
            self.depth.destroy(&self.gpu);
            self.motion.destroy(&self.gpu);
            self.behind.destroy(&self.gpu);
            self.behind_depth.destroy(&self.gpu);
        }
    }
}
