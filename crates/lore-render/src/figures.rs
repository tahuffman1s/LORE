//! Models drawn anywhere, any number of times a frame: people, creatures,
//! the weapon in the player's hands (#29).
//!
//! A model is uploaded once with [`Figures::add`] and drawn as a [`Figure`]
//! wherever a game puts it, each frame. A rigged model's meshes are bent on
//! the graphics card by its skins' joints: the game hands over each joint's
//! skinning matrix (its place times its inverse bind matrix, as
//! `lore_anim::Skeleton::skinning` gives them), and the shader's
//! `vertex_skinned` blends a point's four joints. The parts of a model that
//! no skin bends are drawn the same way, with one joint of their own that
//! is their place in the model, so the whole model is drawn by one pipeline.
//!
//! Each frame's joints go to the graphics card as one block, with the same
//! joints of the frame before after it: what a point moved since then is
//! the trail the upscaler follows, and a figure that moved is drawn sharp.
//!
//! Only figures' own points carry joints: a level's meshes have no second
//! stream of numbers and are drawn as before.

use std::sync::Arc;

use ash::vk;
use lore_assets::{MAX_JOINTS, Material, Scene, Transform};

use crate::buffer::Buffer;
use crate::camera::{Mat4, multiply};
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::material::Materials;
use crate::mesh::{Surface, append, most_stretch, moved};
use crate::texture::AnisotropicFiltering;

/// A model uploaded once to be drawn anywhere, any number of times a frame.
///
/// [`Renderer::add_model`](crate::Renderer::add_model) gives it; a level
/// change keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelId(pub u32);

/// One model drawn this frame.
///
/// ```no_run
/// # fn show(renderer: &mut lore_render::Renderer, hero: lore_render::ModelId, pose: &[lore_assets::Transform]) {
/// use lore_render::Figure;
/// let mut place = lore_assets::Pose::default();
/// place.translation = [2.0, 0.0, -3.0];
/// renderer.set_figures(&[Figure {
///     model: hero,
///     transform: place.transform(),
///     joints: pose,
///     seen: true,
///     shadows: true,
/// }]);
/// # }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Figure<'a> {
    /// Which model.
    pub model: ModelId,
    /// Takes the model's space to the world.
    pub transform: Transform,
    /// The skinning matrices of the model's skins, in model space, skin
    /// after skin (skin 0's joints, then skin 1's...). Empty: drawn in the
    /// rest pose. A list too short takes the rest pose for the joints it
    /// leaves out.
    pub joints: &'a [Transform],
    /// Whether it is drawn in the picture (false: only its shadow, e.g. the
    /// player's own body in first person).
    pub seen: bool,
    /// Whether it casts shadows.
    pub shadows: bool,
}

/// The most figures drawn in one frame; any more are left out, and the
/// log says so once.
pub const MOST_FIGURES: usize = 64;

/// The most joints the figures of one frame may have together: as many as
/// [`MOST_FIGURES`] of the largest skins, and room for the parts no skin
/// bends, which take one each.
pub(crate) const MOST_JOINTS: usize = MOST_FIGURES * (MAX_JOINTS + 32);

/// The bytes one joint takes on the graphics card: the first three rows of
/// its matrix, four floats each (the fourth row of a place is always
/// 0, 0, 0, 1).
const JOINT_BYTES: usize = 48;

/// The bytes of the block of joints for one frame: this frame's, then the
/// frame before's.
pub(crate) const JOINT_BUFFER_BYTES: usize = 2 * MOST_JOINTS * JOINT_BYTES;

/// The bytes a figure's point takes in its second stream: four joint
/// numbers of a byte each, then how much each pulls, a byte each from 0 to 1.
pub(crate) const SKIN_BYTES: usize = 8;

/// The sway the shader knows a figure by: it moves by itself, and gathers
/// no snow and no puddles.
pub(crate) const FIGURE_SWAY: f32 = -3.0;

/// The first three rows of a place.
type Rows = [[f32; 4]; 3];

