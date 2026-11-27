//! What an NPC asks of its body each tick.

use lore_nav::{Openings, Walkable};
use lore_physics::{Level, MoveInput, Player};

use crate::walk::{Going, Walk, turned_toward};

/// What a mind may read of the world on a tick: the same for every NPC that
/// tick, filled in once by the world before the first of them steers, with
/// nothing set aside for it.
///
/// Every place in it is as of the start of the tick, before anyone moved.
#[derive(Debug, Clone, Copy)]
pub struct Around<'a> {
    /// The level's shapes, to look along (a ray to see by, a ball to fit
    /// through a gap).
    pub level: &'a Level,
    /// Where a body can walk, to find a way across it.
    pub walkable: &'a Walkable,
    /// Which doors on the walkable ground are open.
    pub openings: &'a Openings,
    /// Which tick of the world this is, counted from 1: what a mind that
    /// waits or takes turns counts by.
    pub tick: u32,
    /// How long a tick is, in seconds.
    pub seconds: f32,
    /// Where the player's feet are, in metres.
    pub player: [f32; 3],
    /// Where the feet of every NPC in the world are, in metres, in the order
    /// of their ids: this one's among them, at its body's own position.
    pub npcs: &'a [[f32; 3]],
    /// How every NPC is moving, in the same order as `npcs`: what those
    /// walking round it see.
    pub going: &'a [Going],
}

/// What an NPC wants of its body: the place its rules fill in.
///
/// Each tick the world asks it for a [`MoveInput`] and steps the NPC's body
/// by it, exactly as it steps the player's by what is pressed. So an NPC can
/// go wherever a player could, and nowhere a player could not.
///
/// It is not `Copy`: it holds the way it is walking.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mind {
    /// The way the NPC's body faces, in radians, turning left from -Z as it
    /// grows: the same yaw the player's view has. It turns toward where
    /// the NPC wants to face a little each tick, as a person turns.
    pub facing: f32,
    /// Where it is walking to, and how far along: see [`Walk`].
    pub walk: Walk,
    /// The way it wants to face, which `facing` turns toward.
    wants: f32,
}

impl Mind {
    /// An NPC that stands where it is, facing `facing`.
    #[must_use]
    pub fn standing(facing: f32) -> Self {
        let facing = if facing.is_finite() { facing } else { 0.0 };
        Self {
            facing,
            walk: Walk::default(),
            wants: facing,
        }
    }

    /// What the NPC `me` (its place in `around.npcs`), whose body is
    /// `body`, asks of it for the coming tick, knowing what `around` says
    /// of the world: a step along the way it is walking ([`Walk`]), or to
    /// stand, turning to face the way it wants.
    pub fn steer(&mut self, me: usize, body: &Player, around: &Around<'_>) -> MoveInput {
        let (mut asked, face) = self
            .walk
            .steer(me, body, self.facing, around)
            .unwrap_or_default();
        if let Some(face) = face.filter(|face| face.is_finite()) {
            self.wants = face;
        }
        self.facing = turned_toward(self.facing, self.wants, around.seconds);
        asked.yaw = self.facing;
        asked
    }

    /// The way it wants to face (radians, turning left from -Z), which
    /// `facing` turns toward: what a walk's end asked for, or the last
    /// [`face`](Self::face).
    #[must_use]
    pub fn wants(&self) -> f32 {
        self.wants
    }

    /// Turns it, as it stands, to face `facing` (radians, turning left
    /// from -Z), a little each tick.
    pub fn face(&mut self, facing: f32) {
        if facing.is_finite() {
            self.wants = facing;
        }
    }
}

#[cfg(test)]
mod tests {
    use lore_nav::Body;
    use lore_physics::MoveSettings;

    use super::*;

    #[test]
    fn a_standing_npc_asks_for_no_step_and_faces_its_way() {
        let body = Player::at([0.0; 3], &MoveSettings::default());
        let level = Level::from_triangles(&[]);
        let walkable = Walkable::none(Body::default());
        let openings = Openings::all_open();
        let around = Around {
            level: &level,
            walkable: &walkable,
            openings: &openings,
            tick: 1,
            seconds: 1.0 / 60.0,
            player: [0.0, 0.0, 5.0],
            npcs: &[[0.0; 3]],
            going: &[Going::default()],
        };
        let asked = Mind::standing(1.5).steer(0, &body, &around);
        assert_eq!(asked.forward, 0.0);
        assert_eq!(asked.right, 0.0);
        assert!(!asked.jump);
        assert_eq!(asked.yaw, 1.5);
    }

    #[test]
    fn a_way_to_face_that_is_not_a_number_is_straight_ahead() {
        assert_eq!(Mind::standing(f32::NAN).facing, 0.0);
    }
}
