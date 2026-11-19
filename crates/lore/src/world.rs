//! The world of a game and the player in it.
//!
//! [`World`] is everything the game's rules know: the level, where the
//! player is, what they hold and the game's script. It is stepped on the
//! fixed tick, and the same input always gives the same world.
//!
//! [`Playing`] is a world with the controls of this computer in the
//! player's hands. Each tick it reads the controls and steps the world;
//! each frame it looks from the player's eyes. It is what `lore run` plays.

use lore_assets::Scene;
use lore_input::{ActionId, ActionMap, InputFrame};
use lore_physics::{MoveInput, MoveMode, MoveSettings, Player};
use lore_script::{CharacterId, Motion, Movement};

use crate::blend;
use crate::game::{Frame, Game};
use crate::look;
use crate::noise::{self, Noise, Walking};
use crate::script_game::ScriptGame;
use crate::shooting::Shooting;
use crate::sound_output::SoundAsk;

mod bodies;
mod climate;
mod fluids;
mod ground;
mod spawn;
mod trail;
mod wet;

pub(crate) use bodies::{Body, Dummy};
pub use bodies::{DummyId, PlayerInput};
pub(crate) use climate::said as said_weather;
pub(crate) use ground::Ground;
pub use ground::{WalkCheck, try_walking};
pub use wet::WaterMark;

/// What a training dummy is called to the game's script.
const DUMMY_NAME: &str = "Training dummy";

/// A game's world: its script, its level and the player in it.
///
/// There is one player, from the moment the world is made. Each tick
/// ([`Game::tick`]) it moves them by what they are asking for
/// ([`ask`](Self::ask)), fires the weapon the script has handed them and
/// then runs the script's tick. Nothing in it reads a clock or rolls a die
/// of its own, so a world given the same input tick after tick goes the
/// same way every time.
///
/// Besides the player a world can hold training dummies
/// ([`add_dummy`](Self::add_dummy)), which stand still to be shot at. The
/// people of a game will stand beside them, as characters of their own
/// and not as more players.
#[derive(Debug)]
pub struct World {
    game: ScriptGame,
    ground: Ground,
    player: Body,
    /// The training dummies, in the order they were put in.
    dummies: Vec<Dummy>,
    /// The number the next dummy is known by. The player is 0 to a weapon.
    next_dummy: u32,
    /// How many ticks have been run.
    tick: u32,
    /// The weapon the player holds, and what they have launched.
    shooting: Shooting,
    /// When the player's feet come down.
    walking: Walking,
    /// The noises the player made in the newest tick.
    noises: Vec<Noise>,
    /// What the player did to the water in the newest tick.
    marks: Vec<WaterMark>,
    /// The trail the player is leaving in the snow, and where they pressed
    /// it down in the newest tick.
    trail: trail::Trail,
    treads: Vec<lore_render::Tread>,
    /// How many times the player has been put at a spawn point.
    spawned: u32,
    /// The weather, and what has followed from it: the snow that lies,
    /// the ice, how far rain has raised the water, the tornado.
    climate: climate::Climate,
    /// Lightning: when the sky is next looked over for a storm to strike
    /// from (a tick), the dice it is looked over with, the strokes that
    /// have fallen since the last frame, and the thunder still on its way:
    /// the tick it arrives, where the stroke fell and how far off that is.
    looks_at: u32,
    dice: u32,
    strokes: Vec<lore_render::Lightning>,
    thunder: Vec<(u32, [f32; 3], f32)>,
    /// Whether the player was put somewhere since this was last taken, by
    /// the script or by a respawn, and the way to face there, if that was
    /// said: yaw, then pitch. [`Playing`] cuts to the new place and turns
    /// the view to match.
    teleported: Option<Option<(f32, f32)>>,
    /// Every drop of fluid in the world, and what the script said of them.
    fluids: fluids::WorldFluids,
}

/// The unit vector the eyes look along at `yaw` and `pitch`, as the
/// player physics has it: yaw 0 looks along -Z and turns left as it grows.
fn aim_of(yaw: f32, pitch: f32) -> [f32; 3] {
    let (sin, cos) = (libm::sinf(yaw), libm::cosf(yaw));
    let (sin_pitch, cos_pitch) = (libm::sinf(pitch), libm::cosf(pitch));
    [-sin * cos_pitch, sin_pitch, -cos * cos_pitch]
}

/// The yaw and pitch that look along `facing`: the other way round from
/// [`aim_of`]. Looking straight up or down has no side to face, so the
/// player's `yaw` stays; the pitch is held to what the view allows.
fn facing_of(facing: [f32; 3], yaw: f32) -> (f32, f32) {
    let [x, y, z] = facing;
    let across = libm::hypotf(x, z);
    let yaw = if across < 1e-6 {
        yaw
    } else {
        libm::atan2f(-x, -z)
    };
    crate::look::turned(yaw, libm::atan2f(y, across), [0.0; 2])
}

/// How the script reads that `player` is moving, looking along `aim`.
fn motion_of(player: &Player, aim: [f32; 3]) -> Motion {
    Motion {
        velocity: player.velocity,
        eye: player.eye_position(),
        aim,
        grounded: player.grounded,
    }
}

/// The way of moving the script asked for, as the physics knows it.
fn mode_of(movement: Movement) -> MoveMode {
    match movement {
        Movement::Walk => MoveMode::Walk,
        Movement::Fly => MoveMode::Fly,
        Movement::Noclip => MoveMode::Noclip,
    }
}

/// How fast sound goes, in metres a second.
const SOUND: f32 = 343.0;
/// The most strokes kept for one frame.
const MOST_STROKES: usize = 8;
/// The most thunder on its way at once, and how far off a stroke can fall
/// and still be heard, in metres. A sky full of storms strikes somewhere
/// within thirty kilometres every few seconds, and thunder from the far
/// ones is a minute and more in coming: so there is room for all that is
/// on its way from within hearing, and what is not heard is not kept.
const MOST_THUNDER: usize = 32;
const THUNDER_HEARD: f32 = 25_000.0;

/// Tells `host` what pools `level` has and how each stands before
/// anything has happened in it, as a world that plays the level does.
pub(crate) fn tell_pools(
    level: &std::sync::Arc<lore_physics::Level>,
    host: &mut lore_script::ScriptHost,
) {
    fluids::WorldFluids::default().take_level(level, host);
}

impl World {
    /// The world of `game` with its player in it, on a flat floor at
    /// height zero until it is given a level
    /// ([`with_level`](Self::with_level)).
    ///
    /// `tick_rate` must be the rate the world is then ticked at.
    #[must_use]
    pub fn new(game: ScriptGame, tick_rate: u32) -> Self {
        Self::on(game, Ground::flat(tick_rate), None)
    }

    /// The world of `game` on `ground`, with the player's feet at `here`
    /// or, with `None`, at a spawn point, facing the way it does.
    fn on(mut game: ScriptGame, ground: Ground, here: Option<[f32; 3]>) -> Self {
        let spawned = 1;
        let player = match here {
            Some(here) => Player::at(here, &ground.rules),
            None => ground.spawn_in(game.map(), std::iter::empty(), spawned),
        };
        let mut asked = PlayerInput::default();
        asked.movement.yaw = ground.facing_at(game.map(), player.position).unwrap_or(0.0);
        game.host_mut()
            .set_position(CharacterId::PLAYER, player.position);
        let aim = aim_of(asked.movement.yaw, 0.0);
        game.host_mut()
            .set_motion(CharacterId::PLAYER, motion_of(&player, aim));
        Self {
            game,
            ground,
            player: Body { player, asked },
            dummies: Vec::new(),
            next_dummy: 1,
            tick: 0,
            shooting: Shooting::default(),
            walking: Walking::default(),
            noises: Vec::new(),
            marks: Vec::new(),
            trail: trail::Trail::default(),
            treads: Vec::with_capacity(8),
            spawned,
            climate: climate::Climate::default(),
            looks_at: 60,
            dice: 0x2F6E_2B1D,
            strokes: Vec::with_capacity(MOST_STROKES),
            thunder: Vec::with_capacity(MOST_THUNDER),
            teleported: None,
            fluids: fluids::WorldFluids::default(),
        }
    }