/// One part of a model, ready to draw.
pub(crate) struct Part {
    /// Where its triangles start in the index buffer, and how many numbers they are.
    pub(crate) first_index: u32,
    pub(crate) index_count: u32,
    /// Where its corner points start in the vertex buffers.
    pub(crate) first_vertex: i32,
    /// Which of the model's materials it is drawn with.
    pub(crate) material: u32,
    pub(crate) surface: Surface,
    /// Both of its sides are drawn.
    pub(crate) double_sided: bool,
    /// Its triangles run clockwise seen from the front, because its place
    /// in the model mirrors it.
    pub(crate) mirrored: bool,
    /// Where its joints start among the figure's.
    pub(crate) joints: u32,
}

/// A part of the model that no skin bends: where it stands in the model,
/// and how many copies of that its joints are (one, unless its mesh is
/// weighted to joints of a skin it is not placed with).
struct Rigid {
    place: Mat4,
    copies: u32,
    /// Where it reaches in the model: a ball round `middle`.
    middle: [f32; 3],
    radius: f32,
}

/// A model's meshes, materials and skins on the graphics card.
pub(crate) struct Model {
    /// The points, the joints of each point, and the triangles; `None`
    /// when the model has nothing to draw.
    pub(crate) buffers: Option<(Buffer, Buffer, Buffer)>,
    pub(crate) materials: Materials,
    /// Solid parts, then cut-out ones, then see-through ones, each kind
    /// grouped by material.
    pub(crate) parts: Vec<Part>,
    /// Each of those joints' skinning matrix at rest.
    rest: Vec<Mat4>,
    /// For each of those joints, the ball round the points it pulls the
    /// most, as they were rigged; `None` for a joint that pulls none most.
    reaches: Vec<Option<([f32; 3], f32)>>,
    /// The parts no skin bends, whose joints come after the skins'.
    rigid: Vec<Rigid>,
    /// How many joints one figure of the model takes.
    matrices: usize,
}

/// A number from 0 to 1 as a byte.
fn unorm8(value: f32) -> u8 {
    // The clamp keeps the product inside a byte; not-a-number becomes 0.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    byte
}

/// A point's four weights as bytes that add up to exactly 255, so a point
/// does not shrink towards the middle of the model when they are rounded.
fn weight_bytes(weights: [f32; 4]) -> [u8; 4] {
    let total: f32 = weights.iter().map(|w| w.max(0.0)).sum();
    if !(total > 0.0 && total.is_finite()) {
        return [255, 0, 0, 0];
    }
    let mut bytes = weights.map(|w| unorm8(w.max(0.0) / total));
    let sum: i32 = bytes.iter().map(|&b| i32::from(b)).sum();
    // What rounding lost or gained goes to the joint that pulls the most.
    let most = (0..4).max_by_key(|&k| bytes[k]).unwrap_or(0);
    let fixed = (i32::from(bytes[most]) + 255 - sum).clamp(0, 255);
    bytes[most] = u8::try_from(fixed).unwrap_or(255);
    bytes
}

/// A ball holding two balls: an approximate one, never too small.
fn grown(ball: Option<([f32; 3], f32)>, other: ([f32; 3], f32)) -> ([f32; 3], f32) {
    let Some((middle, radius)) = ball else {
        return other;
    };
    let (far_middle, far_radius) = other;
    let apart = (0..3)
        .map(|axis| (far_middle[axis] - middle[axis]).powi(2))
        .sum::<f32>()
        .sqrt();
    if apart + far_radius <= radius {
        return (middle, radius);
    }
    if apart + radius <= far_radius {
        return other;
    }
    let new_radius = (apart + radius + far_radius) * 0.5;
    let along = (new_radius - radius) / apart.max(f32::MIN_POSITIVE);
    (
        [0, 1, 2].map(|axis| middle[axis] + (far_middle[axis] - middle[axis]) * along),
        new_radius,
    )
}

