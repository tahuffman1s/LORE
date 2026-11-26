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
//!   [`lore_physics::MoveInput`]: to stand, or to walk somewhere.
//! - [`Walk`]: walking somewhere, round corners and through doors, past
//!   others, and why it cannot get there ([`Trouble`]) when it cannot.
//! - [`Gaze`]: where a head turns to look at someone, within what a person's
//!   neck allows, and how quickly it gets there.
//! - [`push_out`]: how far to move one upright body out of another's way.

mod apart;
mod gaze;
mod mind;
mod walk;

pub use apart::{Upright, push_out};
pub use gaze::{Gaze, HUMAN, NeckLimits, SHARES};
pub use mind::{Around, Mind};
pub use walk::{
    Blocker, Face, Goal, Going, News, Passing, STROLL, Trouble, Walk, pass, turned_toward,
};
