//! Draws a scene's meshes: each with its material, lit by the sun and the
//! sky and shadowed from the sun by the others, nearer ones in front of
//! farther ones.

use std::cell::{Cell, RefCell};
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ash::vk;
use lore_assets::{AlphaMode, LightKind, Material, MeshPart, Scene, Water};

use crate::bounce::{self, Bounce, BounceWorks, colour_range};
use crate::buffer::Buffer;
use crate::camera::{Mat4, multiply, normal_matrix};
use crate::clouds::CloudVolume;
use crate::effects::{Air, BIT_BYTES, MOST_BITS, WEATHER_ROWS};
use crate::error::RenderError;
use crate::eye::EyePass;
use crate::fluid::FluidScene;
use crate::fluid_pass::FluidWorks;
use crate::gpu::Gpu;
use crate::grid::{self, grid_bytes};
use crate::lamp_shadow::{Face, LampAtlas, LampShadows, Shadowing};
use crate::lamps::{self, Lamp, LampFrame, Seen};
use crate::material::{self, Materials};
use crate::scene::{DEPTH, SceneTarget};
use crate::shadow::{self, Cascade, PICTURES, ShadowMap};
use crate::sky::{self, Sky};
use crate::swapchain::COLOUR;
use crate::tracks::TrackMap;
use crate::water::{
    Bodies, Body, GRID_INDICES, GRID_SQUARES, Stir, WaterRows, WaterWorks, body_layout, grid_laid,
};
use crate::weather::eased;

/// The mesh shaders, compiled from `shaders/mesh.wgsl` by the build script.
const SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mesh.spv"));

/// The background colour in linear light: sRGB `#181C24`.
pub(crate) const BACKGROUND: [f32; 4] = [0.0091, 0.0116, 0.0176, 1.0];

/// The bytes one corner point takes on the graphics card: a position of
/// three floats, a surface direction folded into two 16-bit numbers, a
/// tangent and a colour of four bytes each, and two UV maps of two floats.
const VERTEX_BYTES: usize = 40;

/// The size in bytes of the shader's `Draw`: a 4 by 4 matrix, a 3 by 3
/// matrix padded to three columns of four, and one more row of which the
/// first number is used. Vulkan promises room for exactly this much.
const DRAW_BYTES: usize = 128;

/// The size in bytes of the shader's `View`: three 4 by 4 matrices and five
/// rows of four numbers, then for the shadows one row, a row for each
/// shadow picture (the sun's four and the one from above) and a 4 by 4
/// matrix for each.
const VIEW_BYTES: usize = 272 + 16 + PICTURES * (16 + 64) + SKY_BYTES + 64;

/// The part of it that is the sky: three rows for the lines of sight, one
/// for the clouds, a row for each height of the air's light and of the
/// haze's, one for the fog, one for the wind, one for whoever walks and
/// nine for the water: the still water the eye is in or nearest, the
/// wind's waves, the rings round the eye and the eye's own; and two for
/// the light of lightning, seven for the weather and five for the sun, the
/// moon and the stars, and three for how the level's still and running water
/// looks.
const SKY_BYTES: usize = 16 * (3 + 1 + 2 * sky::HEIGHTS + 1 + 1 + 1 + 9 + 2 + WEATHER_ROWS + 5 + 3);

/// How rough a material can be and still mirror the world round it: the
/// shader's `MIRRORS_BELOW`.
const GLOSSY_BELOW: f32 = 0.5;

/// The number the next scene's meshes are known by.
static NEXT_MESHES: AtomicU64 = AtomicU64::new(1);

/// How the scene is seen in one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct View {
    /// Takes a point in the world to where it is drawn, including the
    /// part-of-a-pixel shift the upscaler asks for.
    pub(crate) clip: Mat4,
    /// The same without the shift.
    pub(crate) steady: Mat4,
    /// `steady` as it was in the last frame.
    pub(crate) before: Mat4,
    /// Where the eye is, in metres.
    pub(crate) eye: [f32; 3],
    /// How many sizes larger than usual textures are read (a negative mip
    /// bias, given as a positive number), for a picture that is upscaled.
    pub(crate) sharper: f32,
    /// The way the eye looks, then how far a line of sight leans to the
    /// right and up between the middle of the picture and its edge
    /// ([`Camera::rays`](crate::Camera)). The fourth number of the first
    /// is how many pixels high the picture is, and the fourth numbers of
    /// the last two are the upscaler's shift across and up.
    pub(crate) rays: [[f32; 4]; 3],
    /// How long the scene has been drawn for, in seconds.
    pub(crate) time: f32,
    /// The same, as exactly as it is kept: waves run by it, and after
    /// some hours an f32 would move them in jerks.
    pub(crate) exact: f64,
    /// How many seconds of that have passed since the frame before.
    pub(crate) elapsed: f32,
    /// How much whoever walks is stirring what they walk through: 0
    /// standing still, 1 at a run.
    pub(crate) stir: f32,
    /// A number that differs from frame to frame when the frames are mixed
    /// by the upscaler, which then smooths what is drawn with a pattern
    /// turned by it; 0 when each frame is shown as it is.
    pub(crate) noise: f32,
    /// How near to the eye the nearest thing drawn can be, in metres.
    pub(crate) near: f32,
    /// Whether smooth things mirror the world at all, or only the light of
    /// the sky and the ground.
    pub(crate) reflecting: bool,
    /// Whether the picture kept from the last frame is there to be
    /// mirrored: wet ground, puddles and polished things show the world
    /// in it. Not after a cut, when that picture is of somewhere else.
    pub(crate) mirrors: bool,
    /// Where each point of the world was drawn in that picture.
    pub(crate) kept: Mat4,
    /// Whether this frame's picture is to be kept for the next frame to
    /// mirror, when there is anything in the scene that would.
    pub(crate) keeps: bool,
    /// Whether this frame's picture is copied aside before what is
    /// see-through is drawn, which then mirrors that copy and not the
    /// frame before's: [`MeshPipeline::record`] works it out.
    pub(crate) copied: bool,
    /// Whether the air is lit once the picture is drawn: beams where the
    /// sun reaches it, the glow of lamps in it.
    pub(crate) shafts: bool,
    /// How many lamps cast shadows, and how sharp.
    pub(crate) lamp_shadows: LampShadows,
    /// How much light bounces.
    pub(crate) bounce: Bounce,
}

impl View {
    /// The shader's `View`. With no `shadows`, everything is in the sun;
    /// with no `sky`, none is drawn and no cloud shades the ground.
    fn bytes(
        &self,
        lighting: &Lighting,
        shadows: Option<&[Cascade; PICTURES]>,
        sky: Option<&Sky>,
        water: &WaterRows,
        above: &Above,
    ) -> [u8; VIEW_BYTES] {
        let mut water = *water;
        // The last of its rows is the eye's own.
        water[8] = [self.near, 0.0, 0.0, 0.0];
        let row = |[x, y, z]: [f32; 3], last: f32| [x, y, z, last];
        // The picture is far smaller than where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let pixel = 1.0 / shadow::SIZE as f32;
        // Under a sky that is all cloud the light from above is the
        // cloud's: grey, where a clear sky's is blue.
        // And what the sun sends is no longer a beam but comes from the
        // whole of it: a little over half of what a surface facing up
        // had, which is more than gets through real cloud and what an eye
        // used to the grey makes of it.
        let weigh = |[red, green, blue]: [f32; 3]| 0.2126 * red + 0.7152 * green + 0.0722 * blue;
        let closed = eased(0.6, 1.0, above.weather.cover);
        let through =
            weigh(lighting.sun) * lighting.towards_sun[1].max(0.0) / std::f32::consts::PI * 0.55;
        let grey = weigh(lighting.sky);
        let from_sky = lighting
            .sky
            .map(|part| part + (grey - part) * 0.85 * closed + through * closed);
        let rows = [
            row(self.eye, -self.sharper),
            row(lighting.towards_sun, 0.0),
            row(lighting.sun, 0.0),
            row(from_sky, 0.0),
            row(lighting.ground, 0.0),
            [
                if shadows.is_some() { 1.0 } else { 0.0 },
                pixel,
                shadow::SUN_WIDTH,
                self.noise,
            ],
        ];
        let mut reach = [[0.0; 4]; PICTURES];
        let mut from_world = [[[0.0; 4]; 4]; PICTURES];
        for (number, cascade) in shadows.into_iter().flatten().enumerate() {
            reach[number] = [cascade.texel, cascade.depth, 0.0, 0.0];
            from_world[number] = cascade.clip;
        }
        let air = &above.weather;
        let clouds = [
            if sky.is_some() { air.cover } else { 0.0 },
            self.time,
            sky::CLOUD_HEIGHT,
            if sky.is_some() { 1.0 } else { 0.0 },
        ];
        // Fog is the colour of the sky, so there is none without one.
        let [_, thins, height] = sky::FOG;
        let fog = [
            if sky.is_some() { air.fog } else { 0.0 },
            thins,
            height,
            sky::FOG[0],
        ];
        // The wind, and how long ago the last frame was: what bends in it
        // was where the wind of that moment had it.
        let [along_x, along_z, speed] = air.wind;
        let wind = [along_x, along_z, speed, self.elapsed];
        let mut scattered = [[0.0; 4]; 2 * sky::HEIGHTS];
        if let Some(sky) = sky {
            for (row, [red, green, blue]) in
                scattered.iter_mut().zip(sky.air.iter().chain(&sky.haze))
            {
                *row = [*red, *green, *blue, 0.0];
            }
        }
        let numbers = [self.clip, self.steady, self.before];
        let mut bytes = [0_u8; VIEW_BYTES];
        for (slot, number) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(
            numbers
                .iter()
                .flatten()
                .flatten()
                .chain(rows.iter().flatten())
                .chain(reach.iter().flatten())
                .chain(from_world.iter().flatten().flatten())
                .chain(self.rays.iter().flatten())
                .chain(&clouds)
                .chain(scattered.iter().flatten())
                .chain(&fog)
                .chain(&wind)
                .chain(&[
                    self.stir,
                    match (self.reflecting, self.mirrors) {
                        (false, _) => 0.0,
                        (true, false) => 0.5,
                        (true, true) => 1.0,
                    },
                    if self.copied { 1.0 } else { 0.0 },
                    0.0,
                ])
                .chain(water.iter().flatten())
                .chain(above.flashes.iter().flatten())
                .chain(above.weather.rows.iter().flatten())
                .chain(above.heavens.iter().flatten())
                .chain(above.water_look.iter().flatten())
                .chain(self.kept.iter().flatten()),
        ) {
            *slot = number.to_ne_bytes();
        }
        bytes
    }
}

/// What is in the sky of one frame besides its light, as the shader takes
/// it: where lightning's light comes from, the weather, and the heavens.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Above {
    pub(crate) flashes: [[f32; 4]; 2],
    pub(crate) weather: Air,
    pub(crate) heavens: [[f32; 4]; 5],
    /// How the level's water looks if it says so, as the shader's
    /// `water_look`: all zero if not.
    pub(crate) water_look: [[f32; 4]; 3],
}

/// The light a scene is drawn in: one sun, and the sky and ground around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Lighting {
    /// The direction from any surface to the sun; length one.
    pub(crate) towards_sun: [f32; 3],
    /// The sun's light on a surface that faces it (red, green, blue; linear).
    pub(crate) sun: [f32; 3],
    /// What a matte white surface facing straight up shows of the sky alone.
    pub(crate) sky: [f32; 3],
    /// What a matte white surface facing straight down shows of the ground.
    pub(crate) ground: [f32; 3],
}

impl Default for Lighting {
    /// A warm sun high in the sky to the right and behind the default
    /// camera, under a cool sky.
    ///
    /// The two add up so that a matte surface facing straight up shows
    /// exactly its own colour, which keeps a level that sets no light of its
    /// own looking the way its author painted it.
    fn default() -> Self {
        let towards_sun = Self::TOWARDS_SUN;
        // What the sun adds to a surface facing up, out of a total of 1.
        let share = [0.70, 0.65, 0.58];
        Self {
            towards_sun,
            // A matte surface shows the light on it divided by pi.
            sun: share.map(|part| part * std::f32::consts::PI / towards_sun[1]),
            sky: share.map(|part| 1.0 - part),
            ground: Self::GROUND,
        }
    }
}

impl Lighting {
    /// Up 50 degrees, to the right and towards the default camera. Length one.
    const TOWARDS_SUN: [f32; 3] = [0.399_44, 0.768_92, 0.499_3];
    const GROUND: [f32; 3] = [0.20, 0.19, 0.17];

    /// The light `scene` asks for: its first sun, with the usual sky and
    /// ground, or the usual sun too if it has none.
    pub(crate) fn of(scene: &Scene) -> Self {
        let usual = Self::default();
        let sun = scene.lights.iter().find_map(|light| {
            if light.kind != LightKind::Sun {
                return None;
            }
            let [x, y, z] = light.direction;
            let length = (x * x + y * y + z * z).sqrt();
            let sound = length.is_finite()
                && length > 0.0
                && light.intensity.is_finite()
                && light.color.iter().all(|part| part.is_finite());
            sound.then(|| {
                (
                    // It shines the way it points, so it is found the other way.
                    [-x / length, -y / length, -z / length],
                    light.color.map(|part| (part * light.intensity).max(0.0)),
                )
            })
        });
        match sun {
            Some((towards_sun, sun)) => Self {
                towards_sun,
                sun,
                ..usual
            },
            None => usual,
        }
    }
}

/// What a frame shows of the world.
#[derive(Clone, Copy, Default)]
pub(crate) struct Contents<'a> {
    /// The level's meshes, if a level has been set.
    pub(crate) meshes: Option<&'a Meshes>,
    /// Whether the sky is drawn behind them, and its clouds shade them.
    pub(crate) sky: bool,
    /// Whether the editor's ground grid is drawn over them.
    pub(crate) grid: bool,
    /// Whatever is stirring the water this frame.
    pub(crate) stirs: &'a [Stir],
    /// The drops, spray and bubbles in the air and the water this frame,
    /// as [`Effects::bytes`](crate::effects::Effects::bytes) gives them.
    pub(crate) bits: &'a [u8],
    /// Where the light of each stroke of lightning comes from, and how
    /// bright it is now.
    pub(crate) flashes: [[f32; 4]; 2],
    /// The weather, as [`Effects::air`](crate::effects::Effects::air)
    /// gives it.
    pub(crate) weather: Air,
    /// The sky at the hour it is, if the game has said the hour: with
    /// none, the level's own light is the light.
    pub(crate) day: Option<&'a sky::Day>,
    /// The rows of the map of trodden snow that have changed, as
    /// [`Effects::tracks`](crate::effects::Effects::tracks) gives them.
    pub(crate) tracks: [Option<(u32, &'a [u8])>; 2],
    /// The fluids poured about, and how each kind looks: drawn as one
    /// liquid over the solid world, and nothing at all when there are no
    /// drops.
    pub(crate) fluid: Option<&'a FluidScene>,
    /// The lamps lit this frame.
    pub(crate) lamps: &'a [Lamp],
}

/// How a surface covers what is behind it. Drawn in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Surface {
    /// All of it hides what is behind.
    Solid,
    /// Its alpha cuts holes in it.
    CutOut,
    /// What is behind shows through, by its alpha.
    SeeThrough,
    /// The surface of water: see-through where it is shallow, a mirror of
    /// the sky where it is looked along, and moving.
    Water,
}

