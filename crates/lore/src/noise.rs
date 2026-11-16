//! The noises players make by walking and by using what they hold.
//!
//! A noise is something that happened in the game, not something the
//! speakers were told: the server works out each tick whose foot came down
//! and whose weapon fired, and keeps the list
//! ([`HostedGame::noises`](crate::online::HostedGame::noises)) with where
//! each was made and how loud. The player at this computer hears them
//! because their game reads that list, and anything else in the game that
//! listens can read the same one.

use lore_audio::{Feet, Footfall, FootstepSettings, Footsteps, Place, Play, Range};
use lore_net::{MAX_PLAYERS, PlayerId, Server};
use lore_script::ScriptHost;

use crate::shooting::{Names, alive, name_of};
use crate::sounds::Sounds;

/// What made a noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoiseKind {
    /// One foot coming down while walking, sprinting or creeping.
    Footstep,
    /// Both feet, after a jump or a fall.
    Landing,
    /// A weapon firing. A shotgun blast is one.
    Gunshot,
    /// The click of a trigger pulled on an empty magazine.
    DryFire,
    /// A reload beginning.
    Reload,
}

impl NoiseKind {
    /// The name of the sound it is played with, as a script names it
    /// (`game.audio.play("footstep")`). A file of that name in the game's
    /// `sounds` folder takes the engine's place.
    #[must_use]
    pub const fn sound(self) -> &'static str {
        match self {
            Self::Footstep => "footstep",
            Self::Landing => "landing",
            Self::Gunshot => "gunshot",
            Self::DryFire => "dry-fire",
            Self::Reload => "reload",
        }
    }

    /// How far it carries at full loudness: `far` is where it can no
    /// longer be heard at all, in metres.
    #[must_use]
    pub const fn range(self) -> Range {
        match self {
            Self::Footstep | Self::Landing => Range::FOOTSTEP,
            Self::Gunshot => Range::GUNFIRE,
            Self::DryFire | Self::Reload => Range {
                near: 1.0,
                far: 50.0,
            },
        }
    }
}

/// One noise made in the game during a tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Noise {
    /// What made it.
    pub kind: NoiseKind,
    /// The player who made it.
    pub by: PlayerId,
    /// Where it was made, in metres: the feet for a step or a landing,
    /// the eyes for what a weapon does.
    pub position: [f32; 3],
    /// How loud it was, from 0 to 1: a crouched step is a quarter as loud
    /// as a sprinting one, and a gunshot is 1.
    pub loudness: f32,
    /// How long after the tick began it was made, in seconds: a weapon
    /// that fires faster than the game ticks fires partway through one.
    pub delay: f32,
    /// The server's tick it was made on.
    pub tick: u32,
}

/// Works out when each player's feet come down.
#[derive(Debug)]
pub(crate) struct Walking {
    feet: [Footsteps; MAX_PLAYERS],
    settings: FootstepSettings,
}

impl Default for Walking {
    fn default() -> Self {
        Self {
            feet: [Footsteps::default(); MAX_PLAYERS],
            settings: FootstepSettings::default(),
        }
    }
}

impl Walking {
    /// Forgets how far `id` has walked: for a player who has joined, left
    /// or been put somewhere new, so the next step is counted from there.
    pub(crate) fn forget(&mut self, id: PlayerId) {
        if let Some(feet) = self.feet.get_mut(usize::from(id)) {
            *feet = Footsteps::default();
        }
    }

    /// Adds to `noises` the steps and landings of the tick `server` has
    /// just run: call it after [`Server::tick`], every tick. `step` is the
    /// length of a tick in seconds. The dead make no noise.
    pub(crate) fn tick(
        &mut self,
        server: &Server,
        names: &Names,
        host: &ScriptHost,
        step: f32,
        noises: &mut Vec<Noise>,
    ) {
        for (id, player) in server.players() {
            let Some(walked) = self.feet.get_mut(usize::from(id)) else {
                continue;
            };
            if !name_of(names, id.into()).is_some_and(|name| alive(host, name)) {
                *walked = Footsteps::default();
                continue;
            }
            let [x, y, z] = player.velocity;
            let feet = Feet {
                speed: (x * x + z * z).sqrt(),
                falling: -y,
                grounded: player.grounded,
                crouched: player.crouched,
                sprinting: player.sprinting,
            };
            let (kind, loudness) = match walked.step(&self.settings, &feet, step) {
                Some(Footfall::Step { volume, .. }) => (NoiseKind::Footstep, volume),
                Some(Footfall::Landing { volume }) => (NoiseKind::Landing, volume),
                None => continue,
            };
            noises.push(Noise {
                kind,
                by: id,
                position: player.position,
                loudness,
                delay: 0.0,
                tick: server.tick_number(),
            });
        }
    }
}

/// What to hand the speakers so that the player at this computer hears
/// `noise`: equally in both ears when it is their `own`, and from where it
/// was made otherwise. `nth` is its place among the noises of its tick, so
/// two at once do not play the same take. `None` when the game has no
/// sound of that name ready.
pub(crate) fn play(noise: &Noise, own: bool, nth: u32, sounds: &mut Sounds) -> Option<Play> {
    let seed = noise
        .tick
        .wrapping_mul(31)
        .wrapping_add(u32::from(noise.by).wrapping_mul(7919))
        .wrapping_add(nth);
    let mut play = sounds.take(noise.kind.sound(), seed)?;
    if !own {
        play.place = Place::At(noise.position);
    }
    play.volume *= noise.loudness;
    play.range = noise.kind.range();
    Some(play.after(noise.delay))
}