    /// Plays the game in `level`, usually the game's `level.glb` as
    /// [`level::load`](crate::level::load) reads it: its shapes are solid,
    /// and the player is put at one of its spawn points.
    /// Without this, the player walks a flat floor at height zero.
    ///
    /// The player starts at the `spawn` entities of the game's map file
    /// ([`ScriptGame::map`]). When it has none they start at the level's
    /// markers whose names begin with `spawn` (`spawn`, `spawn.001`,
    /// `Spawn Red`), and with none of those either, at the level's middle.
    ///
    /// Call it first, before the world is ticked or anything is put in
    /// it. What is wrong with the level for walking, such as a spawn point
    /// with no floor under it, is logged as a warning ([`try_walking`]
    /// gives the same list).
    #[must_use]
    pub fn with_level(mut self, level: &Scene) -> Self {
        // Whatever was put in the world before it had its level is not in
        // the new one.
        for dummy in self.dummies.drain(..) {
            self.game.host_mut().remove_character(dummy.name);
        }
        // The level's lamps are the script's to find by name and change.
        self.game
            .host_mut()
            .set_level_lamps(&crate::lamps::of_level(level));
        let ground = Ground::of(Some(level), self.game.map(), self.ground.tick_rate);
        let mut world = Self::on(self.game, ground, None);
        // The level's still water is the world's pools, of the kinds its
        // sheets say.
        (world.fluids).take_level(&world.ground.level, world.game.host_mut());
        world
    }

    /// The game being played.
    #[must_use]
    pub fn game(&self) -> &ScriptGame {
        &self.game
    }

    /// The game being played, to change it.
    pub fn game_mut(&mut self) -> &mut ScriptGame {
        &mut self.game
    }

    /// A number from 0 to 1, different each time and the same on every
    /// run.
    fn dealt(&mut self) -> f32 {
        self.dice ^= self.dice << 13;
        self.dice ^= self.dice >> 17;
        self.dice ^= self.dice << 5;
        // The top 24 bits are exact as an f32.
        #[allow(clippy::cast_precision_loss)]
        let share = (self.dice >> 8) as f32 / 16_777_216.0;
        share
    }

    /// Lets lightning fall at `place`, a place on the ground: the bolt is
    /// seen in the next frame, and its thunder is heard when the sound has
    /// come from there. If more thunder is on its way than is kept, the
    /// farthest gives way to a nearer, and the bolt is seen all the same.
    ///
    /// Only the picture of it waits for a frame. With no frame drawn for
    /// a while, more bolts than one frame can show are not kept to be
    /// seen: but the dice are thrown, the thunder is on its way and the
    /// script is told all the same, so that the world is the same world
    /// however often it is drawn.
    fn strike(&mut self, place: [f32; 3]) {
        let seed = self.dice;
        self.dealt();
        if self.strokes.len() < MOST_STROKES {
            self.strokes
                .push(lore_render::Lightning::down_to(place, seed));
        }
        let eye = self.player.player.position;
        let away = (0..3)
            .map(|axis| (place[axis] - eye[axis]).powi(2))
            .sum::<f32>()
            .sqrt();
        // Whole ticks: sound takes three seconds a kilometre.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ticks = (away / SOUND / self.ground.step.max(1e-4)).min(1e6) as u32;
        let thunder = (self.tick.saturating_add(ticks), place, away);
        if away <= THUNDER_HEARD {
            if self.thunder.len() < MOST_THUNDER {
                self.thunder.push(thunder);
            } else if let Some(farthest) =
                (self.thunder.iter_mut()).max_by(|one, other| one.2.total_cmp(&other.2))
                && farthest.2 > away
            {
                *farthest = thunder;
            }
        }
        // The script hears of it: where it struck.
        self.game.host_mut().lightning_struck(place);
    }

    /// Lightning: every few seconds a few places within thirty kilometres of the player are
    /// looked at, and the first with a thunder cloud over it is struck,
    /// on whatever ground is under it.
    fn storm(&mut self) {
        if self.tick < self.looks_at {
            return;
        }
        let step = self.ground.step.max(1e-4);
        // Truncation is the point: whole ticks.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let wait = ((1.6 + 2.4 * self.dealt()) / step) as u32;
        self.looks_at = self.tick.saturating_add(wait.max(1));
        let (seconds, weather) = (self.sky_seconds(), *self.climate.weather());
        let [x, _, z] = self.player.player.position;
        for _ in 0..4 {
            let spot = [
                x + (self.dealt() - 0.5) * 60_000.0,
                z + (self.dealt() - 0.5) * 60_000.0,
            ];
            if weather.storm_at(spot, seconds) <= 0.5 {
                continue;
            }
            // The ground under it, or a little under where levels are
            // built if the level does not reach so far.
            let ground = (self.ground.level)
                .raycast([spot[0], 4_000.0, spot[1]], [0.0, -1.0, 0.0], 8_000.0)
                .map_or([spot[0], -20.0, spot[1]], |hit| hit.point);
            self.strike(ground);
            return;
        }
    }

    /// How long the world has run, in seconds of its own time: what the
    /// sky's clock shows.
    fn sky_seconds(&self) -> f64 {
        f64::from(self.tick) * f64::from(self.ground.step)
    }

    /// What the sky's clock shows in a frame drawn `alpha` of the way from
    /// the tick before the newest to the newest, as the view is: between
    /// the two, so that clouds, rain and what sways move on every frame
    /// drawn and not only on the frames a tick ran before. The rules
    /// themselves only ever see whole ticks.
    fn sky_seconds_drawn(&self, alpha: f32) -> f64 {
        let alpha = if alpha.is_finite() {
            f64::from(alpha.clamp(0.0, 1.0))
        } else {
            1.0
        };
        ((f64::from(self.tick) - 1.0 + alpha) * f64::from(self.ground.step)).max(0.0)
    }

    /// How far the rain that has fallen has raised the level's still
    /// water, in metres.
    #[must_use]
    pub fn water_rise(&self) -> f32 {
        self.climate.water_rise()
    }

    /// How wet the rain that has fallen has left the ground, and how full
    /// its puddles, counted tick by tick as [`GroundWater`](lore_render::GroundWater)
    /// says: a downpour fills them in a minute, a drizzle leaves a few.
    #[must_use]
    pub fn ground_water(&self) -> lore_render::GroundWater {
        self.climate.ground_water()
    }

    /// The weather of the newest tick, with what has followed from it: how
    /// far the wind has carried the clouds, the snow that lies, the ice on
    /// the water and the tornado if there is one. The picture is drawn in
    /// it ([`Playing`] hands it over each frame).
    #[must_use]
    pub fn weather(&self) -> &lore_render::Weather {
        self.climate.weather()
    }

    /// How many ticks the world has run.
    #[must_use]
    pub fn tick_number(&self) -> u32 {
        self.tick
    }

    /// The player: where they are and how they are moving.
    #[must_use]
    pub fn player(&self) -> &Player {
        &self.player.player
    }

    /// What the player is asking for: what [`ask`](Self::ask) last said,
    /// or, before anything was asked, standing still and facing the way
    /// their spawn point does.
    #[must_use]
    pub fn asked(&self) -> PlayerInput {
        self.player.asked
    }

    /// Says what the player is asking for. Every tick from the next on
    /// moves them by it, and works their weapon by it, until something
    /// else is asked.
    pub fn ask(&mut self, asked: PlayerInput) {
        self.player.asked = asked;
    }

