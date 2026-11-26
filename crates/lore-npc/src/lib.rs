//! What the people and creatures of a LORE world are and decide, as plain
//! data and pure functions.
//!
//! Nothing here holds a world, a script or a picture. The world keeps each
//! NPC's body (a [`lore_physics::Player`], moved exactly as the player's is)
//! and asks this crate what to do with it; the picture asks it where a head
//! turns. The same numbers in always give the same numbers out, on every
//! computer, so an NPC is as repeatable as the rest of the simulation.
//!
//! - [`Mind`]: what an NPC asks of its body each tick, as a
//!   [`lore_physics::MoveInput`]. Today it stands, facing one way; the
//!   rules that walk, follow and flee fill it in.
//! - [`Gaze`]: where a head turns to look at someone, within what a person's
//!   neck allows, and how quickly it gets there.
//! - [`push_out`]: how far to move one upright body out of another's way.
//! - [`Sides`]: who likes whom: the game's sides, how each feels about the
//!   others, and the grudges each person holds. [`Sides::is_enemy`] is what
//!   a rule's "an enemy" means.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod apart;
mod gaze;
mod mind;
mod sides;

pub use apart::{Upright, push_out};
pub use gaze::{Gaze, HUMAN, NeckLimits, SHARES};
pub use mind::{Around, Mind};
pub use sides::{
    Act, DEFAULT_FORGIVES, DEFAULT_REMEMBERS, FORGIVES_ALL, FORGIVING_SECONDS, Feeling,
    FeelingChange, GRUDGES, LONGEST_NAME, MOST_SIDES, Side, SideSaid, Sides, SidesError, Taken,
    Temper, Toward,
};
