//! Lamps' shadows (#400): a lamp with no shadow lights through walls, so
//! the lamps that matter most to the picture each frame are given one.
//!
//! A lamp that shines every way sees the world as six pictures of how far
//! away things are, one for each face of a cube round it; a spot whose beam
//! is no wider than a little over a right angle needs only one, along its
//! beam. Every lamp's pictures are kept in one image (an atlas, as Doom 2016
//! keeps its lights': Courrèges, "DOOM (2016) graphics study"), six to a
//! lamp, and kept from frame to frame: a lamp's pictures are drawn again
//! only when the lamp moves, its light reaches much farther or less far, or
//! the level changes. Only the level casts shadows so far, and it does not
//! move (#337).
//!
//! Which lamps cast shadows is worked out here, on the processor, so it is
//! the same on every card and can be tested without one: the lamps in view
//! whose light reaches farthest for how far they are from the eye, with a
//! lamp that already has a shadow kept unless another matters clearly more,
//! so that shadows do not come and go as the eye moves. A shadow fades in
//! and out over a quarter of a second rather than appearing at once.
//!
//! Each face is drawn a little wider than a quarter turn, so the soft edge
//! of a shadow that falls near the border of one face is read whole from
//! that face's own picture.

use std::sync::Arc;

use crate::camera::Mat4;
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::lamps::{Beam, Lamp};
use crate::shadow::DepthLayers;

/// The most lamps that cast shadows at once: as many as the highest
/// setting allows.
const MOST_SLOTS: usize = 16;

/// How many pictures each lamp is given: one for each face of a cube.
pub(crate) const FACES: usize = 6;

/// How near the lamp the nearest thing its shadow knows of can be, in
/// metres. Nearer than this, a thing casts no shadow.
const NEAR: f32 = 0.05;

/// How much wider than a right angle each face of a cube sees: the tangent
/// of half its width. Shadows' soft edges are read up to `EDGE_ROOM` of
/// the picture from where they fall, which this leaves room for.
const GUARD: f32 = 1.1;

/// How far across a picture the shader reads round a point for a soft edge,
/// at most, as a share of the picture's width (the shader's
/// `LAMP_SOFTEST`). Kept inside the room `GUARD` leaves.
#[cfg(test)]
const EDGE_ROOM: f32 = 0.04;

/// The widest beam, from its middle to its edge in radians, that one
/// picture along it shows: about 55 degrees. A wider spot is given a cube.
const ONE_PICTURE_BEAM: f32 = 0.96;

/// How much wider than its beam a spot's one picture sees, in radians, so
/// that the soft edge of a shadow at the edge of the beam is in it.
const BEAM_MARGIN: f32 = 0.05;

/// Half the angle across a face of a cube from its middle to a corner: the
/// cosine of a corner's direction against the face's own is `1 / √3`.
const CORNER_ANGLE: f32 = 0.955_316_6;

/// How long a shadow takes to fade in or out, in seconds.
const FADE: f32 = 0.25;

/// How many pictures may be drawn in one frame, at most: four lamps'
/// cubes. A lamp whose pictures do not fit waits for the next frame.
const FACES_A_FRAME: usize = 24;

/// How far a lamp may move between one frame and the next, in metres, and
/// still be known for the same lamp: a carried torch, a swinging lantern.
const MOVED: f32 = 1.0;

/// How much more a lamp that already has a shadow is counted when the
/// lamps are chosen, so that two lamps that matter about as much do not
/// take a shadow from each other in turn as the eye moves.
const KEEP: f32 = 1.5;

/// How many lamps cast shadows, and how sharp the shadows are: a player's
/// setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LampShadows {
    /// No lamp casts a shadow: lamplight goes through walls.
    Off,
    /// The four lamps that matter most, a little soft.
    Low,
    /// The eight that matter most.
    #[default]
    Medium,
    /// The sixteen that matter most.
    High,
}

impl LampShadows {
    /// How many lamps cast shadows at once.
    #[must_use]
    pub fn lamps(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Low => 4,
            Self::Medium => 8,
            Self::High => 16,
        }
    }

    /// How many pixels wide and high each picture of a lamp's is.
    #[must_use]
    pub fn size(self) -> u32 {
        match self {
            Self::Off | Self::Low => 256,
            Self::Medium | Self::High => 512,
        }
    }
}