impl Model {
    /// Copies the meshes of `scene`, its materials and their textures, and
    /// works out its skins' rest pose.
    pub(crate) fn new(
        gpu: &Arc<Gpu>,
        scene: &Scene,
        filtering: AnisotropicFiltering,
    ) -> Result<Self, RenderError> {
        let doing = "copying a model to the graphics card";
        let mut vertices = Vec::new();
        let mut skin = Vec::new();
        let mut indices = Vec::new();
        // For each part of each mesh: where it went, and how many joints
        // its points name (one past the highest; 1 with none).
        let mut placed = Vec::with_capacity(scene.meshes.len());
        for mesh in &scene.meshes {
            let mut parts = Vec::with_capacity(mesh.parts.len());
            for part in &mesh.parts {
                let range = append(part, &mut vertices, &mut indices);
                let mut reach = 1;
                if range.is_some() {
                    let points = part.positions.len();
                    let weighted = match (&part.joints, &part.weights) {
                        (Some(joints), Some(weights))
                            if joints.len() == points && weights.len() == points =>
                        {
                            Some((joints, weights))
                        }
                        _ => None,
                    };
                    skin.reserve(points * SKIN_BYTES);
                    for point in 0..points {
                        let (numbers, pulls) =
                            weighted.map_or(([0; 4], [255, 0, 0, 0]), |(joints, weights)| {
                                let pulls = weight_bytes(weights[point]);
                                // A joint that does not pull is still
                                // read, so it is one that is there.
                                let mut numbers = [0; 4];
                                for ((number, joint), pull) in
                                    numbers.iter_mut().zip(joints[point]).zip(pulls)
                                {
                                    if pull > 0 {
                                        *number = u8::try_from(joint).unwrap_or(u8::MAX);
                                    }
                                }
                                (numbers, pulls)
                            });
                        for (number, pull) in numbers.iter().zip(pulls) {
                            if pull > 0 {
                                reach = reach.max(usize::from(*number) + 1);
                            }
                        }
                        skin.extend(numbers);
                        skin.extend(pulls);
                    }
                }
                parts.push((range, reach));
            }
            placed.push(parts);
        }

        // Where each node stands in the model at rest, parents first.
        let mut rest_places: Vec<Mat4> = Vec::with_capacity(scene.nodes.len());
        for node in &scene.nodes {
            let own = node.rest.transform();
            let place = match node
                .parent
                .and_then(|parent| rest_places.get(parent as usize))
            {
                Some(parent) => multiply(parent, &own),
                None => own,
            };
            rest_places.push(place);
        }
        // Each skin's joints, one skin after another.
        let mut starts = Vec::with_capacity(scene.skins.len());
        let mut rest = Vec::new();
        for skin in &scene.skins {
            starts.push(rest.len());
            for joint in skin.joints.iter().take(MAX_JOINTS) {
                let place = rest_places
                    .get(joint.node as usize)
                    .copied()
                    .unwrap_or(IDENTITY);
                rest.push(multiply(&place, &joint.inverse_bind));
            }
        }
        let joints = rest.len();
        // The skin that bends each instance, if any.
        let mut skin_of = vec![None; scene.instances.len()];
        for node in &scene.nodes {
            if let (Some(instance), Some(skin)) = (node.instance, node.skin)
                && let Some(slot) = skin_of.get_mut(instance as usize)
            {
                *slot = Some(skin as usize);
            }
        }

        let fallback = Material::default();
        let mut parts = Vec::new();
        let mut rigid = Vec::new();
        let mut reaches: Vec<Option<([f32; 3], f32)>> = vec![None; joints];
        let mut boxes: Vec<Option<[[f32; 3]; 2]>> = vec![None; joints];
        let mut rigid_joints = 0_usize;
        for (instance, skinned) in scene.instances.iter().zip(&skin_of) {
            let (Some(mesh), Some(ranges)) = (
                scene.meshes.get(instance.mesh as usize),
                placed.get(instance.mesh as usize),
            ) else {
                continue;
            };
            let reach = ranges.iter().map(|(_, reach)| *reach).max().unwrap_or(1);
            // A skin the mesh's points fit is the one that bends it; a mesh
            // naming joints its skin does not have stands still.
            let bent = skinned.and_then(|skin| {
                let count = scene.skins.get(skin)?.joints.len().min(MAX_JOINTS);
                (reach <= count)
                    .then(|| starts.get(skin).copied())
                    .flatten()
            });
            if skinned.is_some() && bent.is_none() {
                log::warn!(
                    "A model's mesh {} names bones its skeleton does not have, so it is \
                     drawn standing still. Export it again with its armature.",
                    mesh.name.as_deref().unwrap_or("without a name")
                );
            }
            let first = match bent {
                Some(start) => start,
                None => {
                    let first = joints + rigid_joints;
                    rigid_joints += reach;
                    first
                }
            };
            let mut ball = None;
            for (part, (range, _)) in mesh.parts.iter().zip(ranges) {
                let Some((first_index, index_count, first_vertex)) = *range else {
                    continue;
                };
                let material = scene
                    .materials
                    .get(part.material as usize)
                    .unwrap_or(&fallback);
                let surface = match Surface::of(material) {
                    Surface::Water => Surface::SeeThrough,
                    surface => surface,
                };
                parts.push(Part {
                    first_index,
                    index_count,
                    first_vertex,
                    material: Materials::number(scene, part.material),
                    surface,
                    double_sided: material.double_sided,
                    mirrored: bent.is_none() && instance.mirrored,
                    joints: u32::try_from(first).unwrap_or(u32::MAX),
                });
                if let Some(start) = bent {
                    // Each point counts towards the ball of the joint
                    // that pulls it most.
                    if let (Some(numbers), Some(weights)) = (&part.joints, &part.weights) {
                        for ((point, numbers), weights) in
                            part.positions.iter().zip(numbers).zip(weights)
                        {
                            let most = (0..4)
                                .max_by(|&a, &b| weights[a].total_cmp(&weights[b]))
                                .unwrap_or(0);
                            if let Some(found) = boxes.get_mut(start + usize::from(numbers[most])) {
                                let [low, high] = found.get_or_insert([*point, *point]);
                                for axis in 0..3 {
                                    low[axis] = low[axis].min(point[axis]);
                                    high[axis] = high[axis].max(point[axis]);
                                }
                            }
                        }
                    }
                } else {
                    // Measured from its points, which a part built in code
                    // has even when it says no bounds.
                    let mut low = [f32::INFINITY; 3];
                    let mut high = [f32::NEG_INFINITY; 3];
                    for point in &part.positions {
                        for axis in 0..3 {
                            low[axis] = low[axis].min(point[axis]);
                            high[axis] = high[axis].max(point[axis]);
                        }
                    }
                    let half = (0..3)
                        .map(|axis| ((high[axis] - low[axis]) * 0.5).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    let middle = [0, 1, 2].map(|axis| (low[axis] + high[axis]) * 0.5);
                    ball = Some(grown(
                        ball,
                        (
                            moved(&instance.transform, middle),
                            half * most_stretch(&instance.transform),
                        ),
                    ));
                }
            }
            if bent.is_none() {
                let (middle, radius) = ball.unwrap_or(([0.0; 3], 0.0));
                rigid.push(Rigid {
                    place: instance.transform,
                    copies: u32::try_from(reach).unwrap_or(1),
                    middle,
                    radius,
                });
            }
        }
        for (reach, found) in reaches.iter_mut().zip(&boxes) {
            *reach = found.map(|[low, high]| {
                let middle = [0, 1, 2].map(|axis| (low[axis] + high[axis]) * 0.5);
                let half = (0..3)
                    .map(|axis| ((high[axis] - low[axis]) * 0.5).powi(2))
                    .sum::<f32>()
                    .sqrt();
                (middle, half)
            });
        }
        // Parts that share a material follow one another.
        parts.sort_by_key(|part| (part.surface, part.material));

        let buffers = if parts.is_empty() {
            None
        } else {
            Some((
                Buffer::holding(gpu, &vertices, vk::BufferUsageFlags::VERTEX_BUFFER, doing)?,
                Buffer::holding(gpu, &skin, vk::BufferUsageFlags::VERTEX_BUFFER, doing)?,
                Buffer::holding(gpu, &indices, vk::BufferUsageFlags::INDEX_BUFFER, doing)?,
            ))
        };
        Ok(Self {
            buffers,
            materials: Materials::new(gpu, scene, filtering)?,
            parts,

            rest,
            reaches,
            rigid,
            matrices: joints + rigid_joints,
        })
    }
}

/// The place that moves nothing.
const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// The first three rows of `place`.
fn rows(place: &Mat4) -> Rows {
    [0, 1, 2].map(|row| place.map(|column| column[row]))
}

/// Where `rows` put `point`.
fn rows_moved(rows: &Rows, point: [f32; 3]) -> [f32; 3] {
    rows.map(|[x, y, z, w]| x * point[0] + y * point[1] + z * point[2] + w)
}

/// The most `rows` stretch anything: the longest of their three axes.
fn rows_stretch(rows: &Rows) -> f32 {
    (0..3)
        .map(|axis| rows.iter().map(|row| row[axis] * row[axis]).sum::<f32>())
        .fold(0.0, f32::max)
        .sqrt()
}

/// One figure of the frame, as it is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shown {
    /// Which model.
    pub(crate) model: u32,
    /// Where its joints start in this frame's block.
    pub(crate) first: u32,
    /// Whether it is drawn in the picture, and whether it casts shadows.
    pub(crate) seen: bool,
    pub(crate) shadows: bool,
    /// Its place mirrors it.
    pub(crate) mirrored: bool,
    /// A ball that holds all of it, as it stands this frame.
    pub(crate) middle: [f32; 3],
    pub(crate) radius: f32,
}

