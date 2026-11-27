//! Draws scenes without a window and checks the pixels.
//!
//! This runs wherever Vulkan 1.3 is installed, including on a software
//! renderer, and is skipped elsewhere. Set `LORE_REQUIRE_VULKAN=1` to make
//! a missing Vulkan a failure instead, and `LORE_REQUIRE_VALIDATION=1` to
//! make a missing validation layer one, as CI does on Linux.

use std::sync::Arc;

use ash::vk;
use lore_assets::{Instance, Material, Mesh, MeshPart, Scene, Transform};
use lore_ui::{Anchor, Color, Draw, DrawList, Theme, Ui, UiInput};

use crate::camera::Camera;
use crate::error::RenderError;
use crate::font::Font;
use crate::gpu::{Gpu, validation_errors};
use crate::look::Said;
use crate::mesh::{Contents, MeshPipeline, Meshes};
use crate::overlay::Overlay;
use crate::swapchain::COLOUR;
use crate::tonemap::{Canvas, Over, Then, TonemapPipeline, exposure_scale};
use crate::ui_pass::UiPass;
use crate::upscale::Upscaling;
use crate::view::SceneView;

pub(crate) const SIZE: u32 = 64;
const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

/// Connects to a graphics card, or returns `None` if this computer has none that will do.
pub(crate) fn gpu() -> Option<Arc<Gpu>> {
    // SAFETY: there is no window, so nothing has to outlive the `Gpu`.
    match unsafe { Gpu::new(None) } {
        Ok(gpu) => {
            gpu.require_validation();
            Some(Arc::new(gpu))
        }
        Err(
            error @ (RenderError::NoVulkan(_)
            | RenderError::OldVulkan { .. }
            | RenderError::NoSuitableGpu { .. }),
        ) => {
            let required = std::env::var("LORE_REQUIRE_VULKAN").is_ok_and(|value| value != "0");
            assert!(!required, "LORE_REQUIRE_VULKAN is set, but: {error}");
            eprintln!("Skipping the drawing test: {error}");
            None
        }
        Err(error) => panic!("{error}"),
    }
}

/// What the last of a run of frames looked like.
pub(crate) struct Shot {
    /// The finished picture's RGBA pixels, row by row from the top.
    pub(crate) pixels: Vec<u8>,
    /// How far each drawn pixel moved since the frame before, as a fraction
    /// of the picture: across, then down.
    pub(crate) motion: Vec<[f32; 2]>,
    /// How many pixels wide the scene was drawn, before any upscaling.
    drawn_width: u32,
    /// Whether the frames were upscaled.
    upscaled: bool,
}

/// A sixteen-bit float, as the motion image stores them.
fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let fraction = f32::from(bits & 0x3ff) / 1024.0;
    match exponent {
        0 => sign * fraction * 2.0_f32.powi(-14),
        31 => sign * f32::INFINITY,
        _ => sign * (1.0 + fraction) * 2.0_f32.powi(exponent - 15),
    }
}

/// Makes a buffer of `bytes` bytes that pixels can be copied into and read from.
unsafe fn readback(gpu: &Gpu, bytes: u64) -> (vk::Buffer, vk::DeviceMemory) {
    let device = gpu.device();
    // SAFETY: the device is live and the settings are valid.
    unsafe {
        let buffer = device
            .create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(bytes)
                    .usage(vk::BufferUsageFlags::TRANSFER_DST),
                None,
            )
            .unwrap();
        let needs = device.get_buffer_memory_requirements(buffer);
        let memory = device
            .allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(needs.size)
                    .memory_type_index(
                        gpu.memory_type(
                            needs.memory_type_bits,
                            vk::MemoryPropertyFlags::HOST_VISIBLE
                                | vk::MemoryPropertyFlags::HOST_COHERENT,
                        )
                        .unwrap(),
                    ),
                None,
            )
            .unwrap();
        device.bind_buffer_memory(buffer, memory, 0).unwrap();
        (buffer, memory)
    }
}

/// The bytes of a buffer made by `readback`.
unsafe fn read(gpu: &Gpu, memory: vk::DeviceMemory, bytes: u64) -> Vec<u8> {
    let device = gpu.device();
    // SAFETY: the memory is `bytes` long, mappable, and nothing is writing it.
    unsafe {
        let mapped = device
            .map_memory(memory, 0, bytes, vk::MemoryMapFlags::empty())
            .unwrap();
        let copy = std::slice::from_raw_parts(mapped.cast::<u8>(), usize::try_from(bytes).unwrap())
            .to_vec();
        device.unmap_memory(memory);
        copy
    }
}

/// Draws `scene` once for each of `cameras`, one frame after another, into
/// a `size` by `size` picture made `stops` brighter, and returns the last.
pub(crate) fn frames(
    gpu: &Arc<Gpu>,
    scene: &Scene,
    cameras: &[Camera],
    stops: f32,
    size: u32,
    upscaling: Upscaling,
) -> Shot {
    frames_shaded(
        gpu,
        scene,
        cameras,
        stops,
        size,
        upscaling,
        crate::occlusion::Shade::default(),
    )
}

/// The same, with the shade in corners found as `shade` says.
pub(crate) fn frames_shaded(
    gpu: &Arc<Gpu>,
    scene: &Scene,
    cameras: &[Camera],
    stops: f32,
    size: u32,
    upscaling: Upscaling,
    shade: crate::occlusion::Shade,
) -> Shot {
    let device = gpu.device();
    let extent = vk::Extent2D {
        width: size,
        height: size,
    };
    let bytes = u64::from(size * size * 4);
    let mut scene_view = SceneView::new(gpu, extent, upscaling).unwrap();
    // These pictures test what they name under the sky's own light; bounce
    // light, which builds up over frames, has tests of its own.
    scene_view.bounce = crate::bounce::Bounce::Off;
    scene_view.shade = shade;
    let drawn = scene_view.render_size();
    let motion_bytes = u64::from(drawn.width * drawn.height * 4);
    let meshes = Meshes::new(gpu, scene, crate::AnisotropicFiltering::default()).unwrap();
    let mesh = MeshPipeline::new(gpu, 1).unwrap();
    let mut tonemap = TonemapPipeline::new(gpu, FORMAT).unwrap();
    let whole = vk::ImageSubresourceLayers {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    };

    // SAFETY: every object is created on the live device from valid settings,
    // used only by the submissions below, each of which is waited for, and
    // destroyed once the last has finished.
    unsafe {
        let image = device
            .create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(FORMAT)
                    .extent(extent.into())
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .usage(
                        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                    ),
                None,
            )
            .unwrap();
        let needs = device.get_image_memory_requirements(image);
        let image_memory = device
            .allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(needs.size)
                    .memory_type_index(
                        gpu.memory_type(needs.memory_type_bits, vk::MemoryPropertyFlags::empty())
                            .unwrap(),
                    ),
                None,
            )
            .unwrap();
        device.bind_image_memory(image, image_memory, 0).unwrap();
        let view = device
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(FORMAT)
                    .subresource_range(COLOUR),
                None,
            )
            .unwrap();
        let (buffer, buffer_memory) = readback(gpu, bytes);
        let (motion_buffer, motion_memory) = readback(gpu, motion_bytes);

        let pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(gpu.queue_family()),
                None,
            )
            .unwrap();
        let commands = device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .command_buffer_count(1),
            )
            .unwrap()[0];
        let fence = device
            .create_fence(&vk::FenceCreateInfo::default(), None)
            .unwrap();
        tonemap
            .read_from(scene_view.picture(), scene_view.output_size())
            .unwrap();

        for camera in cameras {
            device
                .reset_command_pool(pool, vk::CommandPoolResetFlags::empty())
                .unwrap();
            device
                .begin_command_buffer(commands, &vk::CommandBufferBeginInfo::default())
                .unwrap();
            let lamps: Vec<crate::Lamp> = scene
                .lights
                .iter()
                .filter_map(crate::Lamp::from_light)
                .collect();
            scene_view
                .record(
                    commands,
                    0,
                    &mesh,
                    Contents {
                        meshes: Some(&meshes),
                        sky: false,
                        grid: false,
                        stirs: &[],
                        bits: &[],
                        flashes: [[0.0; 4]; 2],
                        weather: crate::effects::Air::default(),
                        day: None,
                        tracks: [None; 2],
                        fluid: None,
                        lamps: &lamps,
                        surfaces: None,
                        fluids: None,
                        figures: None,
                    },
                    camera,
                    1.0 / 60.0,
                )
                .unwrap();
            tonemap
                .record(
                    commands,
                    Canvas {
                        image,
                        view,
                        extent,
                    },
                    &Said::plain(exposure_scale(stops)),
                    Over::default(),
                    None,
                    Then::Copy,
                )
                .unwrap();
            device.cmd_copy_image_to_buffer(
                commands,
                image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                &[vk::BufferImageCopy::default()
                    .image_subresource(whole)
                    .image_extent(extent.into())],
            );
            // The motion image was left for shaders to read; copy it out too.
            let motion = scene_view.target().motion.image;
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&[
                    vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                        .src_access_mask(vk::AccessFlags2::MEMORY_READ)
                        .dst_stage_mask(vk::PipelineStageFlags2::COPY)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
                        .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .image(motion)
                        .subresource_range(COLOUR),
                ]),
            );
            device.cmd_copy_image_to_buffer(
                commands,
                motion,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                motion_buffer,
                &[vk::BufferImageCopy::default()
                    .image_subresource(whole)
                    .image_extent(drawn.into())],
            );
            device.end_command_buffer(commands).unwrap();

            device.reset_fences(&[fence]).unwrap();
            let buffers = [vk::CommandBufferSubmitInfo::default().command_buffer(commands)];
            device
                .queue_submit2(
                    gpu.queue(),
                    &[vk::SubmitInfo2::default().command_buffer_infos(&buffers)],
                    fence,
                )
                .unwrap();
            device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
        }

        let pixels = read(gpu, buffer_memory, bytes);
        let motion = read(gpu, motion_memory, motion_bytes)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|texel| {
                [
                    half(u16::from_ne_bytes([texel[0], texel[1]])),
                    half(u16::from_ne_bytes([texel[2], texel[3]])),
                ]
            })
            .collect();

        device.destroy_fence(fence, None);
        device.destroy_command_pool(pool, None);
        for (buffer, memory) in [(buffer, buffer_memory), (motion_buffer, motion_memory)] {
            device.destroy_buffer(buffer, None);
            device.free_memory(memory, None);
        }
        device.destroy_image_view(view, None);
        device.destroy_image(image, None);
        device.free_memory(image_memory, None);
        Shot {
            pixels,
            motion,
            drawn_width: drawn.width,
            upscaled: scene_view.upscales(),
        }
    }
}

