//! What an NPC asks of its body each tick.

use lore_physics::{MoveInput, Player};

/// What an NPC wants of its body: the place its rules fill in.
///
/// Each tick the world asks it for a [`MoveInput`] and steps the NPC's body
/// by it, exactly as it steps the player's by what is pressed. So an NPC can
/// go wherever a player could, and nowhere a player could not.
///
/// It is small and `Copy`: what a save keeps of it is these fields.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mind {
    /// The way the NPC faces while it stands, in radians, turning left from
    /// -Z as it grows: the same yaw the player's view has.
    pub facing: f32,
}

impl Mind {
    /// An NPC that stands where it is, facing `facing`.
    #[must_use]
    pub fn standing(facing: f32) -> Self {
        Self {
            facing: if facing.is_finite() { facing } else { 0.0 },
        }
    }

    /// What the NPC asks of its body for the coming tick. Today it stands
    /// still and faces the way it was put; the rules that make it walk to
    /// a place or follow someone answer here.
    #[must_use]
    pub fn steer(&self, _body: &Player) -> MoveInput {
        MoveInput {
            yaw: self.facing,
            ..MoveInput::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use lore_physics::MoveSettings;

    use super::*;

    #[test]
    fn a_standing_npc_asks_for_no_step_and_faces_its_way() {
        let body = Player::at([0.0; 3], &MoveSettings::default());
        let asked = Mind::standing(1.5).steer(&body);
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
