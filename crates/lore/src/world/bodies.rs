//! The players in a world: where each one is, what each is asking for,
//! and who each is to the game's script.

use lore_physics::{MoveInput, Player};

/// The most players a world holds.
pub const MAX_PLAYERS: usize = 64;

/// A player's number in a world, from 0 to one less than [`MAX_PLAYERS`].
/// The script's "Player 1" is number 0.
pub type PlayerId = u8;

/// Everything a player is asking for during one tick.
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
}

/// One player in a world.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Body {
    /// Where they are and how they are moving.
    pub(crate) player: Player,
    /// What they are asking for: the next tick moves them by it.
    pub(crate) asked: PlayerInput,
    /// Who they are to the game's script.
    pub(crate) name: lore_script::PlayerId,
}

/// Everyone in a world, each at the place their number names.
#[derive(Debug, Clone)]
pub(crate) struct Bodies([Option<Body>; MAX_PLAYERS]);

impl Default for Bodies {
    fn default() -> Self {
        Self([None; MAX_PLAYERS])
    }
}

impl Bodies {
    /// The lowest number nobody has, if the world has room for one more.
    pub(crate) fn free(&self) -> Option<PlayerId> {
        let free = self.0.iter().position(Option::is_none)?;
        PlayerId::try_from(free).ok()
    }

    /// Gives `body` the number `id`, in place of whoever had it.
    pub(crate) fn put(&mut self, id: PlayerId, body: Body) {
        if let Some(place) = self.0.get_mut(usize::from(id)) {
            *place = Some(body);
        }
    }

    /// Takes out whoever has the number `id`.
    pub(crate) fn take(&mut self, id: PlayerId) -> Option<Body> {
        self.0.get_mut(usize::from(id))?.take()
    }

    pub(crate) fn get(&self, id: PlayerId) -> Option<&Body> {
        self.0.get(usize::from(id))?.as_ref()
    }

    pub(crate) fn get_mut(&mut self, id: PlayerId) -> Option<&mut Body> {
        self.0.get_mut(usize::from(id))?.as_mut()
    }

    /// Everyone, lowest number first.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (PlayerId, &Body)> + '_ {
        (0..)
            .zip(&self.0)
            .filter_map(|(id, body)| Some((id, body.as_ref()?)))
    }

    /// Everyone, lowest number first, to move them.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Body> + '_ {
        self.0.iter_mut().flatten()
    }

    /// Who the target numbered `target` is to the script.
    pub(crate) fn name_of(&self, target: u32) -> Option<lore_script::PlayerId> {
        let id = PlayerId::try_from(target).ok()?;
        self.get(id).map(|body| body.name)
    }

    /// The number of the player the script knows as `name`.
    pub(crate) fn named(&self, name: lore_script::PlayerId) -> Option<PlayerId> {
        self.iter()
            .find(|(_, body)| body.name == name)
            .map(|(id, _)| id)
    }
}