/// Draws `scene` as `camera` sees it, `stops` brighter than it is, into a
/// `SIZE` by `SIZE` image with no upscaling, and returns its RGBA pixels.
pub(crate) fn draw(gpu: &Arc<Gpu>, scene: &Scene, camera: &Camera, stops: f32) -> Vec<u8> {
    frames(gpu, scene, &[*camera], stops, SIZE, Upscaling::Off).pixels
}

/// The red, green and blue of the pixel at `x`, `y`, counted from the top left.
pub(crate) fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let start = ((y * SIZE + x) * 4) as usize;
    [pixels[start], pixels[start + 1], pixels[start + 2]]
}

/// Whether a pixel is the dark background: sRGB #181C24 with the tone
/// curve's toe taken off is about 2, 8, 23 out of 255.
pub(crate) fn is_background([red, green, blue]: [u8; 3]) -> bool {
    red <= 4 && green <= 14 && blue <= 36
}

pub(crate) const MIDDLE: (u32, u32) = (SIZE / 2, SIZE / 2);
const RED: [f32; 4] = [1.0, 0.25, 0.2, 1.0];
const GREEN: [f32; 4] = [0.25, 1.0, 0.3, 1.0];

/// The transform that moves a shape by `x`, `y`, `z` metres and makes it
/// `wide` times as wide.
pub(crate) fn placed(x: f32, y: f32, z: f32, wide: f32) -> Transform {
    [
        [wide, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [x, y, z, 1.0],
    ]
}

/// A square two metres across that stands upright at the origin and faces +Z.
fn square() -> Mesh {
    Mesh {
        name: Some("square".to_owned()),
        parts: vec![MeshPart {
            positions: vec![
                [-1.0, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..MeshPart::default()
        }],
    }
}

/// A scene of squares: each has a colour, a transform and whether both of
/// its sides are drawn.
pub(crate) fn squares(list: &[([f32; 4], Transform, bool)]) -> Scene {
    let mut scene = Scene::default();
    for (number, &(base_color, transform, double_sided)) in list.iter().enumerate() {
        let mut mesh = square();
        mesh.parts[0].material = number as u32;
        scene.meshes.push(mesh);
        scene.materials.push(Material {
            base_color,
            double_sided,
            ..Material::default()
        });
        scene.instances.push(Instance {
            mesh: number as u32,
            transform,
            mirrored: transform[0][0] < 0.0,
            name: None,
            extras: None,
        });
    }
    scene
}

/// Ends a test: the validation layer must have had nothing to say.
pub(crate) fn finish(gpu: Arc<Gpu>) {
    drop(gpu);
    assert_eq!(
        validation_errors(),
        0,
        "the Vulkan validation layer reported mistakes; see the log above"
    );
}

#[test]
fn a_mesh_is_drawn_where_it_stands_over_the_background() {
    let Some(gpu) = gpu() else {
        return;
    };
    eprintln!("Drawing with {}.", gpu.name());
    let camera = Camera::default();

    // Five metres ahead, a two-metre square covers about a quarter of the picture.
    let ahead = draw(
        &gpu,
        &squares(&[(RED, placed(0.0, 0.0, -5.0, 1.0), false)]),
        &camera,
        0.0,
    );
    for (x, y) in [(0, 0), (SIZE - 1, 0), (0, SIZE - 1), (SIZE - 1, SIZE - 1)] {
        let colour = pixel(&ahead, x, y);
        assert!(is_background(colour), "corner {x},{y} is {colour:?}");
    }
    let [red, green, blue] = pixel(&ahead, MIDDLE.0, MIDDLE.1);
    assert!(
        red > 120 && red > green * 2 && red > blue * 2,
        "the middle should be the red square, but is {red},{green},{blue}"
    );
    assert!(is_background(pixel(&ahead, MIDDLE.0 + 13, MIDDLE.1)));
    assert!(is_background(pixel(&ahead, MIDDLE.0, MIDDLE.1 - 13)));

    // Moved a metre and a half to the right, it is right of the middle.
    let right = draw(
        &gpu,
        &squares(&[(RED, placed(1.5, 0.0, -5.0, 1.0), false)]),
        &camera,
        0.0,
    );
    assert!(is_background(pixel(&right, MIDDLE.0, MIDDLE.1)));
    assert!(!is_background(pixel(&right, MIDDLE.0 + 13, MIDDLE.1)));
    assert!(is_background(pixel(&right, MIDDLE.0 - 13, MIDDLE.1)));

    // Moved up, it is above the middle; turning the camera to look up at
    // it brings it back.
    let scene = squares(&[(RED, placed(0.0, 1.5, -5.0, 1.0), false)]);
    let above = draw(&gpu, &scene, &camera, 0.0);
    assert!(is_background(pixel(&above, MIDDLE.0, MIDDLE.1)));
    assert!(!is_background(pixel(&above, MIDDLE.0, MIDDLE.1 - 13)));
    assert!(is_background(pixel(&above, MIDDLE.0, MIDDLE.1 + 13)));
    let looking_up = Camera {
        pitch: (1.5_f32 / 5.0).atan(),
        ..camera
    };
    let found = draw(&gpu, &scene, &looking_up, 0.0);
    assert!(!is_background(pixel(&found, MIDDLE.0, MIDDLE.1)));

    // A scene with nothing in it is all background.
    let empty = draw(&gpu, &Scene::default(), &camera, 0.0);
    assert!(
        empty
            .chunks(4)
            .all(|rgba| is_background([rgba[0], rgba[1], rgba[2]]))
    );

    finish(gpu);
}

#[test]
fn the_nearer_surface_is_in_front_whichever_is_drawn_first() {
    let Some(gpu) = gpu() else {
        return;
    };
    // A red square three metres away and a green wall, eight times as wide,
    // behind it.
    let near = (RED, placed(0.0, 0.0, -3.0, 0.5), false);
    let far = (GREEN, placed(0.0, 0.0, -10.0, 8.0), false);
    for scene in [squares(&[near, far]), squares(&[far, near])] {
        let pixels = draw(&gpu, &scene, &Camera::default(), 0.0);
        let [red, green, _] = pixel(&pixels, MIDDLE.0, MIDDLE.1);
        assert!(red > green, "the near red square should hide the wall");
        let [red, green, _] = pixel(&pixels, MIDDLE.0 + 12, MIDDLE.1);
        assert!(green > red, "the far green wall should show beside it");
    }
    finish(gpu);
}

#[test]
fn the_back_of_a_surface_is_not_drawn_unless_it_has_two_sides() {
    let Some(gpu) = gpu() else {
        return;
    };
    // The square faces +Z. From behind it, looking back along +Z:
    let behind = Camera {
        position: [0.0, 0.0, -10.0],
        yaw: std::f32::consts::PI,
        ..Camera::default()
    };
    let at = placed(0.0, 0.0, -5.0, 1.0);
    let one_sided = draw(&gpu, &squares(&[(RED, at, false)]), &behind, 0.0);
    assert!(is_background(pixel(&one_sided, MIDDLE.0, MIDDLE.1)));
    let two_sided = draw(&gpu, &squares(&[(RED, at, true)]), &behind, 0.0);
    assert!(!is_background(pixel(&two_sided, MIDDLE.0, MIDDLE.1)));

    // A mirrored square still shows its front to a camera in front of it,
    // and still hides its back.
    let mirrored = squares(&[(RED, placed(0.0, 0.0, -5.0, -1.0), false)]);
    let front = draw(&gpu, &mirrored, &Camera::default(), 0.0);
    assert!(!is_background(pixel(&front, MIDDLE.0, MIDDLE.1)));
    let back = draw(&gpu, &mirrored, &behind, 0.0);
    assert!(is_background(pixel(&back, MIDDLE.0, MIDDLE.1)));

    finish(gpu);
}

#[test]
fn a_surface_facing_up_shows_its_material_colour_and_point_colours_tint_it() {
    let Some(gpu) = gpu() else {
        return;
    };
    // A grey floor two metres under a camera that looks straight down.
    let mut scene = Scene::default();
    scene.meshes.push(Mesh {
        name: None,
        parts: vec![MeshPart {
            positions: vec![
                [-9.0, 0.0, 9.0],
                [9.0, 0.0, 9.0],
                [9.0, 0.0, -9.0],
                [-9.0, 0.0, -9.0],
            ],
            normals: vec![[0.0, 1.0, 0.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..MeshPart::default()
        }],
    });
    scene.materials.push(Material {
        base_color: [0.5, 0.5, 0.5, 1.0],
        ..Material::default()
    });
    scene.instances.push(Instance {
        mesh: 0,
        transform: placed(0.0, 0.0, 0.0, 1.0),
        mirrored: false,
        name: None,
        extras: None,
    });
    let down = Camera {
        position: [0.0, 2.0, 0.0],
        pitch: -std::f32::consts::FRAC_PI_2,
        ..Camera::default()
    };
    let pixels = draw(&gpu, &scene, &down, 0.0);
    // Half grey, less the tone curve's 0.04, is 181 out of 255 in sRGB.
    for (x, y) in [(0, 0), MIDDLE, (SIZE - 1, SIZE - 1)] {
        let grey = pixel(&pixels, x, y);
        assert!(
            grey.iter().all(|&channel| channel.abs_diff(181) <= 3),
            "the floor at {x},{y} should be mid grey, but is {grey:?}"
        );
    }

    // Colours painted on the points multiply the material's.
    scene.meshes[0].parts[0].colors = Some(vec![[1.0, 0.0, 0.0, 1.0]; 4]);
    let pixels = draw(&gpu, &scene, &down, 0.0);
    let [red, green, blue] = pixel(&pixels, MIDDLE.0, MIDDLE.1);
    // With no green or blue the tone curve takes nothing off: 188.
    assert!(red.abs_diff(188) <= 3 && green < 10 && blue < 10);

    finish(gpu);
}

#[test]
fn light_brighter_than_the_display_rolls_off_and_keeps_its_colour() {
    let Some(gpu) = gpu() else {
        return;
    };
    let scene = squares(&[(RED, placed(0.0, 0.0, -2.0, 1.0), false)]);
    let camera = Camera::default();
    let normal = draw(&gpu, &scene, &camera, 0.0);
    // Eight times the light: the red is now about 5, far over the display's
    // 1.0, which would be a flat 255 red with nothing done to it.
    let bright = draw(&gpu, &scene, &camera, 3.0);
    let dark = draw(&gpu, &scene, &camera, -3.0);

    let [red, green, blue] = pixel(&bright, MIDDLE.0, MIDDLE.1);
    assert!(
        red > green && red > blue,
        "the square should still be mostly red, but is {red},{green},{blue}"
    );
    assert!(
        red < 255,
        "the brightest red should stop short of the display's limit"
    );
    let [_, normal_green, _] = pixel(&normal, MIDDLE.0, MIDDLE.1);
    assert!(
        green > normal_green,
        "over-bright red should fade towards white: green {normal_green} became {green}"
    );

    // More light is never darker, and less is never brighter, anywhere.
    for index in 0..normal.len() {
        if index % 4 == 3 {
            continue;
        }
        assert!(bright[index] >= normal[index] && normal[index] >= dark[index]);
    }
    assert!(pixel(&dark, MIDDLE.0, MIDDLE.1)[1] < pixel(&normal, MIDDLE.0, MIDDLE.1)[1]);

    finish(gpu);
}

#[test]
fn each_pixel_knows_how_far_it_moved_since_the_last_frame() {
    let Some(gpu) = gpu() else {
        return;
    };
    // A square five metres ahead that fills the middle of the picture.
    let scene = squares(&[(RED, placed(0.0, 0.0, -5.0, 1.0), false)]);
    let start = Camera::default();
    let motion_at = |shot: &Shot, x: u32, y: u32| shot.motion[(y * shot.drawn_width + x) as usize];
    // Half a metre sideways moves a point five metres off by a tenth of the
    // distance from the middle of the picture to its edge at 45 degrees;
    // with this field of view that is 0.0664 of the picture.
    let step = 0.1 / (37.0_f32.to_radians().tan()) / 2.0;
    let near = |a: f32, b: f32| (a - b).abs() < 0.002;

    // Nothing moved: no motion anywhere.
    let still = frames(&gpu, &scene, &[start, start], 0.0, SIZE, Upscaling::Off);
    assert!(still.motion.iter().all(|&[x, y]| x == 0.0 && y == 0.0));

    // The camera steps right, so the square moves left across the picture:
    // it was further right than it is.
    let right = Camera {
        position: [0.5, 0.0, 0.0],
        ..start
    };
    let shot = frames(&gpu, &scene, &[start, right], 0.0, SIZE, Upscaling::Off);
    let [x, y] = motion_at(&shot, MIDDLE.0 - 4, MIDDLE.1);
    assert!(near(x, step) && near(y, 0.0), "moved {x}, {y}");
    assert_eq!(
        motion_at(&shot, 0, 0),
        [0.0, 0.0],
        "the background is still"
    );

    // The camera rises, so the square moves down the picture: it was
    // further up than it is, and up is the smaller number.
    let up = Camera {
        position: [0.0, 0.5, 0.0],
        ..start
    };
    let shot = frames(&gpu, &scene, &[start, up], 0.0, SIZE, Upscaling::Off);
    let [x, y] = motion_at(&shot, MIDDLE.0, MIDDLE.1 + 4);
    assert!(near(x, 0.0) && near(y, -step), "moved {x}, {y}");

    // The first frame has nothing before it.
    let first = frames(&gpu, &scene, &[right], 0.0, SIZE, Upscaling::Off);
    assert!(first.motion.iter().all(|&[x, y]| x == 0.0 && y == 0.0));

    finish(gpu);
}

/// A square turned a little, so that none of its edges runs along the rows
/// or columns of pixels, in front of a camera.
fn tilted_square() -> Scene {
    let (sin, cos) = 0.3_f32.sin_cos();
    let turned = [
        [cos, sin, 0.0, 0.0],
        [-sin, cos, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, -4.0, 1.0],
    ];
    squares(&[(RED, turned, false)])
}

/// `pixels`, a `size` by `size` picture, made `factor` times smaller each
/// way by averaging its light: what a perfect upscaler would be aiming for.
fn shrunk(pixels: &[u8], size: u32, factor: u32) -> Vec<u8> {
    // The sRGB curve and back, since light is what averages, not its encoding.
    let light = |byte: u8| {
        let value = f32::from(byte) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let byte = |light: f32| {
        let value = if light <= 0.003_130_8 {
            light * 12.92
        } else {
            1.055 * light.powf(1.0 / 2.4) - 0.055
        };
        (value * 255.0).round() as u8
    };
    let small = size / factor;
    let mut out = Vec::with_capacity((small * small * 4) as usize);
    for y in 0..small {
        for x in 0..small {
            for channel in 0..4 {
                let mut sum = 0.0;
                for dy in 0..factor {
                    for dx in 0..factor {
                        let at = ((y * factor + dy) * size + x * factor + dx) * 4 + channel;
                        sum += light(pixels[at as usize]);
                    }
                }
                out.push(byte(sum / (factor * factor) as f32));
            }
        }
    }
    out
}

/// How far apart two pictures are: the average difference of a colour
/// channel, out of 255.
fn difference(a: &[u8], b: &[u8]) -> f32 {
    assert_eq!(a.len(), b.len());
    let total: u32 = a
        .iter()
        .zip(b)
        .map(|(a, b)| u32::from(a.abs_diff(*b)))
        .sum();
    total as f32 / a.len() as f32
}

#[test]
fn a_scene_drawn_small_is_upscaled_to_a_sharp_full_size_picture() {
    let Some(gpu) = gpu() else {
        return;
    };
    const OUTPUT: u32 = 128;
    let scene = tilted_square();
    let camera = Camera::default();
    // What the picture should look like: drawn four times too large and averaged down.
    let large = frames(&gpu, &scene, &[camera], 0.0, OUTPUT * 4, Upscaling::Off);
    let ideal = shrunk(&large.pixels, OUTPUT * 4, 4);

    // Drawn at a quarter of the pixels, for long enough to settle.
    let settled = frames(
        &gpu,
        &scene,
        &[camera; 48],
        0.0,
        OUTPUT,
        Upscaling::Performance,
    );
    assert!(settled.upscaled, "this graphics card should run FSR");
    assert_eq!(settled.drawn_width, OUTPUT / 2);
    let pixel = |pixels: &[u8], x: u32, y: u32| {
        let at = ((y * OUTPUT + x) * 4) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    };
    let [red, green, blue] = pixel(&settled.pixels, OUTPUT / 2, OUTPUT / 2);
    assert!(red > 120 && red > green * 2 && red > blue * 2);
    assert!(is_background(pixel(&settled.pixels, 2, 2)));
    let settled_off = difference(&settled.pixels, &ideal);
    eprintln!("Upscaled from a quarter of the pixels: {settled_off:.3} from ideal.");

    // The same pixels simply stretched would be this far off.
    let small = frames(&gpu, &scene, &[camera], 0.0, OUTPUT / 2, Upscaling::Off);
    let mut stretched = Vec::with_capacity(ideal.len());
    for y in 0..OUTPUT {
        for x in 0..OUTPUT {
            let at = ((y / 2 * (OUTPUT / 2) + x / 2) * 4) as usize;
            stretched.extend_from_slice(&small.pixels[at..at + 4]);
        }
    }
    let stretched_off = difference(&stretched, &ideal);
    eprintln!("Stretched: {stretched_off:.3} from ideal.");
    let recovered = settled_off < stretched_off * 0.8;

    // While the camera slides sideways the picture stays that sharp, which
    // needs the motion of every pixel to be right.
    let sliding: Vec<Camera> = (0..48_u8)
        .map(|frame| Camera {
            position: [f32::from(frame) * 0.01 - 0.47, 0.0, 0.0],
            ..camera
        })
        .collect();
    let moved = frames(&gpu, &scene, &sliding, 0.0, OUTPUT, Upscaling::Performance);
    let moved_off = difference(&moved.pixels, &ideal);
    eprintln!("Upscaled while moving: {moved_off:.3} from ideal.");
    let stayed = moved_off < stretched_off * 0.8;

    // Full size with only the edges smoothed.
    let native = frames(&gpu, &scene, &[camera; 32], 0.0, OUTPUT, Upscaling::Native);
    assert!(native.upscaled);
    assert_eq!(native.drawn_width, OUTPUT);
    let plain = frames(&gpu, &scene, &[camera], 0.0, OUTPUT, Upscaling::Off);
    let (native_off, plain_off) = (
        difference(&native.pixels, &ideal),
        difference(&plain.pixels, &ideal),
    );
    eprintln!("Full size: {native_off:.3} smoothed, {plain_off:.3} plain.");
    assert!(
        recovered,
        "upscaling should recover detail: {settled_off} against {stretched_off} for stretching"
    );
    assert!(
        stayed,
        "a moving view should stay sharp: {moved_off} against {stretched_off} for stretching"
    );
    assert!(
        native_off < plain_off * 0.8,
        "smoothing the edges should be closer to ideal than leaving them jagged"
    );

    // With the shade in corners found, what is solid is drawn for its
    // depth first and the picture only where that depth is. The upscaler
    // shifts each frame by part of a pixel, so the two must be shifted
    // alike, frame after frame, or the square would be full of holes. A
    // flat square with nothing near has no shade: still or sliding, the
    // picture is the one drawn without. (Here, and not in a test of its
    // own: FSR runs on one graphics device at a time in a program.)
    let shade = crate::occlusion::Shade {
        quality: crate::AmbientOcclusion::Medium,
        ..crate::occlusion::Shade::default()
    };
    for (cameras, plain) in [(&[camera; 48][..], &settled), (&sliding[..], &moved)] {
        let shaded = frames_shaded(
            &gpu,
            &scene,
            cameras,
            0.0,
            OUTPUT,
            Upscaling::Performance,
            shade,
        );
        assert!(shaded.upscaled);
        let apart = difference(&shaded.pixels, &plain.pixels);
        assert!(
            apart < 0.05,
            "the upscaled picture changed with the shade found: {apart} of 255 a channel"
        );
    }

    finish(gpu);
}

#[test]
fn a_window_dragged_nearly_shut_is_drawn_without_upscaling() {
    let Some(gpu) = gpu() else {
        return;
    };
    // Two pixels square: FSR would be asked to upscale a single pixel.
    let tiny = frames(
        &gpu,
        &tilted_square(),
        &[Camera::default(); 3],
        0.0,
        2,
        Upscaling::Quality,
    );
    assert!(!tiny.upscaled);
    assert_eq!(tiny.drawn_width, 2);
    finish(gpu);
}

/// The size of the pictures the HUD and menus are drawn over in these
/// tests: a quarter of 1920 by 1080, so a UI size of 4 is one pixel.
const WIDE: u32 = 480;
const HIGH: u32 = 270;

/// Draws an empty scene once for each of `lists`, one frame after another,
/// into `WIDE` by `HIGH` images of `format` with that list drawn over it,
/// and returns each frame's RGBA pixels.
fn overlaid(gpu: &Arc<Gpu>, format: vk::Format, lists: &[DrawList<'_>]) -> Vec<Vec<u8>> {
    let extent = vk::Extent2D {
        width: WIDE,
        height: HIGH,
    };
    overlaid_sized(gpu, format, extent, lists).0
}

/// As [`overlaid`], into images `extent` in size, and also how long the
/// graphics card took over all the frames, from each one being handed over
/// to its being finished.
fn overlaid_sized(
    gpu: &Arc<Gpu>,
    format: vk::Format,
    extent: vk::Extent2D,
    lists: &[DrawList<'_>],
) -> (Vec<Vec<u8>>, std::time::Duration) {
    let device = gpu.device();
    let bytes = u64::from(extent.width * extent.height * 4);
    let mut took = std::time::Duration::ZERO;
    let mut scene_view = SceneView::new(gpu, extent, Upscaling::Off).unwrap();
    let mesh = MeshPipeline::new(gpu, 1).unwrap();
    let mut tonemap = TonemapPipeline::new(gpu, format).unwrap();
    let mut ui = UiPass::new(gpu, format, 1).unwrap();
    let mut overlay = Overlay::new(Font::bundled());
    let whole = vk::ImageSubresourceLayers {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    };
    let mut shots = Vec::new();

    // SAFETY: every object is created on the live device from valid settings,
    // used only by the submissions below, each of which is waited for, and
    // destroyed once the last has finished.
    unsafe {
        let mut picture = crate::scene::Picture::null();
        picture
            .create(
                gpu,
                extent,
                format,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                COLOUR,
                "testing",
            )
            .unwrap();
        let (buffer, buffer_memory) = readback(gpu, bytes);
        let pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(gpu.queue_family()),
                None,
            )
            .unwrap();
        let commands = device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .command_buffer_count(1),
            )
            .unwrap()[0];
        let fence = device
            .create_fence(&vk::FenceCreateInfo::default(), None)
            .unwrap();
        tonemap
            .read_from(scene_view.picture(), scene_view.output_size())
            .unwrap();

        for list in lists {
            overlay.set(*list);
            device
                .reset_command_pool(pool, vk::CommandPoolResetFlags::empty())
                .unwrap();
            device
                .begin_command_buffer(commands, &vk::CommandBufferBeginInfo::default())
                .unwrap();
            scene_view
                .record(
                    commands,
                    0,
                    &mesh,
                    Contents::default(),
                    &Camera::default(),
                    1.0 / 60.0,
                )
                .unwrap();
            ui.prepare(commands, 0, &mut overlay, extent).unwrap();
            tonemap
                .record(
                    commands,
                    Canvas {
                        image: picture.image,
                        view: picture.view,
                        extent,
                    },
                    &Said::plain(1.0),
                    Over::ui(Some(&ui)),
                    None,
                    Then::Copy,
                )
                .unwrap();
            device.cmd_copy_image_to_buffer(
                commands,
                picture.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                &[vk::BufferImageCopy::default()
                    .image_subresource(whole)
                    .image_extent(extent.into())],
            );
            device.end_command_buffer(commands).unwrap();
            device.reset_fences(&[fence]).unwrap();
            let buffers = [vk::CommandBufferSubmitInfo::default().command_buffer(commands)];
            let started = std::time::Instant::now();
            device
                .queue_submit2(
                    gpu.queue(),
                    &[vk::SubmitInfo2::default().command_buffer_infos(&buffers)],
                    fence,
                )
                .unwrap();
            device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
            took += started.elapsed();
            shots.push(read(gpu, buffer_memory, bytes));
        }

        device.destroy_fence(fence, None);
        device.destroy_command_pool(pool, None);
        device.destroy_buffer(buffer, None);
        device.free_memory(buffer_memory, None);
        picture.destroy(gpu);
    }
    (shots, took)
}

/// The red, green and blue of the pixel at `x`, `y` of an `overlaid` picture.
fn wide_pixel(pixels: &[u8], x: f32, y: f32) -> [u8; 3] {
    let start = ((y as u32 * WIDE + x as u32) * 4) as usize;
    [pixels[start], pixels[start + 1], pixels[start + 2]]
}

/// Whether two colours are the same to within what rounding can do.
fn close(one: [u8; 3], other: [u8; 3]) -> bool {
    one.iter()
        .zip(other)
        .all(|(one, other)| one.abs_diff(other) <= 2)
}

/// A colour as the bytes a picture stores it in.
fn bytes(color: Color) -> [u8; 3] {
    [color.r, color.g, color.b].map(|part| (part * 255.0).round() as u8)
}

/// A UI updated for a `WIDE` by `HIGH` picture.
fn updated(mut ui: Ui) -> Ui {
    ui.set_text_measure(Box::new(Font::bundled()));
    ui.update(&UiInput::default(), 1.0 / 60.0, [WIDE, HIGH]);
    ui
}

/// The place and size of the first box in `ui` drawn in `color`.
fn box_of(ui: &Ui, color: Color) -> ([f32; 2], [f32; 2]) {
    ui.draw()
        .iter()
        .find_map(|draw| match draw {
            Draw::Rect {
                position,
                size,
                color: found,
                ..
            } if found == color => Some((position, size)),
            _ => None,
        })
        .unwrap()
}

/// The place and height of the line of `words` in `ui`.
fn line_of(ui: &Ui, words: &str) -> ([f32; 2], f32) {
    ui.draw()
        .iter()
        .find_map(|draw| match draw {
            Draw::Text {
                position,
                size,
                text,
                ..
            } if text == words => Some((position, size)),
            _ => None,
        })
        .unwrap()
}

/// How many pixels of the box at `position` of `size` are nearly `color`.
fn count_of(pixels: &[u8], position: [f32; 2], size: [f32; 2], color: [u8; 3]) -> usize {
    let (left, top) = (position[0].floor() as u32, position[1].floor() as u32);
    let (right, bottom) = (
        ((position[0] + size[0]).ceil() as u32).min(WIDE),
        ((position[1] + size[1]).ceil() as u32).min(HIGH),
    );
    (top..bottom)
        .flat_map(|y| (left..right).map(move |x| (x, y)))
        .filter(|&(x, y)| close(wide_pixel(pixels, x as f32, y as f32), color))
        .count()
}

/// The formats a window's images come in: with the sRGB curve built in,
/// which is what the renderer asks a window for, and without.
const WINDOW_FORMATS: [vk::Format; 2] = [vk::Format::R8G8B8A8_SRGB, vk::Format::R8G8B8A8_UNORM];

#[test]
fn the_hud_is_drawn_over_the_picture_in_the_colours_it_asked_for() {
    let Some(gpu) = gpu() else {
        return;
    };
    let orange = Color::rgb(1.0, 0.5, 0.0);
    let glass = Color::rgba(1.0, 1.0, 1.0, 0.5);
    let ink = Color::rgb(0.2, 0.9, 0.4);

    let mut hud = Ui::default();
    hud.hud_rect(Anchor::TopLeft, [0.0, 0.0], [400.0, 200.0], orange);
    hud.hud_rect(Anchor::TopRight, [0.0, 0.0], [400.0, 200.0], glass);
    let words = hud.hud_text(Anchor::BottomLeft, [0.0, 0.0], 200.0, "HUD 100");
    hud.set_color(words, ink).unwrap();
    let hud = updated(hud);

    for format in WINDOW_FORMATS {
        let shots = overlaid(&gpu, format, &[Ui::default().draw(), hud.draw()]);
        // With nothing to draw over it, the picture is the scene's alone.
        assert!(
            shots[0]
                .chunks(4)
                .all(|rgba| is_background([rgba[0], rgba[1], rgba[2]])),
            "{format:?}"
        );
        let shot = &shots[1];

        // A solid box is exactly its colour, whichever way the image stores
        // colours, and exactly where the UI put it: a quarter of the sizes
        // asked for, inside the safe area.
        let (at, size) = box_of(&hud, orange);
        assert_eq!(size, [100.0, 50.0]);
        let middle = wide_pixel(shot, at[0] + 50.0, at[1] + 25.0);
        assert!(close(middle, [255, 128, 0]), "{format:?}: {middle:?}");
        assert_eq!(
            count_of(shot, at, size, [255, 128, 0]),
            100 * 50,
            "{format:?}"
        );
        for (x, y) in [
            (at[0] - 2.0, at[1] + 25.0),
            (at[0] + 102.0, at[1] + 25.0),
            (at[0] + 50.0, at[1] - 2.0),
            (at[0] + 50.0, at[1] + 52.0),
        ] {
            let outside = wide_pixel(shot, x, y);
            assert!(is_background(outside), "{format:?}: {x},{y} is {outside:?}");
        }

        // A see-through box lets the dark scene through: grey, not white.
        let (at, _) = box_of(&hud, glass);
        let [red, green, blue] = wide_pixel(shot, at[0] + 50.0, at[1] + 25.0);
        assert!(
            (100..=200).contains(&red) && red.abs_diff(green) < 12 && blue >= red,
            "{format:?}: {red},{green},{blue}"
        );

        // Text is letters in its colour on the scene, inside its line: some
        // of the line is ink, most is not, and none of it strays.
        let (at, height) = line_of(&hud, "HUD 100");
        assert_eq!(height, 50.0);
        let width = lore_ui::TextMeasure::width(&Font::bundled(), "HUD 100", height);
        let inked = count_of(shot, at, [width, height], bytes(ink));
        let area = (width * height) as usize;
        assert!(
            inked > area / 12 && inked < area / 2,
            "{format:?}: {inked} of {area} pixels are ink"
        );
        let everywhere = count_of(shot, [0.0, 0.0], [WIDE as f32, HIGH as f32], bytes(ink));
        assert_eq!(everywhere, inked, "{format:?}: ink outside the line");
        // The left edge of the "H" is a straight stem at the start of the line.
        let stem = (0..12).any(|step| {
            close(
                wide_pixel(shot, at[0] + step as f32, at[1] + height / 2.0),
                bytes(ink),
            )
        });
        assert!(stem, "{format:?}: no letter at the start of the line");
    }
    finish(gpu);
}

#[test]
fn a_menu_has_round_corners_and_new_letters_reach_the_graphics_card() {
    let Some(gpu) = gpu() else {
        return;
    };
    // Corners a twelfth of the picture's height, so they can be seen.
    let theme = Theme {
        corner: 88.0,
        // Wide enough that the label is clear of the corners.
        button_width: 1200.0,
        row_height: 240.0,
        text_size: 120.0,
        ..Theme::default()
    };
    let mut hud = Ui::new(theme.clone());
    hud.hud_text(Anchor::BottomLeft, [0.0, 0.0], 100.0, "abc");
    let hud = updated(hud);
    let mut menu = Ui::new(theme.clone());
    let screen = menu
        .add_screen("", |screen| {
            screen.button("Quit");
        })
        .unwrap();
    menu.open(screen).unwrap();
    let menu = updated(menu);

    for format in WINDOW_FORMATS {
        // The second frame draws letters the first did not have.
        let shots = overlaid(&gpu, format, &[hud.draw(), menu.draw()]);
        let shot = &shots[1];
        let button = bytes(theme.widget_selected);
        let (at, size) = box_of(&menu, theme.widget_selected);
        let radius = 22.0;
        assert!(size[0] > 3.0 * radius && size[1] > 2.0 * radius, "{size:?}");

        // The middle of each side is the button, right up to the edge.
        for (x, y) in [
            (at[0] + size[0] / 2.0, at[1] + 1.0),
            (at[0] + size[0] / 2.0, at[1] + size[1] - 2.0),
            (at[0] + 1.0, at[1] + size[1] / 2.0),
            (at[0] + size[0] - 2.0, at[1] + size[1] / 2.0),
        ] {
            let side = wide_pixel(shot, x, y);
            assert!(close(side, button), "{format:?}: {x},{y} is {side:?}");
        }
        // Each corner of its box is not: the corner is cut round. Where the
        // round corner ends, it is the button again.
        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let corner = [
                at[0] + 2.0 + x * (size[0] - 5.0),
                at[1] + 2.0 + y * (size[1] - 5.0),
            ];
            let cut = wide_pixel(shot, corner[0], corner[1]);
            assert!(!close(cut, button), "{format:?}: corner {x},{y} is square");
            let inside = [
                at[0] + radius + x * (size[0] - 2.0 * radius - 1.0),
                at[1] + radius + y * (size[1] - 2.0 * radius - 1.0),
            ];
            let filled = wide_pixel(shot, inside[0], inside[1]);
            assert!(
                close(filled, button),
                "{format:?}: {inside:?} is {filled:?}"
            );
        }
        // The edge of the round corner is soft: between the button's colour
        // and what is behind it there are in-between pixels.
        let corner_box = ([at[0], at[1]], [radius, radius]);
        let solid = count_of(shot, corner_box.0, corner_box.1, button);
        assert!(
            solid > 250 && solid < 22 * 22 - 60,
            "{format:?}: {solid} solid pixels in a round corner"
        );

        // "Quit" was not in the first frame's letters, and is drawn.
        let (at, height) = line_of(&menu, "Quit");
        let width = lore_ui::TextMeasure::width(&Font::bundled(), "Quit", height);
        let inked = count_of(shot, at, [width, height], bytes(theme.text_selected));
        let area = (width * height) as usize;
        assert!(
            inked > area / 12 && inked < area / 2,
            "{format:?}: {inked} of {area} pixels of \"Quit\" are ink"
        );
        // And it is in the middle of its button, as the real widths say.
        let centre = at[0] + width / 2.0;
        let (button_at, button_size) = box_of(&menu, theme.widget_selected);
        assert!(
            (centre - (button_at[0] + button_size[0] / 2.0)).abs() < 1.5,
            "{format:?}: the label is off centre"
        );
    }
    finish(gpu);
}

#[test]
fn boxes_have_borders_gradients_shadows_soft_text_and_clips() {
    use lore_ui::{Clip, DrawBuffer, Fill, FontFace, Stop, Transform as Turn};

    let Some(gpu) = gpu() else {
        return;
    };
    let red = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let stops = [
        Stop {
            color: red,
            at: 0.0,
        },
        Stop {
            color: green,
            at: 0.5,
        },
        Stop {
            color: blue,
            at: 1.0,
        },
    ];
    let plain = |position: [f32; 2], size: [f32; 2], fill, border: f32| Draw::Box {
        position,
        size,
        radii: [0.0; 4].into(),
        fill,
        border_widths: [border; 4],
        border_color: Color::WHITE,
        transform: Turn::IDENTITY,
        clip: None,
    };
    let line = |position: [f32; 2], blur: f32, clip: Option<Clip>| Draw::Text {
        position,
        size: 60.0,
        color: Color::WHITE,
        text: "HI",
        font: FontFace::default(),
        letter_spacing: 0.0,
        blur,
        transform: Turn::IDENTITY,
        clip,
    };

    let mut buffer = DrawBuffer::default();
    // A red box with a white border six pixels thick.
    buffer.push(plain([20.0, 20.0], [100.0, 60.0], Fill::Color(red), 6.0));
    // Red, green and blue from left to right.
    buffer.push(plain(
        [160.0, 20.0],
        [100.0, 60.0],
        Fill::Linear {
            angle: 90.0,
            stops: &stops,
        },
        0.0,
    ));
    // A white glow below a dark box: the glow, then the box on top of it.
    buffer.push(Draw::Shadow {
        position: [320.0, 20.0],
        size: [80.0, 40.0],
        radii: [0.0; 4].into(),
        color: Color::WHITE,
        offset: [0.0, 10.0],
        blur: 24.0,
        spread: 0.0,
        inset: false,
        transform: Turn::IDENTITY,
        clip: None,
    });
    buffer.push(plain(
        [320.0, 20.0],
        [80.0, 40.0],
        Fill::Color(Color::BLACK),
        0.0,
    ));
    // Sharp text, soft text, and text cut down to a box.
    buffer.push(line([20.0, 150.0], 0.0, None));
    buffer.push(line([160.0, 150.0], 10.0, None));
    let half = Clip::new([300.0, 150.0], [30.0, 70.0], [0.0; 4].into());
    buffer.push(line([300.0, 150.0], 0.0, Some(half)));

    for format in WINDOW_FORMATS {
        let shots = overlaid(&gpu, format, &[buffer.draw()]);
        let shot = &shots[0];
        let at = |x: f32, y: f32| wide_pixel(shot, x, y);

        // Border and fill, with nothing outside.
        assert!(close(at(22.0, 50.0), [255, 255, 255]), "{format:?}: border");
        assert!(close(at(70.0, 50.0), [255, 0, 0]), "{format:?}: fill");
        assert!(is_background(at(14.0, 50.0)), "{format:?}: outside");

        // Each stop is where it says.
        let [r, g, b] = at(162.0, 50.0);
        assert!(r > 230 && g < 25 && b < 5, "{format:?}: start {r},{g},{b}");
        let [r, g, b] = at(210.0, 50.0);
        assert!(
            g > 230 && r < 15 && b < 15,
            "{format:?}: middle {r},{g},{b}"
        );
        let [r, g, b] = at(257.0, 50.0);
        assert!(b > 230 && r < 15 && g < 25, "{format:?}: end {r},{g},{b}");

        // The glow fades with the distance below the box, and is gone far off.
        let near = at(360.0, 70.0)[0];
        let further = at(360.0, 84.0)[0];
        let far = at(360.0, 100.0)[0];
        assert!(
            near > further && further > far,
            "{format:?}: {near} {further} {far}"
        );
        assert!(near > 60, "{format:?}: glow is faint at {near}");
        assert!(
            is_background(at(360.0, 160.0)),
            "{format:?}: glow reaches too far"
        );
        // The box on top is not lit.
        assert!(
            close(at(360.0, 40.0), [0, 0, 0]),
            "{format:?}: box under glow"
        );

        // Soft text spreads wider and fainter than sharp text.
        let ink = |x: f32| -> (usize, u8) {
            let mut most = 0;
            let mut lit = 0;
            for y in 140..225 {
                for column in x as u32..x as u32 + 120 {
                    let [red, ..] = wide_pixel(shot, column as f32, y as f32);
                    if red > 40 {
                        lit += 1;
                    }
                    most = most.max(red);
                }
            }
            (lit, most)
        };
        let (sharp_lit, sharp_most) = ink(10.0);
        let (soft_lit, soft_most) = ink(150.0);
        assert!(sharp_most > 240, "{format:?}: sharp text is {sharp_most}");
        assert!(
            soft_lit > sharp_lit,
            "{format:?}: {soft_lit} against {sharp_lit}"
        );
        assert!(
            soft_most < sharp_most,
            "{format:?}: {soft_most} against {sharp_most}"
        );

        // Only the left thirty pixels of the clipped text are drawn.
        let lit_in = |from: u32, to: u32| {
            (140..225_u32)
                .flat_map(|y| (from..to).map(move |x| (x, y)))
                .filter(|&(x, y)| wide_pixel(shot, x as f32, y as f32)[0] > 200)
                .count()
        };
        assert!(
            lit_in(300, 330) > 100,
            "{format:?}: nothing inside the clip"
        );
        assert_eq!(lit_in(331, 420), 0, "{format:?}: text outside the clip");
    }
    finish(gpu);
}

#[test]
fn a_faded_group_is_one_picture_a_clip_turns_and_a_shadow_stays_outside_its_box() {
    use lore_ui::{Clip, DrawBuffer, Fill, Transform as Turn};

    let Some(gpu) = gpu() else {
        return;
    };
    let grey = Color::rgb(0.5, 0.5, 0.5);
    let red = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let filled = |position: [f32; 2], size: [f32; 2], color: Color, clip: Option<Clip>| Draw::Box {
        position,
        size,
        radii: [0.0; 4].into(),
        fill: Fill::Color(color),
        border_widths: [0.0; 4],
        border_color: Color::WHITE,
        transform: Turn::IDENTITY,
        clip,
    };
    let half = |color: Color| Color { a: 0.5, ..color };
    let mut buffer = DrawBuffer::default();
    // Everything over one grey, so what is expected is known.
    buffer.push(filled([0.0, 0.0], [480.0, 270.0], grey, None));
    // Red under blue, faded as one: where they overlap, only blue shows.
    buffer.push(Draw::Group { opacity: 0.5 });
    buffer.push(filled([20.0, 20.0], [100.0, 60.0], red, None));
    buffer.push(filled([70.0, 40.0], [100.0, 60.0], blue, None));
    buffer.push(Draw::EndGroup);
    // What blue alone at half looks like.
    buffer.push(filled([200.0, 20.0], [20.0, 20.0], half(blue), None));
    // A group in a group: green, then red under blue, all faded by half
    // and the inner pair by half again.
    buffer.push(Draw::Group { opacity: 0.5 });
    buffer.push(filled([20.0, 150.0], [100.0, 100.0], green, None));
    buffer.push(Draw::Group { opacity: 0.5 });
    buffer.push(filled([40.0, 160.0], [60.0, 60.0], red, None));
    buffer.push(filled([60.0, 180.0], [60.0, 60.0], blue, None));
    buffer.push(Draw::EndGroup);
    buffer.push(Draw::EndGroup);
    // A red box cut by a square turned an eighth of a turn.
    let diamond = Clip {
        transform: Turn::about([375.0, 95.0], 1.0, std::f32::consts::FRAC_PI_4),
        ..Clip::new([325.0, 45.0], [100.0, 100.0], [0.0; 4].into())
    };
    buffer.push(filled([300.0, 20.0], [150.0, 150.0], red, Some(diamond)));
    // A see-through white box with a dark shadow, and one with none.
    buffer.push(Draw::Shadow {
        position: [300.0, 190.0],
        size: [60.0, 60.0],
        radii: [0.0; 4].into(),
        color: Color::BLACK,
        offset: [0.0, 0.0],
        blur: 16.0,
        spread: 4.0,
        inset: false,
        transform: Turn::IDENTITY,
        clip: None,
    });
    let glass = Color::rgba(1.0, 1.0, 1.0, 0.3);
    buffer.push(filled([300.0, 190.0], [60.0, 60.0], glass, None));
    buffer.push(filled([420.0, 190.0], [40.0, 60.0], glass, None));

    for format in WINDOW_FORMATS {
        let shots = overlaid(&gpu, format, &[buffer.draw()]);
        let at = |x: f32, y: f32| wide_pixel(&shots[0], x, y);
        let grey_seen = at(250.0, 120.0);

        // Blue over red in the group is blue alone, faded.
        let blue_half = at(210.0, 30.0);
        assert!(
            close(at(110.0, 70.0), blue_half),
            "{format:?}: {:?} against {blue_half:?}",
            at(110.0, 70.0)
        );
        assert!(close(at(150.0, 90.0), blue_half), "{format:?}");
        // Red where blue is not, and grey outside.
        let [r, g, b] = at(30.0, 30.0);
        assert!(r > g + 40 && r > b + 40, "{format:?}: {r},{g},{b}");
        assert!(close(at(190.0, 120.0), grey_seen), "{format:?}");

        // In the inner group, blue covers red: where all three are, it
        // looks as where only blue and green are. Red shows where blue is
        // not, and the whole is fainter than the inner group alone.
        let blue_on_green = at(110.0, 230.0);
        assert!(
            close(at(90.0, 210.0), blue_on_green),
            "{format:?}: {:?} against {blue_on_green:?}",
            at(90.0, 210.0)
        );
        let [r, _, b] = at(50.0, 170.0);
        assert!(r > b + 20, "{format:?}: {:?}", at(50.0, 170.0));
        assert!(blue_on_green[2] < blue_half[2], "{format:?}");

        // The clip cuts along the turned square: the corners of the
        // straight box round it are not drawn, its middle is.
        assert!(
            close(at(330.0, 50.0), grey_seen),
            "{format:?}: {:?}",
            at(330.0, 50.0)
        );
        assert!(close(at(420.0, 140.0), grey_seen), "{format:?}");
        let [r, g, b] = at(375.0, 95.0);
        assert!(r > 240 && g < 10 && b < 10, "{format:?}: {r},{g},{b}");
        // Its edge is a point of the diamond: 70 from the middle along a
        // side's normal is just inside, 72 just out.
        assert!(at(375.0, 95.0 - 68.0)[0] > 200, "{format:?}");
        assert!(close(at(375.0, 95.0 - 73.0), grey_seen), "{format:?}");

        // The shadow is not seen through the see-through box, and is
        // there round it.
        let clear = at(440.0, 220.0);
        assert!(
            close(at(330.0, 220.0), clear),
            "{format:?}: {:?} against {clear:?}",
            at(330.0, 220.0)
        );
        assert!(at(330.0, 186.0)[0] + 30 < grey_seen[0], "{format:?}");
    }
    finish(gpu);
}

#[test]
fn corners_given_in_pixels_are_round_and_soft_by_a_pixel() {
    use lore_ui::{DrawBuffer, Fill, Transform as Turn};

    let Some(gpu) = gpu() else {
        return;
    };
    let (position, size, radius) = ([40.0_f32, 40.0_f32], [120.0_f32, 80.0_f32], 20.0_f32);
    let mut buffer = DrawBuffer::default();
    let solid = |position: [f32; 2], size: [f32; 2], color: Color, radius: f32| Draw::Box {
        position,
        size,
        radii: [radius; 4].into(),
        fill: Fill::Color(color),
        border_widths: [0.0; 4],
        border_color: Color::WHITE,
        transform: Turn::IDENTITY,
        clip: None,
    };
    buffer.push(solid([0.0, 0.0], [480.0, 270.0], Color::BLACK, 0.0));
    buffer.push(solid(position, size, Color::rgb(1.0, 0.0, 0.0), radius));
    let shots = overlaid(&gpu, vk::Format::R8G8B8A8_UNORM, &[buffer.draw()]);
    // How much of each pixel a circle of the radius at each corner covers,
    // half a pixel either side of its edge.
    let half = [size[0] / 2.0, size[1] / 2.0];
    let middle = [position[0] + half[0], position[1] + half[1]];
    let mut checked = 0;
    for y in 30..130_u32 {
        for x in 30..170_u32 {
            let p = [
                (x as f32 + 0.5 - middle[0]).abs() - half[0] + radius,
                (y as f32 + 0.5 - middle[1]).abs() - half[1] + radius,
            ];
            if p[0] <= 0.0 || p[1] <= 0.0 {
                continue;
            }
            let outside = (p[0] * p[0] + p[1] * p[1]).sqrt() - radius;
            let expected = ((0.5 - outside).clamp(0.0, 1.0) * 255.0).round();
            let red = f32::from(wide_pixel(&shots[0], x as f32, y as f32)[0]);
            assert!(
                (red - expected).abs() <= 1.0,
                "at {x},{y}: {red} where {expected} was expected"
            );
            checked += 1;
        }
    }
    assert!(checked > 1000, "{checked}");
    finish(gpu);
}

#[test]
fn a_box_with_percentage_corners_on_a_wide_box_is_an_ellipse() {
    use lore_ui::{DrawBuffer, Fill, Radii, Transform as Turn};

    let Some(gpu) = gpu() else {
        return;
    };
    let solid =
        |position: [f32; 2], size: [f32; 2], radii: Radii, border: f32, color: Color| Draw::Box {
            position,
            size,
            radii,
            fill: Fill::Color(color),
            border_widths: [border; 4],
            border_color: Color::rgb(0.0, 1.0, 0.0),
            transform: Turn::IDENTITY,
            clip: None,
        };
    let mut buffer = DrawBuffer::default();
    let red = Color::rgb(1.0, 0.0, 0.0);
    buffer.push(solid(
        [0.0, 0.0],
        [480.0, 270.0],
        Radii::SQUARE,
        0.0,
        Color::BLACK,
    ));
    // 200 by 100, reaching 100 across and 50 down at every corner.
    let oval = Radii {
        across: [100.0; 4],
        down: [50.0; 4],
    };
    buffer.push(solid([40.0, 40.0], [200.0, 100.0], oval, 0.0, red));
    // The same with a border six wide, lower down.
    buffer.push(solid([40.0, 160.0], [200.0, 100.0], oval, 6.0, red));
    let shots = overlaid(&gpu, vk::Format::R8G8B8A8_UNORM, &[buffer.draw()]);
    let at = |x: f32, y: f32| wide_pixel(&shots[0], x, y);
    let mut checked = 0;
    for y in 40..140_u32 {
        for x in 40..240_u32 {
            // How far outside the ellipse, roughly in pixels.
            let (dx, dy) = (
                (x as f32 + 0.5 - 140.0) / 100.0,
                (y as f32 + 0.5 - 90.0) / 50.0,
            );
            let k = (dx * dx + dy * dy).sqrt();
            let red = at(x as f32, y as f32)[0];
            if k < 0.97 {
                assert!(red > 250, "at {x},{y}: {red}");
                checked += 1;
            } else if k > 1.03 {
                assert!(red < 5, "at {x},{y}: {red}");
                checked += 1;
            }
        }
    }
    assert!(checked > 15_000, "{checked}");
    // The border follows the ellipse round: green at the ends of both axes,
    // red inside, nothing past it.
    for (x, y) in [
        (43.0, 210.0),
        (236.0, 210.0),
        (140.0, 163.0),
        (140.0, 256.0),
    ] {
        let [r, g, _] = at(x, y);
        assert!(g > 200 && r < 40, "border at {x},{y}: {:?}", at(x, y));
    }
    let [r, g, _] = at(140.0, 210.0);
    assert!(r > 250 && g < 5);
    assert!(at(50.0, 170.0)[1] < 5, "past the border at a corner");
    finish(gpu);
}

/// How long the graphics card takes over a screen full of styled boxes: 320
/// of them with round corners, a border, a shadow and a number, at 1920 by
/// 1080, half of them faded as one panel and half each faded on its own;
/// and the same with each piece faded by itself instead, which is cheaper
/// and lets the pieces show through each other, and with none faded.
/// Printed, not checked: run it on its own with `--ignored --nocapture`, on
/// the software renderer too (`VK_DRIVER_FILES=.../lvp_icd.x86_64.json`).
#[test]
#[ignore = "a measurement, not a check"]
fn the_cost_of_a_screen_of_styled_boxes() {
    use lore_ui::{DrawBuffer, Fill, FontFace, Transform as Turn};

    let Some(gpu) = gpu() else {
        return;
    };
    let extent = vk::Extent2D {
        width: 1920,
        height: 1080,
    };
    let numbers: Vec<String> = (0..320).map(|n| n.to_string()).collect();
    let piece = |buffer: &mut DrawBuffer, n: usize, fade: f32| {
        let position = [8.0 + (n % 19) as f32 * 100.0, 8.0 + (n / 19) as f32 * 58.0];
        let faded = |color: Color| Color {
            a: color.a * fade,
            ..color
        };
        buffer.push(Draw::Shadow {
            position,
            size: [80.0, 40.0],
            radii: [12.0; 4].into(),
            color: faded(Color::rgba(0.0, 0.0, 0.0, 0.5)),
            offset: [3.0, 3.0],
            blur: 6.0,
            spread: 0.0,
            inset: false,
            transform: Turn::IDENTITY,
            clip: None,
        });
        buffer.push(Draw::Box {
            position,
            size: [80.0, 40.0],
            radii: [12.0; 4].into(),
            fill: Fill::Color(faded(Color::rgb(0.2, 0.4, 0.75))),
            border_widths: [2.0; 4],
            border_color: faded(Color::rgb(0.06, 0.12, 0.25)),
            transform: Turn::IDENTITY,
            clip: None,
        });
        buffer.push(Draw::Text {
            position: [position[0] + 4.0, position[1] + 8.0],
            size: 18.0,
            color: faded(Color::WHITE),
            text: &numbers[n],
            font: FontFace::default(),
            letter_spacing: 0.0,
            blur: 0.0,
            transform: Turn::IDENTITY,
            clip: None,
        });
    };
    // Grouped: a panel of 160 faded as one, then 160 more each faded as
    // one on its own.
    let mut grouped = DrawBuffer::default();
    grouped.push(Draw::Group { opacity: 0.5 });
    for n in 0..160 {
        piece(&mut grouped, n, 1.0);
    }
    grouped.push(Draw::EndGroup);
    for n in 160..320 {
        grouped.push(Draw::Group { opacity: 0.7 });
        piece(&mut grouped, n, 1.0);
        grouped.push(Draw::EndGroup);
    }
    // Each piece faded by itself, and not faded at all.
    let mut each_piece = DrawBuffer::default();
    let mut solid = DrawBuffer::default();
    for n in 0..320 {
        piece(&mut each_piece, n, if n < 160 { 0.5 } else { 0.7 });
        piece(&mut solid, n, 1.0);
    }
    let format = vk::Format::R8G8B8A8_SRGB;
    let frames = 30;
    let empty = Ui::default();
    let time = |list: DrawList<'_>| {
        let lists = vec![list; frames];
        overlaid_sized(&gpu, format, extent, &lists[..2]);
        overlaid_sized(&gpu, format, extent, &lists).1.as_secs_f64() * 1000.0 / frames as f64
    };
    let bare = time(empty.draw());
    println!(
        "a frame with no HUD: {bare:.3} ms; the boxes, solid: {:.3} ms, each piece faded: \
         {:.3} ms, faded in groups: {:.3} ms",
        time(solid.draw()) - bare,
        time(each_piece.draw()) - bare,
        time(grouped.draw()) - bare,
    );
    finish(gpu);
}