/// The models a game has added, and the figures of the frame.
#[derive(Default)]
pub(crate) struct Figures {
    models: Vec<Model>,
    /// This frame's figures, and each one's joints in the world.
    shown: Vec<Shown>,
    now: Vec<Rows>,
    /// The figures of the frame last drawn, and their joints.
    was_shown: Vec<Shown>,
    was: Vec<Rows>,
    /// What the graphics card is handed: `now`, then for each of its
    /// joints where it was the frame before.
    bytes: Vec<u8>,
    /// Whether this list of figures has been drawn already: drawn again,
    /// nothing in it has moved since.
    drawn: bool,
    /// Whether the log has said that figures were left out, since there
    /// were last few enough.
    told: bool,
    /// Whether the models' filtering could not be changed, and is not
    /// tried again until a model is added.
    keep: bool,
}

impl Figures {
    /// Adds a model, and gives the number it is drawn by.
    pub(crate) fn add(
        &mut self,
        gpu: &Arc<Gpu>,
        scene: &Scene,
        filtering: AnisotropicFiltering,
    ) -> Result<crate::ModelId, RenderError> {
        let number = u32::try_from(self.models.len()).map_err(|_| RenderError::Driver {
            doing: "copying a model to the graphics card",
            code: vk::Result::ERROR_OUT_OF_HOST_MEMORY,
        })?;
        self.models.push(Model::new(gpu, scene, filtering)?);
        self.keep = false;
        Ok(crate::ModelId(number))
    }