    /// The noises the player made in the newest tick, in the order they
    /// were made: every footstep and landing, every shot, the click of an
    /// empty weapon and each reload, with where it was made and how loud.
    /// The player hears what is in this list ([`Playing`]), and so can
    /// anything else that listens.
    #[must_use]
    pub fn noises(&self) -> &[Noise] {
        &self.noises
    }

    /// What the player did to the water in the newest tick, in the order
    /// it happened: each splash of going in, of a stroke or of a foot in
    /// the shallows, and each lot of bubbles let go under it. The player
    /// sees what is in this list ([`Playing`]): drops, rings and bubbles.
    #[must_use]
    pub fn water_marks(&self) -> &[WaterMark] {
        &self.marks
    }

    /// Where the player pressed the snow down in the newest tick: a print
    /// where a foot came down, and the furrow of wading through snow that
    /// is deep. The player sees what is in this list ([`Playing`]): tracks.
    #[must_use]
    pub fn treads(&self) -> &[lore_render::Tread] {
        &self.treads
    }

    /// Stands a training dummy with its feet at `feet`: something the
    /// size of a standing person that stays where it is put and can be
    /// shot. It does nothing by itself, and the script meets it only as
    /// the `hit.victim` of a shot that lands on it, called
    /// "Training dummy".
    ///
    /// It is here so that shooting can be tried and tested before a game
    /// has people in it.
    pub fn add_dummy(&mut self, feet: [f32; 3]) -> DummyId {
        let id = DummyId(self.next_dummy);
        self.next_dummy = self.next_dummy.saturating_add(1);
        let host = self.game.host_mut();
        let name = host.add_character(DUMMY_NAME);
        host.set_position(name, feet);
        self.dummies.push(Dummy { id, feet, name });
        id
    }

    /// Where the feet of the dummy numbered `id` are, if it is still in
    /// the world.
    #[must_use]
    pub fn dummy(&self, id: DummyId) -> Option<[f32; 3]> {
        let dummy = self.dummies.iter().find(|dummy| dummy.id == id)?;
        Some(dummy.feet)
    }

    /// Puts the feet of the dummy numbered `id` at `feet`. A shot is
    /// judged against where it then stands.
    pub fn move_dummy(&mut self, id: DummyId, feet: [f32; 3]) {
        if let Some(dummy) = self.dummies.iter_mut().find(|dummy| dummy.id == id) {
            dummy.feet = feet;
            self.game.host_mut().set_position(dummy.name, feet);
        }
    }

    /// Takes the dummy numbered `id` out of the world.
    pub fn remove_dummy(&mut self, id: DummyId) {
        if let Some(index) = self.dummies.iter().position(|dummy| dummy.id == id) {
            let dummy = self.dummies.remove(index);
            self.game.host_mut().remove_character(dummy.name);
        }
    }

    /// Puts the player back at one of the map's spawn points, away from
    /// any dummy standing on one, if they have died since the last tick:
    /// facing the way the spawn point does, as at the start, and seen
    /// there at once rather than glided to.
    fn respawn(&mut self) {
        if !self.game.host_mut().take_respawn() {
            return;
        }
        self.spawned = self.spawned.wrapping_add(1);
        let standing = self.dummies.iter().map(|dummy| dummy.feet);
        let Body { player, asked } = &mut self.player;
        *player = self
            .ground
            .spawn_in(self.game.map(), standing, self.spawned);
        let facing = (self.ground)
            .facing_at(self.game.map(), player.position)
            .map(|yaw| (yaw, 0.0));
        if let Some((yaw, pitch)) = facing {
            asked.movement.yaw = yaw;
            asked.movement.pitch = pitch;
            asked.pitch = pitch;
        }
        // The script is told this tick, and reads where they now are.
        // Being put somewhere is not a step taken or a fall ended.
        self.walking = Walking::default();
        self.teleported = Some(facing);
    }

    /// Puts the player where the script moved them with
    /// `game.player:teleport`, if it did since the last tick: still, and
    /// facing as it said. The script reads the new place at once, and
    /// [`Playing`] is told to turn the view.
    fn teleport(&mut self) {
        let Some(teleport) = self.game.host_mut().take_teleport() else {
            return;
        };
        let Body { player, asked } = &mut self.player;
        player.position = teleport.position;
        player.velocity = [0.0; 3];
        // Whatever they were doing where they were is over: a climb out of
        // the water does not go on where they are now, and the first step
        // there finds out whether they stand.
        player.hoist = 0.0;
        player.grounded = false;
        // Being put somewhere is not a step taken or a fall ended.
        self.walking = Walking::default();
        let facing = teleport
            .facing
            .map(|way| facing_of(way, asked.movement.yaw));
        if let Some((yaw, pitch)) = facing {
            asked.movement.yaw = yaw;
            asked.movement.pitch = pitch;
            asked.pitch = pitch;
        }
        let aim = aim_of(asked.movement.yaw, asked.pitch);
        let host = self.game.host_mut();
        host.set_position(CharacterId::PLAYER, player.position);
        host.set_motion(CharacterId::PLAYER, motion_of(player, aim));
        // A teleport that says no way to face, in the tick of a respawn
        // that did, leaves the view facing the spawn's way.
        self.teleported = Some(facing.or(self.teleported.flatten()));
    }

    /// Whether the player was put somewhere since this was last asked, by
    /// the script or by a respawn, and the yaw and pitch to face there, if
    /// either said.
    pub(crate) fn take_teleported(&mut self) -> Option<Option<(f32, f32)>> {
        self.teleported.take()
    }
}