/// One picture that has to be drawn this frame: which picture of the atlas
/// it is, and how a point of the world is placed in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Face {
    /// Which picture of the atlas.
    pub(crate) layer: usize,
    /// Takes a point of the world to the picture: x and y from -1 to 1
    /// across it (y up), depth from 0 at `NEAR` to 1 at the lamp's reach.
    pub(crate) clip: Mat4,
    /// Where the lamp is, how far it sees, and the face's own directions
    /// (across, up and ahead) with how far it leans to each side for each
    /// metre ahead, for asking what is in it.
    at: [f32; 3],
    far: f32,
    axes: [[f32; 3]; 3],
    lean: f32,
}

impl Face {
    /// Whether something `radius` metres round `middle` can be seen in this
    /// picture, and so can cast a shadow in it.
    pub(crate) fn holds(&self, middle: [f32; 3], radius: f32) -> bool {
        let from = minus(middle, self.at);
        if dot(from, from) > (self.far + radius) * (self.far + radius) {
            return false;
        }
        let [across, up, ahead] = self.axes.map(|axis| dot(from, axis));
        let beyond =
            |side: f32| (side.abs() - self.lean * ahead) / (1.0 + self.lean * self.lean).sqrt();
        ahead > NEAR - radius && beyond(across) < radius && beyond(up) < radius
    }
}

/// What a slot of the atlas holds: the lamp its pictures were drawn for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Held {
    at: [f32; 3],
    /// How far its pictures see, in metres.
    far: f32,
    beam: Option<Beam>,
    level: u64,
}

impl Held {
    /// Whether `lamp`, whose light reaches `reach` metres, in the level
    /// numbered `level`, can use these pictures as they are.
    fn fits(&self, lamp: &Lamp, reach: f32, level: u64) -> bool {
        self.level == level
            && self.at == lamp.at
            && self.beam == lamp.beam
            && reach <= self.far
            && self.far <= reach * 4.0
    }
}

/// Six pictures of the atlas and the lamp they show.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Slot {
    held: Option<Held>,
    /// How much of its shadow is drawn, from 0 to 1: it fades in and out.
    strength: f32,
    /// Whether one of this frame's chosen lamps has it.
    chosen: bool,
}

/// What [`ShadowSlots::choose`] needs to know of a frame besides its lamps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shadowing {
    /// How many lamps may cast shadows: how many slots the atlas has.
    pub(crate) slots: usize,
    /// The number the level's meshes are known by: drawing another level
    /// draws every picture again.
    pub(crate) level: u64,
    /// How many seconds have passed since the frame before.
    pub(crate) elapsed: f32,
}

impl Default for Shadowing {
    /// No lamp casts a shadow.
    fn default() -> Self {
        Self {
            slots: 0,
            level: 0,
            elapsed: 0.0,
        }
    }
}

/// Which lamps have shadows, kept from frame to frame.
#[derive(Debug, Default)]
pub(crate) struct ShadowSlots {
    slots: Vec<Slot>,
    /// The lamps that might cast: how much each matters, and which it is.
    /// Kept between frames so that choosing needs no new memory.
    candidates: Vec<(f32, usize)>,
    /// The chosen lamps that have no pictures yet, kept likewise.
    wanting: Vec<usize>,
    /// The pictures to draw this frame.
    faces: Vec<Face>,
}

/// A lamp's shadow as the shader takes it, in three of its numbers: the
/// fourth row of the lamp and two spare numbers of the others.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shadow {
    /// The first picture of the lamp's slot, the scale of its pictures
    /// (one over the tangent of half the width each sees), and the two
    /// numbers that give a picture's depth from how far ahead a point is,
    /// `a + b / ahead`. All -1 for a lamp with no shadow.
    pub(crate) row: [f32; 4],
    /// 1 if the lamp's one picture is along its beam, 0 for a cube.
    pub(crate) along_beam: f32,
    /// How much of the shadow is drawn, from 0 to 1.
    pub(crate) strength: f32,
}