    /// The model numbered `number`.
    pub(crate) fn model(&self, number: u32) -> Option<&Model> {
        self.models.get(number as usize)
    }

    /// Reads textures seen at a slant as `filtering` says from now on.
    ///
    /// # Safety
    ///
    /// Nothing still being drawn may use the models: the caller has waited
    /// for the graphics card to go idle.
    pub(crate) unsafe fn set_filtering(
        &mut self,
        filtering: AnisotropicFiltering,
    ) -> Result<(), RenderError> {
        for model in &mut self.models {
            if model.materials.filtering() != filtering {
                // SAFETY: the caller says nothing is drawing with these.
                unsafe { model.materials.set_filtering(filtering) }?;
            }
        }
        Ok(())
    }

    /// Takes the filtering each model reads with now as what was asked,
    /// after it could not be changed: it is not tried again.
    pub(crate) fn keep_filtering(&mut self) {
        self.keep = true;
    }

    /// Whether any model reads textures otherwise than `filtering` says.
    pub(crate) fn filtering_differs(&self, filtering: AnisotropicFiltering) -> bool {
        !self.keep
            && self
                .models
                .iter()
                .any(|model| model.materials.filtering() != filtering)
    }

    /// The figures to draw from now on, in place of those before.
    pub(crate) fn set(&mut self, figures: &[crate::Figure<'_>]) {
        // The list drawn last is the one this frame moved on from; a list
        // that was replaced before it was drawn never was on the screen.
        if self.drawn {
            std::mem::swap(&mut self.shown, &mut self.was_shown);
            std::mem::swap(&mut self.now, &mut self.was);
        }
        self.shown.clear();
        self.now.clear();
        let mut left_out = 0;
        for figure in figures {
            let Some(model) = self.models.get(figure.model.0 as usize) else {
                continue;
            };
            if self.shown.len() >= MOST_FIGURES || self.now.len() + model.matrices > MOST_JOINTS {
                left_out += 1;
                continue;
            }
            let first = self.now.len();
            let world = &figure.transform;
            let mut low = [f32::INFINITY; 3];
            let mut high = [f32::NEG_INFINITY; 3];
            let mut take = |middle: [f32; 3], radius: f32| {
                for axis in 0..3 {
                    low[axis] = low[axis].min(middle[axis] - radius);
                    high[axis] = high[axis].max(middle[axis] + radius);
                }
            };
            for (joint, (rest, reach)) in model.rest.iter().zip(&model.reaches).enumerate() {
                let skinning = figure.joints.get(joint).unwrap_or(rest);
                let placed = rows(&multiply(world, skinning));
                if let Some((middle, radius)) = reach {
                    take(rows_moved(&placed, *middle), radius * rows_stretch(&placed));
                }
                self.now.push(placed);
            }
            for part in &model.rigid {
                let placed = rows(&multiply(world, &part.place));
                take(
                    rows_moved(&placed, part.middle),
                    part.radius * rows_stretch(&placed),
                );
                for _ in 0..part.copies {
                    self.now.push(placed);
                }
            }
            let middle = [0, 1, 2].map(|axis| (low[axis] + high[axis]) * 0.5);
            let radius = (0..3)
                .map(|axis| ((high[axis] - low[axis]) * 0.5).powi(2))
                .sum::<f32>()
                .sqrt();
            let [x, y, z] = [0, 1, 2].map(|column| {
                let [a, b, c, _] = world[column];
                [a, b, c]
            });
            let turned = x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0])
                + x[2] * (y[0] * z[1] - y[1] * z[0]);
            self.shown.push(Shown {
                model: figure.model.0,
                first: u32::try_from(first).unwrap_or(0),
                seen: figure.seen,
                shadows: figure.shadows,
                mirrored: turned < 0.0,
                // A figure with nothing to draw holds nothing.
                middle: if radius.is_finite() { middle } else { [0.0; 3] },
                radius: if low[0] <= high[0] { radius } else { 0.0 },
            });
        }
        if left_out > 0 && !self.told {
            log::warn!(
                "{left_out} figures were not drawn this frame: at most {MOST_FIGURES} figures, \
                 with {MOST_JOINTS} joints together, are drawn in a frame. Draw only the \
                 figures near enough to be seen."
            );
        }
        self.told = left_out > 0;