impl Surface {
    /// In the order of the pipelines that draw them.
    const ALL: [Self; 4] = [Self::Solid, Self::CutOut, Self::SeeThrough, Self::Water];

    fn of(material: &Material) -> Self {
        match material.alpha_mode {
            AlphaMode::Opaque => Self::Solid,
            AlphaMode::Mask { .. } => Self::CutOut,
            AlphaMode::Blend => Self::SeeThrough,
        }
    }
}

/// One mesh part placed in the world.
struct Draw {
    /// Where the part stands.
    world: Mat4,
    /// Turns its surface directions to match.
    normal: [[f32; 4]; 3],
    /// Which of the scene's materials it is drawn with, as
    /// [`Materials::set`] numbers them.
    material: u32,
    surface: Surface,
    /// The middle of the part in the world, which see-through parts are
    /// put in order by.
    middle: [f32; 3],
    /// How far from its middle it reaches at most, in metres.
    radius: f32,
    /// Where its triangles start in the index buffer, and how many numbers they are.
    first_index: u32,
    index_count: u32,
    /// Where its corner points start in the vertex buffer.
    first_vertex: i32,
    /// Its triangles run clockwise seen from the front, because it is mirrored.
    mirrored: bool,
    /// Both of its sides are drawn.
    double_sided: bool,
    /// How far the wind moves it: 0 for a thing that stands still.
    sway: f32,
}

/// Where one mesh part's data sits in the buffers, or `None` if it was left out.
type Placed = Option<(u32, u32, i32)>;

/// A scene's meshes and materials, copied to the graphics card and ready to draw.
pub(crate) struct Meshes {
    /// `None` when the scene has nothing to draw.
    buffers: Option<(Buffer, Buffer)>,
    /// Solid parts, then cut-out ones, each kind grouped by material; then
    /// see-through ones.
    draws: Vec<Draw>,
    /// How many of `draws` come before the first see-through one.
    see_through_from: usize,
    /// Whether any of them is water, which looks through a copy of the
    /// picture behind it.
    has_water: bool,
    /// Whether any material is smooth enough to mirror the world round
    /// it, which it does in the picture kept from the frame before.
    glossy: bool,
    /// The still water in the scene, lakes and ponds, each in the shape it
    /// took when it filled its hollow.
    bodies: Bodies,
    /// The same water as the level's maker's file gave it, for what flies
    /// to ask where the surface is.
    waters: Vec<Water>,
    materials: Materials,
    lighting: Lighting,
    /// The sky under that light.
    sky: Sky,
    /// A number no other scene's meshes have, by which the shadows drawn
    /// for these are told from another level's.
    id: u64,
    /// The corners of the box that holds every part: the smallest x, y and
    /// z, then the largest.
    bounds: [[f32; 3]; 2],
}

impl Meshes {
    /// Copies the meshes of `scene`, its materials and the textures they
    /// use to the graphics card.
    ///
    /// Every mesh is stored once however many times it is placed. A part
    /// whose lists do not match is left out with a warning.
    pub(crate) fn new(gpu: &Arc<Gpu>, scene: &Scene) -> Result<Self, RenderError> {
        let doing = "copying the level's meshes to the graphics card";
        let mut vertices: Vec<u8> = Vec::new();
        let mut indices: Vec<u8> = Vec::new();
        let mut placed: Vec<Vec<Placed>> = Vec::with_capacity(scene.meshes.len());
        for (number, mesh) in scene.meshes.iter().enumerate() {
            let mut parts = Vec::with_capacity(mesh.parts.len());
            for part in &mesh.parts {
                let range = append(part, &mut vertices, &mut indices);
                if range.is_none() && !part.indices.is_empty() {
                    log::warn!(
                        "Mesh {} ({number}) has a part that cannot be drawn: its triangles \
                         name points it does not have, or its lists differ in length. \
                         Export the file again; if you built the scene in code, check that \
                         `positions`, `normals` and any of `tangents`, `uv0`, `uv1` and \
                         `colors` are the same length.",
                        mesh.name.as_deref().unwrap_or("without a name"),
                    );
                }
                parts.push(range);
            }
            placed.push(parts);
        }

        let fallback = Material::default();
        let mut draws = Vec::new();
        for (number, instance) in (0_u32..).zip(&scene.instances) {
            // A sheet of water that filled its hollow is drawn as the
            // shape it took, not as the sheet.
            if scene
                .waters
                .iter()
                .any(|water| water.sources.contains(&number))
            {
                continue;
            }
            let (Some(mesh), Some(parts)) = (
                scene.meshes.get(instance.mesh as usize),
                placed.get(instance.mesh as usize),
            ) else {
                continue;
            };
            let water = instance.is_water();
            for (part, range) in mesh.parts.iter().zip(parts) {
                let Some((first_index, index_count, first_vertex)) = *range else {
                    continue;
                };
                let material = scene
                    .materials
                    .get(part.material as usize)
                    .unwrap_or(&fallback);
                let [low, high] = [part.bounds.min, part.bounds.max];
                // Half the box's diagonal, stretched as far as the part is
                // stretched the most.
                let half: f32 = (0..3)
                    .map(|axis| ((high[axis] - low[axis]) * 0.5).powi(2))
                    .sum::<f32>()
                    .sqrt();
                let stretch = (0..3)
                    .map(|column| {
                        let [x, y, z, _] = instance.transform[column];
                        (x * x + y * y + z * z).sqrt()
                    })
                    .fold(0.0, f32::max);
                draws.push(Draw {
                    world: instance.transform,
                    normal: normal_matrix(&instance.transform),
                    material: Materials::number(scene, part.material),
                    surface: if water {
                        Surface::Water
                    } else {
                        Surface::of(material)
                    },
                    middle: moved(
                        &instance.transform,
                        [0, 1, 2].map(|axis| (low[axis] + high[axis]) * 0.5),
                    ),
                    radius: half * stretch,
                    first_index,
                    index_count,
                    first_vertex,
                    mirrored: instance.mirrored,
                    // Water is seen from underneath as well.
                    double_sided: material.double_sided || water,
                    // Water is marked by a sway below nothing: its points
                    // rise and fall with its waves.
                    // Water is marked by a sway of -1 and what floats on
                    // it by -2: both rise and fall with the waves.
                    sway: if water {
                        -1.0
                    } else if instance.floats() {
                        -2.0
                    } else {
                        instance.wind()
                    },
                });
            }
        }
        // Parts that share a material follow one another, so the material
        // is handed to the graphics card once for all of them.
        draws.sort_by_key(|draw| (draw.surface, draw.material));
        let see_through_from = draws
            .iter()
            .position(|draw| draw.surface >= Surface::SeeThrough)
            .unwrap_or(draws.len());

        let buffers = if draws.is_empty() {
            None
        } else {
            Some((
                Buffer::holding(gpu, &vertices, vk::BufferUsageFlags::VERTEX_BUFFER, doing)?,
                Buffer::holding(gpu, &indices, vk::BufferUsageFlags::INDEX_BUFFER, doing)?,
            ))
        };
        let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
        for draw in draws.iter().filter(|draw| draw.radius.is_finite()) {
            let [low, high] = &mut bounds;
            for ((low, high), middle) in low.iter_mut().zip(high.iter_mut()).zip(draw.middle) {
                *low = low.min(middle - draw.radius);
                *high = high.max(middle + draw.radius);
            }
        }
        Ok(Self {
            buffers,
            has_water: !scene.waters.is_empty()
                || draws.iter().any(|draw| draw.surface == Surface::Water),
            // A picture of roughness can make any of it smooth.
            glossy: scene.materials.iter().any(|material| {
                material.roughness < GLOSSY_BELOW || material.metallic_roughness_texture.is_some()
            }),
            bodies: Bodies::new(gpu, scene)?,
            waters: scene.waters.clone(),
            draws,
            see_through_from,
            materials: Materials::new(gpu, scene)?,
            lighting: Lighting::of(scene),
            sky: Sky::of(&Lighting::of(scene)),
            id: NEXT_MESHES.fetch_add(1, Ordering::Relaxed),
            bounds,
        })
    }
}

impl Meshes {
    /// The still water of the scene, each body in the shape it took.
    pub(crate) fn waters(&self) -> &[Water] {
        &self.waters
    }

    /// Whether any material is smooth enough to mirror the world round it.
    pub(crate) fn glossy(&self) -> bool {
        self.glossy
    }
}

/// Where `transform` puts `point`.
fn moved(transform: &Mat4, point: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|row| {
        transform[0][row] * point[0]
            + transform[1][row] * point[1]
            + transform[2][row] * point[2]
            + transform[3][row]
    })
}

/// A number from -1 to 1 as a signed 16-bit one.
fn snorm16(value: f32) -> i16 {
    // The clamp keeps the product inside what an `i16` holds; not-a-number becomes 0.
    #[allow(clippy::cast_possible_truncation)]
    let packed = (value.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    packed
}

/// A number from -1 to 1 as a signed byte.
fn snorm8(value: f32) -> i8 {
    #[allow(clippy::cast_possible_truncation)]
    let packed = (value.clamp(-1.0, 1.0) * 127.0).round() as i8;
    packed
}

/// A number from 0 to 1 as a byte.
fn unorm8(value: f32) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let packed = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    packed
}

/// A share of light from 0 to 1 as a byte, on the curve a screen uses
/// (sRGB): even steps of the byte look like even steps of brightness, so
/// dark colours keep as many steps as light ones, where steps of light
/// itself would band in the dark. The shader's `vertex` takes it back.
fn srgb8(value: f32) -> u8 {
    let light = value.clamp(0.0, 1.0);
    let seen = if light <= 0.003_130_8 {
        light * 12.92
    } else {
        1.055 * light.powf(1.0 / 2.4) - 0.055
    };
    unorm8(seen)
}

/// A direction folded flat onto a square (octahedral encoding), which takes
/// two numbers instead of three and loses nothing a screen can show.
///
/// The shader's `unfolded` is the way back. A direction with no length, or
/// one that is not a number, comes out as straight up the Z axis.
fn folded([x, y, z]: [f32; 3]) -> [i16; 2] {
    let sum = x.abs() + y.abs() + z.abs();
    if !sum.is_finite() || sum == 0.0 {
        return [0, 0];
    }
    let (x, y) = (x / sum, y / sum);
    if z >= 0.0 {
        [snorm16(x), snorm16(y)]
    } else {
        // The lower half is folded outwards over the corners.
        [
            snorm16((1.0 - y.abs()).copysign(x)),
            snorm16((1.0 - x.abs()).copysign(y)),
        ]
    }
}

/// Adds one part's points and triangles to the ends of `vertices` and
/// `indices`, and says where they went. Adds nothing and returns `None` if
/// the part is empty or does not hold together.
fn append(part: &MeshPart, vertices: &mut Vec<u8>, indices: &mut Vec<u8>) -> Placed {
    fn fits<T>(list: Option<&Vec<T>>, points: usize) -> bool {
        list.is_none_or(|list| list.len() == points)
    }
    let points = part.positions.len();
    let whole = part.indices.len() - part.indices.len() % 3;
    if whole == 0
        || part.normals.len() != points
        || !fits(part.tangents.as_ref(), points)
        || !fits(part.uv0.as_ref(), points)
        || !fits(part.uv1.as_ref(), points)
        || !fits(part.colors.as_ref(), points)
        || part.indices[..whole]
            .iter()
            .any(|&index| index as usize >= points)
    {
        return None;
    }
    let first_index = u32::try_from(indices.len() / 4).ok()?;
    let index_count = u32::try_from(whole).ok()?;
    let first_vertex = i32::try_from(vertices.len() / VERTEX_BYTES).ok()?;
    // The last point's number must fit too.
    first_index.checked_add(index_count)?;
    first_vertex.checked_add(i32::try_from(points).ok()?)?;

    vertices.reserve(points * VERTEX_BYTES);
    for (number, (position, normal)) in part.positions.iter().zip(&part.normals).enumerate() {
        let each =
            |list: &Option<Vec<[f32; 2]>>| list.as_ref().map_or([0.0; 2], |list| list[number]);
        for value in position {
            vertices.extend_from_slice(&value.to_ne_bytes());
        }
        for value in folded(*normal) {
            vertices.extend_from_slice(&value.to_ne_bytes());
        }
        // No tangent is all zeroes, which the shader takes as "do not bend".
        let tangent = part
            .tangents
            .as_ref()
            .map_or([0.0, 0.0, 0.0, 1.0], |tangents| tangents[number]);
        vertices.extend(tangent.map(|value| snorm8(value).to_ne_bytes()[0]));
        // A point with no colour of its own is white, so the material's shows.
        let [red, green, blue, alpha] = part
            .colors
            .as_ref()
            .map_or([1.0; 4], |colours| colours[number]);
        vertices.extend([srgb8(red), srgb8(green), srgb8(blue), unorm8(alpha)]);
        for value in each(&part.uv0).iter().chain(&each(&part.uv1)) {
            vertices.extend_from_slice(&value.to_ne_bytes());
        }
    }
    indices.reserve(whole * 4);
    for index in &part.indices[..whole] {
        indices.extend_from_slice(&index.to_ne_bytes());
    }
    Some((first_index, index_count, first_vertex))
}
/// Draws meshes into a [`SceneTarget`].
pub(crate) struct MeshPipeline {
    gpu: Arc<Gpu>,
    set_layout: vk::DescriptorSetLayout,
    /// What a material's set holds; each scene's materials make sets like it.
    material_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    /// What a body of still water's set holds; each scene's make sets like it.
    body_layout: vk::DescriptorSetLayout,
    /// One for each [`Surface`], in order.
    pipelines: [vk::Pipeline; 4],
    /// Draws still water from the shape it took when it filled its hollow.
    body_pipeline: vk::Pipeline,
    /// The wind's waves and the rings: `None` only while the pipeline is
    /// being built.
    water: Option<WaterWorks>,
    /// The body of water whose depths each frame's set was last pointed
    /// at; 0 for none.
    near_water: RefCell<Vec<u64>>,
    /// Where the snow is trodden: `None` only while the pipeline is being
    /// built.
    tracks: Option<TrackMap>,
    /// Draws the drops, spray and bubbles a splash throws up.
    bit_pipeline: vk::Pipeline,
    /// Those bits, for each frame that can be in progress at once.
    bits: Vec<Buffer>,
    /// Draw what the sun sees of solid parts and of cut-out ones.
    shadow_pipelines: [vk::Pipeline; 2],
    /// What the sun sees: `None` only while the pipeline is being built.
    shadows: Option<ShadowMap>,
    /// Light that bounces: `None` only while the pipeline is being built.
    bounce: Option<BounceWorks>,
    /// What the lamps that cast shadows see: made when a lamp first needs
    /// it, and made again when the player's setting changes.
    lamp_atlas: RefCell<Option<LampAtlas>>,
    /// The pictures each frame's set was last pointed at for the lamps'
    /// shadows.
    lamp_bound: RefCell<Vec<vk::ImageView>>,
    /// The setting the atlas could not be made for, if it could not: it is
    /// not tried again until the setting changes.
    lamp_atlas_failed: Cell<Option<LampShadows>>,
    /// What the clouds are shaped from: `None` only while the pipeline is
    /// being built.
    clouds: Option<CloudVolume>,
    /// Draws the fluids a game pours about: `None` only while the pipeline
    /// is being built.
    fluid: Option<FluidWorks>,
    /// Draws the sky behind everything.
    sky: vk::Pipeline,
    /// Lays the light in the air over the finished picture: beams where the
    /// sun reaches the air, and the glow of lamps in it.
    air: vk::Pipeline,
    /// The sky of a scene that sets no light of its own.
    usual_sky: Sky,
    /// Draws the editor's ground grid, with the same layout and view.
    grid: vk::Pipeline,
    pool: vk::DescriptorPool,
    /// The [`View`] of each frame that can be in progress at once, with the
    /// set that points the shader at it.
    views: Vec<(Buffer, vk::DescriptorSet)>,
    /// The copy of the picture each of those sets was last pointed at:
    /// a window that changes size gets a new one.
    behind: RefCell<Vec<u64>>,
    /// Measures each frame's picture for the eye to adapt to: `None` only
    /// while the pipeline is being built.
    eye: Option<EyePass>,
    /// The lamps of each frame that can be in progress at once, as the
    /// shader's `Lamps` takes them.
    lamp_buffers: Vec<(Buffer, Buffer)>,
    /// The lamps of the frame being recorded, sorted into the grid of the
    /// view: kept between frames so that it needs no new memory.
    lamp_frame: RefCell<LampFrame>,
    /// The see-through parts of the frame being recorded, farthest first:
    /// how far each is from the eye, squared, and which it is. Kept between
    /// frames so that putting them in order needs no new memory.
    order: RefCell<Vec<(f32, usize)>>,
}

