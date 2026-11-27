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
//! - [`Sides`]: who likes whom: the game's sides, how each feels about the
//!   others, and the grudges each person holds. [`Sides::is_enemy`] is what
//!   a rule's "an enemy" means.
//! - [`Senses`]: what an NPC sees (a cone that walls block) and hears (the
//!   very noises the player hears), kept as plain data for awareness and
//!   rules to read.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod apart;
mod bot;
mod gaze;
mod mind;
mod senses;
mod sides;
mod walk;

pub use apart::{Upright, push_out};
pub use bot::{
    Arms, BETWEEN_BURSTS, BURST, Bot, BotNews, HUNT_FOR, Hands, KEEP_OFF, KEEP_WITHIN, LOOK_FOR,
    Seen, Skill, Spot, Step, Stuck,
};
pub use gaze::{Gaze, HUMAN, NeckLimits, SHARES};
pub use mind::{Around, Mind};
pub use senses::{
    CLEARLY, HEARD_FROM, Heard, KEEP_LOST, Keenness, LOOK_EVERY, LOST_BELOW, Looker, MOST_HEARD,
    MOST_SIGHTED, NIGHT, SEEN_FROM, Seeable, Sensed, Senses, Sight, Sighting, Sound, THROUGH_WALL,
    arriving, daylight, haze, lamp_lux, light_helps, lit, seen_openly, showing,
};
pub use sides::{
    Act, DEFAULT_FORGIVES, DEFAULT_REMEMBERS, FORGIVES_ALL, FORGIVING_SECONDS, Feeling,
    FeelingChange, GRUDGES, LONGEST_NAME, MOST_SIDES, Side, SideSaid, Sides, SidesError, Taken,
    Temper, Toward,
};
pub use walk::{
    Blocker, Face, Goal, Going, News, Passing, STROLL, Trouble, Walk, pass, turned_toward,
};