impl Shadow {
    /// The numbers of a lamp with no shadow.
    pub(crate) const NONE: Self = Self {
        row: [-1.0, 1.0, 0.0, 0.0],
        along_beam: 0.0,
        strength: 0.0,
    };
}

impl ShadowSlots {
    /// Chooses which of the lamps `near` (how far each is from the eye, and
    /// which of `lamps` it is, nearest first) cast shadows this frame, and
    /// gives in `shadows` each one's numbers, in the same order. Only a
    /// lamp `viewed` says lights what is seen is given a new shadow. The
    /// pictures that have to be drawn first are then [`faces`](Self::faces).
    pub(crate) fn choose(
        &mut self,
        lamps: &[Lamp],
        near: &[(f32, usize)],
        viewed: &[bool],
        how: &Shadowing,
        shadows: &mut Vec<Shadow>,
    ) {
        shadows.clear();
        shadows.resize(near.len(), Shadow::NONE);
        self.faces.clear();
        let how = &Shadowing {
            slots: how.slots.min(MOST_SLOTS),
            ..*how
        };
        if self.slots.len() != how.slots {
            self.slots = vec![
                Slot {
                    held: None,
                    strength: 0.0,
                    chosen: false,
                };
                how.slots
            ];
        }
        if how.slots == 0 {
            return;
        }
        // A frame long after the one before (the first, or after a pause)
        // shows each shadow as it should be at once.
        let fresh = !(how.elapsed > 0.0 && how.elapsed < 0.25);
        let step = if fresh { 1.0 } else { how.elapsed / FADE };

        // How much each lamp in view that casts a shadow matters: how far
        // its light reaches for how far it is from the eye.
        self.candidates.clear();
        for (place, &(far, number)) in near.iter().enumerate() {
            let lamp = &lamps[number];
            if !lamp.casts_shadows || !viewed.get(number).copied().unwrap_or(false) {
                continue;
            }
            let reach = lamp.reach();
            let mut matters = reach / far.max(0.5);
            if self.held_by(lamp, reach, how.level).is_some() {
                matters *= KEEP;
            }
            self.candidates.push((matters, place));
        }
        self.candidates
            .sort_unstable_by(|one, other| other.0.total_cmp(&one.0).then(one.1.cmp(&other.1)));
        self.candidates.truncate(how.slots);

        for slot in &mut self.slots {
            slot.chosen = false;
        }
        // Lamps whose pictures are drawn already keep them; the rest are
        // given the slot that is drawing least, and its pictures are drawn,
        // as many as fit in this frame.
        self.wanting.clear();
        for index in 0..self.candidates.len() {
            let place = self.candidates[index].1;
            let lamp = &lamps[near[place].1];
            match self.held_by(lamp, lamp.reach(), how.level) {
                Some(slot) => self.slots[slot].chosen = true,
                None => self.wanting.push(place),
            }
        }
        // The slots whose shadows can be seen: those of the lamps in view.
        // A slot of a lamp out of view, or of another level, shows nothing
        // and may be taken at once.
        let mut shown = [false; MOST_SLOTS];
        for &(_, number) in near {
            let lamp = &lamps[number];
            if lamp.casts_shadows
                && let Some(slot) = self.held_by(lamp, lamp.reach(), how.level)
                && let Some(shown) = shown.get_mut(slot)
            {
                *shown = true;
            }
        }
        for index in 0..self.wanting.len() {
            let place = self.wanting[index];
            let lamp = &lamps[near[place].1];
            let faces = faces_of(lamp);
            if self.faces.len() + faces.iter().filter(|face| **face).count() > FACES_A_FRAME {
                continue;
            }
            // The same lamp moved (carried, swung) or reaching farther or
            // less far keeps its own slot, and as much of its shadow as it
            // had. Any other takes a slot whose shadow has faded out, the
            // empty first: a shadow still fading is not cut off, and the
            // new lamp waits for it.
            let own = self.moved_from(lamp, how.level);
            let free = || {
                (0..self.slots.len())
                    .filter(|slot| {
                        let seen = shown.get(*slot).copied().unwrap_or(false);
                        let slot = &self.slots[*slot];
                        !slot.chosen && (fresh || slot.strength <= 0.0 || !seen)
                    })
                    .min_by_key(|slot| self.slots[*slot].held.is_some())
            };
            let Some(slot) = own.or_else(free) else {
                continue;
            };
            let strength = if own.is_some() {
                self.slots[slot].strength
            } else if fresh {
                1.0
            } else {
                0.0
            };
            let reach = lamp.reach();
            let held = Held {
                at: lamp.at,
                far: far_for(reach),
                beam: lamp.beam,
                level: how.level,
            };
            let numbers = pictured(lamp, held.far);
            for (face, drawn) in faces.into_iter().enumerate() {
                if drawn {
                    self.faces
                        .push(numbers.face(slot * FACES + face, face, held.far));
                }
            }
            self.slots[slot] = Slot {
                held: Some(held),
                strength,
                chosen: true,
            };
        }

        for slot in &mut self.slots {
            let aim = if slot.chosen { 1.0 } else { 0.0 };
            slot.strength = if slot.strength < aim {
                (slot.strength + step).min(aim)
            } else {
                (slot.strength - step).max(aim)
            };
        }
        // Every lamp in view whose pictures are drawn, chosen or fading,
        // casts as much shadow as its slot's strength.
        for (shadow, &(_, number)) in shadows.iter_mut().zip(near) {
            let lamp = &lamps[number];
            if !lamp.casts_shadows {
                continue;
            }
            let Some(slot) = self.held_by(lamp, lamp.reach(), how.level) else {
                continue;
            };
            let strength = self.slots[slot].strength;
            let Some(held) = self.slots[slot].held.filter(|_| strength > 0.0) else {
                continue;
            };
            let numbers = pictured(lamp, held.far);
            #[allow(clippy::cast_precision_loss)]
            let first = (slot * FACES) as f32;
            *shadow = Shadow {
                row: [first, numbers.scale, numbers.a, numbers.b],
                along_beam: if numbers.along_beam { 1.0 } else { 0.0 },
                strength,
            };
        }
    }