impl Game for World {
    /// Steps the world by one tick. `input` is what the game's script reads
    /// (`game.input`); the player moves by what was asked of them
    /// ([`World::ask`]).
    fn tick(&mut self, input: &InputFrame, step: f32) {
        // Time stood still: only the script runs, so that it hears the
        // player and can start time again. Nothing in the world moves,
        // and what the last tick made is not made again.
        if self.game.host().time_speed() == 0.0 {
            self.noises.clear();
            self.marks.clear();
            self.treads.clear();
            self.game.tick(input, step);
            self.teleport();
            // What the script asked of the fluids is done, but nothing
            // moves: a clear still clears, and a spray hangs in the air.
            self.fluids.hear(self.game.host_mut());
            return;
        }
        // A player who died since the last tick is back at a spawn point
        // before they move.
        self.respawn();
        self.tick = self.tick.saturating_add(1);
        // The weather moves on: what falls on the player raises the water
        // or settles as snow, the cold freezes the lakes, the tornado
        // travels. It is said to the level before anything is moved in it,
        // since the level's shapes are shared.
        let seconds = self.sky_seconds();
        self.climate.step(
            self.game.host_mut(),
            &self.ground.level,
            self.player.player.position,
            seconds,
            self.ground.step,
        );
        self.storm();
        // The fluids move, with what the script asked of them in its last
        // tick, and what is now under the player's feet is found.
        self.fluids.hear(self.game.host_mut());
        self.fluids.step(
            &self.ground.level,
            self.ground.step,
            &self.player.player,
            (self.climate.water_rise(), self.climate.ice()),
        );
        let Ground {
            level,
            rules,
            step: tick,
            ..
        } = &self.ground;
        let Body { player, asked } = &mut self.player;
        // How the script wants the player to move, and how fast it flies.
        let mut moving = asked.movement;
        moving.mode = mode_of(self.game.host().movement());
        let settings = MoveSettings {
            fly_speed: self.game.host().fly_speed(),
            ..*rules
        };
        let rules = &settings;
        if moving.mode == MoveMode::Walk {
            // A gale pushes the player, a tornado takes them off their feet,
            // and deep snow is slow to walk through. None of it reaches
            // someone who flies.
            self.climate.blow(player, rules.gravity, seconds, *tick);
            if player.grounded {
                let slowed = self.climate.snow_slows();
                moving.forward *= slowed;
                moving.right *= slowed;
            }
            // Fluid underfoot: oil slides, honey drags, glue holds, slime
            // bounces.
            // What a pool does to someone in it, by how heavy and how
            // thick it is, is the level's to say as they move.
            moving.footing = self.fluids.footing();
        }
        let before = *player;
        let walking = moving.mode == MoveMode::Walk;
        player.step(level, rules, &moving, *tick);
        // The script reads where the player is as of this tick, in what it
        // is told of below and in its own tick, and how they are moving.
        let host = self.game.host_mut();
        host.set_position(CharacterId::PLAYER, player.position);
        let aim = aim_of(asked.movement.yaw, asked.pitch);
        host.set_motion(CharacterId::PLAYER, motion_of(player, aim));
        // What was heard of the tick just run: when a foot came down, and
        // below, what the weapon did.
        self.noises.clear();
        self.walking
            .tick(player, self.tick, *tick, &mut self.noises);
        // And what their feet did to the snow, if any lies.
        self.treads.clear();
        let landed = (self.noises.last())
            .is_some_and(|noise| noise.kind == crate::noise::NoiseKind::Landing);
        self.trail.tick(
            player,
            asked.movement.yaw,
            (self.climate.snow(), landed),
            &mut self.treads,
        );
        // Thunder that has got here from a stroke that fell a while ago.
        let (now, noises) = (self.tick, &mut self.noises);
        self.thunder.retain(|(arrives, position, away)| {
            if *arrives > now {
                return true;
            }
            noises.push(Noise {
                kind: if *away < 1_500.0 {
                    crate::noise::NoiseKind::Thunder
                } else {
                    crate::noise::NoiseKind::FarThunder
                },
                position: *position,
                loudness: 1.0,
                pitch: 1.0,
                delay: 0.0,
                tick: now,
            });
            false
        });
        // What they did to the water, if they are in any: seen and heard.
        self.marks.clear();
        wet::tick(
            &before,
            player,
            level,
            asked.movement.yaw,
            (self.tick, *tick),
            &mut self.noises,
            &mut self.marks,
        );
        // A foot that came down in a puddle came down in what the puddle
        // is: a splash in water, a squelch in honey or mud.
        if let Some((pitch, quiet)) = self.fluids.puddle_voice()
            && let Some(footfall) = (self.noises.iter_mut().rev())
                .find(|noise| noise.tick == self.tick && noise.kind.is_footfall())
        {
            footfall.kind = crate::noise::NoiseKind::SmallSplash;
            footfall.loudness = (footfall.loudness * 1.2).clamp(0.2, 1.0) * quiet;
            footfall.pitch = pitch;
        }
        // Fluid that came down in this tick is heard from where it did.
        for fall in self.fluids.falls() {
            self.noises.push(Noise {
                kind: if fall.into_pool {
                    crate::noise::NoiseKind::Pour
                } else {
                    crate::noise::NoiseKind::Patter
                },
                position: fall.place,
                loudness: fall.loudness,
                pitch: fall.pitch,
                delay: 0.0,
                tick: self.tick,
            });
        }
        // What they threw up is drops of what they are in.
        self.fluids.splash(level, &self.marks);
        // The weapon the player holds, by what they are pressing. The
        // script hears of every shot, hit and reload here.
        self.shooting.tick(
            &self.player,
            &self.dummies,
            self.tick,
            level,
            rules,
            *tick,
            self.game.host_mut(),
            &mut self.noises,
        );
        // The script hears how much fluid there is and what the player
        // stands in, before its own tick. Only someone walking with their
        // feet down stands in anything.
        let standing = self.player.player.grounded && walking;
        let hiss = self.fluids.tell(
            self.game.host_mut(),
            level,
            *tick,
            (&self.player.player, standing),
        );
        // What burns them is heard burning them.
        if let Some(loudness) = hiss {
            self.noises.push(Noise {
                kind: crate::noise::NoiseKind::Sizzle,
                position: self.player.player.position,
                loudness,
                pitch: 1.0,
                delay: 0.0,
                tick: self.tick,
            });
        }
        self.game.tick(input, step);
        // The script may have put the player somewhere.
        self.teleport();
        // Lightning the script asked for in this tick falls where it asked,
        // on whatever is highest there: taken here, before a frame can show
        // it with no thunder to follow.
        for (place, _) in self.game.host_mut().take_strikes() {
            let [x, _, z] = place;
            let struck = (self.ground.level)
                .raycast([x, 4_000.0, z], [0.0, -1.0, 0.0], 8_000.0)
                .map_or(place, |hit| hit.point);
            self.strike(struck);
        }
    }

    /// The script's own presenting, such as its HUD and its rumble.
    fn frame(&mut self, frame: &mut Frame) {
        // The sky and the water are drawn where the rules have them, at
        // the moment between the last two ticks the view is drawn at.
        let alpha = frame.alpha();
        frame.set_sky_clock(self.sky_seconds_drawn(alpha));
        frame.set_water_rise(self.climate.water_rise());
        frame.set_ground_water(self.climate.ground_water());
        // The world sends the bolts, so that it knows where each fell.
        frame.set_storm_bolts(false);
        for stroke in self.strokes.drain(..) {
            frame.strike(stroke);
        }
        self.fluids.frame(frame);
        self.game.frame(frame);
        // After the script's own frame, which says the weather as it
        // asked for it: this is the weather with what followed from it.
        frame.set_weather(self.climate.weather_drawn(alpha));
    }
}

/// The actions that move the player, looked up once. An action the map does
/// not have reads as not pressed.
#[derive(Debug, Clone, Copy)]
struct Controls {
    walk: Option<ActionId>,
    look: Option<ActionId>,
    jump: Option<ActionId>,
    crouch: Option<ActionId>,
    sprint: Option<ActionId>,
    fire: Option<ActionId>,
    aim: Option<ActionId>,
    reload: Option<ActionId>,
}

impl Controls {
    fn new(actions: &ActionMap) -> Self {
        let find = |name| actions.action(name).ok();
        Self {
            walk: find("move"),
            look: find("look"),
            jump: find("jump"),
            crouch: find("crouch"),
            sprint: find("sprint"),
            fire: find("fire"),
            aim: find("aim"),
            reload: find("reload"),
        }
    }

    /// What `input` asks of the player for one tick of `step` seconds. The
    /// view, `yaw` and `pitch`, is turned first.
    fn asked(self, input: &InputFrame, step: f32, yaw: &mut f32, pitch: &mut f32) -> PlayerInput {
        let held = |action: Option<ActionId>| action.is_some_and(|action| input.pressed(action));
        let [right, forward] = self.walk.map_or([0.0; 2], |walk| input.axis2d(walk));
        if let Some(look) = self.look {
            // Everything the frames since the last tick showed being turned
            // by, so what is asked for here (a step, a shot) goes where the
            // player was looking on the screen.
            let turn = look::asked(input, look, step);
            (*yaw, *pitch) = look::turned(*yaw, *pitch, turn);
        }
        PlayerInput {
            movement: MoveInput {
                forward,
                right,
                yaw: *yaw,
                jump: held(self.jump),
                crouch: held(self.crouch),
                sprint: held(self.sprint),
                pitch: *pitch,
                ..MoveInput::default()
            },
            pitch: *pitch,
            fire: held(self.fire),
            aim: held(self.aim),
            reload: held(self.reload),
        }
    }
}

/// A view that moves further than this in one tick, in metres, has been put
/// somewhere new (a respawn, a teleport) and is not drawn gliding there.
/// Falling at terminal speed at 20 ticks a second is under 3 m a tick.
const CUT_METRES: f32 = 5.0;

