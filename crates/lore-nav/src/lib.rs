//! Walkable ground: where a body of a given size can walk in a level, and
//! the way from one place to another over it.
//!
//! The ground is worked out from the level's own triangles, with nothing to
//! place: [`Walkable::build`] takes the triangles a body collides with and
//! the body's size and abilities ([`Body`]), and gives the floor such a body
//! can stand on, as polygons joined edge to edge. Steps no higher than the
//! body climbs join; a slope steeper than it walks, a ledge higher than a
//! step, a gap narrower than the body and a ceiling lower than it crouches
//! do not. A passage the body only fits through crouching is kept, and said
//! to be one ([`Leg::Crouch`]).
//!
//! The ground is cut into square tiles of about ten metres, so a change in
//! one place rebuilds the tiles there ([`Walkable::rebuild`]), not the
//! world.
//!
//! On it, [`Walkable::way`] finds the shortest way from one point to
//! another as a list of corners ([`Way`]), [`Walkable::nearest`] the
//! walkable point nearest a place, and [`Walkable::can_reach`] whether one
//! place can be reached from another at all.
//!
//! Some ways open and shut while a game runs. A door ([`Gate`]) is part of
//! the ground, marked: shut, nobody walks through it. A [`Link`] joins two
//! places the floor does not, such as a jump, a ladder or a platform that
//! carries people across. Which are open is said with [`Openings`], kept by
//! whoever runs the world and handed to each question, so the ground itself
//! never changes when a door opens.
//!
//! Everything here is a pure function of its input: the same triangles give
//! the same polygons, and the same question the same answer. Maths whose
//! digits could differ between platforms (sines, cosines) goes through
//! software functions (`libm`), the builder's too, and ties are settled by
//! order, never by a hash or a clock.

mod build;
mod walkable;
mod way;

#[cfg(test)]
mod tests;

pub use build::Shapes;
pub use walkable::{Islands, Problem, Spot, Walkable};
pub use way::{Leg, NoWay, Scratch, Way, Waypoint};

/// The size and abilities of what walks: the ground is built for one body.
///
/// The defaults are a person's, the same as the character controller's own
/// (`lore_physics::MoveSettings`), so the ground says where a player could
/// walk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// Half its width, in metres: how far from a wall its middle stays.
    pub radius: f32,
    /// How tall it stands, in metres.
    pub height: f32,
    /// How tall it is crouching, in metres. Ground with room over it for
    /// this but not for `height` is walked crouching.
    pub crouch_height: f32,
    /// The highest step it walks up without jumping, in metres.
    pub climb: f32,
    /// The steepest slope it walks up, in degrees from level.
    pub steepest_slope: f32,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            radius: 0.35,
            height: 1.8,
            crouch_height: 1.1,
            climb: 0.4,
            steepest_slope: 45.0,
        }
    }
}

/// How finely the ground is measured. The defaults suit a person: a finer
/// grain finds narrower ways and takes longer to build.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grain {
    /// The side of one cell across the ground, in metres.
    pub cell: f32,
    /// The height of one cell, in metres.
    pub cell_height: f32,
    /// How many cells a tile has along each side.
    pub tile_cells: u16,
}

impl Default for Grain {
    fn default() -> Self {
        Self {
            cell: 0.15,
            cell_height: 0.1,
            // 9.6 m: a door that is moved rebuilds little, and a tile is
            // quick to build.
            tile_cells: 64,
        }
    }
}

/// A door: a place on the ground that is walked through when it is open and
/// not when it is shut. The ground under it, and as far round it as the
/// body is wide, is the gate's.
///
/// Gates are numbered by their place in the list the ground is built with
/// ([`Shapes::gates`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    /// Its outline seen from above, as `[x, z]` corners in order round it.
    /// It must not bend inwards.
    pub outline: Vec<[f32; 2]>,
    /// The height of its foot, in metres.
    pub low: f32,
    /// The height of its top, in metres.
    pub high: f32,
}

impl Gate {
    /// The gate of an upright box: `middle` is its middle, `half` half its
    /// size along its own x, y and z, and it is turned `yaw` radians to the
    /// left about the upright, as a thing in a map is.
    #[must_use]
    pub fn of_box(middle: [f32; 3], half: [f32; 3], yaw: f32) -> Self {
        let (sin, cos) = (libm::sinf(yaw), libm::cosf(yaw));
        // A turn to the left about +Y: +X goes towards -Z.
        let corner =
            |x: f32, z: f32| [middle[0] + x * cos + z * sin, middle[2] - x * sin + z * cos];
        let [hx, hy, hz] = half;
        Self {
            outline: vec![
                corner(-hx, -hz),
                corner(-hx, hz),
                corner(hx, hz),
                corner(hx, -hz),
            ],
            low: middle[1] - hy,
            high: middle[1] + hy,
        }
    }
}

/// Which gate: its place in the list the ground was built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GateId(pub u32);

/// Which link: the number [`Walkable::add_link`] gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LinkId(pub u32);

/// How a link is crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinkKind {
    /// A jump across a gap or up onto a ledge.
    Jump,
    /// Climbing a ladder.
    Ladder,
    /// Riding something that moves, such as a platform or a lift.
    Ride,
}

/// A way between two places the floor does not join: a jump, a ladder, a
/// platform. Open until said otherwise ([`Openings::set_link`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Link {
    /// Where it starts, on or near the ground.
    pub from: [f32; 3],
    /// Where it ends, on or near the ground.
    pub to: [f32; 3],
    /// Whether it is crossed from `to` to `from` as well.
    pub both_ways: bool,
    /// How it is crossed.
    pub kind: LinkKind,
}

/// Which gates and links are shut. Everything is open until it is said to
/// be shut, so a fresh `Openings` has every way open.
///
/// Kept apart from the ground so that the ground can be shared, and never
/// changes when a door opens. Shutting a gate or link with a higher number
/// than any before sets memory aside; nothing else does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Openings {
    shut_gates: Vec<bool>,
    shut_links: Vec<bool>,
}

impl Openings {
    /// Every way open.
    #[must_use]
    pub fn all_open() -> Self {
        Self::default()
    }

    /// Opens or shuts a gate.
    pub fn set_gate(&mut self, gate: GateId, open: bool) {
        set(&mut self.shut_gates, gate.0, !open);
    }

    /// Opens or shuts a link.
    pub fn set_link(&mut self, link: LinkId, open: bool) {
        set(&mut self.shut_links, link.0, !open);
    }

    /// Whether a gate is open.
    #[must_use]
    pub fn gate_open(&self, gate: GateId) -> bool {
        !self
            .shut_gates
            .get(gate.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Whether a link is open.
    #[must_use]
    pub fn link_open(&self, link: LinkId) -> bool {
        !self
            .shut_links
            .get(link.0 as usize)
            .copied()
            .unwrap_or(false)
    }
}

/// Sets `flags[index]`, growing the list if it is too short. A flag past
/// the end reads `false`, so setting one there to `false` changes nothing.
fn set(flags: &mut Vec<bool>, index: u32, value: bool) {
    let index = index as usize;
    if index >= flags.len() {
        if !value {
            return;
        }
        flags.resize(index + 1, false);
    }
    flags[index] = value;
}