    /// The slot whose pictures `lamp` can use as they are.
    fn held_by(&self, lamp: &Lamp, reach: f32, level: u64) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.held.is_some_and(|held| held.fits(lamp, reach, level)))
    }

    /// The slot not yet chosen this frame that most likely held `lamp`
    /// before it moved: the nearest within [`MOVED`] whose lamp shone the
    /// same way (every way, or in a beam), in the same level.
    fn moved_from(&self, lamp: &Lamp, level: u64) -> Option<usize> {
        let apart = |held: &Held| {
            let off = minus(held.at, lamp.at);
            dot(off, off)
        };
        (0..self.slots.len())
            .filter_map(|slot| {
                let found = &self.slots[slot];
                let held = found.held.filter(|held| {
                    !found.chosen
                        && held.level == level
                        && held.beam.is_some() == lamp.beam.is_some()
                        && apart(held) <= MOVED * MOVED
                })?;
                Some((apart(&held), slot))
            })
            .min_by(|one, other| one.0.total_cmp(&other.0).then(one.1.cmp(&other.1)))
            .map(|(_, slot)| slot)
    }

    /// The pictures [`choose`](Self::choose) found have to be drawn.
    pub(crate) fn faces(&self) -> &[Face] {
        &self.faces
    }
}

/// How far a lamp's pictures see when its light reaches `reach` metres: a
/// little farther, in steps, so that a lamp whose reach changes as it
/// flickers keeps its pictures.
fn far_for(reach: f32) -> f32 {
    let steps = (reach.max(NEAR * 4.0).log2() * 4.0).ceil();
    let far = (steps / 4.0).exp2();
    if far.is_finite() { far } else { reach }
}

/// How a lamp's pictures place a point: their scale, the two numbers that
/// give depth, and whether there is one picture along the beam.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pictured {
    at: [f32; 3],
    scale: f32,
    a: f32,
    b: f32,
    along_beam: bool,
    pointing: [f32; 3],
}