/// Where the player's eyes are and which way they face.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Seen {
    eye: [f32; 3],
    yaw: f32,
    pitch: f32,
}

/// Whether eyes went further from `before` to `now` than anyone walks or
/// falls in one tick. A distance that is not a number counts as far.
fn cut(before: [f32; 3], now: [f32; 3]) -> bool {
    let moved: f32 = (0..3).map(|axis| (now[axis] - before[axis]).powi(2)).sum();
    moved.is_nan() || moved > CUT_METRES * CUT_METRES
}

/// A world with the controls of this computer in its player's hands.
///
/// Each tick it turns the view by what the mouse and the stick did, asks
/// the world for what is pressed and steps the world. Each frame it looks
/// from the player's eyes and plays what was heard.
#[derive(Debug)]
pub struct Playing {
    world: World,
    controls: Controls,
    yaw: f32,
    pitch: f32,
    /// Where the eyes were after the newest tick.
    eye: [f32; 3],
    /// Where the eyes were on the tick before the newest, to draw frames
    /// between the two from. `None` when the newest tick is to be drawn as
    /// it is: before the first tick, and when the eyes were put somewhere
    /// new.
    before: Option<[f32; 3]>,
    /// The noises of the ticks since the last frame, for the frame to play.
    heard: Vec<Noise>,
    /// What was done to the water in the ticks since the last frame, for
    /// the frame to show.
    marks: Vec<WaterMark>,
    /// Where the snow was pressed down in the ticks since the last frame,
    /// for the frame to show.
    treads: Vec<lore_render::Tread>,
    /// Whether the rumble of being under water is playing.
    under: bool,
    /// How loud each of the weather's own sounds was last said to be, if
    /// it is playing: the rain, the wind and the roar of a tornado.
    weather_heard: [Option<f32>; 3],
    /// Where the roar of the tornado was last said to come from.
    roar_from: [f32; 3],
}

/// The most marks on the water kept for a frame to show.
const MOST_MARKS: usize = 64;

/// What the rumble heard with the head under water is known by, so that
/// it can be stopped: no script's sound has this number.
const UNDER_WATER: lore_script::SoundId = lore_script::SoundId(u64::MAX);

/// What the weather's own sounds are known by: the rain, the wind, and the
/// roar of a tornado. Each plays for as long as there is any of it.
const WEATHER_SOUNDS: [lore_script::SoundId; 3] = [
    lore_script::SoundId(u64::MAX - 1),
    lore_script::SoundId(u64::MAX - 2),
    lore_script::SoundId(u64::MAX - 3),
];

/// How far off a tornado is heard at its loudest, and how far off at all,
/// in metres: its roar carries for kilometres.
const TORNADO_HEARD: lore_audio::Range = lore_audio::Range {
    near: 120.0,
    far: 4_000.0,
};

/// The most noises kept for a frame to play. With more than this in a
/// quarter of a second the newest are left out: nobody could tell them
/// apart.
const MOST_HEARD: usize = 128;

/// A noise this many seconds old when a frame is next drawn is not
/// played: a window that drew nothing for a while does not owe a burst of
/// everything that happened meanwhile.
const HEARD_FOR: f32 = 0.25;

/// Whether `noise` is new enough to play on the tick `now`.
#[allow(clippy::cast_precision_loss)]
fn fresh(noise: &Noise, now: u32, tick_rate: u32) -> bool {
    now.wrapping_sub(noise.tick) as f32 <= HEARD_FOR * tick_rate as f32
}

impl Playing {
    /// The action set the game's script has chosen: `gameplay` unless it
    /// said otherwise.
    #[must_use]
    pub fn action_set(&self) -> lore_input::ActionSetId {
        self.world.game().host().action_set()
    }

    /// Starts `game` with its player in the world. The player walks
    /// `level`, usually the game's `level.glb` as
    /// [`level::load`](crate::level::load) reads it (see
    /// [`World::with_level`]), or a flat floor with `None`, and starts at
    /// a spawn point, facing the way it does.
    ///
    /// The player is moved by the actions in `actions` called `move`,
    /// `look`, `jump`, `crouch`, `sprint`, `fire`, `aim` and `reload`.
    /// `actions` and `tick_rate` must be the ones the game is then run
    /// with.
    #[must_use]
    pub fn start(
        mut game: ScriptGame,
        level: Option<&Scene>,
        actions: &ActionMap,
        tick_rate: u32,
    ) -> Self {
        // The level's lamps are the script's to find by name and change.
        if let Some(level) = level {
            game.host_mut()
                .set_level_lamps(&crate::lamps::of_level(level));
        }
        let ground = Ground::of(level, game.map(), tick_rate);
        Self::enter(World::on(game, ground, None), actions)
    }

    /// Starts `game` on `ground` with its player in the world, as the
    /// editor does to play from where its camera is: the player's eyes
    /// start at `eye`, looking the way `yaw` and `pitch` say
    /// ([`Frame::look_from`]), and the player drops to the floor from
    /// there.
    ///
    /// With nothing under `eye` to land on, the player starts as in any
    /// game, at a spawn point, and `false` is given with the game.
    #[cfg(feature = "editor")]
    pub(crate) fn start_from(
        game: ScriptGame,
        ground: &Ground,
        actions: &ActionMap,
        eye: [f32; 3],
        [yaw, pitch]: [f32; 2],
    ) -> (Self, bool) {
        let feet = ground
            .feet_under(eye)
            .filter(|_| yaw.is_finite() && pitch.is_finite());
        let mut playing = Self::enter(World::on(game, ground.clone(), feet), actions);
        if feet.is_some() {
            (playing.yaw, playing.pitch) = look::turned(yaw, pitch, [0.0; 2]);
        }
        (playing, feet.is_some())
    }

    /// Takes the controls of `world`'s player.
    fn enter(world: World, actions: &ActionMap) -> Self {
        let yaw = world.asked().movement.yaw;
        let eye = world.player().eye_position();
        Self {
            world,
            controls: Controls::new(actions),
            yaw,
            pitch: 0.0,
            eye,
            before: None,
            heard: Vec::with_capacity(MOST_HEARD),
            marks: Vec::with_capacity(MOST_MARKS),
            treads: Vec::with_capacity(MOST_MARKS),
            under: false,
            weather_heard: [None; 3],
            roar_from: [0.0; 3],
        }
    }

    /// The world being played in.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// The world being played, to change it.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Where the player's eyes were after the newest tick. A frame is drawn
    /// from between there and where they were a tick earlier.
    #[must_use]
    pub fn eye_position(&self) -> [f32; 3] {
        self.eye
    }

    /// Which way the player faces, in radians, turning left from -Z as it
    /// grows.
    #[must_use]
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// How far the player looks up (positive) or down, in radians.
    #[must_use]
    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// The view as of the newest tick.
    fn seen(&self) -> Seen {
        Seen {
            eye: self.eye,
            yaw: self.yaw,
            pitch: self.pitch,
        }
    }

    /// The view to draw a frame from that is `alpha` of the way from the
    /// tick before to the newest tick, and has turned by `turn` (to the
    /// right, then upwards) since the newest.
    ///
    /// The eyes are between where the two ticks had them. The angles are
    /// not blended: they are the newest tick's plus the turn no tick has
    /// seen yet, so the view is never behind the hand.
    fn seen_at(&self, alpha: f32, turn: [f32; 2]) -> Seen {
        let now = self.seen();
        let (yaw, pitch) = look::turned(now.yaw, now.pitch, turn);
        Seen {
            eye: match self.before {
                Some(before) => blend::position(before, now.eye, alpha),
                None => now.eye,
            },
            yaw,
            pitch,
        }
    }
}