        // This frame's joints, then where each was in the frame before: the
        // same figure is the one at the same place in the list, if it is of
        // the same model; any other has not moved.
        self.bytes.clear();
        let push = |bytes: &mut Vec<u8>, rows: &Rows| {
            for number in rows.iter().flatten() {
                bytes.extend_from_slice(&number.to_ne_bytes());
            }
        };
        for rows in &self.now {
            push(&mut self.bytes, rows);
        }
        for (number, shown) in self.shown.iter().enumerate() {
            let count = self
                .models
                .get(shown.model as usize)
                .map_or(0, |model| model.matrices);
            let first = shown.first as usize;
            let before = match self.was_shown.get(number) {
                Some(was) if was.model == shown.model => {
                    self.was.get(was.first as usize..was.first as usize + count)
                }
                _ => None,
            };
            let joints = before.or_else(|| self.now.get(first..first + count));
            for rows in joints.into_iter().flatten() {
                push(&mut self.bytes, rows);
            }
        }
        self.drawn = false;
    }

    /// Gets the figures ready for a frame to be drawn, and gives what the
    /// graphics card is handed of them. Drawn a second time, nothing in
    /// them has moved since the first.
    pub(crate) fn ready(&mut self) -> &[u8] {
        if self.drawn {
            let half = self.now.len() * JOINT_BYTES;
            if self.bytes.len() == 2 * half {
                self.bytes.copy_within(..half, half);
            }
        }
        self.drawn = true;
        &self.bytes
    }

    /// The block of joints to hand the graphics card, as [`ready`](Self::ready) left it.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Where last frame's joints start in the block, counted in joints.
    pub(crate) fn before(&self) -> u32 {
        u32::try_from(self.now.len()).unwrap_or(0)
    }

    /// This frame's figures.
    pub(crate) fn shown(&self) -> &[Shown] {
        &self.shown
    }

    /// Whether any figure casting a shadow lies in the ball `holds` asks of.
    pub(crate) fn casting_in(&self, holds: impl Fn([f32; 3], f32) -> bool) -> bool {
        self.shown
            .iter()
            .any(|shown| shown.shadows && holds(shown.middle, shown.radius))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_add_up_to_a_whole_byte() {
        for weights in [
            [1.0 / 3.0; 4],
            [0.5, 0.5, 0.0, 0.0],
            [0.7, 0.2, 0.1, 0.0],
            [0.0; 4],
            [f32::NAN, 0.0, 0.0, 0.0],
            [2.0, 2.0, 0.0, 0.0],
        ] {
            let bytes = weight_bytes(weights);
            let sum: u32 = bytes.iter().map(|&b| u32::from(b)).sum();
            assert_eq!(sum, 255, "{weights:?} gave {bytes:?}");
        }
        assert_eq!(weight_bytes([0.25, 0.75, 0.0, 0.0])[1], 191);
    }

    #[test]
    fn a_ball_grows_to_hold_another() {
        let (middle, radius) = grown(Some(([0.0; 3], 1.0)), ([4.0, 0.0, 0.0], 1.0));
        assert!((middle[0] - 2.0).abs() < 1e-5 && (radius - 3.0).abs() < 1e-5);
        assert_eq!(
            grown(Some(([0.0; 3], 5.0)), ([1.0, 0.0, 0.0], 1.0)),
            ([0.0; 3], 5.0)
        );
        assert_eq!(grown(None, ([1.0; 3], 2.0)), ([1.0; 3], 2.0));
    }

    #[test]
    fn rows_place_and_stretch_as_the_matrix_does() {
        let mut place = IDENTITY;
        place[0][0] = 2.0;
        place[3] = [1.0, 2.0, 3.0, 1.0];
        let rows = rows(&place);
        assert_eq!(rows_moved(&rows, [1.0, 1.0, 1.0]), [3.0, 3.0, 4.0]);
        assert!((rows_stretch(&rows) - 2.0).abs() < 1e-6);
        assert_eq!(
            rows_moved(&rows, [1.0, 0.0, 0.0]),
            moved(&place, [1.0, 0.0, 0.0])
        );
    }
}