impl MeshPipeline {
    /// Builds the pipeline, for up to `frames` frames in progress at once.
    pub(crate) fn new(gpu: &Arc<Gpu>, frames: usize) -> Result<Self, RenderError> {
        let doing = "preparing the shaders";
        let device = gpu.device();
        let mut mesh = Self {
            gpu: Arc::clone(gpu),
            set_layout: vk::DescriptorSetLayout::null(),
            material_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            body_layout: vk::DescriptorSetLayout::null(),
            pipelines: [vk::Pipeline::null(); 4],
            body_pipeline: vk::Pipeline::null(),
            water: None,
            near_water: RefCell::new(vec![0; frames.max(1)]),
            tracks: None,
            bit_pipeline: vk::Pipeline::null(),
            bits: Vec::with_capacity(frames.max(1)),
            shadow_pipelines: [vk::Pipeline::null(); 2],
            shadows: None,
            bounce: None,
            lamp_atlas: RefCell::new(None),
            lamp_bound: RefCell::new(vec![vk::ImageView::null(); frames.max(1)]),
            lamp_atlas_failed: Cell::new(None),
            clouds: None,
            fluid: None,
            sky: vk::Pipeline::null(),
            air: vk::Pipeline::null(),
            usual_sky: Sky::of(&Lighting::default()),
            grid: vk::Pipeline::null(),
            pool: vk::DescriptorPool::null(),
            views: Vec::with_capacity(frames),
            behind: RefCell::new(vec![0; frames.max(1)]),
            eye: None,
            lamp_buffers: Vec::with_capacity(frames.max(1)),
            lamp_frame: RefCell::new(LampFrame::new()),
            order: RefCell::new(Vec::new()),
        };
        // Dropping `mesh` cleans up whatever was created; destroying a null handle is allowed.
        let count = u32::try_from(frames.max(1)).unwrap_or(1);
        let code = ash::util::read_spv(&mut Cursor::new(SHADER)).map_err(|_| {
            // Only possible if the build script wrote a broken file.
            RenderError::Driver {
                doing,
                code: vk::Result::ERROR_INVALID_SHADER_NV,
            }
        })?;

        let shadows = ShadowMap::new(gpu)?;
        let (shadow_view, shadow_sampler) = (shadows.pictures.view, shadows.sampler);
        mesh.shadows = Some(shadows);
        let clouds = CloudVolume::new(gpu)?;
        let (cloud_view, cloud_sampler) = (clouds.view, clouds.sampler);
        mesh.clouds = Some(clouds);
        for _ in 0..frames.max(1) {
            mesh.bits.push(Buffer::writable(
                gpu,
                (MOST_BITS * BIT_BYTES) as u64,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                doing,
            )?);
        }
        let water = WaterWorks::new(gpu, frames)?;
        let water_pictures = water.pictures();
        let (repeating, stopping) = (water.repeating, water.stopping);
        mesh.water = Some(water);
        let tracks = TrackMap::new(gpu, frames)?;
        let track_view = tracks.view();
        mesh.tracks = Some(tracks);
        mesh.bounce = Some(BounceWorks::new(gpu, frames)?);
        let both_stages = |binding: u32, kind: vk::DescriptorType| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(binding)
                .descriptor_type(kind)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
        };
        // The view, then the shadow pictures and what reads them, then the
        // block the clouds are shaped from and what reads that: the wind
        // is read from it too, where points are placed.
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(3)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(4)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            // What water looks through: the picture and its depth as they
            // were before it was drawn.
            // The picture is asked its size where what flies is placed.
            both_stages(5, vk::DescriptorType::SAMPLED_IMAGE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(6)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // The water: the wind's waves (two pictures and what reads
            // them), the rings (a picture and what reads it), and how
            // deep the still water nearest the eye is. Read where points
            // are placed too: the surface is moved by its waves.
            both_stages(7, vk::DescriptorType::SAMPLED_IMAGE),
            both_stages(8, vk::DescriptorType::SAMPLED_IMAGE),
            both_stages(9, vk::DescriptorType::SAMPLER),
            both_stages(10, vk::DescriptorType::SAMPLED_IMAGE),
            both_stages(11, vk::DescriptorType::SAMPLER),
            both_stages(12, vk::DescriptorType::SAMPLED_IMAGE),
            // Where the snow near the eye is trodden.
            vk::DescriptorSetLayoutBinding::default()
                .binding(13)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // The lamps, and the grid of the view they are sorted into.
            vk::DescriptorSetLayoutBinding::default()
                .binding(14)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(15)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // The picture's depth, for the light in the air once it is drawn.
            vk::DescriptorSetLayoutBinding::default()
                .binding(16)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // The lamps' shadows.
            vk::DescriptorSetLayoutBinding::default()
                .binding(17)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // Bounce light: what this frame traces and where the grids are
            // (read where the level is drawn into its copy and where the
            // probes' squares are placed), the copy of the level read and
            // written, the probes' light (whose size places the squares)
            // and depth, and this frame's rays.
            both_stages(18, vk::DescriptorType::STORAGE_BUFFER),
            vk::DescriptorSetLayoutBinding::default()
                .binding(19)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(20)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            both_stages(21, vk::DescriptorType::SAMPLED_IMAGE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(22)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(23)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // The way each block of the copy faces, read and written.
            vk::DescriptorSetLayoutBinding::default()
                .binding(24)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(25)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        ];
        let sizes = [
            vk::DescriptorType::UNIFORM_BUFFER,
            vk::DescriptorType::STORAGE_BUFFER,
            vk::DescriptorType::SAMPLED_IMAGE,
            vk::DescriptorType::STORAGE_IMAGE,
            vk::DescriptorType::SAMPLER,
        ]
        .map(|kind| {
            // Each frame's set has one view, the lamps, their grid and
            // bounce light's list, sixteen pictures, two written, and four
            // readers.
            let each = match kind {
                vk::DescriptorType::UNIFORM_BUFFER => 1,
                vk::DescriptorType::STORAGE_BUFFER => 3,
                vk::DescriptorType::SAMPLED_IMAGE => 16,
                vk::DescriptorType::STORAGE_IMAGE => 2,
                _ => 4,
            };
            vk::DescriptorPoolSize::default()
                .ty(kind)
                .descriptor_count(count * each)
        });
        let ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .size(DRAW_BYTES as u32)];
        // SAFETY: the device is alive, and each object is created from valid
        // settings that live until the call returns, and stored in `mesh`
        // straight away.
        unsafe {
            mesh.set_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            mesh.material_layout = material::set_layout(gpu)?;
            mesh.body_layout = body_layout(gpu)?;
            let set_layouts = [mesh.set_layout];
            let both = [mesh.set_layout, mesh.material_layout, mesh.body_layout];
            mesh.layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&both)
                        .push_constant_ranges(&ranges),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            mesh.pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(count)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            for _ in 0..count {
                let buffer = Buffer::writable(
                    gpu,
                    VIEW_BYTES as u64,
                    vk::BufferUsageFlags::UNIFORM_BUFFER,
                    doing,
                )?;
                let set = device
                    .allocate_descriptor_sets(
                        &vk::DescriptorSetAllocateInfo::default()
                            .descriptor_pool(mesh.pool)
                            .set_layouts(&set_layouts),
                    )
                    .map_err(RenderError::driver(doing))?
                    .first()
                    .copied()
                    .unwrap_or_default();
                let buffers = [vk::DescriptorBufferInfo::default()
                    .buffer(buffer.handle)
                    .range(VIEW_BYTES as u64)];
                let lamp_buffer = Buffer::writable(
                    gpu,
                    lamps::LAMP_BUFFER_BYTES as u64,
                    vk::BufferUsageFlags::STORAGE_BUFFER,
                    doing,
                )?;
                let cell_buffer = Buffer::writable(
                    gpu,
                    lamps::CELL_BUFFER_BYTES as u64,
                    vk::BufferUsageFlags::STORAGE_BUFFER,
                    doing,
                )?;
                let lamp_info = [vk::DescriptorBufferInfo::default()
                    .buffer(lamp_buffer.handle)
                    .range(lamps::LAMP_BUFFER_BYTES as u64)];
                let cell_info = [vk::DescriptorBufferInfo::default()
                    .buffer(cell_buffer.handle)
                    .range(lamps::CELL_BUFFER_BYTES as u64)];
                let pictures = [vk::DescriptorImageInfo::default()
                    .image_view(shadow_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                let samplers = [vk::DescriptorImageInfo::default().sampler(shadow_sampler)];
                let volumes = [vk::DescriptorImageInfo::default()
                    .image_view(cloud_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                let volume_samplers = [vk::DescriptorImageInfo::default().sampler(cloud_sampler)];
                let water_views = water_pictures.map(|view| {
                    [vk::DescriptorImageInfo::default()
                        .image_view(view)
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
                });
                let water_samplers = [repeating, stopping]
                    .map(|sampler| [vk::DescriptorImageInfo::default().sampler(sampler)]);
                let trodden = [vk::DescriptorImageInfo::default()
                    .image_view(track_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                let water_write = |binding: u32, info| picture_write(set, binding, info);
                let reader_write = |binding: u32, info| {
                    picture_write(set, binding, info).descriptor_type(vk::DescriptorType::SAMPLER)
                };
                let writes = [
                    water_write(7, &water_views[0]),
                    water_write(8, &water_views[1]),
                    reader_write(9, &water_samplers[0]),
                    water_write(10, &water_views[2]),
                    reader_write(11, &water_samplers[1]),
                    water_write(12, &water_views[3]),
                    water_write(13, &trodden),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .buffer_info(&buffers),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&pictures),
                    // Until a lamp casts a shadow, the sun's stand in for
                    // the lamps' (and are never read as theirs).
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(17)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&pictures),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(2)
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .image_info(&samplers),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(3)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&volumes),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(4)
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .image_info(&volume_samplers),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(14)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(&lamp_info),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(15)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(&cell_info),
                ];
                device.update_descriptor_sets(&writes, &[]);
                if let Some(bounce) = &mesh.bounce {
                    let infos = bounce.infos(mesh.views.len());
                    device.update_descriptor_sets(&infos.writes(set), &[]);
                }
                mesh.views.push((buffer, set));
                mesh.lamp_buffers.push((lamp_buffer, cell_buffer));
            }
        }
        let layout = mesh.layout;

        // SAFETY: `code` is SPIR-V produced and validated by naga at build time.
        let module = unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)
        }
        .map_err(RenderError::driver(doing))?;

        let bindings = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(VERTEX_BYTES as u32)
            .input_rate(vk::VertexInputRate::VERTEX)];
        // Position, folded normal, tangent, colour and the two UV maps.
        let formats = [
            (vk::Format::R32G32B32_SFLOAT, 0),
            (vk::Format::R16G16_SNORM, 12),
            (vk::Format::R8G8B8A8_SNORM, 16),
            (vk::Format::R8G8B8A8_UNORM, 20),
            (vk::Format::R32G32_SFLOAT, 24),
            (vk::Format::R32G32_SFLOAT, 32),
        ];
        let mut attributes = [vk::VertexInputAttributeDescription::default(); 6];
        for ((location, attribute), (format, offset)) in (0_u32..).zip(&mut attributes).zip(formats)
        {
            *attribute = attribute.location(location).format(format).offset(offset);
        }
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&bindings)
            .vertex_attribute_descriptions(&attributes);
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        // The viewport and scissor are set while drawing, so a resized window
        // does not need a new pipeline. Which side of a triangle is its front
        // and whether backs are skipped are set for each mesh.
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
        let formats = [SceneTarget::FORMAT, SceneTarget::MOTION_FORMAT];

        let mut failed = None;
        for (surface, pipeline) in Surface::ALL.into_iter().zip(&mut mesh.pipelines) {
            let see_through = surface >= Surface::SeeThrough;
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vertex"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(match surface {
                        Surface::Solid => c"fragment",
                        Surface::CutOut => c"fragment_cutout",
                        Surface::SeeThrough => c"fragment_blend",
                        Surface::Water => c"fragment_water",
                    }),
            ];
            // Depth is 1 at the camera and 0 far away, so the greater depth
            // wins. A see-through surface is hidden by what is in front of
            // it but hides nothing itself.
            let depth = vk::PipelineDepthStencilStateCreateInfo::default()
                .depth_test_enable(true)
                .depth_write_enable(!see_through)
                .depth_compare_op(vk::CompareOp::GREATER);
            // The colour, then how far the pixel moved. A see-through
            // surface is mixed over the colour by its alpha. The movement
            // image has to be mixed the same way, as only some graphics
            // cards can treat the two differently, so the shader writes it
            // with an alpha of nothing and what is behind is left alone.
            let attachment = vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA);
            let attachments = [if see_through {
                attachment
                    .blend_enable(true)
                    .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                    .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                    .color_blend_op(vk::BlendOp::ADD)
                    .src_alpha_blend_factor(vk::BlendFactor::ZERO)
                    .dst_alpha_blend_factor(vk::BlendFactor::ONE)
                    .alpha_blend_op(vk::BlendOp::ADD)
            } else {
                attachment
            }; 2];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
            let mut rendering = vk::PipelineRenderingCreateInfo::default()
                .color_attachment_formats(&formats)
                .depth_attachment_format(SceneTarget::DEPTH_FORMAT);
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&vertex_input)
                .input_assembly_state(&assembly)
                .viewport_state(&viewport)
                .rasterization_state(&raster)
                .multisample_state(&multisample)
                .depth_stencil_state(&depth)
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
                Ok(pipelines) if !pipelines.is_empty() => *pipeline = pipelines[0],
                other => {
                    failed =
                        Some(other.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN));
                    break;
                }
            }
        }
        // Still water that filled its hollow: a grid of points laid round
        // the eye, two numbers each, mixed over what is behind it as other
        // water is.
        if failed.is_none() {
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vertex_body"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(c"fragment_body"),
            ];
            let bindings = [vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(8)
                .input_rate(vk::VertexInputRate::VERTEX)];
            let attributes = [vk::VertexInputAttributeDescription::default()
                .location(0)
                .format(vk::Format::R32G32_SFLOAT)
                .offset(0)];
            let grid_input = vk::PipelineVertexInputStateCreateInfo::default()
                .vertex_binding_descriptions(&bindings)
                .vertex_attribute_descriptions(&attributes);
            // Its depth is written: what is drawn after it (see-through
            // things, spray, the light in the air) stops at the surface
            // as it does at the ground, and the upscaler reads the surface
            // where it reads the surface's movement. What is under the
            // water was copied aside for it before.
            let depth = vk::PipelineDepthStencilStateCreateInfo::default()
                .depth_test_enable(true)
                .depth_write_enable(true)
                .depth_compare_op(vk::CompareOp::GREATER);
            let attachments = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA)
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ZERO)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE)
                .alpha_blend_op(vk::BlendOp::ADD); 2];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
            let mut rendering = vk::PipelineRenderingCreateInfo::default()
                .color_attachment_formats(&formats)
                .depth_attachment_format(SceneTarget::DEPTH_FORMAT);
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&grid_input)
                .input_assembly_state(&assembly)
                .viewport_state(&viewport)
                .rasterization_state(&raster)
                .multisample_state(&multisample)
                .depth_stencil_state(&depth)
                .color_blend_state(&blend)
                .dynamic_state(&dynamic)
                .layout(layout)
                .push_next(&mut rendering);
            // SAFETY: as for the pipelines above.
            let created = unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&info),
                    None,
                )
            };
            match created {
                Ok(pipelines) if !pipelines.is_empty() => mesh.body_pipeline = pipelines[0],
                other => {
                    failed =
                        Some(other.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN));
                }
            }
            // Drops, spray and bubbles: a square for each, which the
            // shader makes the corners of, from two rows of four numbers
            // that are handed over once for each square.
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vertex_bit"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(c"fragment_bit"),
            ];
            let bindings = [vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(BIT_BYTES as u32)
                .input_rate(vk::VertexInputRate::INSTANCE)];
            let attributes = [0_u32, 1].map(|row| {
                vk::VertexInputAttributeDescription::default()
                    .location(row)
                    .format(vk::Format::R32G32B32A32_SFLOAT)
                    .offset(row * 16)
            });
            let bit_input = vk::PipelineVertexInputStateCreateInfo::default()
                .vertex_binding_descriptions(&bindings)
                .vertex_attribute_descriptions(&attributes);
            // Bits are tested against depth but write none, as before
            // still water wrote its own: each is a square of which only
            // the middle is a drop, and a bit drawn later behind another's
            // clear corners must still show.
            let bit_depth = vk::PipelineDepthStencilStateCreateInfo::default()
                .depth_test_enable(true)
                .depth_write_enable(false)
                .depth_compare_op(vk::CompareOp::GREATER);
            let info = info
                .stages(&stages)
                .vertex_input_state(&bit_input)
                .depth_stencil_state(&bit_depth);
            // SAFETY: as for the pipelines above.
            let created = unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&info),
                    None,
                )
            };
            match created {
                Ok(pipelines) if !pipelines.is_empty() => mesh.bit_pipeline = pipelines[0],
                other => {
                    failed =
                        Some(other.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN));
                }
            }
        }
        // What the sun sees: only how far away each surface is, so there is
        // no colour to write, and a solid surface needs no fragment shader
        // at all. The nearer surface wins, with depth running from 0 at the
        // sun. Each surface is pushed a little away from the sun, more the
        // more it slopes, so that it does not shadow itself.
        let shadow_raster = raster
            .depth_bias_enable(true)
            .depth_bias_constant_factor(2.0)
            .depth_bias_slope_factor(2.0);
        let shadow_depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
        let no_colour = vk::PipelineColorBlendStateCreateInfo::default();
        for (cut_out, pipeline) in [false, true].into_iter().zip(&mut mesh.shadow_pipelines) {
            if failed.is_some() {
                break;
            }
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vertex_shadow"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(c"fragment_shadow_cutout"),
            ];
            let stages = if cut_out { &stages[..] } else { &stages[..1] };
            let mut rendering =
                vk::PipelineRenderingCreateInfo::default().depth_attachment_format(shadow::FORMAT);
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(stages)
                .vertex_input_state(&vertex_input)
                .input_assembly_state(&assembly)
                .viewport_state(&viewport)
                .rasterization_state(&shadow_raster)
                .multisample_state(&multisample)
                .depth_stencil_state(&shadow_depth)
                .color_blend_state(&no_colour)
                .dynamic_state(&dynamic)
                .layout(layout)
                .push_next(&mut rendering);
            // SAFETY: as for the pipelines above; the module holds both
            // entry points named in `stages`.
            let created = unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&info),
                    None,
                )
            };
            match created {
                Ok(pipelines) if !pipelines.is_empty() => *pipeline = pipelines[0],
                other => {
                    failed =
                        Some(other.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN));
                }
            }
        }
        // Bounce light's: a card that cannot make them draws shade as it was
        // drawn before, and says so once.
        if failed.is_none()
            && let Some(bounce) = &mut mesh.bounce
        {
            let shared = bounce::Shared {
                assembly: &assembly,
                viewport: &viewport,
                raster: &raster,
                multisample: &multisample,
                dynamic: &dynamic,
            };
            // SAFETY: the module and layout are live and hold what
            // `bounce.wgsl` names.
            let made = unsafe { bounce.make_pipelines(module, layout, &vertex_input, &shared) };
            if let Err(code) = made {
                log::warn!(
                    "Light does not bounce: the graphics driver could not make its shaders ({code:?})."
                );
            }
        }
        // The sky: one triangle with no points of its own, drawn only where
        // nothing else was (where the depth is still the farthest), and
        // hiding nothing.
        if failed.is_none() {
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vertex_sky"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(c"fragment_sky"),
            ];
            let no_points = vk::PipelineVertexInputStateCreateInfo::default();
            let depth = vk::PipelineDepthStencilStateCreateInfo::default()
                .depth_test_enable(true)
                .depth_write_enable(false)
                .depth_compare_op(vk::CompareOp::EQUAL);
            let attachments = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA); 2];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
            let mut rendering = vk::PipelineRenderingCreateInfo::default()
                .color_attachment_formats(&formats)
                .depth_attachment_format(SceneTarget::DEPTH_FORMAT);
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&no_points)
                .input_assembly_state(&assembly)
                .viewport_state(&viewport)
                .rasterization_state(&raster)
                .multisample_state(&multisample)
                .depth_stencil_state(&depth)
                .color_blend_state(&blend)
                .dynamic_state(&dynamic)
                .layout(layout)
                .push_next(&mut rendering);
            // SAFETY: as for the pipelines above.
            let created = unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&info),
                    None,
                )
            };
            match created {
                Ok(pipelines) if !pipelines.is_empty() => mesh.sky = pipelines[0],
                other => {
                    failed =
                        Some(other.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN));
                }
            }
        }
        // The light in the air: one triangle over the finished picture,
        // which adds the glow of lamps and takes away the sun's light from
        // air in shadow. Its colour may be below zero, so it is added as it
        // is; it never takes away more than the fog gave.
        if failed.is_none() {
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vertex_air"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(c"fragment_air"),
            ];
            let no_points = vk::PipelineVertexInputStateCreateInfo::default();
            let attachments = [vk::PipelineColorBlendAttachmentState::default()
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::ONE)
                .dst_color_blend_factor(vk::BlendFactor::ONE)
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
            let colour_only = [SceneTarget::FORMAT];
            let mut rendering =
                vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&colour_only);
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&no_points)
                .input_assembly_state(&assembly)
                .viewport_state(&viewport)
                .rasterization_state(&raster)
                .multisample_state(&multisample)
                .color_blend_state(&blend)
                .dynamic_state(&dynamic)
                .layout(layout)
                .push_next(&mut rendering);
            // SAFETY: as for the pipelines above.
            let created = unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&info),
                    None,
                )
            };
            match created {
                Ok(pipelines) if !pipelines.is_empty() => mesh.air = pipelines[0],
                other => {
                    failed =
                        Some(other.map_or_else(|(_, code)| code, |_| vk::Result::ERROR_UNKNOWN));
                }
            }
        }
        let fluid = if failed.is_none() {
            let sets = [mesh.set_layout, mesh.material_layout, mesh.body_layout];
            Some(FluidWorks::new(gpu, frames, module, sets, &ranges))
        } else {
            None
        };
        // SAFETY: the module is only needed while pipelines are being created.
        unsafe { device.destroy_shader_module(module, None) };
        if let Some(code) = failed {
            return Err(RenderError::driver(doing)(code));
        }
        mesh.fluid = fluid.transpose()?;
        mesh.grid = grid::pipeline(gpu, layout)?;
        mesh.eye = Some(EyePass::new(gpu, frames)?);
        Ok(mesh)
    }

    /// Records measuring how bright `target`'s picture is, for the eye.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), after which this is recorded.
    pub(crate) unsafe fn measure(
        &self,
        commands: vk::CommandBuffer,
        frame: usize,
        target: &SceneTarget,
    ) {
        if let Some(eye) = &self.eye {
            // SAFETY: the caller's guarantees are the pass's.
            unsafe { eye.record(commands, frame, target) };
        }
    }

    /// The measuring of frame `frame`'s picture, into `bytes`, if one was
    /// recorded and not yet taken.
    ///
    /// # Errors
    ///
    /// Fails if the driver cannot show the bytes.
    ///
    /// # Safety
    ///
    /// The commands last recorded for frame `frame` must have finished.
    pub(crate) unsafe fn measured(
        &self,
        frame: usize,
        bytes: &mut Vec<u8>,
    ) -> Result<bool, RenderError> {
        match &self.eye {
            // SAFETY: the caller's guarantee is the pass's.
            Some(eye) => unsafe { eye.take(frame, bytes) },
            None => Ok(false),
        }
    }

    /// Records clearing `target` and drawing `meshes` into it as seen
    /// through `view`, and then the editor's ground grid if `grid` says
    /// where the eye is.
    ///
    /// The target's previous contents are discarded, and its three images
    /// are left ready for shaders to read.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver refuses to take the view.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this pipeline's device that is
    /// being recorded. `target` and `meshes` must belong to the same device,
    /// stay alive until the commands have run, and the target must only be
    /// in use by earlier commands on the same queue. `frame` picks which of
    /// the pipeline's frames this is, and the commands last recorded with
    /// the same `frame` must have finished.
    pub(crate) unsafe fn record(
        &self,
        commands: vk::CommandBuffer,
        frame: usize,
        target: &SceneTarget,
        view: &View,
        contents: Contents<'_>,
    ) -> Result<(), RenderError> {
        let Contents {
            meshes,
            sky,
            grid,
            stirs,
            bits,
            flashes,
            weather,
            day,
            tracks,
            fluid,
            lamps,
        } = contents;
        // How wet rain has made the ground, from 0 to 1.
        let wet_ground = weather.rows[0][2];
        // How the level's water looks, if its game has said: the first
        // row's last number is 1 when it has, and all is zero when not.
        let water_look = fluid
            .and_then(FluidScene::water)
            .map_or([[0.0; 4]; 3], |look| {
                let [takes, scatters, glows] = look.rows();
                [
                    [takes[0], takes[1], takes[2], 1.0],
                    [scatters[0], scatters[1], scatters[2], takes[3]],
                    glows,
                ]
            });
        // Fluid is worked out in pictures of its own, made now so that
        // nothing fails once rendering has begun; a frame with none draws
        // exactly what it did before.
        let fluid = match (&self.fluid, fluid) {
            // SAFETY: the caller's guarantees cover this frame's buffers
            // and set, and the target.
            (Some(works), Some(scene)) => unsafe { works.prepare(frame, scene, target) }?,
            _ => None,
        };
        // What flies is handed over before anything is recorded.
        let bit_buffer = self.bits.get(frame % self.bits.len().max(1));
        let bits = &bits[..bits.len().min(MOST_BITS * BIT_BYTES)];
        if let (Some(buffer), false) = (bit_buffer, bits.is_empty()) {
            // SAFETY: the buffer was made writable and long enough for
            // every bit there can be, and the caller guarantees the
            // commands that read it last have finished.
            unsafe { buffer.write(bits) }?;
        }
        // The grid is laid out round the eye.
        let grid = grid.then_some(view.eye);
        let device = self.gpu.device();
        let Some((view_buffer, view_set)) = self.views.get(frame % self.views.len().max(1)) else {
            return Ok(());
        };
        // Water looks through this target's own copy of the picture.
        if let Ok(mut behind) = self.behind.try_borrow_mut()
            && let Some(bound) = behind.get_mut(frame % self.views.len().max(1))
            && *bound != target.id
        {
            let pictures = [
                target.behind.view,
                target.behind_depth.view,
                target.depth.view,
            ]
            .map(|view| {
                [vk::DescriptorImageInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
            });
            // The copies water looks through, and the depth itself, which
            // the light in the air reads once the picture is drawn.
            let writes = [(5, 0_usize), (6, 1), (16, 2)].map(|(binding, picture)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(*view_set)
                    .dst_binding(binding)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&pictures[picture])
            });
            // SAFETY: the caller guarantees the commands last recorded with
            // this frame's set have finished, so it is not in use.
            unsafe { device.update_descriptor_sets(&writes, &[]) };
            *bound = target.id;
        }
        let lighting = day.map_or_else(
            || meshes.map_or_else(Lighting::default, |meshes| meshes.lighting),
            |day| day.lighting,
        );
        // With no hour said, the sun is the level's light and there is no
        // moon and no star.
        let [x, y, z] = lighting.towards_sun;
        let [red, green, blue] = lighting.sun;
        let heavens = day.map_or(
            [
                [x, y, z, 0.0],
                [0.0; 4],
                [0.0, 1.0, 0.0, 0.0],
                [red, green, blue, 0.0],
                [0.0; 4],
            ],
            |day| day.heavens,
        );
        // The sun's shadows of a level that has anything in it, drawn
        // before the picture that reads them.
        let cascades = meshes
            .filter(|meshes| meshes.buffers.is_some())
            .map(|meshes| shadow::cascades(view.eye, lighting.towards_sun, meshes.bounds));
        if let (Some(meshes), Some(cascades)) = (meshes, &cascades) {
            // SAFETY: the caller's guarantees cover the shadow pictures,
            // which are this pipeline's own.
            unsafe { self.record_shadows(commands, meshes, cascades) };
        }
        let sky = sky.then(|| {
            day.map_or_else(
                || meshes.map_or(&self.usual_sky, |meshes| &meshes.sky),
                |day| &day.sky,
            )
        });
        // Where the snow has been trodden since the last frame.
        if let Some(map) = &self.tracks {
            // SAFETY: the caller's guarantees cover the map, which is this
            // pipeline's own, and no pass has been begun yet.
            unsafe { map.record(commands, frame, tracks) }?;
        }
        // The water: its waves and rings are worked out before the picture
        // that reads them, and everything drawn is told which still water
        // the eye is in or nearest.
        // All of it stands as high as rain has raised it, each body no
        // higher than its brim.
        let rise = weather.rows[1][1];
        let near = meshes.and_then(|meshes| meshes.bodies.nearest(view.eye, rise));
        let near_rise = near.map_or(0.0, |body| body.risen(rise));
        let mut water = [[0.0; 4]; 9];
        if let Some(works) = &self.water {
            // SAFETY: the caller's guarantees cover the water's pictures,
            // which are this pipeline's own, and the body, which is the
            // meshes'.
            water = unsafe {
                works.record(
                    commands,
                    frame,
                    view.eye,
                    (view.exact, view.elapsed),
                    (
                        near,
                        meshes.is_some_and(|meshes| meshes.has_water),
                        meshes.map_or(f32::NAN, |meshes| meshes.bodies.fetch()),
                        near_rise,
                    ),
                    (stirs, weather.wind),
                )
            }?;
            // The eye is under water when it is under the water as it
            // stands now, not as the level was built.
            if near.is_some() {
                water[0][0] += near_rise;
            }
            let depths = near.map_or(works.pictures()[3], Body::depths);
            let id = near.map_or(0, |body| body.id);
            if let Ok(mut bound) = self.near_water.try_borrow_mut()
                && let Some(bound) = bound.get_mut(frame % self.views.len().max(1))
                && *bound != id
            {
                let pictures = [vk::DescriptorImageInfo::default()
                    .image_view(depths)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                let writes = [vk::WriteDescriptorSet::default()
                    .dst_set(*view_set)
                    .dst_binding(12)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&pictures)];
                // SAFETY: the caller guarantees the commands last recorded
                // with this frame's set have finished, so it is not in use.
                unsafe { device.update_descriptor_sets(&writes, &[]) };
                *bound = id;
            }
        }
        // Water sees what is under it: the picture so far is copied aside
        // for it to read, after what is solid is drawn, and drawing carries
        // on over the same picture. So do wet ground, puddles and polished
        // things, a frame late: what they mirror is this picture, read
        // while the next is drawn.
        let mirrored =
            view.keeps && (wet_ground > 0.0 || meshes.is_some_and(|meshes| meshes.glossy));
        let copies = meshes.is_some_and(|meshes| meshes.has_water)
            || !bits.is_empty()
            || fluid.is_some()
            || mirrored;
        let view = &View {
            copied: copies,
            ..*view
        };
        // SAFETY: the buffer was made writable and `VIEW_BYTES` long, and the
        // caller guarantees the commands that read it last have finished.
        unsafe {
            view_buffer.write(&view.bytes(
                &lighting,
                cascades.as_ref(),
                sky,
                &water,
                &Above {
                    flashes,
                    // The shader reads the water nearest the eye as risen
                    // as far as that water is.
                    weather: {
                        let mut seen = weather;
                        seen.rows[1][1] = near_rise;
                        seen
                    },
                    heavens,
                    water_look,
                },
            ))
        }?;
        // The lamps, sorted into the grid of this view, each drawn as bright
        // as the hour draws a lux.
        let per_lux = day.map_or(lamps::DAYLIGHT_PER_LUX, |day| day.per_lux);
        let [forward, right, up] = view.rays.map(|[x, y, z, _]| [x, y, z]);
        let seen = Seen {
            eye: view.eye,
            rays: [forward, right, up],
            size: [target.extent.width, target.extent.height],
        };
        // SAFETY: the caller's guarantees cover this frame's set, which is
        // not in use, and the atlas, which is this pipeline's own.
        let shadowing = unsafe { self.lamp_atlas_for(frame, view, lamps, meshes) }?;
        if let (Some((lamp_buffer, cell_buffer)), Ok(mut frame_lamps)) = (
            self.lamp_buffers
                .get(frame % self.lamp_buffers.len().max(1)),
            self.lamp_frame.try_borrow_mut(),
        ) {
            // With light bouncing, lamps out of view near the eye light the
            // probes round it too.
            let around = if view.bounce != Bounce::Off
                && self.bounce.as_ref().is_some_and(BounceWorks::works)
            {
                bounce::LAMPS_AROUND
            } else {
                0.0
            };
            let packed = frame_lamps.pack(lamps, &seen, per_lux, &shadowing, around);
            // SAFETY: the buffers were made writable and as long as the most
            // `pack` gives, and the caller guarantees the commands that read
            // them last have finished. With no lamp in view the cells are
            // not read, and are left as they were.
            unsafe {
                lamp_buffer.write(packed.lamps)?;
                cell_buffer.write(packed.cells)?;
            }
            // The shadows' pictures that are not drawn yet are drawn before
            // the picture that reads them.
            if let (Some(meshes), Ok(atlas)) = (meshes, self.lamp_atlas.try_borrow())
                && let Some(atlas) = atlas.as_ref()
            {
                // SAFETY: the caller's guarantees cover the atlas, which is
                // this pipeline's own, and no pass has been begun yet.
                unsafe { self.record_lamp_shadows(commands, meshes, atlas, frame_lamps.faces()) };
            }
        }
        // Bounce light: the copy of the level, the rays and the probes,
        // before the picture that reads them.
        if let Some(works) = &self.bounce {
            // SAFETY: the caller's guarantees cover this frame's set and
            // buffer, and bounce light's pictures, which are this
            // pipeline's own; no pass has been begun yet.
            unsafe { self.record_bounce(commands, frame, *view_set, works, view, meshes) }?;
        }
        let readers =
            vk::PipelineStageFlags2::FRAGMENT_SHADER | vk::PipelineStageFlags2::COMPUTE_SHADER;
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: target.extent,
        };
        let depth_stages = vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
            | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS;

        // The last frame's upscaling or tone mapping may still be reading the images.
        let colour_to_draw = |image: vk::Image| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(readers)
                .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(COLOUR)
        };
        let colour_to_read = |image: vk::Image| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(readers)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(COLOUR)
        };
        let to_draw = [
            colour_to_draw(target.colour.image),
            colour_to_draw(target.motion.image),
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(depth_stages | readers)
                .src_access_mask(vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE)
                .dst_stage_mask(depth_stages)
                .dst_access_mask(
                    vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                )
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(target.depth.image)
                .subresource_range(DEPTH),
        ];
        let to_finish = [
            colour_to_read(target.colour.image),
            colour_to_read(target.motion.image),
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(depth_stages)
                .src_access_mask(vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE)
                .dst_stage_mask(readers)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(target.depth.image)
                .subresource_range(DEPTH),
        ];

        let attachment = |view: vk::ImageView, clear: [f32; 4]| {
            vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue { float32: clear },
                })
        };
        // The background is one flat colour, so it has not visibly moved.
        let attachments = [
            attachment(target.colour.view, BACKGROUND),
            attachment(target.motion.view, [0.0; 4]),
        ];
        let depth = vk::RenderingAttachmentInfo::default()
            .image_view(target.depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: SceneTarget::FARTHEST,
                    stencil: 0,
                },
            });
        let rendering = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .color_attachments(&attachments)
            .depth_attachment(&depth);
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

        // SAFETY: the caller guarantees the command buffer is recording and
        // the target and meshes are live and of this device; every struct
        // passed lives until its call returns. The draws only name points
        // and triangles that `Meshes::new` put in the buffers.
        unsafe {
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_draw),
            );
            device.cmd_begin_rendering(commands, &rendering);
            device.cmd_set_viewport(commands, 0, &[viewport]);
            device.cmd_set_scissor(commands, 0, &[area]);
            device.cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &[*view_set],
                &[],
            );
            let drawn = meshes.and_then(|meshes| Some((meshes, meshes.buffers.as_ref()?)));
            if let Some((meshes, (vertices, indices))) = drawn {
                device.cmd_bind_vertex_buffers(commands, 0, &[vertices.handle], &[0]);
                device.cmd_bind_index_buffer(commands, indices.handle, 0, vk::IndexType::UINT32);
                let solid = &meshes.draws[..meshes.see_through_from.min(meshes.draws.len())];
                let mut last = None;
                for draw in solid {
                    self.draw(commands, meshes, draw, last);
                    last = Some(draw);
                }
            }
            // The sky fills what the solid parts left open, before anything
            // see-through is mixed over it.
            if sky.is_some() {
                device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.sky);
                device.cmd_set_cull_mode(commands, vk::CullModeFlags::NONE);
                device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
                device.cmd_draw(commands, 3, 1, 0, 0);
            }
            if copies {
                device.cmd_end_rendering(commands);
                Self::keep_behind(device, commands, target);
                target.kept.set(true);
                if let Some(fluid) = &fluid {
                    fluid.record_passes(commands, *view_set);
                }
                let kept =
                    attachments.map(|attachment| attachment.load_op(vk::AttachmentLoadOp::LOAD));
                let kept_depth = depth.load_op(vk::AttachmentLoadOp::LOAD);
                let again = vk::RenderingInfo::default()
                    .render_area(area)
                    .layer_count(1)
                    .color_attachments(&kept)
                    .depth_attachment(&kept_depth);
                device.cmd_begin_rendering(commands, &again);
                device.cmd_set_viewport(commands, 0, &[viewport]);
                device.cmd_set_scissor(commands, 0, &[area]);
                device.cmd_bind_descriptor_sets(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.layout,
                    0,
                    &[*view_set],
                    &[],
                );
                if let Some(fluid) = &fluid {
                    fluid.shade(commands);
                }
            }
            // Still water first, each body from the shape it took, the
            // coarsest of its grids first.
            if let (Some(meshes), Some((points, triangles))) =
                (meshes, self.water.as_ref().and_then(WaterWorks::grid))
                && !meshes.bodies.list.is_empty()
            {
                device.cmd_bind_pipeline(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.body_pipeline,
                );
                device.cmd_set_cull_mode(commands, vk::CullModeFlags::NONE);
                device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
                device.cmd_bind_vertex_buffers(commands, 0, &[points.handle], &[0]);
                device.cmd_bind_index_buffer(commands, triangles.handle, 0, vk::IndexType::UINT32);
                for body in &meshes.bodies.list {
                    device.cmd_bind_descriptor_sets(
                        commands,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.layout,
                        2,
                        &[body.set],
                        &[],
                    );
                    for grid in (0..body.grids(view.eye)).rev() {
                        // A grid none of this body lies in draws nothing.
                        if !body.drawn_by(view.eye, grid) {
                            continue;
                        }
                        device.cmd_push_constants(
                            commands,
                            self.layout,
                            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                            0,
                            &body_bytes(body, view.eye, grid, body.risen(rise)),
                        );
                        device.cmd_draw_indexed(commands, GRID_INDICES, 1, 0, 0, 0);
                    }
                }
            }
            if let Some((meshes, (vertices, indices))) = drawn {
                device.cmd_bind_vertex_buffers(commands, 0, &[vertices.handle], &[0]);
                device.cmd_bind_index_buffer(commands, indices.handle, 0, vk::IndexType::UINT32);
                let draws = &meshes.draws;
                let see_through = &draws[meshes.see_through_from.min(draws.len())..];
                // The sky changed what is bound, so the first of these sets
                // everything again.
                let mut last = None;
                // Farthest first, so that each is mixed over what is behind it.
                if let Ok(mut order) = self.order.try_borrow_mut() {
                    order.clear();
                    order.extend(see_through.iter().enumerate().map(|(number, draw)| {
                        let away = [0, 1, 2].map(|axis| draw.middle[axis] - view.eye[axis]);
                        (away.iter().map(|part| part * part).sum::<f32>(), number)
                    }));
                    order.sort_unstable_by(|one, other| other.0.total_cmp(&one.0));
                    for draw in order
                        .iter()
                        .filter_map(|(_, number)| see_through.get(*number))
                    {
                        self.draw(commands, meshes, draw, last);
                        last = Some(draw);
                    }
                }
            }
            // What a splash threw up, over the water it came out of.
            if let (Some(buffer), false) = (bit_buffer, bits.is_empty()) {
                device.cmd_bind_pipeline(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.bit_pipeline,
                );
                device.cmd_set_cull_mode(commands, vk::CullModeFlags::NONE);
                device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
                device.cmd_bind_vertex_buffers(commands, 0, &[buffer.handle], &[0]);
                // There is room for a few thousand at most.
                #[allow(clippy::cast_possible_truncation)]
                let count = (bits.len() / BIT_BYTES) as u32;
                device.cmd_draw(commands, 6, count, 0, 0);
            }
            // After the meshes, so that its see-through lines lie over them.
            if let Some(eye) = grid {
                device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.grid);
                device.cmd_push_constants(
                    commands,
                    self.layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &grid_bytes(eye),
                );
                device.cmd_draw(commands, 6, 1, 0, 0);
            }
            device.cmd_end_rendering(commands);
            if view.shafts && sky.is_some() {
                // The light in the air reads the finished depth and is
                // added over the colour, which stays to be drawn into.
                let [colour_done, motion_done, depth_done] = to_finish;
                let colour_again = vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                    .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                    .dst_access_mask(
                        vk::AccessFlags2::COLOR_ATTACHMENT_READ
                            | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                    )
                    .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(target.colour.image)
                    .subresource_range(COLOUR);
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default()
                        .image_memory_barriers(&[depth_done, colour_again]),
                );
                let colour = [attachments[0].load_op(vk::AttachmentLoadOp::LOAD)];
                let over = vk::RenderingInfo::default()
                    .render_area(area)
                    .layer_count(1)
                    .color_attachments(&colour);
                device.cmd_begin_rendering(commands, &over);
                device.cmd_set_viewport(commands, 0, &[viewport]);
                device.cmd_set_scissor(commands, 0, &[area]);
                device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, self.air);
                device.cmd_bind_descriptor_sets(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.layout,
                    0,
                    &[*view_set],
                    &[],
                );
                device.cmd_set_cull_mode(commands, vk::CullModeFlags::NONE);
                device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
                device.cmd_draw(commands, 3, 1, 0, 0);
                device.cmd_end_rendering(commands);
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default()
                        .image_memory_barriers(&[colour_done, motion_done]),
                );
            } else {
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_finish),
                );
            }
        }
        Ok(())
    }
}