impl Playing {
    /// Shows and sounds what the player is doing to the water: the marks
    /// of the ticks since the last frame, the rings and wake of a body
    /// that is in it now, and the hush of a head that is under it.
    fn show_water(&mut self, frame: &mut Frame, eye: [f32; 3]) {
        for mark in self.marks.drain(..) {
            match mark {
                // The white spray of a splash is water's. What is thrown
                // out of honey or lava is drops of that, which the world's
                // fluids have already thrown.
                WaterMark::Splash { place, .. }
                    if !(self.world.fluids).mostly_water(&self.world.ground.level, place) => {}
                WaterMark::Splash {
                    place,
                    moving,
                    wide,
                } => frame.splash(lore_render::Splash {
                    place,
                    moving,
                    wide,
                }),
                WaterMark::Bubbles { place, count } => frame.bubbles(place, count),
            }
        }
        let player = *self.world.player();
        let [along_x, _, along_z] = player.velocity;
        let speed = along_x.hypot(along_z);
        // Someone standing or swimming in the water moves it as they move:
        // legs are narrower than a body, and one well under the surface
        // does not trouble it at all.
        let eyes_under = self
            .world
            .ground
            .level
            .water_around(eye)
            .map_or(0.0, |surface| surface - eye[1]);
        if player.submerged > 0.0 && eyes_under < 0.5 {
            let stir = if player.swimming {
                lore_render::Stir {
                    place: [eye[0], eye[2]],
                    wide: 0.3,
                    lift: 0.0,
                    moving: [along_x, along_z],
                    body: 0.025,
                    churn: (speed / 3.0).min(1.0) * 0.12,
                }
            } else {
                let deep = player.submerged.min(1.0);
                lore_render::Stir {
                    place: [eye[0], eye[2]],
                    wide: 0.16 + 0.14 * deep,
                    lift: 0.0,
                    moving: [along_x, along_z],
                    body: 0.02 + 0.04 * deep,
                    churn: (speed / 4.0).min(1.0) * 0.3 * (deep * 4.0).min(1.0),
                }
            };
            frame.stir(stir);
        }
        self.hear_weather(frame);
        let player = *self.world.player();
        // With the ears under, everything is muffled and the water hums.
        frame.hear_under_water(player.head_under);
        if player.head_under != self.under {
            self.under = player.head_under;
            if self.under {
                if let Some(rumble) = self.world.game.sounds_mut().take("underwater", 0) {
                    frame.sound(SoundAsk::Play(UNDER_WATER, rumble.looping()));
                }
            } else {
                frame.sound(SoundAsk::Stop(UNDER_WATER));
            }
        }
    }
}

impl Playing {
    /// Lets the weather be heard: rain as loud as it falls on the player
    /// (hail and sleet rattle louder than they fall; snow is silent), wind
    /// as loud as the air moves where they stand, which under a storm is
    /// three times the wind, and the roar of a tornado from where it is.
    /// Each sound starts when there is some of it and stops when there is
    /// none.
    fn hear_weather(&mut self, frame: &mut Frame) {
        let climate = &self.world.climate;
        let falling = climate.falling();
        let rain = falling.rain.max(0.8 * falling.sleet).max(falling.hail);
        let eye = self.world.player().eye_position();
        let seconds = self.world.sky_seconds();
        let [along_x, _, along_z] = climate.weather().wind_at(eye, seconds);
        let wind = along_x.hypot(along_z);
        let tornado = climate.weather().tornado;
        // How loud each is, from 0 to 1.
        let ease = |low: f32, high: f32, value: f32| {
            let share = ((value - low) / (high - low)).clamp(0.0, 1.0);
            share * share * (3.0 - 2.0 * share)
        };
        let loud = [
            if rain > 0.01 { 0.12 + 0.88 * rain } else { 0.0 },
            // The breeze of a fair day is not heard.
            ease(6.0, 30.0, wind),
            tornado.map_or(0.0, |tornado| ease(0.0, 0.3, tornado.strength)),
        ];
        let names = ["rain", "wind", "wind"];
        for which in 0..3 {
            // A tornado is louder than any wind.
            let loud = loud[which] * if which == 2 { 1.6 } else { 1.0 };
            let id = WEATHER_SOUNDS[which];
            let said = &mut self.weather_heard[which];
            if loud <= 0.004 {
                if said.take().is_some() {
                    frame.sound(SoundAsk::Stop(id));
                }
                continue;
            }
            if said.is_none() {
                let Some(mut play) = self.world.game.sounds_mut().take(names[which], 0) else {
                    continue;
                };
                if let (2, Some(tornado)) = (which, tornado) {
                    // A tornado roars: the wind, an octave down, from
                    // where it stands.
                    play.place = lore_audio::Place::At(tornado.place);
                    play.range = TORNADO_HEARD;
                    play.pitch = 0.5;
                }
                frame.sound(SoundAsk::Play(id, play.volume(loud).looping()));
                *said = Some(loud);
                if let Some(tornado) = tornado {
                    self.roar_from = tornado.place;
                }
            } else if said.is_some_and(|said| (said - loud).abs() > 0.01) {
                // Said again only when it has changed enough to hear: a
                // frame in steady rain asks for nothing.
                frame.sound(SoundAsk::Loud(id, loud));
                *said = Some(loud);
            }
            // The roar goes with the tornado, a stride at a time.
            if let (2, Some(tornado)) = (which, tornado)
                && (0..3).any(|axis| (tornado.place[axis] - self.roar_from[axis]).abs() > 1.0)
            {
                frame.sound(SoundAsk::Move(id, tornado.place));
                self.roar_from = tornado.place;
            }
        }
    }
}

impl Game for Playing {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        let before = self.eye;
        let asked = self
            .controls
            .asked(input, step, &mut self.yaw, &mut self.pitch);
        self.world.ask(asked);
        self.world.tick(input, step);
        // A script or a respawn that put the player somewhere may have
        // turned them too.
        let teleported = self.world.take_teleported();
        if let Some((yaw, pitch)) = teleported.flatten() {
            (self.yaw, self.pitch) = look::turned(yaw, pitch, [0.0; 2]);
        }
        self.eye = self.world.player().eye_position();
        // Being put somewhere is a cut, not a glide.
        self.before = (teleported.is_none() && !cut(before, self.eye)).then_some(before);
        // What no frame came for in time is let go, so the newest noises
        // always have room.
        let (now, tick_rate) = (self.world.tick, self.world.ground.tick_rate);
        self.heard.retain(|noise| fresh(noise, now, tick_rate));
        let room = MOST_HEARD.saturating_sub(self.heard.len());
        let noises = self.world.noises();
        self.heard
            .extend_from_slice(&noises[..noises.len().min(room)]);
        let room = MOST_MARKS.saturating_sub(self.marks.len());
        let marks = self.world.water_marks();
        self.marks
            .extend_from_slice(&marks[..marks.len().min(room)]);
        let room = MOST_MARKS.saturating_sub(self.treads.len());
        let treads = self.world.treads();
        self.treads
            .extend_from_slice(&treads[..treads.len().min(room)]);
    }

    /// Looks from the player's eyes and plays what was heard since the
    /// last frame: the player's own feet and weapon, equally in both ears.
    ///
    /// A frame drawn between two ticks sees from between where the eyes
    /// were on the one and on the other, so walking is smooth however many
    /// frames are drawn to a tick. Which way the eyes face is as of this
    /// very frame: the newest tick's angles and what the mouse and the
    /// stick have done since.
    fn frame(&mut self, frame: &mut Frame) {
        // First: this reads the game's sound files, if it has not yet.
        self.world.frame(frame);
        let (now, tick_rate) = (self.world.tick, self.world.ground.tick_rate);
        for (nth, noise) in (0..).zip(self.heard.drain(..)) {
            if !fresh(&noise, now, tick_rate) {
                continue;
            }
            let sounds = self.world.game.sounds_mut();
            if let Some(play) = noise::play(&noise, nth, sounds) {
                frame.sound(SoundAsk::Once(play));
            }
        }
        let turn = self.controls.look.map_or([0.0; 2], |look| {
            look::asked(frame.input(), look, frame.since_tick())
        });
        let seen = self.seen_at(frame.alpha(), turn);
        frame.look_from(seen.eye, seen.yaw, seen.pitch);
        self.show_water(frame, seen.eye);
        for tread in self.treads.drain(..) {
            frame.tread(tread);
        }
    }
}