/// The numbers of `lamp`'s pictures that see out to `far` metres.
fn pictured(lamp: &Lamp, far: f32) -> Pictured {
    let far = far.max(NEAR * 2.0);
    let (along_beam, scale, pointing) = match lamp.beam {
        Some(beam) if beam.outer <= ONE_PICTURE_BEAM => (
            true,
            1.0 / (beam.outer.max(0.01) + BEAM_MARGIN).tan(),
            unit(beam.pointing),
        ),
        _ => (false, 1.0 / GUARD, [0.0, -1.0, 0.0]),
    };
    Pictured {
        at: lamp.at,
        scale,
        a: far / (far - NEAR),
        b: -far * NEAR / (far - NEAR),
        along_beam,
        pointing,
    }
}

impl Pictured {
    /// The directions across, up and ahead of picture `face`: along the
    /// beam for a spot's one picture, else a face of the cube. The shader's
    /// `lamp_shadow_place` turns a point the same way.
    fn axes(&self, face: usize) -> [[f32; 3]; 3] {
        if self.along_beam {
            let ahead = self.pointing;
            let lean = if ahead[1].abs() > 0.99 {
                [0.0, 0.0, 1.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let across = unit(cross(lean, ahead));
            return [across, cross(ahead, across), ahead];
        }
        match face {
            0 => [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            1 => [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
            2 => [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            3 => [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]],
            4 => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            _ => [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        }
    }

    /// Picture `face` of this lamp, which is picture `layer` of the atlas.
    fn face(&self, layer: usize, face: usize, far: f32) -> Face {
        let axes = self.axes(face);
        let [across, up, ahead] = axes;
        let (s, a, b) = (self.scale, self.a, self.b);
        let column = |axis: usize| [s * across[axis], s * up[axis], a * ahead[axis], ahead[axis]];
        let clip = [
            column(0),
            column(1),
            column(2),
            [
                -s * dot(across, self.at),
                -s * dot(up, self.at),
                -a * dot(ahead, self.at) + b,
                -dot(ahead, self.at),
            ],
        ];
        Face {
            layer,
            clip,
            at: self.at,
            far,
            axes,
            lean: 1.0 / s,
        }
    }
}

/// Which of a lamp's six pictures have to be drawn: only the first for a
/// spot that has one picture along its beam, and for a wider spot only the
/// faces of the cube its beam reaches into.
fn faces_of(lamp: &Lamp) -> [bool; FACES] {
    match lamp.beam {
        None => [true; FACES],
        Some(beam) if beam.outer <= ONE_PICTURE_BEAM => [true, false, false, false, false, false],
        Some(beam) => {
            let widest = beam.outer + CORNER_ANGLE;
            if widest >= std::f32::consts::PI {
                return [true; FACES];
            }
            let pointing = unit(beam.pointing);
            let least = widest.cos();
            let cube = Pictured {
                at: lamp.at,
                scale: 1.0,
                a: 0.0,
                b: 0.0,
                along_beam: false,
                pointing,
            };
            std::array::from_fn(|face| dot(cube.axes(face)[2], pointing) >= least)
        }
    }
}

/// The lamps' pictures on the graphics card.
pub(crate) struct LampAtlas {
    pub(crate) pictures: DepthLayers,
    /// What it was made for.
    pub(crate) shadows: LampShadows,
}

impl LampAtlas {
    /// Room for the pictures of as many lamps as `shadows` says.
    pub(crate) fn new(gpu: &Arc<Gpu>, shadows: LampShadows) -> Result<Self, RenderError> {
        let doing = "making room for the lamps' shadows";
        let count = u32::try_from(shadows.lamps() * FACES).unwrap_or(u32::MAX);
        Ok(Self {
            pictures: DepthLayers::new(gpu, shadows.size(), count.max(1), doing)?,
            shadows,
        })
    }
}

fn dot(one: [f32; 3], other: [f32; 3]) -> f32 {
    one[0] * other[0] + one[1] * other[1] + one[2] * other[2]
}

fn minus(one: [f32; 3], other: [f32; 3]) -> [f32; 3] {
    [one[0] - other[0], one[1] - other[1], one[2] - other[2]]
}

fn cross(one: [f32; 3], other: [f32; 3]) -> [f32; 3] {
    [
        one[1] * other[2] - one[2] * other[1],
        one[2] * other[0] - one[0] * other[2],
        one[0] * other[1] - one[1] * other[0],
    ]
}

fn unit(direction: [f32; 3]) -> [f32; 3] {
    let length = dot(direction, direction).sqrt();
    if length > 0.0 && length.is_finite() {
        direction.map(|part| part / length)
    } else {
        [0.0, -1.0, 0.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where `face` puts a point: across and up from -1 to 1, and depth.
    fn placed(face: &Face, point: [f32; 3]) -> [f32; 3] {
        let row = |row: usize| {
            face.clip[0][row] * point[0]
                + face.clip[1][row] * point[1]
                + face.clip[2][row] * point[2]
                + face.clip[3][row]
        };
        let w = row(3);
        [row(0) / w, row(1) / w, row(2) / w]
    }

    /// The shader's `lamp_shadow_place`, as Rust: which face of the cube a
    /// point `from` the lamp is in, and where.
    fn shader_place(numbers: &Pictured, from: [f32; 3]) -> (usize, [f32; 3]) {
        let [x, y, z] = from;
        let a = from.map(f32::abs);
        let (face, across, up, ahead) = if numbers.along_beam {
            let [across, up, ahead] = numbers.axes(0);
            (0, dot(from, across), dot(from, up), dot(from, ahead))
        } else if a[0] >= a[1] && a[0] >= a[2] {
            if x > 0.0 {
                (0, -z, y, a[0])
            } else {
                (1, z, y, a[0])
            }
        } else if a[1] >= a[2] {
            if y > 0.0 {
                (2, x, -z, a[1])
            } else {
                (3, x, z, a[1])
            }
        } else if z > 0.0 {
            (4, x, y, a[2])
        } else {
            (5, -x, y, a[2])
        };
        let s = numbers.scale;
        (
            face,
            [
                across * s / ahead,
                up * s / ahead,
                numbers.a + numbers.b / ahead,
            ],
        )
    }

    fn bulb(at: [f32; 3]) -> Lamp {
        Lamp {
            at,
            candela: 64.0,
            reach: 10.0,
            ..Lamp::default()
        }
    }

    fn close(one: [f32; 3], other: [f32; 3]) -> bool {
        one.iter()
            .zip(other)
            .all(|(one, other)| (one - other).abs() < 1e-4)
    }

    #[test]
    fn each_face_places_a_point_where_the_shader_looks_for_it() {
        let lamp = bulb([3.0, 2.0, -1.0]);
        let numbers = pictured(&lamp, 16.0);
        for from in [
            [4.0, 0.5, -1.0],
            [-3.0, 1.0, 2.0],
            [0.5, 5.0, 0.2],
            [1.0, -6.0, -2.0],
            [-1.0, 1.5, 3.0],
            [2.0, -1.0, -7.0],
        ] {
            let (face, seen) = shader_place(&numbers, from);
            let drawn = numbers.face(face, face, 16.0);
            let point = [0, 1, 2].map(|axis| lamp.at[axis] + from[axis]);
            assert!(
                close(placed(&drawn, point), seen),
                "{from:?} on face {face}"
            );
            // Inside the picture, and in its depth.
            assert!(seen[0].abs() < 1.0 && seen[1].abs() < 1.0);
            assert!(seen[2] > 0.0 && seen[2] < 1.0);
            assert!(drawn.holds(point, 0.0));
        }
    }

    #[test]
    fn a_narrow_spot_has_one_picture_along_its_beam_and_a_wide_one_a_cube() {
        let torch = Lamp {
            beam: Some(Beam {
                pointing: [1.0, -1.0, 0.0],
                inner: 0.2,
                outer: 0.35,
            }),
            ..bulb([0.0, 2.0, 0.0])
        };
        assert_eq!(faces_of(&torch), [true, false, false, false, false, false]);
        let numbers = pictured(&torch, 16.0);
        assert!(numbers.along_beam);
        let along = [3.0, -3.0, 0.0];
        let (_, seen) = shader_place(&numbers, along);
        assert!(seen[0].abs() < 1e-4 && seen[1].abs() < 1e-4);
        let drawn = numbers.face(0, 0, 16.0);
        assert!(close(placed(&drawn, [3.0, -1.0, 0.0]), seen));
        // The edge of the beam is inside the picture, near its border.
        let edge = 0.35_f32;
        let pointing = unit([1.0, -1.0, 0.0]);
        let rim = [0, 1, 2]
            .map(|axis| (pointing[axis] * edge.cos() + [0.0, 0.0, 1.0][axis] * edge.sin()) * 2.0);
        let (_, rim) = shader_place(&numbers, rim);
        let out = rim[0].abs().max(rim[1].abs());
        assert!(out > 0.8 && out < 1.0, "{out}");

        // A street lamp shines down and out; nothing it lights is above it.
        let street = Lamp {
            beam: Some(Beam {
                pointing: [0.0, -1.0, 0.0],
                inner: 0.7,
                outer: 1.22,
            }),
            ..bulb([0.0, 6.0, 0.0])
        };
        assert_eq!(faces_of(&street), [true, true, false, true, true, true]);
    }

    #[test]
    fn a_soft_edge_is_read_whole_from_the_face_it_falls_on() {
        // Each face of a cube is drawn wider than its quarter turn, by more
        // than the widest soft edge the shader reads round a point.
        let room = (1.0 - 1.0 / GUARD) / 2.0;
        assert!(EDGE_ROOM < room, "{EDGE_ROOM} against {room}");
    }

    #[test]
    fn the_highest_setting_fits_the_slots_there_are() {
        for shadows in [
            LampShadows::Off,
            LampShadows::Low,
            LampShadows::Medium,
            LampShadows::High,
        ] {
            assert!(shadows.lamps() <= MOST_SLOTS);
        }
    }

    #[test]
    fn a_face_holds_only_what_it_sees() {
        let lamp = bulb([0.0, 2.0, 0.0]);
        let numbers = pictured(&lamp, 16.0);
        let east = numbers.face(0, 0, 16.0);
        assert!(east.holds([5.0, 2.0, 0.0], 0.5));
        assert!(!east.holds([-5.0, 2.0, 0.0], 0.5));
        assert!(!east.holds([30.0, 2.0, 0.0], 0.5));
        // Off to the side, but large enough to reach in.
        assert!(east.holds([3.0, 2.0, 5.0], 2.5));
        assert!(!east.holds([3.0, 2.0, 5.0], 0.5));
    }

    fn choose(
        slots: &mut ShadowSlots,
        lamps: &[Lamp],
        eye: [f32; 3],
        how: &Shadowing,
    ) -> Vec<Shadow> {
        let mut near: Vec<(f32, usize)> = lamps
            .iter()
            .enumerate()
            .map(|(number, lamp)| (dot(minus(lamp.at, eye), minus(lamp.at, eye)).sqrt(), number))
            .collect();
        near.sort_by(|one, other| one.0.total_cmp(&other.0).then(one.1.cmp(&other.1)));
        let mut shadows = Vec::new();
        slots.choose(lamps, &near, &vec![true; lamps.len()], how, &mut shadows);
        // Back in the lamps' own order.
        let mut ordered = vec![Shadow::NONE; lamps.len()];
        for (shadow, (_, number)) in shadows.into_iter().zip(near) {
            ordered[number] = shadow;
        }
        ordered
    }

    fn street(count: usize) -> Vec<Lamp> {
        #[allow(clippy::cast_precision_loss)]
        (0..count)
            .map(|number| bulb([0.0, 3.0, -4.0 * number as f32]))
            .collect()
    }

    #[test]
    fn the_nearest_lamps_cast_shadows_and_their_pictures_are_drawn_once() {
        let lamps = street(6);
        let mut slots = ShadowSlots::default();
        let how = Shadowing {
            slots: 2,
            level: 1,
            elapsed: 0.0,
        };
        let shadows = choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &how);
        let cast: Vec<bool> = shadows.iter().map(|shadow| shadow.strength > 0.0).collect();
        assert_eq!(cast, [true, true, false, false, false, false]);
        assert_eq!(slots.faces().len(), 12);
        assert!(
            (shadows[0].strength - 1.0).abs() < 1e-6,
            "the first frame shows them at once"
        );

        // Nothing has moved: nothing is drawn again.
        let how = Shadowing {
            elapsed: 1.0 / 60.0,
            ..how
        };
        let again = choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &how);
        assert_eq!(again, shadows);
        assert!(slots.faces().is_empty());

        // Another level draws them again.
        let other = Shadowing { level: 2, ..how };
        choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &other);
        assert_eq!(slots.faces().len(), 12);
    }

    #[test]
    fn walking_on_hands_the_shadows_over_and_they_fade() {
        let lamps = street(6);
        let mut slots = ShadowSlots::default();
        let how = Shadowing {
            slots: 2,
            level: 1,
            elapsed: 0.0,
        };
        choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &how);
        let how = Shadowing {
            elapsed: 0.05,
            ..how
        };
        // Down the street, past the third lamp, the fourth and fifth matter
        // most: the first two shadows fade out, and then theirs fade in.
        let eye = [0.0, 1.7, -14.0];
        let shadows = choose(&mut slots, &lamps, eye, &how);
        assert!(shadows[0].strength > 0.0 && shadows[0].strength < 1.0);
        assert_eq!(shadows[3].strength, 0.0);
        let mut seen_between = false;
        for _ in 0..20 {
            let now = choose(&mut slots, &lamps, eye, &how);
            seen_between |= now[3].strength > 0.0 && now[3].strength < 1.0;
        }
        assert!(seen_between, "the new shadow faded in");
        let settled = choose(&mut slots, &lamps, eye, &how);
        assert!((settled[3].strength - 1.0).abs() < 1e-6);
        assert!((settled[4].strength - 1.0).abs() < 1e-6);
        assert_eq!(settled[0].strength, 0.0);
    }

    #[test]
    fn a_lamp_that_is_told_not_to_casts_no_shadow_and_a_moving_one_is_drawn_each_frame() {
        let mut lamps = street(3);
        lamps[0].casts_shadows = false;
        let mut slots = ShadowSlots::default();
        let how = Shadowing {
            slots: 1,
            level: 1,
            elapsed: 0.0,
        };
        let shadows = choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &how);
        assert_eq!(shadows[0], Shadow::NONE);
        assert!(shadows[1].strength > 0.0);

        let how = Shadowing {
            elapsed: 1.0 / 60.0,
            ..how
        };
        // Carried along, it is drawn again each frame in its own slot and
        // keeps all of its shadow.
        for _ in 0..5 {
            lamps[1].at[0] += 0.1;
            let moved = choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &how);
            assert_eq!(slots.faces().len(), 6);
            assert!(
                (moved[1].strength - 1.0).abs() < 1e-6,
                "{}",
                moved[1].strength
            );
            assert!(slots.faces().iter().all(|face| face.layer < FACES));
        }
    }

    #[test]
    fn a_new_lamp_waits_for_the_shadow_it_replaces_to_fade() {
        let lamps = street(6);
        let mut slots = ShadowSlots::default();
        let how = Shadowing {
            slots: 1,
            level: 1,
            elapsed: 0.0,
        };
        choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &how);
        let how = Shadowing {
            elapsed: 0.05,
            ..how
        };
        // Down the street the fifth lamp matters most, but the first's
        // shadow fades out before the slot is drawn again.
        let eye = [0.0, 1.7, -16.0];
        let first = choose(&mut slots, &lamps, eye, &how);
        assert!(first[0].strength > 0.5 && first[0].strength < 1.0);
        assert!(slots.faces().is_empty());
        let mut frames = 1;
        while slots.faces().is_empty() {
            choose(&mut slots, &lamps, eye, &how);
            frames += 1;
            assert!(frames < 20, "the slot never came free");
        }
        assert!(frames >= 5, "it waited {frames} frames");
    }

    #[test]
    fn a_flicker_keeps_the_pictures() {
        let mut lamps = street(1);
        lamps[0].reach = 0.0;
        let mut slots = ShadowSlots::default();
        let how = Shadowing {
            slots: 1,
            level: 1,
            elapsed: 0.0,
        };
        choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &how);
        let how = Shadowing {
            elapsed: 1.0 / 60.0,
            ..how
        };
        for flicker in [0.8, 1.2, 0.9, 1.1] {
            lamps[0].candela = 64.0 * flicker;
            choose(&mut slots, &lamps, [0.0, 1.7, 1.0], &how);
            assert!(slots.faces().is_empty(), "{flicker}");
        }
    }
}