impl MeshPipeline {
    /// Records copying the colour and depth drawn so far into the target's
    /// `behind` pictures, between two halves of the mesh pass.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), outside a render pass, with the
    /// target's colour and depth in the states the mesh pass draws in; they
    /// are left in those states.
    unsafe fn keep_behind(device: &ash::Device, commands: vk::CommandBuffer, target: &SceneTarget) {
        let depth_stages = vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
            | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS;
        let barrier = |image: vk::Image, range: vk::ImageSubresourceRange| {
            vk::ImageMemoryBarrier2::default()
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(range)
        };
        let copying = vk::PipelineStageFlags2::ALL_TRANSFER;
        let to_copy = [
            barrier(target.colour.image, COLOUR)
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(copying)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL),
            barrier(target.depth.image, DEPTH)
                .src_stage_mask(depth_stages)
                .src_access_mask(vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE)
                .dst_stage_mask(copying)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
                .old_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL),
            // How far each pixel moved, written before the pass is broken
            // off and written again after: the second half waits for the
            // first.
            barrier(target.motion.image, COLOUR)
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(
                    vk::AccessFlags2::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                )
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
            // What the copies held is thrown away; the last frame's water
            // may still be reading them.
            barrier(target.behind.image, COLOUR)
                .src_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_stage_mask(copying)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL),
            barrier(target.behind_depth.image, DEPTH)
                .src_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_stage_mask(copying)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL),
        ];
        let to_carry_on = [
            barrier(target.colour.image, COLOUR)
                .src_stage_mask(copying)
                .src_access_mask(vk::AccessFlags2::TRANSFER_READ)
                .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(
                    vk::AccessFlags2::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                )
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
            barrier(target.depth.image, DEPTH)
                .src_stage_mask(copying)
                .src_access_mask(vk::AccessFlags2::TRANSFER_READ)
                .dst_stage_mask(depth_stages)
                .dst_access_mask(
                    vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                )
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL),
            barrier(target.behind.image, COLOUR)
                .src_stage_mask(copying)
                .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
            barrier(target.behind_depth.image, DEPTH)
                .src_stage_mask(copying)
                .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
        ];
        let whole = |aspect: vk::ImageAspectFlags| {
            let layers = vk::ImageSubresourceLayers {
                aspect_mask: aspect,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };
            [vk::ImageCopy::default()
                .src_subresource(layers)
                .dst_subresource(layers)
                .extent(target.extent.into())]
        };
        // SAFETY: the caller guarantees the command buffer is recording
        // outside a render pass and that the target is this device's; each
        // image is copied in the state the barrier before left it in, and
        // each copy is the size and format of what it is copied from.
        unsafe {
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_copy),
            );
            device.cmd_copy_image(
                commands,
                target.colour.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                target.behind.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &whole(vk::ImageAspectFlags::COLOR),
            );
            device.cmd_copy_image(
                commands,
                target.depth.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                target.behind_depth.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &whole(vk::ImageAspectFlags::DEPTH),
            );
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_carry_on),
            );
        }
    }

    /// Records drawing what the sun sees of `meshes` into each of
    /// `cascades` that does not hold it already.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), before its render pass begins.
    unsafe fn record_shadows(
        &self,
        commands: vk::CommandBuffer,
        meshes: &Meshes,
        cascades: &[Cascade; PICTURES],
    ) {
        let (Some(shadows), true) = (&self.shadows, meshes.buffers.is_some()) else {
            return;
        };
        let stale = shadows.take_stale(meshes.id, cascades);
        for (layer, cascade) in cascades.iter().enumerate() {
            if !stale[layer] {
                continue;
            }
            // SAFETY: the caller's guarantees cover these.
            unsafe {
                shadows.pictures.begin(commands, layer);
                self.draw_casters(commands, meshes, &cascade.clip, |draw| {
                    cascade.holds(draw.middle, draw.radius)
                });
                shadows.pictures.end(commands, layer);
            }
        }
    }

    /// Records drawing what each lamp's picture `faces` sees of `meshes`
    /// into the lamps' atlas.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), before its render pass begins.
    unsafe fn record_lamp_shadows(
        &self,
        commands: vk::CommandBuffer,
        meshes: &Meshes,
        atlas: &LampAtlas,
        faces: &[Face],
    ) {
        for face in faces {
            if face.layer >= atlas.pictures.count() {
                continue;
            }
            // SAFETY: the caller's guarantees cover these.
            unsafe {
                atlas.pictures.begin(commands, face.layer);
                self.draw_casters(commands, meshes, &face.clip, |draw| {
                    face.holds(draw.middle, draw.radius)
                });
                atlas.pictures.end(commands, face.layer);
            }
        }
    }

    /// Records drawing how far away from a light each solid part of
    /// `meshes` that `seen` lets through is, placed in its picture by
    /// `clip`, into the depth picture being drawn.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), between the `begin` and `end` of a
    /// depth picture.
    unsafe fn draw_casters(
        &self,
        commands: vk::CommandBuffer,
        meshes: &Meshes,
        clip: &Mat4,
        seen: impl Fn(&Draw) -> bool,
    ) {
        let Some((vertices, indices)) = &meshes.buffers else {
            return;
        };
        let device = self.gpu.device();
        let casters = &meshes.draws[..meshes.see_through_from.min(meshes.draws.len())];
        // SAFETY: the caller guarantees the command buffer is recording a
        // depth picture and that the meshes are live and of this device;
        // the draws only name points, triangles and materials that
        // `Meshes::new` made.
        unsafe {
            device.cmd_bind_vertex_buffers(commands, 0, &[vertices.handle], &[0]);
            device.cmd_bind_index_buffer(commands, indices.handle, 0, vk::IndexType::UINT32);
            // Both sides cast: a wall one triangle thick, or a rock with no
            // underside, still stops the light.
            device.cmd_set_cull_mode(commands, vk::CullModeFlags::NONE);
            device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
            let mut last: Option<&Draw> = None;
            for draw in casters.iter().filter(|draw| seen(draw)) {
                let cut_out = draw.surface == Surface::CutOut;
                if last.is_none_or(|last| last.surface != draw.surface) {
                    device.cmd_bind_pipeline(
                        commands,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.shadow_pipelines[usize::from(cut_out)],
                    );
                }
                // Only a cut-out part reads its material, for its holes.
                if cut_out && last.is_none_or(|last| last.material != draw.material) {
                    device.cmd_bind_descriptor_sets(
                        commands,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.layout,
                        1,
                        &[meshes.materials.set(draw.material)],
                        &[],
                    );
                }
                device.cmd_push_constants(
                    commands,
                    self.layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &casting_bytes(&multiply(clip, &draw.world), &draw.world),
                );
                device.cmd_draw_indexed(
                    commands,
                    draw.index_count,
                    1,
                    draw.first_index,
                    draw.first_vertex,
                    0,
                );
                last = Some(draw);
            }
        }
    }

    /// Records bounce light's passes for this frame: the probes' light
    /// forgotten if the eye has jumped, the level drawn into its copy if
    /// the copy has moved, the rays of the probes whose turn it is traced,
    /// and what they found folded into the probes.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), before its render pass begins, with
    /// `set` this frame's set.
    unsafe fn record_bounce(
        &self,
        commands: vk::CommandBuffer,
        frame: usize,
        set: vk::DescriptorSet,
        works: &BounceWorks,
        view: &View,
        meshes: Option<&Meshes>,
    ) -> Result<(), RenderError> {
        let level = meshes.filter(|meshes| meshes.buffers.is_some());
        let setting = if works.works() {
            view.bounce
        } else {
            Bounce::Off
        };
        let Ok(mut grid) = works.grid.try_borrow_mut() else {
            return Ok(());
        };
        // Held still (time does not pass), the same view is drawn the same.
        let still = view.elapsed <= 0.0;
        let plan = grid.plan(
            view.eye,
            level.map_or(0, |meshes| meshes.id),
            setting,
            still,
        );
        let Some(buffer) = works.buffers.get(frame % works.buffers.len().max(1)) else {
            return Ok(());
        };
        // SAFETY: the buffer was made writable and as long as the most a
        // plan gives, and the caller guarantees the commands that read it
        // last have finished.
        unsafe { buffer.write(plan.bytes) }?;
        let (listed, rays, voxelize, clear) = (plan.listed, plan.rays, plan.voxelize, plan.clear);
        let voxel_box = grid.voxel_box();
        drop(grid);
        let (Some(meshes), true) = (level, listed > 0) else {
            return Ok(());
        };
        let device = self.gpu.device();
        let barrier = |barriers: &[vk::ImageMemoryBarrier2<'_>]| {
            // SAFETY: the caller guarantees the command buffer is recording
            // outside a render pass; the images are bounce light's own.
            unsafe {
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(barriers),
                );
            }
        };
        let read =
            vk::PipelineStageFlags2::FRAGMENT_SHADER | vk::PipelineStageFlags2::VERTEX_SHADER;
        let drawing = vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT;
        let readable = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        let atlases = [works.light.image, works.depth.image];
        // SAFETY: as above; every struct lives until its call returns.
        unsafe {
            device.cmd_bind_descriptor_sets(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &[set],
                &[],
            );
            device.cmd_set_cull_mode(commands, vk::CullModeFlags::NONE);
            device.cmd_set_front_face(commands, vk::FrontFace::COUNTER_CLOCKWISE);
        }
        if clear {
            barrier(&atlases.map(|image| {
                bounce::whole(image, readable, vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .src_stage_mask(read)
                    .dst_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            }));
            for image in atlases {
                // SAFETY: as above.
                unsafe {
                    device.cmd_clear_color_image(
                        commands,
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearColorValue { float32: [0.0; 4] },
                        &[colour_range()],
                    );
                }
            }
            barrier(&atlases.map(|image| {
                bounce::whole(image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, readable)
                    .src_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(read)
                    .dst_access_mask(vk::AccessFlags2::SHADER_READ)
            }));
        }
        let general = vk::ImageLayout::GENERAL;
        if voxelize && let Some([low, high]) = voxel_box {
            let blocks = [works.voxels.image, works.normals.image];
            barrier(&blocks.map(|image| {
                bounce::whole(image, general, general)
                    .src_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            }));
            for image in blocks {
                // SAFETY: as above.
                unsafe {
                    device.cmd_clear_color_image(
                        commands,
                        image,
                        general,
                        &vk::ClearColorValue { float32: [0.0; 4] },
                        &[colour_range()],
                    );
                }
            }
            barrier(&blocks.map(|image| {
                bounce::whole(image, general, general)
                    .src_stage_mask(vk::PipelineStageFlags2::CLEAR)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags2::SHADER_STORAGE_WRITE)
            }));
            let side = bounce::VOXEL_PICTURE;
            // SAFETY: as above; the meshes are live and of this device.
            unsafe {
                begin_drawing(device, commands, side, side, &[]);
                device.cmd_bind_pipeline(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    works.voxel_pipeline,
                );
                self.draw_level(commands, meshes, |draw| {
                    (0..3).all(|axis| {
                        draw.middle[axis] + draw.radius >= low[axis]
                            && draw.middle[axis] - draw.radius <= high[axis]
                    })
                });
                device.cmd_end_rendering(commands);
            }
            barrier(&blocks.map(|image| {
                bounce::whole(image, general, general)
                    .src_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .src_access_mask(vk::AccessFlags2::SHADER_STORAGE_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags2::SHADER_READ)
            }));
        }

        // The rays: what was there is thrown away.
        let rays_layout = vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;
        barrier(&[
            bounce::whole(works.rays.image, vk::ImageLayout::UNDEFINED, rays_layout)
                .src_stage_mask(read)
                .dst_stage_mask(drawing)
                .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE),
        ]);
        let rays_wide = rays;
        let rays = [vk::RenderingAttachmentInfo::default()
            .image_view(works.rays.view)
            .image_layout(rays_layout)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE)];
        // SAFETY: as above.
        unsafe {
            begin_drawing(device, commands, rays_wide, listed, &rays);
            device.cmd_bind_pipeline(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                works.trace_pipeline,
            );
            device.cmd_draw(commands, 3, 1, 0, 0);
            device.cmd_end_rendering(commands);
        }
        barrier(&[bounce::whole(works.rays.image, rays_layout, readable)
            .src_stage_mask(drawing)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
            .dst_access_mask(vk::AccessFlags2::SHADER_READ)]);

        // The probes: blended into what they held.
        barrier(&atlases.map(|image| {
            bounce::whole(image, readable, rays_layout)
                .src_stage_mask(read)
                .dst_stage_mask(drawing)
                .dst_access_mask(
                    vk::AccessFlags2::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                )
        }));
        let probes = [works.light.view, works.depth.view].map(|view| {
            vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(rays_layout)
                .load_op(vk::AttachmentLoadOp::LOAD)
                .store_op(vk::AttachmentStoreOp::STORE)
        });
        // SAFETY: as above.
        unsafe {
            begin_drawing(
                device,
                commands,
                bounce::ATLAS[0],
                bounce::ATLAS[1],
                &probes,
            );
            device.cmd_bind_pipeline(
                commands,
                vk::PipelineBindPoint::GRAPHICS,
                works.probe_pipeline,
            );
            device.cmd_draw(commands, 6, listed, 0, 0);
            device.cmd_end_rendering(commands);
        }
        barrier(&atlases.map(|image| {
            bounce::whole(image, rays_layout, readable)
                .src_stage_mask(drawing)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(read)
                .dst_access_mask(vk::AccessFlags2::SHADER_READ)
        }));
        Ok(())
    }

    /// Records drawing each solid part of `meshes` that `seen` lets
    /// through, each three times (the instance says which axis it is seen
    /// along), placed in the world as it stands, with its material.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), inside a pass, with a pipeline of
    /// this pipeline's layout bound.
    unsafe fn draw_level(
        &self,
        commands: vk::CommandBuffer,
        meshes: &Meshes,
        seen: impl Fn(&Draw) -> bool,
    ) {
        let Some((vertices, indices)) = &meshes.buffers else {
            return;
        };
        let device = self.gpu.device();
        let solid = &meshes.draws[..meshes.see_through_from.min(meshes.draws.len())];
        // SAFETY: the caller guarantees the command buffer is recording a
        // pass and that the meshes are live and of this device; the draws
        // only name points, triangles and materials that `Meshes::new` made.
        unsafe {
            device.cmd_bind_vertex_buffers(commands, 0, &[vertices.handle], &[0]);
            device.cmd_bind_index_buffer(commands, indices.handle, 0, vk::IndexType::UINT32);
            let mut last: Option<&Draw> = None;
            for draw in solid.iter().filter(|draw| seen(draw)) {
                if last.is_none_or(|last| last.material != draw.material) {
                    device.cmd_bind_descriptor_sets(
                        commands,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.layout,
                        1,
                        &[meshes.materials.set(draw.material)],
                        &[],
                    );
                }
                device.cmd_push_constants(
                    commands,
                    self.layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    &draw_bytes(draw),
                );
                device.cmd_draw_indexed(
                    commands,
                    draw.index_count,
                    3,
                    draw.first_index,
                    draw.first_vertex,
                    0,
                );
                last = Some(draw);
            }
        }
    }

    /// Whether every probe of bounce light has traced since it came into
    /// the grid: a picture held still is drawn again until it has.
    pub(crate) fn bounce_settled(&self) -> bool {
        self.bounce
            .as_ref()
            .is_none_or(|works| works.grid.try_borrow().is_ok_and(|grid| grid.settled()))
    }

    /// Makes the lamps' atlas as `view` asks, or frees it, and points this
    /// frame's set at it; then says how this frame's lamps may cast
    /// shadows. Making it waits for the graphics card to finish what it was
    /// given before, which happens only when the setting changes or a lamp
    /// first needs a shadow.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), before its render pass begins.
    unsafe fn lamp_atlas_for(
        &self,
        frame: usize,
        view: &View,
        lamps: &[Lamp],
        meshes: Option<&Meshes>,
    ) -> Result<Shadowing, RenderError> {
        let device = self.gpu.device();
        let (Ok(mut atlas), Ok(mut bound)) = (
            self.lamp_atlas.try_borrow_mut(),
            self.lamp_bound.try_borrow_mut(),
        ) else {
            return Ok(Shadowing::default());
        };
        let wanted = view.lamp_shadows;
        let level = meshes.filter(|meshes| meshes.buffers.is_some());
        let needed = wanted.lamps() > 0
            && level.is_some()
            && lamps.iter().any(|lamp| lamp.casts_shadows)
            && self.lamp_atlas_failed.get() != Some(wanted);
        let fits = atlas.as_ref().map(|atlas| atlas.shadows);
        // An atlas made for another setting is freed; one that is not
        // needed yet is not made, but one already made is kept for the
        // lamps that come and go.
        if fits.is_some_and(|fits| fits != wanted) || (fits.is_some() && wanted.lamps() == 0) {
            // SAFETY: waiting for the device is always allowed; once it is
            // idle nothing reads the atlas, so it may be freed.
            unsafe { device.device_wait_idle() }
                .map_err(RenderError::driver("making room for the lamps' shadows"))?;
            *atlas = None;
            // A new view may be given the freed one's handle: every set is
            // pointed again.
            bound.fill(vk::ImageView::null());
        }
        if atlas.is_none() && needed {
            // SAFETY: as above.
            unsafe { device.device_wait_idle() }
                .map_err(RenderError::driver("making room for the lamps' shadows"))?;
            match LampAtlas::new(&self.gpu, wanted) {
                Ok(made) => *atlas = Some(made),
                // A card without the memory for them draws lamps with no
                // shadows, as the setting "off" does, until the setting
                // changes.
                Err(error) => {
                    log::warn!(
                        "Lamps cast no shadows: {error} Set \"Lamp shadows\" lower in the \
                         graphics settings to try again."
                    );
                    self.lamp_atlas_failed.set(Some(wanted));
                }
            }
            bound.fill(vk::ImageView::null());
        }
        let slot = frame % self.views.len().max(1);
        let pictures = atlas.as_ref().map_or_else(
            || self.shadows.as_ref().map(|shadows| shadows.pictures.view),
            |atlas| Some(atlas.pictures.view),
        );
        if let (Some(pictures), Some((_, set)), Some(bound)) =
            (pictures, self.views.get(slot), bound.get_mut(slot))
            && *bound != pictures
        {
            let info = [vk::DescriptorImageInfo::default()
                .image_view(pictures)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let writes = [vk::WriteDescriptorSet::default()
                .dst_set(*set)
                .dst_binding(17)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(&info)];
            // SAFETY: the caller guarantees the commands last recorded with
            // this frame's set have finished, so it is not in use.
            unsafe { device.update_descriptor_sets(&writes, &[]) };
            *bound = pictures;
        }
        // With no level drawn this frame, nothing in the atlas may be
        // read: it holds what an earlier level's lamps saw.
        Ok(match (atlas.as_ref(), level) {
            (Some(atlas), Some(level)) => Shadowing {
                slots: atlas.shadows.lamps(),
                level: level.id,
                elapsed: view.elapsed,
            },
            _ => Shadowing::default(),
        })
    }

    /// Records drawing one mesh part, changing only what differs from the
    /// part drawn before it.
    ///
    /// # Safety
    ///
    /// As for [`record`](Self::record), inside its render pass, with the
    /// buffers of `meshes` bound; `draw` and `last` must be parts of `meshes`.
    unsafe fn draw(
        &self,
        commands: vk::CommandBuffer,
        meshes: &Meshes,
        draw: &Draw,
        last: Option<&Draw>,
    ) {
        let device = self.gpu.device();
        let changed =
            |differs: fn(&Draw, &Draw) -> bool| last.is_none_or(|last| differs(last, draw));
        // SAFETY: the caller guarantees the command buffer is recording
        // inside the mesh pass and that `draw` names points, triangles and a
        // material that `meshes` holds on this device.
        unsafe {
            if changed(|last, draw| last.surface != draw.surface) {
                let pipeline = self.pipelines[draw.surface as usize];
                device.cmd_bind_pipeline(commands, vk::PipelineBindPoint::GRAPHICS, pipeline);
            }
            if changed(|last, draw| last.material != draw.material) {
                device.cmd_bind_descriptor_sets(
                    commands,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.layout,
                    1,
                    &[meshes.materials.set(draw.material)],
                    &[],
                );
            }
            // Triangles are listed anticlockwise seen from the front; a
            // mirrored mesh is the other way round.
            if changed(|last, draw| last.mirrored != draw.mirrored) {
                device.cmd_set_front_face(
                    commands,
                    if draw.mirrored {
                        vk::FrontFace::CLOCKWISE
                    } else {
                        vk::FrontFace::COUNTER_CLOCKWISE
                    },
                );
            }
            if changed(|last, draw| last.double_sided != draw.double_sided) {
                device.cmd_set_cull_mode(
                    commands,
                    if draw.double_sided {
                        vk::CullModeFlags::NONE
                    } else {
                        vk::CullModeFlags::BACK
                    },
                );
            }
            device.cmd_push_constants(
                commands,
                self.layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                &draw_bytes(draw),
            );
            device.cmd_draw_indexed(
                commands,
                draw.index_count,
                1,
                draw.first_index,
                draw.first_vertex,
                0,
            );
        }
    }
}