#[cfg(test)]
mod heard;
#[cfg(test)]
mod rates;
#[cfg(test)]
mod tools;
#[cfg(test)]
mod wetted;

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    #[test]
    fn lightning_a_script_asks_for_is_seen_at_once_and_heard_when_its_thunder_arrives() {
        use lore_input::Input;

        let actions = ActionMap::standard();
        // On its first tick, a stroke 343 metres from the player: a second
        // off, as sound goes.
        let script = ScriptGame::from_source(
            r#"local struck = false
            game.onTick(function()
                if not struck then
                    struck = true
                    game.weather.strike(game.player.position + vector.create(343, 0, 0))
                end
            end)"#,
            &actions,
        );
        let mut game = Playing::start(script, None, &actions, 60);
        let mut input = Input::new(actions);
        let (mut seen_at, mut heard_at) = (None, None);
        for tick in 1..=80_u32 {
            let stood = game.world.player.player.position;
            game.tick(input.tick(), 1.0 / 60.0);
            let mut frame = Frame::new(0.0);
            game.frame(&mut frame);
            if let Some(stroke) = frame.take_strikes().first()
                && seen_at.is_none()
            {
                seen_at = Some(tick);
                // It comes down out of the clouds onto the place asked for.
                assert!((stroke.to[0] - stood[0] - 343.0).abs() < 0.5, "{stroke:?}");
                assert!(stroke.from[1] > stroke.to[1] + 1_000.0, "{stroke:?}");
            }
            let thunder = (game.world.noises().iter())
                .any(|noise| noise.kind == crate::noise::NoiseKind::Thunder);
            if thunder && heard_at.is_none() {
                heard_at = Some(tick);
            }
        }
        // It is seen in the frame after the tick that asked for it, and the
        // thunder is a second behind the flash.
        assert_eq!(seen_at, Some(1));
        assert!(matches!(heard_at, Some(60..=62)), "{heard_at:?}");
    }

    #[test]
    fn a_stormy_world_is_the_same_world_whether_or_not_it_is_drawn() {
        use lore_input::Input;

        // Two minutes of a sky full of storms, one drawn after every tick
        // and one never drawn: more bolts fall than a frame could show.
        let lived = |drawn: bool| {
            let actions = ActionMap::standard();
            let script = ScriptGame::from_source(r#"game.weather.set("storm", 0)"#, &actions);
            let mut game = Playing::start(script, None, &actions, 60);
            let mut input = Input::new(actions);
            let mut heard = Vec::new();
            for tick in 0..7200_u32 {
                game.tick(input.tick(), 1.0 / 60.0);
                for noise in game.world.noises() {
                    heard.push((tick, noise.kind, noise.position.map(f32::to_bits)));
                }
                if drawn {
                    game.frame(&mut Frame::new(0.0));
                }
            }
            (game.world.dice, heard)
        };
        let (dice, heard) = lived(true);
        assert!(heard.len() > 8, "{}", heard.len());
        assert_eq!(lived(false), (dice, heard));
    }

    #[test]
    fn whoever_comes_down_on_snow_leaves_a_mark_in_it_and_on_bare_ground_none() {
        use lore_input::Input;

        let marks = |script: &str| {
            let actions = ActionMap::standard();
            let script = ScriptGame::from_source(script, &actions);
            let mut game = Playing::start(script, None, &actions, 60);
            let mut input = Input::new(actions);
            let mut treads = Vec::new();
            for _ in 0..120 {
                game.tick(input.tick(), 1.0 / 60.0);
                let mut frame = Frame::new(0.0);
                game.frame(&mut frame);
                treads.extend(frame.take_treads());
            }
            assert!(game.world.player().grounded);
            (treads, game.world.player().position)
        };
        let (treads, feet) = marks(
            "game.weather.setTemperature(-5, 0)
            game.weather.setSnowDepth(0.3)",
        );
        // Where they came down, and nothing more while they stand there.
        assert!((1..=4).contains(&treads.len()), "{treads:?}");
        for tread in &treads {
            assert!((tread.place[0] - feet[0]).hypot(tread.place[1] - feet[2]) < 0.3);
        }
        assert!(marks("").0.is_empty());
    }

    #[test]
    fn a_bolt_asked_for_in_a_sky_full_of_storms_is_always_seen_and_heard() {
        use lore_input::Input;

        let actions = ActionMap::standard();
        // Two minutes into a sky that is all storm, when thunder from far
        // strokes is on its way from all round, a stroke a hundred metres
        // off every two seconds.
        let script = ScriptGame::from_source(
            r#"local ticks = 0
            game.weather.set("storm", 0)
            game.onTick(function()
                ticks += 1
                if ticks > 7200 and ticks % 120 == 0 then
                    game.weather.strike(game.player.position + vector.create(100, 0, 0))
                end
            end)"#,
            &actions,
        );
        let mut game = Playing::start(script, None, &actions, 60);
        let mut input = Input::new(actions);
        let (mut seen, mut heard, mut of_its_own) = (0, 0, 0);
        for _ in 1..=8460_u32 {
            let stood = game.world.player.player.position;
            game.tick(input.tick(), 1.0 / 60.0);
            let mut frame = Frame::new(0.0);
            game.frame(&mut frame);
            for stroke in frame.take_strikes() {
                if (stroke.to[0] - stood[0] - 100.0).abs() < 0.5
                    && (stroke.to[2] - stood[2]).abs() < 0.5
                {
                    seen += 1;
                } else {
                    of_its_own += 1;
                }
            }
            heard += (game.world.noises().iter())
                .filter(|noise| noise.kind == crate::noise::NoiseKind::Thunder)
                .count();
            assert!(game.world.thunder.len() <= MOST_THUNDER);
        }
        // The storm sent plenty of its own all the while.
        assert!(of_its_own > 20, "{of_its_own}");
        assert_eq!(seen, 10);
        assert!(heard >= 10, "{heard}");
    }

    #[test]
    fn a_script_played_alone_rumbles_the_players_gamepad() {
        use lore_input::{GamepadButton, Input, Rumble};

        let actions = ActionMap::standard();
        let script = ScriptGame::from_source(
            r#"game.onTick(function()
                if game.input.justPressed("fire") then game.input.rumble(0.2, 0.6, 0.1) end
            end)"#,
            &actions,
        );
        let mut game = Playing::start(script, None, &actions, 60);
        let mut input = Input::new(actions);
        let asked = |game: &mut Playing, input: &mut Input| {
            game.tick(input.tick(), 1.0 / 60.0);
            let mut frame = Frame::new(0.0);
            game.frame(&mut frame);
            frame.rumble_requested()
        };

        assert_eq!(asked(&mut game, &mut input), None);
        input.press(GamepadButton::RightTrigger);
        assert_eq!(
            asked(&mut game, &mut input),
            Some(Rumble::new(0.2, 0.6, 0.1))
        );
        assert_eq!(asked(&mut game, &mut input), None);
    }

    #[test]
    fn a_frame_between_two_ticks_is_seen_from_between_them() {
        use lore_input::{Input, Key};

        let actions = ActionMap::standard();
        let script = ScriptGame::from_source("", &actions);
        let mut game = Playing::start(script, None, &actions, 60);
        let mut input = Input::new(actions);
        let view = |game: &mut Playing, alpha: f32| {
            let mut frame = Frame::new(alpha);
            game.frame(&mut frame);
            frame.view().unwrap()
        };

        // Before the first tick there is one view, whatever the frame.
        assert_eq!(view(&mut game, 0.0), view(&mut game, 1.0));

        // Walk until a tick has moved the eyes.
        input.press(Key::W);
        let mut walked = None;
        for _ in 0..300 {
            let before = game.seen();
            game.tick(input.tick(), 1.0 / 60.0);
            let after = game.seen();
            assert_eq!(game.before, Some(before.eye));
            let moved = (after.eye[2] - before.eye[2]).abs();
            if moved > 0.01 {
                walked = Some((before, after));
                break;
            }
        }
        let (before, after) = walked.expect("the player walks when W is held");

        // A frame right on the tick before, halfway, and right on the newest.
        assert_eq!(view(&mut game, 0.0).position, before.eye);
        assert_eq!(view(&mut game, 1.0).position, after.eye);
        let halfway = view(&mut game, 0.5).position;
        for axis in 0..3 {
            let middle = (before.eye[axis] + after.eye[axis]) / 2.0;
            assert!((halfway[axis] - middle).abs() < 1e-5, "{halfway:?}");
        }
        // Frames drawn at an even pace between two ticks move the eyes by
        // even steps: the same walk at 60 frames a second or at 240.
        let quarters: Vec<f32> = (0..=4)
            .map(|frame| view(&mut game, frame as f32 / 4.0).position[2])
            .collect();
        let stride = (after.eye[2] - before.eye[2]) / 4.0;
        for pair in quarters.windows(2) {
            assert!((pair[1] - pair[0] - stride).abs() < 1e-5, "{quarters:?}");
        }
        // Drawing does not change the game: the ticks are where they were.
        assert_eq!(game.seen(), after);
    }

    #[test]
    fn eyes_put_somewhere_new_are_not_glided_there() {
        assert!(!cut([0.0, 1.6, 0.0], [0.0, 1.6, -0.1]));
        // A respawn across the level, and a place that is not a number.
        assert!(cut([0.0, 1.6, 0.0], [0.0, 1.6, -28.0]));
        assert!(cut([0.0, 1.6, 0.0], [f32::NAN, 1.6, 0.0]));
    }

    /// A game played alone, and its controls.
    fn playing_alone() -> (Playing, lore_input::Input) {
        let actions = ActionMap::standard();
        let script = ScriptGame::from_source("", &actions);
        let game = Playing::start(script, None, &actions, 60);
        (game, lore_input::Input::new(actions))
    }

    const STEP: f32 = 1.0 / 60.0;

    /// The view of a frame drawn `alpha` of the way to the next tick, with
    /// what `input` holds that no tick has seen.
    fn drawn(game: &mut Playing, input: &lore_input::Input, alpha: f32) -> crate::game::View {
        let mut frame = Frame::between_ticks(input, alpha, STEP);
        game.frame(&mut frame);
        frame.view().unwrap()
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn the_view_follows_the_mouse_on_every_frame_without_waiting_for_a_tick() {
        let (mut game, mut input) = playing_alone();
        let (yaw, pitch) = (game.yaw(), game.pitch());

        // Three frames with no tick between them: each has turned further.
        let mut turned = 0.0;
        for alpha in [0.2, 0.5, 0.8] {
            input.move_mouse(40.0, 10.0);
            turned += 40.0;
            let view = drawn(&mut game, &input, alpha);
            assert!(near(
                view.yaw,
                (yaw - turned * look::MOUSE_TURN).rem_euclid(TAU)
            ));
            assert!(near(view.pitch, pitch + turned / 4.0 * look::MOUSE_TURN));
        }
        // Drawing changed nothing the game's rules know.
        assert_eq!((game.yaw(), game.pitch()), (yaw, pitch));

        // The tick takes in the same movement: the picture does not jump,
        // and nothing is turned twice.
        let before = drawn(&mut game, &input, 1.0);
        game.tick(input.tick(), STEP);
        let after = drawn(&mut game, &input, 0.0);
        assert!(near(after.yaw, before.yaw) && near(after.pitch, before.pitch));
        assert_eq!((after.yaw, after.pitch), (game.yaw(), game.pitch()));
    }

    #[test]
    fn the_sky_is_drawn_between_two_ticks_as_the_view_is() {
        let (mut game, mut input) = playing_alone();
        for _ in 0..30 {
            game.tick(input.tick(), STEP);
        }
        // The sky's clock and how far the wind has carried the air, as a
        // frame `alpha` of the way between the last two ticks shows them.
        let sky = |game: &mut Playing, input: &lore_input::Input, alpha: f32| {
            let mut frame = Frame::between_ticks(input, alpha, STEP);
            game.frame(&mut frame);
            let clock = frame.take_water().1.unwrap();
            let drift = frame.take_weather().unwrap().drift.unwrap();
            (clock, drift)
        };
        let (start, carried) = sky(&mut game, &input, 0.0);
        let (middle, half) = sky(&mut game, &input, 0.5);
        let (end, all) = sky(&mut game, &input, 1.0);
        let step = f64::from(STEP);
        // Each frame is further on, by as much of a tick as it is.
        assert!((start - 29.0 * step).abs() < 1e-9, "{start}");
        assert!((middle - 29.5 * step).abs() < 1e-9, "{middle}");
        assert!((end - 30.0 * step).abs() < 1e-9, "{end}");
        let moved = (all[0] - carried[0]).hypot(all[1] - carried[1]);
        assert!(moved > 0.0, "the wind blows");
        for axis in 0..2 {
            assert!((half[axis] - (carried[axis] + all[axis]) * 0.5).abs() < 1e-9);
        }
        // The next tick starts where the last frame before it was.
        game.tick(input.tick(), STEP);
        let (after, carried_after) = sky(&mut game, &input, 0.0);
        assert!((after - end).abs() < 1e-9);
        for axis in 0..2 {
            assert!((carried_after[axis] - all[axis]).abs() < 1e-9);
        }
    }

    #[test]
    fn the_view_follows_the_stick_by_how_long_it_was_held() {
        use lore_input::{Stick, StickAxis};

        let (mut game, mut input) = playing_alone();
        let yaw = game.yaw();
        input.set_stick(Stick::RightStick, StickAxis::X, -1.0);

        // Held hard left: a quarter of a tick on, a quarter of a tick's
        // turn, and so on. The frame rate is not in it.
        for alpha in [0.25, 0.5, 1.0] {
            let view = drawn(&mut game, &input, alpha);
            let expected = yaw + look::STICK_TURN * STEP * alpha;
            assert!(near(view.yaw, expected.rem_euclid(TAU)), "{alpha}");
        }
        let before = drawn(&mut game, &input, 1.0);
        game.tick(input.tick(), STEP);
        let after = drawn(&mut game, &input, 0.0);
        assert!(near(after.yaw, before.yaw));
    }

    #[test]
    fn a_shot_goes_where_the_screen_centre_was_when_the_trigger_was_pressed() {
        use lore_input::MouseButton;

        let (mut game, mut input) = playing_alone();
        // A flick and a click inside one tick.
        input.move_mouse(-250.0, 120.0);
        input.press(MouseButton::MouseLeft);
        let on_screen = drawn(&mut game, &input, 0.9);

        // The tick that sees the click fires along the view that was on
        // the screen, not one from a tick earlier: that very tick, and to
        // the last bit.
        game.tick(input.tick(), STEP);
        assert!(near(game.yaw(), on_screen.yaw));
        assert!(near(game.pitch(), on_screen.pitch));
        let fired = game.world.asked();
        assert!(fired.fire, "{fired:?}");
        assert_eq!(
            (fired.movement.yaw, fired.pitch),
            (game.yaw(), game.pitch())
        );
    }
}
