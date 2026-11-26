//! Who is in a world: the player, and the training dummies that stand in
//! it to be shot at. The NPCs are in `npcs`, beside the player rather than
//! as more players.

use lore_physics::{MoveInput, Player};
use lore_script::CharacterId;

/// Everything the player is asking for during one tick.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerInput {
    /// Walking, jumping, crouching, sprinting and which way the player
    /// faces.
    pub movement: MoveInput,
    /// Looking up (positive) or down, in radians. Straight up and straight
    /// down are the limits.
    pub pitch: f32,
    /// The trigger is held.
    pub fire: bool,
    /// Aiming down the sights.
    pub aim: bool,
    /// Reload is held.
    pub reload: bool,
    /// The use button (`interact`) is held.
    pub interact: bool,
    /// The dodge button is held.
    pub dodge: bool,
    /// The item button (`use_item`) is held: it throws what is thrown.
    pub use_item: bool,
}

/// The player in a world.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Body {
    /// Where they are and how they are moving.
    pub(crate) player: Player,
    /// What they are asking for: the next tick moves them by it.
    pub(crate) asked: PlayerInput,
}

/// The number a training dummy is known by for as long as it stands in a
/// world ([`World::add_dummy`](super::World::add_dummy)). It is never
/// given to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DummyId(pub(crate) u32);

/// A training dummy: something the size of a standing person that stays
/// where it is put and can be shot. It does nothing by itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Dummy {
    pub(crate) id: DummyId,
    /// Where its feet are, in metres.
    pub(crate) feet: [f32; 3],
    /// Who it is to the game's script.
    pub(crate) name: CharacterId,
}

/// Someone besides the player who can be shot or struck, as the weapons
/// and a swing see them this tick: a training dummy, or an NPC on its feet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Standing {
    /// The number weapons know it by. The player is 0.
    pub(crate) target: u32,
    /// Who it is to the game's script.
    pub(crate) who: CharacterId,
    /// Where its feet are, in metres.
    pub(crate) feet: [f32; 3],
    /// How tall it is, in metres.
    pub(crate) height: f32,
    /// Where its head is, if it has one: a shot there is a head shot.
    pub(crate) head: Option<[f32; 3]>,
}

impl Dummy {
    /// The dummy as something to shoot and strike: the size of a standing
    /// person, its head the top of it.
    pub(crate) fn standing(&self, height: f32) -> Standing {
        Standing {
            target: self.id.0,
            who: self.name,
            feet: self.feet,
            height,
            head: None,
        }
    }
}