/// Pointing binding `binding` of `set` at the picture `info` names.
/// Records the start of drawing into `attachments` over an area `width` by
/// `height` pixels from the corner, with the viewport and scissor set to it.
///
/// # Safety
///
/// `commands` must be recording outside a render pass, and the attachments
/// must be in the layouts they say.
unsafe fn begin_drawing(
    device: &ash::Device,
    commands: vk::CommandBuffer,
    width: u32,
    height: u32,
    attachments: &[vk::RenderingAttachmentInfo<'_>],
) {
    let area = vk::Rect2D {
        offset: vk::Offset2D::default(),
        extent: vk::Extent2D { width, height },
    };
    // Sizes are far below where an f32 stops being exact.
    #[allow(clippy::cast_precision_loss)]
    let viewport = vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    };
    // SAFETY: the caller guarantees the command buffer and attachments.
    unsafe {
        device.cmd_begin_rendering(
            commands,
            &vk::RenderingInfo::default()
                .render_area(area)
                .layer_count(1)
                .color_attachments(attachments),
        );
        device.cmd_set_viewport(commands, 0, &[viewport]);
        device.cmd_set_scissor(commands, 0, &[area]);
    }
}

fn picture_write(
    set: vk::DescriptorSet,
    binding: u32,
    info: &[vk::DescriptorImageInfo; 1],
) -> vk::WriteDescriptorSet<'_> {
    vk::WriteDescriptorSet::default()
        .dst_set(set)
        .dst_binding(binding)
        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
        .image_info(info)
}

