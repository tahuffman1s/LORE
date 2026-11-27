//! The layers a group of HUD or screen pieces faded as one is drawn into:
//! pictures the size of the screen, each an image of its own, drawn into
//! by itself and read by the shader that lays it over the picture. A pass
//! that draws into one layer reads the others through a set of its own in
//! which that layer's place holds a blank picture of one pixel, so no pass
//! can read the image it draws into.
//!
//! Every layer is kept in the general layout from the moment it is made,
//! so it is always fit both to be drawn into and to be read: a shader that
//! could read it never finds it in a layout it cannot read, and drawing a
//! group needs a barrier but never a change of layout.

use ash::vk;

use crate::buffer::once;
use crate::error::RenderError;
use crate::gpu::Gpu;

/// One layer: an image, its memory and the view of it.
pub(crate) struct Layer {
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub(crate) view: vk::ImageView,
}

impl Layer {
    const fn null() -> Self {
        Self {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
        }
    }

    /// Makes a layer of `extent` pixels in `format`, cleared and in the
    /// general layout; on failure, what was made is left in `self`.
    fn create(
        &mut self,
        gpu: &Gpu,
        extent: vk::Extent2D,
        format: vk::Format,
        doing: &'static str,
    ) -> Result<(), RenderError> {
        let device = gpu.device();
        let range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        // SAFETY: the device is alive, and each object is created from
        // valid settings and stored in `self` straight away, for `destroy`
        // to free should a later step fail. The memory is as large as the
        // image asks for and of a kind it allows. The image is used by the
        // one submission `once` waits for.
        unsafe {
            self.image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(extent.into())
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .usage(
                            vk::ImageUsageFlags::COLOR_ATTACHMENT
                                | vk::ImageUsageFlags::SAMPLED
                                | vk::ImageUsageFlags::TRANSFER_DST,
                        ),
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
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(range),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            // Into the general layout, cleared, before anything can read it.
            let image = self.image;
            once(gpu, doing, |commands| {
                let to_clear = [vk::ImageMemoryBarrier2::default()
                    .dst_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::GENERAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_clear),
                );
                device.cmd_clear_color_image(
                    commands,
                    image,
                    vk::ImageLayout::GENERAL,
                    &vk::ClearColorValue::default(),
                    &[range],
                );
                let to_use = [vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(
                        vk::PipelineStageFlags2::FRAGMENT_SHADER
                            | vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                    )
                    .dst_access_mask(
                        vk::AccessFlags2::SHADER_SAMPLED_READ
                            | vk::AccessFlags2::COLOR_ATTACHMENT_READ
                            | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                    )
                    .old_layout(vk::ImageLayout::GENERAL)
                    .new_layout(vk::ImageLayout::GENERAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_use),
                );
            })
        }
    }

    /// # Safety
    ///
    /// Made on `gpu` (or null), and no longer in use.
    unsafe fn destroy(&mut self, gpu: &Gpu) {
        let device = gpu.device();
        // SAFETY: the caller guarantees the handles are this device's and
        // idle; destroying a null handle is allowed.
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
        *self = Self::null();
    }
}

/// Every layer, and the blank one read in place of the layer a pass draws
/// into, and of layers there are not.
pub(crate) struct Layers {
    /// One for each layer.
    pub(crate) each: Vec<Layer>,
    /// One clear pixel.
    pub(crate) blank: Layer,
    /// The size of every layer but the blank one.
    pub(crate) extent: vk::Extent2D,
}

impl Layers {
    /// Nothing; destroying it does nothing.
    pub(crate) const fn null() -> Self {
        Self {
            each: Vec::new(),
            blank: Layer::null(),
            extent: vk::Extent2D {
                width: 0,
                height: 0,
            },
        }
    }

    /// How many layers there are.
    pub(crate) fn count(&self) -> u32 {
        u32::try_from(self.each.len()).unwrap_or(0)
    }

    /// The view the shader reads as layer `index` while drawing into layer
    /// `drawing`, if any: the blank one for that layer and for layers there
    /// are not.
    pub(crate) fn view(&self, index: usize, drawing: Option<usize>) -> vk::ImageView {
        if drawing == Some(index) {
            return self.blank.view;
        }
        self.each
            .get(index)
            .map_or(self.blank.view, |layer| layer.view)
    }

    /// Makes `count` layers of `extent` pixels in `format`, in the general
    /// layout. Waits for the graphics card: layers are made when the window
    /// changes size or more are needed, not every frame.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver reports a failure or has no memory of a
    /// kind the images can use.
    pub(crate) fn create(
        gpu: &Gpu,
        extent: vk::Extent2D,
        count: u32,
        format: vk::Format,
    ) -> Result<Self, RenderError> {
        let doing = "making room to draw faded groups of the HUD and menus";
        let mut layers = Self {
            extent,
            ..Self::null()
        };
        let made = (|| {
            let one = vk::Extent2D {
                width: 1,
                height: 1,
            };
            layers.blank.create(gpu, one, format, doing)?;
            if extent.width > 0 && extent.height > 0 {
                for _ in 0..count {
                    layers.each.push(Layer::null());
                    if let Some(layer) = layers.each.last_mut() {
                        layer.create(gpu, extent, format, doing)?;
                    }
                }
            }
            Ok::<(), RenderError>(())
        })();
        match made {
            Ok(()) => Ok(layers),
            Err(error) => {
                // SAFETY: nothing has used what was made; destroying it, or
                // a null handle, is allowed.
                unsafe { layers.destroy(gpu) };
                Err(error)
            }
        }
    }

    /// A barrier on every layer: what was drawn into them, or read from
    /// them, before it is done before what comes after it. No layout
    /// changes, so one barrier on memory covers them all.
    pub(crate) fn barrier() -> vk::MemoryBarrier2<'static> {
        let used = vk::PipelineStageFlags2::FRAGMENT_SHADER
            | vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT;
        let access = vk::AccessFlags2::SHADER_SAMPLED_READ
            | vk::AccessFlags2::COLOR_ATTACHMENT_READ
            | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE;
        vk::MemoryBarrier2::default()
            .src_stage_mask(used)
            .src_access_mask(access)
            .dst_stage_mask(used)
            .dst_access_mask(access)
    }

    /// Frees the layers.
    ///
    /// # Safety
    ///
    /// They must have been made on `gpu` (or be null), and the graphics card
    /// must have finished using them.
    pub(crate) unsafe fn destroy(&mut self, gpu: &Gpu) {
        // SAFETY: the caller guarantees the layers are this device's and idle.
        unsafe {
            for layer in &mut self.each {
                layer.destroy(gpu);
            }
            self.blank.destroy(gpu);
        }
        self.each.clear();
    }
}
