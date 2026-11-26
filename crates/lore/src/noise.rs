//! The noises the player makes by walking and by using what they hold.
//!
//! A noise is something that happened in the game, not something the
//! speakers were told: the world works out each tick whether a foot came
//! down and what the weapon did, and keeps the list
//! ([`World::noises`](crate::world::World::noises)) with where each was
//! made and how loud. The player hears them because their game reads that
//! list, and anything else in the game that listens can read the same one.

use lore_audio::{Feet, Footfall, FootstepSettings, Footsteps, Play, Range};

use lore_script::CharacterId;
#[cfg(test)]
use lore_script::SoundName;

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
    /// Fluid coming down on the ground or a wall: a stream, a spill.
    Patter,
    /// Fluid coming down into a pool.
    Pour,
    /// Thunder from a stroke of lightning close enough for its crack to
    /// be heard.
    Thunder,
    /// Thunder from far off: the rumble alone.
    FarThunder,
    /// A rocket or grenade going off, heard from where it did.
    Explosion,
    /// A bullet or a rocket striking a surface, heard from where it did.
    Impact,
    /// A sound a script played once at a place (`game.audio.playAt`): in
    /// the list so the NPCs hear it, but neither played again from it (the
    /// script's own request plays it) nor told to `game.onNoise`.
    Script,
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
            Self::Patter => "drip",
            Self::Pour => "splash-small",
            Self::Thunder => "thunder",
            Self::FarThunder => "thunder-far",
            Self::Explosion => "explosion",
            Self::Impact => "impact",
            Self::Script => "",
        }
    }

    /// Whether it is made somewhere in the world and heard from there,
    /// not by the player themself, whose own noises are heard equally in
    /// both ears.
    #[must_use]
    pub const fn is_at_a_place(self) -> bool {
        matches!(
            self,
            Self::Patter | Self::Pour | Self::Explosion | Self::Impact | Self::Script
        )
    }

    /// Whether the player made it, as against the world: thunder, dripping
    /// fluid, a blast and a strike are the world's.
    #[must_use]
    pub const fn is_by_player(self) -> bool {
        !matches!(
            self,
            Self::Patter
                | Self::Pour
                | Self::Explosion
                | Self::Impact
                | Self::Script
                | Self::Thunder
                | Self::FarThunder
        )
    }

    /// Whether it is the sound of the world going on (thunder, fluid
    /// pattering or pouring) rather than something someone did. NPCs keep
    /// it among what they heard, but it does not draw their attention and
    /// is not told to their `onHear`: in rain it would come several times a
    /// second. The one place that says which kinds are.
    #[must_use]
    pub const fn is_of_the_world(self) -> bool {
        matches!(
            self,
            Self::Thunder | Self::FarThunder | Self::Patter | Self::Pour
        )
    }

    /// Whether a weapon makes it: a shot, its dry click or reload, a blast,
    /// a bullet striking.
    #[must_use]
    pub const fn is_of_a_weapon(self) -> bool {
        matches!(
            self,
            Self::Gunshot | Self::DryFire | Self::Reload | Self::Explosion | Self::Impact
        )
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
            | Self::Sizzle
            | Self::Pour => Range::FOOTSTEP,
            Self::Splash => Range {
                near: 2.0,
                far: 70.0,
            },
            Self::Bubbles | Self::Patter => Range {
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
            // A blast is heard across a valley.
            Self::Explosion => Range {
                near: 10.0,
                far: 300.0,
            },
            Self::Impact => Range {
                near: 1.0,
                far: 40.0,
            },
            Self::DryFire | Self::Reload => Range {
                near: 1.0,
                far: 50.0,
            },
            // Most sounds' reach; a script's own carries as it is played.
            Self::Script => Range {
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
    /// How far it carries. The kind's own range unless what made it
    /// (a weapon that says `heardWithin`) set another.
    pub range: Range,
    /// The sound it is played with, when what made it named one (a weapon's
    /// `fireSound`). `None` plays the kind's own: [`NoiseKind::sound`].
    /// A short id into the world's [`Names`], not the name itself.
    pub sound: Option<NameId>,
    /// The weapon that made it, for a shot, a dry click or a reload.
    pub weapon: Option<NameId>,
    /// Who made it: the player for a step, a landing, a shot or a splash;
    /// nobody for thunder, dripping fluid, a blast or a strike.
    pub by: Option<CharacterId>,
    /// Set when a script's `game.onNoise` function returned `false`: the
    /// noise stays in the list, for anything that listens, but is not played.
    pub silenced: bool,
}

impl Noise {
    /// A noise of `kind` made at `position`, with the kind's own sound and
    /// range.
    #[must_use]
    pub fn new(
        kind: NoiseKind,
        position: [f32; 3],
        loudness: f32,
        pitch: f32,
        delay: f32,
        tick: u32,
    ) -> Self {
        Self {
            kind,
            position,
            loudness,
            pitch,
            delay,
            tick,
            range: kind.range(),
            sound: None,
            weapon: None,
            by: kind.is_by_player().then_some(CharacterId::PLAYER),
            silenced: false,
        }
    }

    /// How a script's `game.onNoise` function is told of it.
    #[must_use]
    pub fn told<'a>(&self, names: &'a Names) -> lore_script::NoiseTold<'a> {
        lore_script::NoiseTold {
            sound: self.sound_name(names),
            position: self.position,
            loudness: self.loudness,
            heard_within: self.range.far,
            by: self.by,
            weapon: self.weapon.map(|id| names.get(id)),
        }
    }

    /// The name of the sound it is played with.
    #[must_use]
    pub fn sound_name<'a>(&self, names: &'a Names) -> &'a str {
        self.sound
            .map_or_else(|| self.kind.sound(), |id| names.get(id))
    }
}