/// The shader's `Draw` for one of the grids a body of still water is drawn
/// with as seen from `eye`: grid 0 is the finest, and each one after it is
/// twice as wide with its points twice as far apart.
///
/// Where the mesh shader keeps how a mesh stands, this keeps how the grid
/// is laid (how far apart its points are, whether it is the finest, how
/// many squares it has a side, where the next finer grid's middle is,
/// round which this one is left out, and where its own middle is: the
/// eye, to the nearest two points, so that the grid moves by whole squares
/// and its points stay where they were); and where the shader keeps how
/// surface directions turn, the body itself: where its grid of depths
/// starts and how far apart its points are, how many there are and how
/// high the water stands, risen `rise` metres by rain, and the water's
/// colour. Its clearness is the number after. The rise itself is kept
/// beside where the finer grid's middle is.
fn body_bytes(body: &Body, eye: [f32; 3], grid: u32, rise: f32) -> [u8; DRAW_BYTES] {
    let (apart, middle) = grid_laid(eye, grid);
    let finer = grid
        .checked_sub(1)
        .map_or(middle, |finer| grid_laid(eye, finer).1);
    // Sizes are far below the point where f32 stops being exact.
    #[allow(clippy::cast_precision_loss)]
    let (squares, size) = (GRID_SQUARES as f32, body.size.map(|points| points as f32));
    let finest = if grid == 0 { 1.0 } else { 0.0 };
    let numbers = [
        [apart, finest, squares, 0.0],
        [finer[0], finer[1], rise, 0.0],
        [0.0; 4],
        [middle[0], body.level + rise, middle[1], 1.0],
        [body.origin[0], body.origin[1], body.cell, 0.0],
        [size[0], size[1], body.level + rise, 0.0],
        [body.colour[0], body.colour[1], body.colour[2], 0.0],
        [body.colour[3], 0.0, 0.0, 0.0],
    ];
    let mut bytes = [0_u8; DRAW_BYTES];
    for (slot, number) in bytes
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(numbers.iter().flatten())
    {
        *slot = number.to_ne_bytes();
    }
    bytes
}

