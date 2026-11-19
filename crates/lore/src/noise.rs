//! The noises the player makes by walking and by using what they hold.
//!
//! A noise is something that happened in the game, not something the
//! speakers were told: the world works out each tick whether a foot came
//! down and what the weapon did, and keeps the list
//! ([`World::noises`](crate::world::World::noises)) with where each was
//! made and how loud. The player hears them because their game reads that
//! list, and anything else in the game that listens can read the same one.

use lore_audio::{Feet, Footfall, FootstepSettings, Footsteps, Play, Range};

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
    /// A body dropping into water.
    Splash,
    /// A foot coming down in shallow water, or something small going in.
    SmallSplash,
    /// A step through water up to the knees or deeper.
    Wade,
    /// One stroke of a swimmer at the surface.
    SwimStroke,
    /// The head coming up out of the water.
    Surface,
    /// Air let go under water.
    Bubbles,
    /// Something burning in a liquid that burns: acid, lava.
    Sizzle,
    /// Thunder from a stroke of lightning close enough for its crack to
    /// be heard.
    Thunder,
    /// Thunder from far off: the rumble alone.
    FarThunder,
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
            Self::Splash => "splash",
            Self::SmallSplash => "splash-small",
            Self::Wade => "wade",
            Self::SwimStroke => "swim-stroke",
            Self::Surface => "surface",
            Self::Bubbles => "bubbles",
            Self::Sizzle => "sizzle",
            Self::Thunder => "thunder",
            Self::FarThunder => "thunder-far",
        }
    }

    /// Whether it is a foot coming down on the ground: a step or a landing.
    #[must_use]
    pub const fn is_footfall(self) -> bool {
        matches!(self, Self::Footstep | Self::Landing)
    }

    /// How far it carries at full loudness: `far` is where it can no
    /// longer be heard at all, in metres.
    #[must_use]
    pub const fn range(self) -> Range {
        match self {
            Self::Footstep
            | Self::Landing
            | Self::SmallSplash
            | Self::Wade
            | Self::SwimStroke
            | Self::Surface
            | Self::Sizzle => Range::FOOTSTEP,
            Self::Splash => Range {
                near: 2.0,
                far: 70.0,
            },
            Self::Bubbles => Range {
                near: 1.0,
                far: 15.0,
            },
            Self::Gunshot => Range::GUNFIRE,
            // Thunder is heard a long way off: ten miles, on a still night.
            Self::Thunder => Range {
                near: 400.0,
                far: 16_000.0,
            },
            Self::FarThunder => Range {
                near: 3_000.0,
                far: 30_000.0,
            },
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
    /// Where it was made, in metres: the feet for a step or a landing,
    /// the eyes for what a weapon does.
    pub position: [f32; 3],
    /// How loud it was, from 0 to 1: a crouched step is a quarter as loud
    /// as a sprinting one, and a gunshot is 1.
    pub loudness: f32,
    /// How high and fast it sounds: 1 is as its sound was recorded, and a
    /// half is an octave lower and twice as long. What is done in a thick
    /// or a heavy fluid sounds lower than in water.
    pub pitch: f32,
    /// How long after the tick began it was made, in seconds: a weapon
    /// that fires faster than the game ticks fires partway through one.
    pub delay: f32,
    /// The tick it was made on.
    pub tick: u32,
}

/// Works out when the player's feet come down. A new one has walked
/// nowhere yet: start again with one when the player is put somewhere new,
/// so the next step is counted from there.
#[derive(Debug, Default)]
pub(crate) struct Walking {
    feet: Footsteps,
    settings: FootstepSettings,
}

impl Walking {
    /// Adds to `noises` the step or landing of the tick numbered `tick`,
    /// which has just moved `player`: call it every tick, after they have
    /// moved. `step` is the length of a tick in seconds.
    pub(crate) fn tick(
        &mut self,
        player: &lore_physics::Player,
        tick: u32,
        step: f32,
        noises: &mut Vec<Noise>,
    ) {
        let [x, y, z] = player.velocity;
        let feet = Feet {
            speed: (x * x + z * z).sqrt(),
            falling: -y,
            grounded: player.grounded,
            crouched: player.crouched,
            sprinting: player.sprinting,
        };
        let (kind, loudness) = match self.feet.step(&self.settings, &feet, step) {
            Some(Footfall::Step { volume, .. }) => (NoiseKind::Footstep, volume),
            Some(Footfall::Landing { volume }) => (NoiseKind::Landing, volume),
            None => return,
        };
        noises.push(Noise {
            kind,
            position: player.position,
            loudness,
            pitch: 1.0,
            delay: 0.0,
            tick,
        });
    }
}

/// What to hand the speakers so that the player hears `noise`, which they
/// made themself: equally in both ears. `nth` is its place among the
/// noises of its tick, so two at once do not play the same take. `None`
/// when the game has no sound of that name ready.
pub(crate) fn play(noise: &Noise, nth: u32, sounds: &mut Sounds) -> Option<Play> {
    let seed = noise.tick.wrapping_mul(31).wrapping_add(nth);
    let mut play = sounds.take(noise.kind.sound(), seed)?;
    play.volume *= noise.loudness;
    if noise.pitch.is_finite() {
        play.pitch = (play.pitch * noise.pitch).clamp(0.25, 4.0);
    }
    play.range = noise.kind.range();
    Some(play.after(noise.delay))
}