/// What a noise an NPC heard was: its kind and the sound it was played
/// with, kept with what the NPC heard (`lore_npc::Heard::what`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Which {
    /// What made it.
    pub kind: NoiseKind,
    /// The sound it was played with, when not the kind's own: a short id
    /// into the world's [`Names`].
    pub sound: Option<NameId>,
}

/// A short id for a name in [`Names`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameId(std::num::NonZeroU16);

/// The sound and weapon names noises carry by id. A world fills it in the
/// order names are first met in its ticks, so the same input gives the same
/// ids, in a replay too.
#[derive(Debug, Default, Clone)]
pub struct Names {
    names: Vec<Box<str>>,
}

impl Names {
    /// The id of `name`, adding it the first time. `None` only after
    /// 65 535 different names, when the noise keeps the kind's own sound.
    pub fn intern(&mut self, name: &str) -> Option<NameId> {
        let at = match self.names.iter().position(|known| &**known == name) {
            Some(at) => at,
            // A full table takes no more names, so it cannot grow for ever.
            None if self.names.len() >= usize::from(u16::MAX) => return None,
            None => {
                self.names.push(name.into());
                self.names.len() - 1
            }
        };
        let id = u16::try_from(at + 1).ok()?;
        std::num::NonZeroU16::new(id).map(NameId)
    }

    /// The name an id stands for.
    #[must_use]
    pub fn get(&self, id: NameId) -> &str {
        self.names
            .get(usize::from(id.0.get()) - 1)
            .map_or("", |name| name)
    }
}

// A town of listeners keeps many noises: it stays small.
const _: () = assert!(std::mem::size_of::<Noise>() <= 56);

/// Works out when the player's feet come down. A new one has walked
/// nowhere yet: start again with one when the player is put somewhere new,
/// so the next step is counted from there.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Walking {
    feet: Footsteps,
}