/// How many bytes the shader's `Casting` takes: a matrix and three rows.
const CASTING_BYTES: usize = 16 * 4 + 16 * 3;

/// The shader's `Casting` for one mesh part drawn into a shadow picture:
/// `picture` takes its points to the picture, and `world` to where they
/// stand, of which the first three rows are kept.
fn casting_bytes(picture: &Mat4, world: &Mat4) -> [u8; CASTING_BYTES] {
    let rows = [0, 1, 2].map(|row| world.map(|column| column[row]));
    let numbers = picture.iter().chain(&rows).flatten();
    let mut bytes = [0_u8; CASTING_BYTES];
    for (slot, number) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(numbers) {
        *slot = number.to_ne_bytes();
    }
    bytes
}

/// The shader's `Draw` for one mesh part.
fn draw_bytes(draw: &Draw) -> [u8; DRAW_BYTES] {
    // A mirrored mesh has its normal map's sideways direction turned around.
    let handedness = [if draw.mirrored { -1.0 } else { 1.0 }, draw.sway];
    let numbers = draw
        .world
        .iter()
        .chain(&draw.normal)
        .flatten()
        .chain(&handedness);
    let mut bytes = [0_u8; DRAW_BYTES];
    for (slot, number) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(numbers) {
        *slot = number.to_ne_bytes();
    }
    bytes
}