impl Walking {
    /// Adds to `noises` the step or landing of the tick numbered `tick`,
    /// which has just moved `player`: call it every tick, after they have
    /// moved. `step` is the length of a tick in seconds, and `said` is what
    /// the script set for the stride and the loudness of steps.
    pub(crate) fn tick(
        &mut self,
        player: &lore_physics::Player,
        tick: u32,
        step: f32,
        said: &lore_script::MoveSaid,
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
        let usual = FootstepSettings::default();
        // The sprinting and crouching strides keep the engine's proportion
        // to the walking one.
        let scale = said.stride / usual.walk_stride;
        let settings = FootstepSettings {
            walk_stride: said.stride,
            sprint_stride: usual.sprint_stride * scale,
            crouch_stride: usual.crouch_stride * scale,
            ..usual
        };
        let (kind, loudness) = match self.feet.step(&settings, &feet, step) {
            Some(Footfall::Step { volume, .. }) => (NoiseKind::Footstep, volume),
            Some(Footfall::Landing { volume }) => (NoiseKind::Landing, volume),
            None => return,
        };
        let loudness = loudness * said.step_loudness;
        noises.push(Noise::new(kind, player.position, loudness, 1.0, 0.0, tick));
    }
}

/// What to hand the speakers so that the player hears `noise`, which they
/// made themself: equally in both ears. `nth` is its place among the
/// noises of its tick, so two at once do not play the same take. `None`
/// when the game has no sound of that name ready.
///
/// `tweak` is what the script did to the sound the noise is played with
/// (`game.audio.setVolume`, `game.audio.replace`): it changes what is
/// played and nothing about the noise.
pub(crate) fn play(
    noise: &Noise,
    nth: u32,
    names: &Names,
    sounds: &mut Sounds,
    tweak: lore_script::SoundTweak,
) -> Option<Play> {
    let seed = noise.tick.wrapping_mul(31).wrapping_add(nth);
    // A script's sound is already played by its own request.
    if noise.silenced || tweak.volume <= 0.0 || noise.kind == NoiseKind::Script {
        return None;
    }
    // A replacement the game has no sound ready for must not silence the
    // engine's own: fall back to the sound it would have played.
    let mut play = tweak
        .replace
        .as_ref()
        .and_then(|name| sounds.take(name.as_str(), seed))
        .or_else(|| sounds.take(noise.sound_name(names), seed))?;
    play.volume *= noise.loudness * tweak.volume;
    if noise.pitch.is_finite() {
        play.pitch = (play.pitch * noise.pitch).clamp(0.25, 4.0);
    }
    play.range = noise.range;
    // What someone else made is heard from where they made it.
    let by_another = noise.by.is_some_and(|by| by != CharacterId::PLAYER);
    if noise.kind.is_at_a_place() || by_another {
        play.place = lore_audio::Place::At(noise.position);
    }
    // A noise of the world keeps the world's time: it waits while the
    // game is paused and is slow in slow motion.
    Some(play.after(noise.delay).in_world())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lore_script::{MoveSaid, SoundTweak};

    #[test]
    fn a_replacement_with_no_sound_ready_plays_the_engines_own() {
        let mut sounds = Sounds::starter();
        let noise = Noise::new(NoiseKind::Footstep, [0.0; 3], 0.6, 1.0, 0.0, 1);
        let swapped = |name: &str| SoundTweak {
            volume: 1.0,
            replace: SoundName::new(name),
        };
        // The game has no "boots" sound, so the starter footstep plays.
        assert!(play(&noise, 0, &Names::default(), &mut sounds, swapped("boots")).is_some());
        // One it has is played in its place.
        assert!(
            play(
                &noise,
                0,
                &Names::default(),
                &mut sounds,
                swapped("landing")
            )
            .is_some()
        );
        // Turned right down, nothing plays.
        let off = SoundTweak {
            volume: 0.0,
            replace: None,
        };
        assert!(play(&noise, 0, &Names::default(), &mut sounds, off).is_none());
    }

    #[test]
    fn names_get_ids_in_the_order_they_are_met() {
        let mut names = Names::default();
        let bow = names.intern("bow").unwrap();
        let twang = names.intern("twang").unwrap();
        assert_ne!(bow, twang);
        assert_eq!(names.intern("bow"), Some(bow));
        assert_eq!(names.get(twang), "twang");
        let mut again = Names::default();
        assert_eq!(again.intern("bow"), Some(bow));
    }

    #[test]
    fn the_stride_a_script_starts_with_is_the_engines_own() {
        assert_eq!(
            MoveSaid::default().stride,
            FootstepSettings::default().walk_stride
        );
    }
}