impl Drop for MeshPipeline {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping the pipeline, so it is not in use. Destroying the pool
        // frees its sets.
        unsafe {
            let device = self.gpu.device();
            for pipeline in self.pipelines.into_iter().chain(self.shadow_pipelines) {
                device.destroy_pipeline(pipeline, None);
            }
            device.destroy_pipeline(self.body_pipeline, None);
            device.destroy_pipeline(self.bit_pipeline, None);
            device.destroy_pipeline(self.sky, None);
            device.destroy_pipeline(self.air, None);
            device.destroy_pipeline(self.grid, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            device.destroy_descriptor_set_layout(self.material_layout, None);
            device.destroy_descriptor_set_layout(self.body_layout, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use lore_assets::Light;

    use super::*;

    fn triangle() -> MeshPart {
        MeshPart {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
            ..MeshPart::default()
        }
    }

    /// What the shader's `unfolded` makes of a folded direction.
    fn unfolded([x, y]: [i16; 2]) -> [f32; 3] {
        let (x, y) = (f32::from(x) / 32767.0, f32::from(y) / 32767.0);
        let z = 1.0 - x.abs() - y.abs();
        let under = (-z).max(0.0);
        let (x, y) = (x - under.copysign(x), y - under.copysign(y));
        let length = (x * x + y * y + z * z).sqrt();
        [x / length, y / length, z / length]
    }

    #[test]
    fn a_folded_direction_unfolds_to_itself() {
        let mut directions = vec![
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        // And a spread of others, all round.
        for step in 0..200_u16 {
            let turn = f32::from(step) * 2.399_963;
            let height = 1.0 - f32::from(step) / 99.5;
            let across = (1.0 - height * height).max(0.0).sqrt();
            directions.push([across * turn.cos(), height, across * turn.sin()]);
        }
        for direction in directions {
            let back = unfolded(folded(direction));
            let apart: f32 = (0..3)
                .map(|axis| (back[axis] - direction[axis]).abs())
                .sum();
            assert!(apart < 2e-4, "{direction:?} came back as {back:?}");
        }
        // A direction that is no direction is still one the shader can use.
        assert_eq!(unfolded(folded([0.0; 3])), [0.0, 0.0, 1.0]);
        assert_eq!(unfolded(folded([f32::NAN; 3])), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn parts_are_laid_end_to_end() {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        assert_eq!(
            append(&triangle(), &mut vertices, &mut indices),
            Some((0, 3, 0))
        );
        let textured = MeshPart {
            tangents: Some(vec![[1.0, 0.0, 0.0, -1.0]; 3]),
            uv0: Some(vec![[0.25, 0.5]; 3]),
            uv1: Some(vec![[2.0, 3.0]; 3]),
            colors: Some(vec![[1.0, 0.5, 0.0, 2.0]; 3]),
            ..triangle()
        };
        assert_eq!(
            append(&textured, &mut vertices, &mut indices),
            Some((3, 3, 3))
        );
        assert_eq!(vertices.len(), 6 * VERTEX_BYTES);
        assert_eq!(indices.len(), 6 * 4);
        // A point with no tangent, colour or UV of its own: no bend, white,
        // so the material's colour shows, and the texture's corner.
        assert_eq!(vertices[16..20], [0, 0, 0, 127]);
        assert_eq!(vertices[20..24], [255; 4]);
        assert!(vertices[24..40].iter().all(|&byte| byte == 0));
        // One with all of them, in the order the pipeline reads them.
        let second = &vertices[3 * VERTEX_BYTES..];
        assert_eq!(
            second[12..16],
            [0; 4],
            "a normal along +Z folds to the middle"
        );
        assert_eq!(second[16..20], [127, 0, 0, 129]);
        // A colour brighter than white is white; red, green and blue are
        // on the screen's curve, where half the light is 188, and alpha,
        // which is a share and not a light, is not.
        assert_eq!(second[20..24], [255, 188, 0, 255]);
        assert_eq!(second[24..28], 0.25_f32.to_ne_bytes());
        assert_eq!(second[36..40], 3.0_f32.to_ne_bytes());
    }

    #[test]
    fn dark_point_colours_keep_their_steps() {
        // The darkest hundredth of light has two dozen steps of the byte,
        // where as light itself it would have three.
        assert!(srgb8(0.01) >= 25 && unorm8(0.01) <= 3);
        // Each byte is the one its own light comes back to, so nothing is
        // lost on the way there and back.
        let mut before = 0;
        for byte in 0..=255_u8 {
            let light = f32::from(byte) / 255.0;
            // As the shader takes it back.
            let back = if light <= 0.040_45 {
                light / 12.92
            } else {
                ((light + 0.055) / 1.055).powf(2.4)
            };
            assert_eq!(srgb8(back), byte, "{byte} came back as {}", srgb8(back));
            assert!(byte == 0 || srgb8(back) > before);
            before = srgb8(back);
        }
        assert_eq!((srgb8(-1.0), srgb8(f32::NAN), srgb8(2.0)), (0, 0, 255));
    }

    #[test]
    fn a_part_that_does_not_hold_together_is_left_out() {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let beyond = MeshPart {
            indices: vec![0, 1, 3],
            ..triangle()
        };
        let short = MeshPart {
            normals: vec![[0.0, 0.0, 1.0]; 2],
            ..triangle()
        };
        let colours = MeshPart {
            colors: Some(vec![[1.0; 4]; 2]),
            ..triangle()
        };
        let tangents = MeshPart {
            tangents: Some(vec![[1.0; 4]; 4]),
            ..triangle()
        };
        let uv = MeshPart {
            uv1: Some(vec![[1.0; 2]; 1]),
            ..triangle()
        };
        let empty = MeshPart::default();
        for part in [beyond, short, colours, tangents, uv, empty] {
            assert_eq!(append(&part, &mut vertices, &mut indices), None);
        }
        assert!(vertices.is_empty() && indices.is_empty());

        // Two numbers left over after the last whole triangle are dropped.
        let ragged = MeshPart {
            indices: vec![0, 1, 2, 0, 1],
            ..triangle()
        };
        assert_eq!(
            append(&ragged, &mut vertices, &mut indices),
            Some((0, 3, 0))
        );
    }

    fn number(bytes: &[u8], at: usize) -> f32 {
        f32::from_ne_bytes([0, 1, 2, 3].map(|byte| bytes[at * 4 + byte]))
    }

    #[test]
    fn a_draw_fills_the_shader_block_exactly() {
        let mut world = [[0.0; 4]; 4];
        for (axis, column) in world.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        let mut draw = Draw {
            world,
            normal: normal_matrix(&world),
            material: 0,
            surface: Surface::Solid,
            middle: [0.0; 3],
            radius: 1.0,
            first_index: 0,
            index_count: 3,
            first_vertex: 0,
            mirrored: false,
            double_sided: false,
            sway: 0.5,
        };
        let bytes = draw_bytes(&draw);
        assert_eq!(number(&bytes, 0), 1.0, "the matrix comes first");
        assert_eq!(number(&bytes, 16), 1.0, "then the surface directions");
        assert_eq!(number(&bytes, 28), 1.0, "then which way round it is");
        draw.mirrored = true;
        assert_eq!(number(&draw_bytes(&draw), 28), -1.0);
        assert_eq!(
            number(&draw_bytes(&draw), 29),
            0.5,
            "and how far the wind moves it"
        );
    }

    #[test]
    fn a_caster_says_where_it_stands_in_the_world_by_rows() {
        let mut picture = [[0.0; 4]; 4];
        picture[3][3] = 2.0;
        let mut world = [[0.0; 4]; 4];
        for (axis, column) in world.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        // Moved to 5, 6, 7 and half as wide along x.
        world[0][0] = 0.5;
        world[3] = [5.0, 6.0, 7.0, 1.0];
        let bytes = casting_bytes(&picture, &world);
        assert!(bytes.len() <= DRAW_BYTES, "it fits where a draw does");
        assert_eq!(number(&bytes, 15), 2.0, "the picture's matrix comes first");
        // Then each row: a point's x is the first row against (x, y, z, 1).
        assert_eq!(
            [16, 19, 20, 23, 24, 27].map(|at| number(&bytes, at)),
            [0.5, 5.0, 0.0, 6.0, 0.0, 7.0]
        );
        assert_eq!(number(&bytes, 21), 1.0);
    }

    #[test]
    fn a_view_fills_the_shader_block_exactly() {
        let mut matrix = [[0.0; 4]; 4];
        matrix[3][3] = 7.0;
        let view = View {
            clip: matrix,
            steady: matrix,
            before: matrix,
            eye: [1.0, 2.0, 3.0],
            sharper: 1.5,
            rays: [
                [1.0, 2.0, 3.0, 4.0],
                [5.0, 6.0, 7.0, 8.0],
                [9.0, 10.0, 11.0, 12.0],
            ],
            time: 13.0,
            exact: 13.0,
            elapsed: 0.25,
            stir: 0.5,
            noise: 9.0,
            near: 0.05,
            reflecting: true,
            mirrors: true,
            kept: {
                let mut kept = [[0.0; 4]; 4];
                kept[3][3] = 11.0;
                kept
            },
            keeps: true,
            copied: true,
            shafts: false,
            lamp_shadows: LampShadows::Off,
            bounce: Bounce::Off,
        };
        let lighting = Lighting {
            towards_sun: [0.0, 1.0, 0.0],
            sun: [4.0, 5.0, 6.0],
            sky: [0.1, 0.2, 0.3],
            ground: [0.4, 0.5, 0.6],
        };
        let bytes = view.bytes(&lighting, None, None, &[[0.0; 4]; 9], &Above::default());
        assert_eq!(
            number(&bytes, 47),
            7.0,
            "the last number of the third matrix"
        );
        // The eye, and textures read one and a half sizes larger.
        assert_eq!(
            [48, 49, 50, 51].map(|at| number(&bytes, at)),
            [1.0, 2.0, 3.0, -1.5]
        );
        assert_eq!([52, 53, 54].map(|at| number(&bytes, at)), [0.0, 1.0, 0.0]);
        assert_eq!([56, 57, 58].map(|at| number(&bytes, at)), [4.0, 5.0, 6.0]);
        assert_eq!([60, 61, 62].map(|at| number(&bytes, at)), [0.1, 0.2, 0.3]);
        assert_eq!([64, 65, 66].map(|at| number(&bytes, at)), [0.4, 0.5, 0.6]);

        // The shadows follow: one row, a row for each picture, then a
        // matrix for each.
        assert_eq!(number(&bytes, 68), 0.0, "no shadows");
        let cascades = shadow::cascades([0.0; 3], [0.0, 1.0, 0.0], [[-1.0; 3], [1.0; 3]]);
        let bytes = view.bytes(
            &lighting,
            Some(&cascades),
            None,
            &[[0.0; 4]; 9],
            &Above::default(),
        );
        assert_eq!(number(&bytes, 68), 1.0, "shadows");
        assert_eq!(number(&bytes, 71), 9.0, "the frame's noise");
        assert_eq!(number(&bytes, 72), cascades[0].texel);
        assert_eq!(number(&bytes, 73), cascades[0].depth);
        assert_eq!(number(&bytes, 84), cascades[3].texel);
        assert_eq!(
            number(&bytes, 88),
            cascades[4].texel,
            "the picture from above"
        );
        assert_eq!(number(&bytes, 92), cascades[0].clip[0][0]);
        assert_eq!(
            number(&bytes, 171),
            1.0,
            "the last number of the last matrix"
        );
        assert_eq!(bytes.len(), 172 * 4 + SKY_BYTES + 64);

        // Then the sky: the lines of sight, the clouds, and the light the
        // air and the haze scatter, a row for each height.
        assert_eq!(number(&bytes, 172), 1.0);
        assert_eq!(number(&bytes, 183), 12.0);
        assert_eq!(number(&bytes, 184), 0.0, "no clouds without a sky");
        assert_eq!(number(&bytes, 185), 13.0, "the time");
        assert_eq!(number(&bytes, 186), sky::CLOUD_HEIGHT);
        assert_eq!(number(&bytes, 187), 0.0, "no sky");
        let above = Sky::of(&lighting);
        // The water: the body the eye is nearest, the layers of waves,
        // the rings, and the eye's own, which the view fills in itself.
        let mut water = [[0.0; 4]; 9];
        water[0] = [2.0, 0.25, 1.0, 0.8];
        water[1] = [3.0, 4.0, 5.0, 6.0];
        water[3] = [5.4, 0.01, 0.17, 0.68];
        water[7] = [-6.0, -7.0, 0.025, 512.0];
        water[8] = [99.0; 4];
        let bytes = view.bytes(
            &lighting,
            None,
            Some(&above),
            &water,
            &Above {
                flashes: [[1.0, 2.0, 3.0, 0.5], [0.0; 4]],
                weather: Air {
                    rows: [
                        [0.25, 0.0, 0.5, 1.0],
                        [0.75, 0.0, 0.0, 0.0],
                        [0.0; 4],
                        [0.0; 4],
                        [0.0; 4],
                        [0.3, 0.0, 0.0, 0.0],
                        [0.4, 0.0, 0.0, 0.0],
                    ],
                    cover: 0.6,
                    fog: 0.002,
                    wind: [0.0, 1.0, 9.0],
                },
                heavens: [[9.0; 4]; 5],
                water_look: [[0.5; 4], [0.25; 4], [0.75; 4]],
            },
        );
        assert_eq!(number(&bytes, 184), 0.6, "how much of the sky is cloud");
        assert_eq!(number(&bytes, 187), 1.0, "a sky");
        assert_eq!(number(&bytes, 188), above.air[0][0]);
        assert_eq!(number(&bytes, 280), above.air[23][0]);
        assert_eq!(number(&bytes, 284), above.haze[0][0]);
        assert_eq!(number(&bytes, 378), above.haze[23][2]);
        assert_eq!(number(&bytes, 380), 0.002, "the fog");
        assert_eq!(number(&bytes, 381), sky::FOG[1]);
        assert_eq!(number(&bytes, 385), 1.0, "the wind");
        assert_eq!(number(&bytes, 386), 9.0);
        assert_eq!(number(&bytes, 387), 0.25, "since the last frame");
        assert_eq!(number(&bytes, 388), 0.5, "how much the walker stirs");
        assert_eq!(
            number(&bytes, 389),
            1.0,
            "and that smooth things mirror the world"
        );
        assert_eq!(
            number(&bytes, 390),
            1.0,
            "this frame's picture too, once copied"
        );
        assert_eq!(number(&bytes, 392), 2.0, "the water's surface");
        assert_eq!(number(&bytes, 399), 6.0, "and how many points its grid has");
        assert_eq!(number(&bytes, 404), 5.4, "the first layer of waves");
        assert_eq!(number(&bytes, 423), 512.0, "the rings");
        assert_eq!(number(&bytes, 424), 0.05, "how near the eye sees");
        assert_eq!(number(&bytes, 428), 1.0, "where lightning's light is");
        assert_eq!(number(&bytes, 431), 0.5, "and how bright");
        assert_eq!(number(&bytes, 436), 0.25, "how hard it rains");
        assert_eq!(number(&bytes, 440), 0.75, "how stormy the sky is");
        assert_eq!(number(&bytes, 456), 0.3, "how strong the tornado is");
        assert_eq!(number(&bytes, 460), 0.4, "how deep the snow lies");
        assert_eq!(number(&bytes, 464), 9.0, "the sun, the moon and the stars");
        assert_eq!(number(&bytes, 484), 0.5, "the look of the water");
        assert_eq!(number(&bytes, 495), 0.75, "and how much it bends light");
        assert_eq!(
            number(&bytes, 511),
            11.0,
            "last, the matrix of the picture kept for them to mirror"
        );
        assert_eq!(bytes.len(), 512 * 4);
    }

    #[test]
    fn the_usual_light_shows_a_surface_facing_up_in_its_own_colour() {
        let usual = Lighting::default();
        let [x, y, z] = usual.towards_sun;
        assert!((x * x + y * y + z * z - 1.0).abs() < 1e-4);
        // The sun's light on the ground, as a matte white surface shows it,
        // and the sky's: together exactly white.
        for (sun, sky) in usual.sun.iter().zip(usual.sky) {
            let shown = sun * y / std::f32::consts::PI + sky;
            assert!((shown - 1.0).abs() < 1e-5, "{shown}");
        }
    }

    #[test]
    fn a_level_with_a_sun_is_lit_by_it() {
        let light = |kind, direction| Light {
            name: None,
            kind,
            color: [1.0, 0.5, 0.25],
            intensity: 2.0,
            range: None,
            position: [0.0; 3],
            direction,
        };
        let mut scene = Scene::default();
        assert_eq!(Lighting::of(&scene), Lighting::default());

        // A lamp is not a sun, and a sun that points nowhere cannot be used.
        scene.lights.push(light(LightKind::Point, [0.0, -1.0, 0.0]));
        scene.lights.push(light(LightKind::Sun, [0.0; 3]));
        assert_eq!(Lighting::of(&scene), Lighting::default());

        // The first sun that can be: it shines down, so it is found up.
        scene.lights.push(light(LightKind::Sun, [0.0, -2.0, 0.0]));
        scene.lights.push(light(LightKind::Sun, [1.0, 0.0, 0.0]));
        let lighting = Lighting::of(&scene);
        assert_eq!(lighting.towards_sun, [0.0, 1.0, 0.0]);
        assert_eq!(lighting.sun, [2.0, 1.0, 0.5]);
        assert_eq!(lighting.sky, Lighting::default().sky);
    }

    #[test]
    fn solid_parts_are_drawn_before_cut_out_and_see_through_ones() {
        assert!(Surface::Solid < Surface::CutOut && Surface::CutOut < Surface::SeeThrough);
        assert!(Surface::SeeThrough < Surface::Water);
        let with = |alpha_mode| Material {
            alpha_mode,
            ..Material::default()
        };
        assert_eq!(Surface::of(&with(AlphaMode::Opaque)), Surface::Solid);
        assert_eq!(
            Surface::of(&with(AlphaMode::Mask { cutoff: 0.5 })),
            Surface::CutOut
        );
        assert_eq!(Surface::of(&with(AlphaMode::Blend)), Surface::SeeThrough);
        // The pipelines are kept in the same order.
        for (number, surface) in Surface::ALL.into_iter().enumerate() {
            assert_eq!(surface as usize, number);
        }
    }

    #[test]
    fn a_point_is_moved_by_a_transform() {
        let transform = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [10.0, 20.0, 30.0, 1.0],
        ];
        assert_eq!(moved(&transform, [1.0, 2.0, 3.0]), [12.0, 22.0, 33.0]);
    }
}
