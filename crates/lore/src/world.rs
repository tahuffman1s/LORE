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
use lore_script::{CharacterId, Motion, Movement, ScriptHost};

use crate::blend;
use crate::character::{Boom, Character, Facing, View};
use crate::game::{Frame, Game};
use crate::look;
use crate::noise::{self, Noise, Walking};
use crate::script_game::ScriptGame;
use crate::shooting::Shooting;
use crate::sound_output::SoundAsk;

mod bodies;
mod bot;
#[cfg(test)]
mod bot_plays;
mod climate;
mod fluids;
mod ground;
#[cfg(test)]
mod npc_walks;
mod npcs;
mod sensing;
mod spawn;
mod trail;
mod walk;
mod wet;
mod witness;

pub(crate) use bodies::{Body, Dummy, Standing};
pub use bodies::{DummyId, PlayerInput};
pub use bot::{BotLine, FINISHED as BOT_FINISHED};
pub(crate) use climate::said as said_weather;
pub(crate) use ground::Ground;
pub use ground::{WalkCheck, try_walking};
pub use npcs::Npc;
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
/// Besides the player a world holds the game's NPCs ([`npcs`](Self::npcs)),
/// each a body like the player's that its own mind moves, and can hold
/// training dummies ([`add_dummy`](Self::add_dummy)), which stand still to
/// be shot at.
#[derive(Debug)]
pub struct World {
    game: ScriptGame,
    ground: Ground,
    player: Body,
    /// The training dummies, in the order they were put in.
    dummies: Vec<Dummy>,
    /// The NPCs, in the order of their ids.
    npcs: npcs::Npcs,
    /// What the world's people make of what is done in front of them.
    witnessing: witness::Witnessing,
    /// What the NPCs see and hear, kept from tick to tick.
    sensing: sensing::Sensing,
    /// The number the next dummy or NPC is known by to a weapon. The player
    /// is 0.
    next_target: u32,
    /// The dummies and the NPCs on their feet, as this tick's shots and
    /// swings see them: kept between ticks so a tick sets no memory aside.
    standing: Vec<Standing>,
    /// How many ticks have been run.
    tick: u32,
    /// The weapon the player holds, and what they have launched.
    shooting: Shooting,
    /// When the player's feet come down.
    walking: Walking,
    /// Who each solid thing caught this tick, if anyone: kept from tick
    /// to tick so a tick sets no memory aside.
    caught: Vec<Option<CharacterId>>,
    /// The noises the player made in the newest tick.
    noises: Vec<Noise>,
    /// The sound and weapon names those noises carry by id.
    names: crate::noise::Names,
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
    /// The solid things in the level: crates, barrels, doors.
    things: things::Things,
    /// What the use button is doing, and which thing the player means.
    using: using::Using,
    /// The things that can be used and their ids, kept between ticks so a
    /// tick sets no memory aside.
    usable: Vec<using::Usable>,
    usable_ids: Vec<using::UseKey>,
    /// The bot, once the script has given it something to do.
    bot: Option<Box<bot::BotPlay>>,
    /// Whether the fire button was down on the last tick: a throw is a
    /// press, not a hold.
    fire_was: bool,
    /// The level's ladders: what the map places as type `ladder`, each
    /// the space a climber is on it in.
    ladders: Vec<lore_script::Placed>,
    /// The dodge the player is in, or last made.
    dodge: Dodge,
    /// Which solid thing the player stood on after the last tick.
    standing_on: Option<usize>,
    /// How far what the player stood on has turned them since the view
    /// was last told, in radians to the left.
    carried_turn: f32,
}

/// A dodge, from tick to tick.
#[derive(Debug, Clone, Copy, Default)]
struct Dodge {
    /// How many seconds of it are left.
    left: f32,
    /// How many seconds until another can begin.
    rest: f32,
    /// The way it goes, on the ground, one unit long.
    way: [f32; 2],
    /// Whether its button was down on the last tick.
    was_down: bool,
    /// The clip for a dodge that began this tick, until the body is told.
    began: Option<&'static str>,
}

/// How long after a dodge ends before another can begin, in seconds.
const DODGE_REST: f32 = 0.25;

/// How far off the lock-on button looks for someone to hold the view on,
/// in metres.
const LOCK_REACH: f32 = 25.0;

/// How far in front of the eyes a carried thing is held, and how far under
/// them, in metres.
const CARRY_AHEAD: f32 = 1.35;
const CARRY_LOW: f32 = 0.35;

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
        crouched: player.crouched,
        sprinting: player.sprinting,
        swimming: player.swimming,
        on_ladder: player.on_ladder,
        climbing: player.hoist > 0.0,
        // The world says so when it steps one.
        dodging: false,
        // Until the tick works out what is in the way: far along the look.
        aim_point: {
            let eye = player.eye_position();
            std::array::from_fn(|i| eye[i] + aim[i] * 1000.0)
        },
        aim_blocked: false,
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
    fn on(mut game: ScriptGame, mut ground: Ground, here: Option<[f32; 3]>) -> Self {
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
        let things = Self::things_of(&game, &ground);
        ground.walk_round(&things);
        let ladders = crate::script_game::ladders_of(game.map());
        // Every NPC the game has stands in the world from its first tick,
        // however the world was made.
        let mut next_target = 1;
        let npcs = npcs::Npcs::of(
            game.host_mut(),
            (&ground.level, &ground.rules),
            player.position,
            &mut next_target,
        );
        npcs.tell(game.host_mut());
        let mut world = Self {
            game,
            ground,
            player: Body { player, asked },
            dummies: Vec::new(),
            npcs,
            witnessing: witness::Witnessing::default(),
            sensing: sensing::Sensing::default(),
            next_target,
            standing: Vec::new(),
            tick: 0,
            shooting: Shooting::default(),
            walking: Walking::default(),
            caught: Vec::new(),
            noises: Vec::new(),
            names: crate::noise::Names::default(),
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
            things,
            using: using::Using::default(),
            usable: Vec::new(),
            usable_ids: Vec::new(),
            bot: None,
            fire_was: false,
            ladders,
            dodge: Dodge::default(),
            standing_on: None,
            carried_turn: 0.0,
        };
        // The ground's still water is the world's pools, of the kinds its
        // sheets say, and its script is told what they are called. Here,
        // where every world is made, so that a game that is played, one
        // started from the editor and one in a test all have them: a
        // world given its level some other way once had none.
        (world.fluids).take_level(&world.ground.level, world.game.host_mut());
        world.tell_things();
        let doors = walk::door_names(&world.things, world.game.map());
        (world.ground).tell_walking(doors, world.game.host_mut());
        world
    }

    /// The solid things the game's map places on `ground`. What the map
    /// asks for that cannot be done is logged.
    fn things_of(game: &ScriptGame, ground: &Ground) -> things::Things {
        let (things, problems) =
            things::Things::of(game.map(), &ground.level, ground.tick_rate, |file| {
                game.read_model(file)
            });
        for problem in problems {
            log::warn!("{problem}");
        }
        things
    }

    /// Tells the script where every solid thing is and how it is moving.
    fn tell_things(&mut self) {
        let host = self.game.host_mut();
        self.things.each(|id, told| {
            host.set_thing_motion(
                id,
                lore_script::ThingMotion {
                    position: told.position,
                    facing: told.facing,
                    middle: told.middle,
                    velocity: told.velocity,
                    moves: told.moves,
                    weight: told.weight,
                    resting: told.resting,
                    sliding: told.sliding,
                    open: told.open,
                },
            );
        });
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
        Self::on(self.game, ground, None)
    }

    /// Puts a newer version of the level under a world that is being
    /// played, as when its file is exported again: the new shapes are
    /// solid from the next tick, its still water and lamps take the place
    /// of the old, and the player stays where they stand. Anything else in
    /// the world stays too.
    pub fn change_level(&mut self, level: &Scene) {
        self.game
            .host_mut()
            .set_level_lamps(&crate::lamps::of_level(level));
        let mut ground = Ground::of(Some(level), self.game.map(), self.ground.tick_rate);
        // What a script made of how the player moves is not the level's.
        ground.rules = self.ground.rules;
        self.ground = ground;
        // Ways found on the old ground, and its door numbers, mean nothing.
        self.npcs.level_changed();
        self.bot_level_changed();
        // Solid things stand on the new shapes, back where the map has them.
        self.things = Self::things_of(&self.game, &self.ground);
        self.ground.walk_round(&self.things);
        let doors = walk::door_names(&self.things, self.game.map());
        (self.ground).tell_walking(doors, self.game.host_mut());
        self.tell_things();
        (self.fluids).take_level(&self.ground.level, self.game.host_mut());
    }

    /// Where a person can walk in the level: the ground, worked out from
    /// the level's shapes and the solid things in it that never move, with
    /// each door a gate ([`openings`](Self::openings) says which are open).
    /// The flat floor of a game with no level is one open square.
    ///
    /// It is built on a thread of its own when the level is loaded or
    /// saved; the first time it is asked for, this waits until it is done.
    /// What it holds is the same however long that takes.
    pub fn walkable(&self) -> &lore_nav::Walkable {
        self.ground.walkable()
    }

    /// Which ways on the [`walkable`](Self::walkable) ground are open: a
    /// door's gate is open while the door is open, as of the last tick.
    #[must_use]
    pub fn openings(&self) -> &lore_nav::Openings {
        self.ground.openings()
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

    /// How hard the game is, as the numbers the engine's own people follow:
    /// how much they hurt the player, how fast they notice and how well they
    /// aim, each 1 at the middle of `game.difficulty`'s dial, 0.5 at its
    /// easiest step and 1.5 at its hardest. It changes only on a tick.
    #[must_use]
    pub fn difficulty(&self) -> lore_script::DifficultyScales {
        // The dial and the assists start from the player's settings file,
        // not from the seed or the presses: a save (#73) and a replay file
        // (#193) must hold the starting dial and assists, and any change to
        // them from outside the game, for the same game to come back.
        self.game.host().difficulty_scales()
    }

    /// The hour of the day on the world's clock, from 0 up to 24 (13.5 is
    /// half past one in the afternoon), or `None` in a game with no time of
    /// day: one whose script never set `game.clock`. Plain data that moves
    /// only on ticks, so it is paused and sped up with the game, the same in
    /// a replay, and what anything in the world that keeps hours (people's
    /// days, shops) reads. The script's part of a tick moves it on, so
    /// during [`tick`](crate::game::Game::tick) it is the hour the last tick
    /// left.
    #[must_use]
    pub fn time_of_day(&self) -> Option<f64> {
        self.game.host().time_of_day()
    }

    /// The day it is on the world's clock, counting the first as 1, or
    /// `None` in a game with no time of day.
    #[must_use]
    pub fn day(&self) -> Option<u32> {
        self.game.host().day()
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

    /// How many rays the NPCs' senses cast at the level in the newest tick:
    /// what a crowd's sight and hearing cost.
    #[must_use]
    pub fn sense_rays(&self) -> u32 {
        self.sensing.rays
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

    /// The names the noises of this world carry by id.
    #[must_use]
    pub fn names(&self) -> &crate::noise::Names {
        &self.names
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
        let id = DummyId(self.next_target);
        self.next_target = self.next_target.saturating_add(1);
        let host = self.game.host_mut();
        let name = host.add_character(DUMMY_NAME);
        host.set_position(name, feet);
        self.dummies.push(Dummy { id, feet, name });
        id
    }

    /// The game's NPCs, in the order of their ids: who each is, its body
    /// and its mind. A world made with no level has them too.
    #[must_use]
    pub fn npcs(&self) -> &[Npc] {
        self.npcs.list()
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
        let standing = (self.dummies.iter().map(|dummy| dummy.feet))
            .chain(self.npcs.list().iter().map(|npc| npc.body.position));
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
    /// The clip to play for a dodge that began on the last tick, once.
    pub(crate) fn take_dodge_began(&mut self) -> Option<&'static str> {
        self.dodge.began.take()
    }

    /// How far what the player stands on has turned them since this was
    /// last asked, in radians to the left: the view turns with it.
    pub(crate) fn take_carried_turn(&mut self) -> f32 {
        std::mem::take(&mut self.carried_turn)
    }

    pub(crate) fn take_teleported(&mut self) -> Option<Option<(f32, f32)>> {
        self.teleported.take()
    }
}

impl Game for World {
    fn level_saved(&mut self, level: &Scene) {
        self.change_level(level);
    }

    fn typing_ended(&mut self) {
        self.game.typing_ended();
    }

    fn listening_ended(&mut self) {
        self.game.listening_ended();
    }

    fn take_listening(&mut self) -> Option<Option<lore_input::Hears>> {
        self.game.take_listening()
    }

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
        // The NPCs the script made, moved or took out in its last tick.
        (self.npcs).hear(
            self.game.host_mut(),
            (&self.ground.level, &self.ground.rules),
            self.player.player.position,
            &mut self.next_target,
        );
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
        // What the script asked of solid things in its last tick is done
        // before they move.
        for ask in self.game.host_mut().take_thing_asks() {
            match ask {
                lore_script::ThingAsk::Push { id, impulse } => self.things.push(&id, impulse),
                lore_script::ThingAsk::Put { id, position } => self.things.put(&id, position),
                lore_script::ThingAsk::Glide {
                    id,
                    position,
                    facing,
                    seconds,
                } => match self.things.glide(&id, position, facing, seconds) {
                    Ok(()) => true,
                    Err(why) => {
                        log::warn!("The thing placed as \"{id}\" was not moved: {why}.");
                        false
                    }
                },
                lore_script::ThingAsk::Open { id, open } => self.things.open(&id, open),
                lore_script::ThingAsk::Drop => {
                    if let Some(carried) = self.things.carried().map(str::to_owned) {
                        self.things.let_go(None);
                        self.game.host_mut().set_carrying(None);
                        self.game.host_mut().dropped(&carried, false);
                    }
                    true
                }
            };
        }
        self.fluids.step(
            &self.ground.level,
            self.ground.step,
            &self.player.player,
            (self.climate.water_rise(), self.climate.ice()),
        );
        // Which doors are open, for whoever finds a way this tick: as the
        // script and the use button left them by the end of the last.
        self.ground.follow_doors(&self.things);
        self.game.host_mut().set_openings(self.ground.openings());
        let Ground {
            level,
            rules,
            step: tick,
            ..
        } = &self.ground;
        let Body { player, asked } = &mut self.player;
        // The NPCs move by the engine's own numbers, not the player's.
        let npc_rules = *rules;
        // How the script wants the player to move, and how fast it flies.
        let mut moving = asked.movement;
        moving.mode = mode_of(self.game.host().movement());
        let said = self.game.host().moves();
        let can = self.game.host().abilities();
        // A dodge: a press of its button, in a game that has them, throws
        // the player the way they are pushing (or backward, standing
        // still) for a moment in which nothing else moves them.
        let pressed = asked.dodge && !self.dodge.was_down;
        self.dodge.was_down = asked.dodge;
        self.dodge.rest = (self.dodge.rest - *tick).max(0.0);
        let (sin, cos) = moving.yaw.sin_cos();
        let (ahead, aside) = ([-sin, -cos], [cos, -sin]);
        if can.dodge
            && pressed
            && self.dodge.left <= 0.0
            && self.dodge.rest <= 0.0
            && player.grounded
            && !player.swimming
            && moving.mode == MoveMode::Walk
        {
            let pushed = moving.forward.hypot(moving.right);
            let (forward, right) = if pushed > 0.2 {
                (moving.forward / pushed, moving.right / pushed)
            } else {
                (-1.0, 0.0)
            };
            self.dodge.way = [
                ahead[0] * forward + aside[0] * right,
                ahead[1] * forward + aside[1] * right,
            ];
            self.dodge.left = said.dodge_seconds;
            self.dodge.rest = said.dodge_seconds + DODGE_REST;
            self.dodge.began = Some(if right.abs() > forward.abs() {
                if right > 0.0 {
                    "dodge_right"
                } else {
                    "dodge_left"
                }
            } else {
                "roll"
            });
        }
        let dodging = self.dodge.left > 0.0;
        if dodging {
            self.dodge.left -= *tick;
            let way = self.dodge.way;
            moving.forward = way[0] * ahead[0] + way[1] * ahead[1];
            moving.right = way[0] * aside[0] + way[1] * aside[1];
            moving.jump = false;
            moving.sprint = false;
        }
        // The dodge button never crouches, though on a gamepad it is the
        // crouch button too.
        if can.dodge && (asked.dodge || dodging) {
            moving.crouch = false;
        }
        let dodge_pace = said.dodge_distance / said.dodge_seconds.max(0.05);
        let settings = MoveSettings {
            fly_speed: self.game.host().fly_speed(),
            walk_speed: said.walk_speed,
            sprint_speed: said.sprint_speed,
            crouch_speed: said.crouch_speed,
            swim_speed: said.swim_speed,
            // A sprinting swimmer is as much faster as the engine's own is.
            swim_sprint_speed: said.swim_speed * rules.swim_sprint_speed
                / rules.swim_speed.max(f32::EPSILON),
            jump_height: said.jump_height,
            gravity: said.gravity,
            // What the game lets the player do: each is a switch.
            vault_height: if can.vault { said.vault_height } else { 0.0 },
            climb_height: if can.climb { said.climb_height } else { 0.0 },
            ladder_speed: said.ladder_speed,
            swim: can.swim,
            ..*rules
        };
        let settings = if dodging {
            // At the dodge's own pace from its first tick to its last.
            MoveSettings {
                walk_speed: dodge_pace,
                sprint_speed: dodge_pace,
                crouch_speed: dodge_pace,
                ground_acceleration: 200.0,
                ..settings
            }
        } else {
            settings
        };
        // At one of the level's ladders, for someone who climbs them.
        moving.ladder =
            can.ladders && (self.ladders.iter()).any(|ladder| ladder.holds(player.position));
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
        let walking = moving.mode == MoveMode::Walk;
        // Doors, lifts and platforms slide first, and take whoever stands
        // on one along and round with it. Then every solid thing moves on
        // by a tick, so the player walks the level as it now is.
        let rider = (self.standing_on.filter(|_| walking)).map(|under| (under, player.position));
        let slid = self.things.slide(*tick, rider);
        if rider.is_some() {
            let (along, round) = slid.carried;
            player.position = std::array::from_fn(|i| player.position[i] + along[i]);
            moving.yaw += round;
            self.carried_turn += round;
        }
        self.things.step(rules.gravity, level);
        let before = *player;
        player.step(level, rules, &moving, *tick);
        // A dodge ends on its feet: what is left of its speed is a walk's,
        // so it covers the distance asked and does not slide on after.
        if dodging && self.dodge.left <= 0.0 {
            let pace = player.velocity[0].hypot(player.velocity[2]);
            let most = said.walk_speed * 0.5;
            if pace > most && pace > 0.0 {
                player.velocity[0] *= most / pace;
                player.velocity[2] *= most / pace;
            }
        }
        // Then each NPC, in the order of their ids, and the player is kept
        // out of those on their feet: a person is walked into, not through.
        let this_tick = npcs::Tick {
            ground: &self.ground,
            tick: self.tick,
            player: player.position,
            things: &self.things,
        };
        (self.npcs).step(self.game.host(), &this_tick, *tick);
        // A shut door an NPC stands before is opened for it, as the
        // player's use opens one.
        (self.npcs).open_doors(&mut self.things, self.game.host_mut(), self.tick, *tick);
        // Whoever the view was held on lets go of it once they are down.
        let host = self.game.host_mut();
        if let Some(locked) = host.locked_on()
            && host.health(locked).is_none_or(|(now, _)| now <= 0.0)
        {
            host.lock_on(None);
        }
        if walking {
            self.npcs.keep_out(player, rules, level);
        }
        // What the player walks into is pushed, and every solid thing
        // moves on by a tick.
        if walking {
            let (sin, cos) = moving.yaw.sin_cos();
            let wish = [
                -sin * moving.forward + cos * moving.right,
                -cos * moving.forward - sin * moving.right,
            ];
            let long = wish[0].hypot(wish[1]);
            if long > 0.1 {
                let wish = [wish[0] / long, wish[1] / long];
                (self.things).pushed_by(player, rules, level, wish, *tick);
            }
        }
        // What the player now stands on, to ride it on the next tick, and
        // whether something sliding has them caught against a wall: it
        // goes back a tick and waits for them.
        self.standing_on = None;
        // Who each thing has caught, the player first, then the NPCs in
        // the order of their ids: in a list kept from tick to tick.
        self.caught.clear();
        self.caught.resize(self.things.len(), None);
        if walking && !self.things.is_empty() {
            let (things, standing_on, caught) =
                (&self.things, &mut self.standing_on, &mut self.caught);
            player.touching(level, rules, 0.08, |body, toward, apart| {
                if toward[1] < -0.7 {
                    *standing_on = Some(body);
                } else if apart < -things::CAUGHT
                    && things.sliding(body)
                    && let Some(slot) = caught.get_mut(body)
                {
                    *slot = Some(CharacterId::PLAYER);
                }
            });
        }
        // A door or a lift swinging into an NPC waits for them as it
        // waits for the player.
        if self.things.any_sliding() {
            (self.npcs).caught_by(&self.things, level, &npc_rules, (&mut self.caught, *tick));
        }
        let mut blocked: Vec<(String, CharacterId)> = Vec::new();
        for body in 0..self.things.len() {
            if let Some(who) = self.caught.get(body).copied().flatten() {
                if let Some(id) = self.things.caught_in(body, *tick) {
                    blocked.push((id.to_owned(), who));
                }
            } else {
                self.things.clear_of(body);
            }
        }
        // The script reads where the player is as of this tick, in what it
        // is told of below and in its own tick, and how they are moving.
        let host = self.game.host_mut();
        host.set_position(CharacterId::PLAYER, player.position);
        self.npcs.tell(host);
        // Who got where they were walking, and who cannot.
        self.npcs.tell_news(host);
        // Who can be shot and struck this tick: the dummies and the NPCs
        // on their feet.
        self.standing.clear();
        (self.standing)
            .extend((self.dummies.iter()).map(|dummy| dummy.standing(npc_rules.standing_height)));
        self.npcs.standing(&npc_rules, &mut self.standing);
        // What slid where it was going, and what found the player in its way.
        for id in &slid.arrived {
            host.thing_arrived(id);
        }
        for (id, who) in &blocked {
            host.thing_blocked(id, *who);
        }
        // And where every solid thing has got to.
        if !self.things.is_empty() {
            self.things.each(|id, told| {
                host.set_thing_motion(
                    id,
                    lore_script::ThingMotion {
                        position: told.position,
                        facing: told.facing,
                        middle: told.middle,
                        velocity: told.velocity,
                        moves: told.moves,
                        weight: told.weight,
                        resting: told.resting,
                        sliding: told.sliding,
                        open: told.open,
                    },
                );
            });
        }
        // A hard landing hurts, through the same path as a weapon.
        if walking && player.grounded && !before.grounded {
            let hurt = fall_damage(-before.velocity[1]);
            if hurt > 0.0 && host.setting_on(FALL_DAMAGE) != Some(false) {
                host.damage(CharacterId::PLAYER, hurt, None, "fall");
            }
        }
        let aim = aim_of(asked.movement.yaw, asked.pitch);
        let mut motion = motion_of(player, aim);
        motion.dodging = dodging;
        // Where a shot would land now, and whether cover stops it short of
        // the crosshair: for a game's own second mark.
        (motion.aim_point, motion.aim_blocked) =
            self.shooting
                .aim(player.eye_position(), &self.standing, level, rules, aim);
        host.set_motion(CharacterId::PLAYER, motion);
        // Which usable thing the player means, and whether they use it.
        let eyes = player.eye_position();
        self.usable.clear();
        // The ids are written over the ones kept from the last tick, so a
        // tick with the same things as the last sets no memory aside.
        let (usable, ids) = (&mut self.usable, &mut self.usable_ids);
        host.each_usable(|meant, at, said| {
            match ids.get_mut(usable.len()) {
                Some(kept) => kept.set(meant),
                None => ids.push(using::UseKey::of(meant)),
            }
            usable.push(using::Usable {
                at,
                reach: said.reach,
                seconds: said.seconds,
            });
        });
        self.usable_ids.truncate(self.usable.len());
        // The crosshair's line: the camera's, from level with the body on.
        let from = self.shooting.camera.map_or(eyes, |camera| {
            let gap: [f32; 3] = std::array::from_fn(|i| eyes[i] - camera[i]);
            let skip = (0..3).map(|i| gap[i] * aim[i]).sum::<f32>().max(0.0);
            std::array::from_fn(|i| camera[i] + aim[i] * skip)
        });
        let chosen = (self.using.target.as_ref())
            .and_then(|target| self.usable_ids.iter().position(|id| id == target));
        let throwing = asked.fire && !self.fire_was;
        self.fire_was = asked.fire;
        if let Some(carried) = self.things.carried() {
            // What is in the hands is held out in front, a little under
            // the eyes so it does not fill the view. The use button lets
            // it go and the fire button throws it at what the crosshair
            // is on. Caught on a wall, it is let go by itself.
            let hands: [f32; 3] = std::array::from_fn(|i| {
                eyes[i] + aim[i] * CARRY_AHEAD - if i == 1 { CARRY_LOW } else { 0.0 }
            });
            let carried_one = Some(lore_script::Usable::Thing(carried));
            let (used, _) = self.using.tick(carried_one, 0.0, asked.interact, *tick);
            host.set_use_target(carried_one, 0.0);
            let let_go = if throwing {
                Some(Some((motion.aim_point, player.velocity)))
            } else if used {
                Some(None)
            } else {
                None
            };
            let carried = carried.to_owned();
            match let_go {
                Some(thrown) => {
                    self.things.let_go(thrown);
                    host.set_carrying(None);
                    host.dropped(&carried, thrown.is_some());
                }
                None if !self.things.hold(hands) => {
                    host.set_carrying(None);
                    host.dropped(&carried, false);
                }
                None => {}
            }
        } else {
            let meant = using::pick(&self.usable, eyes, from, aim, level, chosen);
            let target = meant.map(|nth| self.usable_ids[nth].meant());
            let seconds = meant.map_or(0.0, |nth| self.usable[nth].seconds);
            let (used, progress) = self.using.tick(target, seconds, asked.interact, *tick);
            host.set_use_target(target, progress);
            if let (true, Some(lore_script::Usable::Npc(npc))) = (used, target) {
                host.use_npc(npc);
            }
            if let (true, Some(lore_script::Usable::Thing(target))) = (used, target) {
                host.use_thing(target);
                // Using a door opens it, or shuts it.
                if let Some(open) = self.things.door(target) {
                    self.things.open(target, !open);
                }
                // Using what can be carried picks it up.
                if host.carryable(target) && self.things.pick_up(target) {
                    host.set_carrying(Some(target));
                    host.picked_up(target);
                }
            }
        }
        // Who walked into a zone the script watches, and who left.
        host.step_zones();
        // What was heard of the tick just run: when a foot came down, and
        // below, what the weapon did.
        self.noises.clear();
        self.walking
            .tick(player, self.tick, *tick, &said, &mut self.noises);
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
            let kind = if *away < 1_500.0 {
                crate::noise::NoiseKind::Thunder
            } else {
                crate::noise::NoiseKind::FarThunder
            };
            noises.push(Noise::new(kind, *position, 1.0, 1.0, 0.0, now));
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
            // The script's own loudness for steps is kept on top of it, and
            // silent steps stay silent.
            let scale = said.step_loudness;
            if scale > 0.0 {
                footfall.loudness =
                    ((footfall.loudness / scale * 1.2).clamp(0.2, 1.0) * quiet) * scale;
            }
            footfall.pitch = pitch;
        }
        // The NPCs' steps, each heard from where it was made.
        (self.npcs).footsteps(self.tick, *tick, &mut self.noises);
        // Fluid that came down in this tick is heard from where it did.
        for fall in self.fluids.falls() {
            let kind = if fall.into_pool {
                crate::noise::NoiseKind::Pour
            } else {
                crate::noise::NoiseKind::Patter
            };
            self.noises.push(Noise::new(
                kind,
                fall.place,
                fall.loudness,
                fall.pitch,
                0.0,
                self.tick,
            ));
        }
        // What they threw up is drops of what they are in.
        self.fluids.splash(level, &self.marks);
        // The weapon the player holds, by what they are pressing. The
        // script hears of every shot, hit and reload here.
        self.shooting.tick(
            &self.player,
            &self.standing,
            self.tick,
            level,
            rules,
            *tick,
            self.game.host_mut(),
            &mut self.noises,
            &mut self.names,
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
            self.noises.push(Noise::new(
                crate::noise::NoiseKind::Sizzle,
                self.player.player.position,
                loudness,
                1.0,
                0.0,
                self.tick,
            ));
        }
        // The script hears each noise of the tick, in the order made; one
        // it answers `false` to is kept in the list but not played. The
        // table it is handed for each is handed again to the NPCs that
        // hear it.
        self.game.host_mut().start_noises();
        for noise in &mut self.noises {
            if !self.game.host_mut().noise_made(&noise.told(&self.names)) {
                noise.silenced = true;
            }
        }
        self.game.tick(input, step);
        // What the script played at a place in its tick is heard by the
        // NPCs as the player hears it.
        let (now, noises, names) = (self.tick, &mut self.noises, &mut self.names);
        self.game.take_script_noises(|noise, range| {
            let mut heard = Noise::new(
                crate::noise::NoiseKind::Script,
                noise.position,
                noise.loudness,
                1.0,
                0.0,
                now,
            );
            heard.range = range;
            heard.sound = names.intern(&noise.sound);
            heard.by = noise.by;
            noises.push(heard);
        });
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
        // Last, what each NPC sees and hears of the tick, the script's own
        // sounds among it, and what the script is told of that.
        let this_tick = sensing::Tick {
            tick: self.tick,
            level: &self.ground.level,
            rules: &self.ground.rules,
            player: &self.player.player,
            noises: &self.noises,
            names: &self.names,
            weather: self.climate.weather(),
        };
        (self.sensing).tick(&mut self.npcs, self.game.host_mut(), &this_tick);
        // Who holds what against whom: what was done up to now, who
        // witnessed it, and what people near each other told each other.
        // Last of all, so each NPC's senses are those of this very tick:
        // the noise of a shot made on it is already heard.
        let (player_eyes, dt) = (self.player.player.eye_position(), self.ground.step);
        (self.witnessing).step(
            self.game.host_mut(),
            self.npcs.list(),
            player_eyes,
            (self.tick, dt),
        );
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
        self.things.frame(frame, alpha);
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
    switch_view: Option<ActionId>,
    interact: Option<ActionId>,
    block: Option<ActionId>,
    dodge: Option<ActionId>,
    lock_on: Option<ActionId>,
    use_item: Option<ActionId>,
    next_item: Option<ActionId>,
    previous_item: Option<ActionId>,
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
            switch_view: find("switch_view"),
            interact: find("interact"),
            block: find("block"),
            dodge: find("dodge"),
            lock_on: find("lock_on"),
            use_item: find("use_item"),
            next_item: find("next_item"),
            previous_item: find("previous_item"),
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
            interact: held(self.interact),
            dodge: held(self.dodge),
            use_item: held(self.use_item),
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
    /// The player's body, and the camera at its eyes or over its shoulder.
    /// `None` only if the engine's own body could not be read.
    character: Option<Character>,
    /// The camera a script directs with `game.camera`.
    shot: crate::shot::ShotCamera,
    /// What the player holds (`game.player:hold`): its model, and whether
    /// it is in the left hand.
    held: Option<(std::sync::Arc<lore_assets::Scene>, bool)>,
    /// The models of the weapons the player has held, by the name the
    /// weapon gave: read once each. `None` for one that could not be read,
    /// which was said in the log.
    weapon_models:
        std::collections::HashMap<std::rc::Rc<str>, Option<crate::weapon_motion::Pieces>>,
    /// How the weapon in the player's hands is seen to move, and the spent
    /// cases it has thrown out.
    weapon_motion: crate::weapon_motion::WeaponMotion,
    /// Where the weapon was last drawn, for where its cases come out.
    weapon_at: Option<lore_assets::Transform>,
    /// Whether a stick grenade's pulled cord is drawn with what is held.
    cord: bool,
    /// What is in flight this frame, by model: kept to draw with.
    flying: Vec<(std::rc::Rc<str>, std::rc::Rc<str>, lore_assets::Transform)>,
    /// How far ahead of its grip each weapon model reaches, worked out once
    /// from its shape: what says where the left hand goes.
    weapon_lengths: std::collections::HashMap<std::rc::Rc<str>, f32>,
    /// The NPCs as they are drawn: never read by the world.
    pub(crate) npc_figures: crate::npc_figures::NpcFigures,
    /// The input of the tick being run, as the bot pressed it, kept so a
    /// tick sets no memory aside.
    bot_frame: InputFrame,
}

/// How far ahead of its grip a weapon must reach to be held in both hands
/// by a fore-end, in metres: a pistol is shorter, and the left hand cups
/// the right instead.
const LONG_GUN: f32 = 0.3;

/// Where the left hand cups the right on a short weapon, in the weapon's
/// space: a little left of the grip, below and ahead of it.
const CUPPED: [f32; 3] = [-0.035, -0.045, -0.015];

/// How a body holds a weapon whose model reaches `ahead` metres ahead of
/// its grip, as the script said or else: one hand for a thrown one, otherwise the left hand on the fore-end of a long one (a
/// third of the way along what is ahead of the grip, under the barrel) or
/// cupping the right on a short one.
fn hold_of(ahead: f32, thrown: bool, said: lore_script::HoldSaid) -> crate::character::Hold {
    if said.one_handed.unwrap_or(thrown) {
        return crate::character::Hold {
            support: None,
            cupped: false,
            moved: lore_anim::math::IDENTITY,
        };
    }
    let long = ahead > LONG_GUN;
    let usual = if long {
        [0.0, -0.035, -(ahead * 0.25).min(0.25)]
    } else {
        CUPPED
    };
    crate::character::Hold {
        support: Some(said.support.unwrap_or(usual)),
        cupped: !long,
        moved: lore_anim::math::IDENTITY,
    }
}

/// How far ahead of its grip (along -Z) the weapon model `model` reaches,
/// in metres.
fn reach_ahead(model: &lore_assets::Scene) -> f32 {
    let mut ahead = 0.0_f32;
    for instance in &model.instances {
        let Some(mesh) = model.meshes.get(instance.mesh as usize) else {
            continue;
        };
        for part in &mesh.parts {
            for point in &part.positions {
                let [_, _, z] = lore_anim::math::apply(&instance.transform, *point);
                ahead = ahead.max(-z);
            }
        }
    }
    ahead
}

/// The player's setting that, turned off, lets them land from any height
/// unhurt.
const FALL_DAMAGE: &str = "fall_damage";

/// How fast a body may hit the ground unhurt, in metres a second: a drop
/// of about six metres (v² = 2gh).
const SAFE_LANDING: f32 = 11.0;

/// The health a landing at `speed` metres a second takes: none up to
/// [`SAFE_LANDING`], then growing with the square of the excess, as the
/// energy to soak up does. Ten metres takes about 18 points of 100;
/// twenty, about 150.
fn fall_damage(speed: f32) -> f32 {
    if !speed.is_finite() || speed <= SAFE_LANDING {
        return 0.0;
    }
    let over = speed - SAFE_LANDING;
    2.0 * over * over
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
            character: Character::shared_mannequin().map(|mut character| {
                character.face(yaw);
                character
            }),
            shot: crate::shot::ShotCamera::default(),
            held: None,
            weapon_models: std::collections::HashMap::new(),
            weapon_motion: crate::weapon_motion::WeaponMotion::default(),
            weapon_at: None,
            cord: false,
            flying: Vec::new(),
            weapon_lengths: std::collections::HashMap::new(),
            npc_figures: crate::npc_figures::NpcFigures::mannequin(),
            bot_frame: InputFrame::default(),
        }
        .with_body_said()
    }

    /// Tells the script which clips the body has.
    fn with_body_said(self) -> Self {
        if let Some(character) = &self.character {
            self.world
                .game
                .host()
                .set_body_clips("mannequin", character.clip_names());
        }
        self
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
    /// Moves the player's body on by one tick: which way it faces, what it
    /// plays, and where the camera hangs. The script says which view and
    /// facing it wants; the player may switch the view when the game allows.
    fn step_character(&mut self, input: &InputFrame, step: f32) {
        if let Some(model) = self.world.game.host().take_body_model() {
            self.change_body(&model);
        }
        if let Some(held) = self.world.game.host().take_held() {
            self.change_held(held);
        }
        let dodged = self.world.take_dodge_began();
        let Some(character) = &mut self.character else {
            return;
        };
        let host = self.world.game.host();
        let mut wanted = host.camera_view();
        if wanted.switchable
            && self
                .controls
                .switch_view
                .is_some_and(|action| input.just_pressed(action))
        {
            wanted.third = !wanted.third;
            host.said_view(wanted.third);
        }
        // Seen from a script's shot, the body is drawn whole.
        if host.camera().shot != lore_script::ShotSaid::Player {
            wanted.third = true;
        }
        character.view = if wanted.third {
            View::Third
        } else {
            View::First
        };
        character.facing = if wanted.aimed {
            Facing::Aim
        } else {
            Facing::Move
        };
        character.boom = Boom {
            distance: wanted.distance,
            height: wanted.height,
            shoulder: wanted.shoulder,
        };
        for ask in host.take_animation_asks() {
            character.ask(&ask);
        }
        // A dodge that began on the last tick is played, if the body has
        // a clip for it.
        if let Some(clip) = dodged {
            character.ask(&lore_script::AnimationAsk::Play {
                clip: clip.to_owned(),
                fade: 0.08,
                speed: 1.0,
                looping: Some(false),
                upper_body: false,
            });
        }
        character.guard(host.blocking());
        let player = *self.world.player();
        character.tick(
            &player,
            (self.yaw, self.pitch),
            &self.world.ground.level,
            step,
            |clip, moment| host.heard_moment(clip, &moment.name),
        );
        host.set_playing(character.playing_names());
        // A swing hits what it passes through, once each, through health.
        let rules = &self.world.ground.rules;
        // As of the last tick, less whoever the script has since killed
        // or taken out.
        let around: Vec<crate::character::Struck> = (self.world.standing.iter())
            .filter(|body| host.health(body.who).is_some_and(|(now, _)| now > 0.0))
            .map(|body| crate::character::Struck {
                who: body.who,
                feet: body.feet,
                height: body.height,
                radius: rules.radius,
            })
            .collect();
        let mut blows = Vec::new();
        character.swing_through(player.position, &around, |blow| blows.push(blow));
        let host = self.world.game.host_mut();
        for blow in blows {
            host.damage(
                blow.victim,
                blow.damage,
                Some(CharacterId::PLAYER),
                &blow.clip,
            );
        }
    }

    /// Holds the view on the next character in front of the player: the
    /// nearest, then the next nearest each time it is asked, and after the
    /// last, nobody; the player's enemies (`game.sides`) before anyone
    /// else. The dead and those out of reach are passed over.
    fn next_lock(&mut self) {
        let eye = self.world.player().eye_position();
        let next = lock_after(self.world.game.host(), eye, self.yaw);
        self.world.game.host_mut().lock_on(next);
    }

    /// Holds the view on whoever the script locked on to: the camera turns
    /// toward their chest at a steady speed, whatever the look action does.
    fn hold_lock(&mut self, step: f32) {
        let host = self.world.game.host();
        let Some(target) = host.locked_on().and_then(|id| host.character_position(id)) else {
            return;
        };
        let eye = self.world.player().eye_position();
        let (dx, dy, dz) = (
            target[0] - eye[0],
            target[1] + 1.2 - eye[1],
            target[2] - eye[2],
        );
        let flat = (dx * dx + dz * dz).sqrt();
        if flat < 0.3 {
            return;
        }
        let yaw = libm::atan2f(-dx, -dz).rem_euclid(std::f32::consts::TAU);
        let pitch = libm::atan2f(dy, flat).clamp(-0.6, 0.4);
        self.yaw = crate::character::turn_toward(self.yaw, yaw, 8.0 * step);
        self.pitch += (pitch - self.pitch).clamp(-4.0 * step, 4.0 * step);
    }

    /// Makes the player's body `model`: one of the engine's own, or a model
    /// file in the game's folder, which plays the mannequin's clips by bone
    /// name as well as its own. A model that cannot be read is said in the
    /// log, and the body stays as it was.
    fn change_body(&mut self, model: &str) {
        let made = match model {
            "mannequin" => Character::shared_mannequin()
                .ok_or_else(|| "the engine's mannequin could not be read".to_owned()),
            "mannequin_female" => Character::female_mannequin().map_err(|e| e.to_string()),
            file => self.world.game.read_model(file).and_then(|scene| {
                Character::with_mannequin_clips(scene).map_err(|e| e.to_string())
            }),
        };
        match made {
            Ok(mut character) => {
                if let Some(old) = &self.character {
                    character.view = old.view;
                    character.face(old.yaw());
                }
                self.world
                    .game
                    .host()
                    .set_body_clips(model, character.clip_names());
                self.character = Some(character);
            }
            Err(why) => log::error!("game.player:setModel(\"{model}\"): {why}"),
        }
    }

    /// Puts `held` in the player's hand, or empties it: a built-in thing
    /// by name, or a model file of the game's.
    fn change_held(&mut self, held: Option<lore_script::HeldSaid>) {
        self.held = held.and_then(|held| {
            let model = match crate::props::built_in(&held.model) {
                Some(model) => Ok(model),
                None => (self.world.game)
                    .read_model(&held.model)
                    .map(std::sync::Arc::new),
            };
            match model {
                Ok(model) => Some((model, held.left)),
                Err(why) => {
                    log::error!("game.player:hold(\"{}\"): {why}", held.model);
                    None
                }
            }
        });
    }

    /// Where the player's feet are for a frame `alpha` of the way from the
    /// last tick to this one.
    fn feet_at(&self, alpha: f32) -> [f32; 3] {
        let player = self.world.player();
        match self.before {
            Some(before) => {
                let was = [before[0], before[1] - player.eye_height, before[2]];
                blend::position(was, player.position, alpha)
            }
            None => player.position,
        }
    }

    /// The model of the weapon in the player's hands, if they hold one
    /// that has a model: one of the engine's own, or a file of the game's,
    /// read the first time it is held.
    fn weapon_model(
        &mut self,
    ) -> Option<(
        crate::weapon_motion::Pieces,
        crate::character::Hold,
        crate::weapon_motion::Pose,
    )> {
        if self.weapon_motion.following_through() {
            return None;
        }
        // An armed grenade is held wound back in place of what is held,
        // the one on the belt or the one in the hands.
        let host = self.world.game.host();
        let armed = [
            host.held_weapon(CharacterId::PLAYER),
            host.thrown_weapon(CharacterId::PLAYER),
        ]
        .into_iter()
        .flatten()
        .find(|weapon| weapon.state.armed.is_some());
        let winding = armed.is_some();
        let held = match armed {
            Some(armed) => armed,
            None => host.held_weapon(CharacterId::PLAYER)?,
        };
        let name = held.model.clone()?;
        let thrown = held.settings.thrown;
        let pieces = self.read_weapon_model(&name, &held.name)?;
        let ahead = *self.weapon_lengths.entry(name).or_insert_with(|| {
            let mut ahead = reach_ahead(&pieces.still);
            for (_, part) in &pieces.parts {
                ahead = ahead.max(reach_ahead(part));
            }
            ahead
        });
        let mut hold = hold_of(ahead, thrown, held.hold);
        let mut pose = self.weapon_motion.pose(&held);
        if winding {
            hold.support = None;
            pose.whole = crate::weapon_motion::wound_back();
            self.cord = held.settings.arming == lore_weapons::Arming::OnPull;
        } else {
            self.cord = false;
        }
        hold.moved = pose.whole;
        Some((pieces, hold, pose))
    }

    /// The model called `name` (one of the engine's, or a file of the
    /// game's) for the weapon `weapon`, read once.
    fn read_weapon_model(
        &mut self,
        name: &std::rc::Rc<str>,
        weapon: &str,
    ) -> Option<crate::weapon_motion::Pieces> {
        if let Some(model) = self.weapon_models.get(name) {
            return model.clone();
        }
        let model = crate::props::weapon(name).or_else(|| {
            match self.world.game.read_model(name) {
                Ok(model) => Some(std::sync::Arc::new(model)),
                // The engine's own stand-in, if it has one by that name,
                // rather than empty hands.
                Err(why) => {
                    log::error!("The weapon \"{weapon}\" has the model \"{name}\": {why}");
                    crate::props::weapon(&weapon.to_lowercase())
                }
            }
        });
        let pieces = model.map(|model| crate::weapon_motion::split(&model));
        self.weapon_models
            .insert(std::rc::Rc::clone(name), pieces.clone());
        pieces
    }

    /// Draws each grenade, rocket and slow bullet in flight with its
    /// weapon's model, turning as its weapon says, and lying on its side
    /// once it has come to rest.
    fn show_in_flight(&mut self, frame: &mut Frame) {
        // Kept between frames, so drawing them sets no memory aside.
        let mut shown = std::mem::take(&mut self.flying);
        shown.clear();
        let host = self.world.game.host();
        for (weapon, settings, projectile) in self.world.shooting.in_flight() {
            if let Some(model) = host.weapon_model(weapon) {
                let place = crate::weapon_motion::flying(settings.spin, projectile);
                shown.push((model, std::rc::Rc::clone(weapon), place));
            }
        }
        for (model, weapon, place) in &shown {
            if let Some(pieces) = self.read_weapon_model(model, weapon) {
                frame.show_thing(&pieces.still, *place, true);
                for (_, part) in &pieces.parts {
                    frame.show_thing(part, *place, true);
                }
            }
        }
        self.flying = shown;
    }

    /// Hands the frame the player's body, drawn between the last two ticks.
    /// `eyes` is true when the picture is seen from the player's own eyes,
    /// where a weapon is drawn in front of them rather than in the hand.
    fn show_body(&mut self, frame: &mut Frame, camera: [f32; 3], look: (f32, f32), eyes: bool) {
        let weapon = if self.held.is_none() {
            self.weapon_model()
        } else {
            None
        };
        let feet = self.feet_at(frame.alpha());
        let Some(character) = &mut self.character else {
            return;
        };
        let model = std::sync::Arc::clone(character.model());
        character.set_hold(weapon.as_ref().map(|(_, hold, _)| *hold));
        let drawn = character.figures(frame.alpha(), feet, camera, look);
        let seen = drawn.seen;
        let weapon_at = drawn.weapon;
        frame.show_figure(&model, &drawn);
        // What is in the hand is one object in both views: where the hand
        // is drawn, there it is.
        if let Some((thing, left)) = &self.held
            && let Some(place) = character.held_place(*left)
        {
            frame.show_thing(thing, place, seen);
        }
        // The weapon is where the body holds it, its hands on it, and each
        // moving part where it has moved to.
        self.weapon_at = weapon_at;
        if let (Some((pieces, _, pose)), Some(place)) = (&weapon, weapon_at) {
            frame.show_thing(&pieces.still, place, seen || eyes);
            for (part, model) in &pieces.parts {
                let at = lore_anim::math::combine(&place, pose.part(*part));
                frame.show_thing(model, at, seen || eyes);
            }
        }
        // A stick grenade's cord, pulled, hanging from where it is held.
        if self.cord
            && let Some(place) = weapon_at
        {
            frame.show_thing(&crate::props::cord(), place, seen || eyes);
        }
        // The spent cases it has thrown out.
        let (case, cases) = self.weapon_motion.cases();
        for place in cases {
            frame.show_thing(case, place, true);
        }
        self.show_in_flight(frame);
    }

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
                    frame.sound(SoundAsk::Play(UNDER_WATER, rumble.looping().in_world()));
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
                frame.sound(SoundAsk::Play(id, play.volume(loud).looping().in_world()));
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
    fn level_saved(&mut self, level: &Scene) {
        self.world.change_level(level);
    }

    fn typing_ended(&mut self) {
        self.world.typing_ended();
    }

    fn listening_ended(&mut self) {
        self.world.listening_ended();
    }

    fn take_listening(&mut self) -> Option<Option<lore_input::Hears>> {
        self.world.take_listening()
    }

    fn tick(&mut self, input: &InputFrame, step: f32) {
        // A bot that plays presses the player's buttons: the tick is run
        // from what it pressed, as from a device.
        let mut pressed = std::mem::take(&mut self.bot_frame);
        let view = [self.yaw, self.pitch];
        if (self.world).bot_frame(input, view, &self.controls, &mut pressed) {
            self.tick_by(&pressed, step);
        } else {
            self.tick_by(input, step);
        }
        self.bot_frame = pressed;
    }

    fn frame(&mut self, frame: &mut Frame) {
        self.draw(frame);
    }
}

impl Playing {
    /// Steps the world by one tick of `input`.
    fn tick_by(&mut self, input: &InputFrame, step: f32) {
        let before = self.eye;
        let mut asked = self
            .controls
            .asked(input, step, &mut self.yaw, &mut self.pitch);
        // In a game that has them: the block button raises the guard, and
        // the lock-on button holds the view on the next one in front.
        let can = self.world.game.host().abilities();
        let held = |action: Option<ActionId>| action.is_some_and(|action| input.pressed(action));
        let guard = can.block && held(self.controls.block);
        self.world.game.host_mut().set_guard_held(guard);
        if can.lock_on && (self.controls.lock_on).is_some_and(|action| input.just_pressed(action)) {
            self.next_lock();
        }
        // The item buttons bring up the next weapon carried, or the one
        // before. With one weapon or none they do nothing.
        if can.switch_weapons {
            for (action, forward) in [
                (self.controls.next_item, true),
                (self.controls.previous_item, false),
            ] {
                if action.is_some_and(|action| input.just_pressed(action)) {
                    let host = self.world.game.host_mut();
                    host.cycle_weapon(CharacterId::PLAYER, forward);
                }
            }
        }
        // A shot that steers: forward is away from the camera, whichever
        // way the player's own view faces.
        let host = self.world.game.host();
        if let Some(yaw) =
            crate::shot::steer_yaw(&host.camera(), |who| host.character_position(who))
        {
            asked.movement.yaw = yaw;
        }
        if self.world.game.host().locked_on().is_some() {
            self.hold_lock(step);
            asked.movement.yaw = self.yaw;
            asked.pitch = self.pitch;
        }
        self.world.ask(asked);
        // Shots go where the crosshair is, seen from where the camera hung
        // on the last tick.
        let camera = self.character.as_ref().map(|character| {
            character.camera(
                1.0,
                self.eye,
                (self.yaw, self.pitch),
                &self.world.ground.level,
            )
        });
        self.world.shooting.camera = camera.filter(|camera| *camera != self.eye);
        self.world.tick(input, step);
        // A turning platform under the player's feet turns their view.
        let round = self.world.take_carried_turn();
        if round != 0.0 {
            self.yaw = (self.yaw + round).rem_euclid(std::f32::consts::TAU);
        }
        // A script or a respawn that put the player somewhere may have
        // turned them too.
        let teleported = self.world.take_teleported();
        if let Some((yaw, pitch)) = teleported.flatten() {
            (self.yaw, self.pitch) = look::turned(yaw, pitch, [0.0; 2]);
            if let Some(character) = &mut self.character {
                character.face(self.yaw);
            }
        }
        self.eye = self.world.player().eye_position();
        self.step_character(input, step);
        // The NPCs breathe, fall and look at the player.
        self.npc_figures.tick(&self.world, self.eye, step);
        // The weapon in the hands: the moments of its motion and its cases.
        let player = *self.world.player();
        let held = self.world.game.host().held_weapon(CharacterId::PLAYER);
        let thrown = self.world.game.host().thrown_weapon(CharacterId::PLAYER);
        let speed = player.velocity[0].hypot(player.velocity[2]);
        let walk = self.world.ground.rules.walk_speed;
        self.weapon_motion.tick(
            held.as_ref(),
            thrown.as_ref(),
            (self.yaw, self.pitch),
            speed,
            walk,
            player.velocity,
            self.weapon_at.as_ref(),
            &self.world.ground.level,
            step,
        );
        // The throw itself, by the body's clip, the arm following through.
        if self.weapon_motion.just_thrown()
            && let Some(character) = &mut self.character
        {
            character.ask(&lore_script::AnimationAsk::Play {
                clip: "throw".to_owned(),
                fade: 0.05,
                speed: 1.6,
                looping: Some(false),
                upper_body: true,
            });
        }
        // Being put somewhere is a cut, not a glide.
        self.before = (teleported.is_none() && !cut(before, self.eye)).then_some(before);
        // What no frame came for in time is let go, so the newest noises
        // always have room.
        let (now, tick_rate) = (self.world.tick, self.world.ground.tick_rate);
        self.heard.retain(|noise| fresh(noise, now, tick_rate));
        let room = MOST_HEARD.saturating_sub(self.heard.len());
        // A script's own sound is played by its request, not from here.
        let noises = (self.world.noises().iter())
            .filter(|noise| noise.kind != crate::noise::NoiseKind::Script)
            .take(room);
        self.heard.extend(noises);
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
    fn draw(&mut self, frame: &mut Frame) {
        let (now, tick_rate) = (self.world.tick, self.world.ground.tick_rate);
        // Where this picture is seen from is worked out first, so that the
        // script's HUD, drawn next, can put a piece on a place in the world.
        // While the bot plays, the person's mouse does not turn the view.
        let turn = (self.controls.look)
            .filter(|_| !self.world.bot_drove())
            .map_or([0.0; 2], |look| {
                look::asked(frame.input(), look, frame.since_tick())
            });
        let seen = self.seen_at(frame.alpha(), turn);
        let camera = self.character.as_ref().map_or(seen.eye, |character| {
            character.camera(
                frame.alpha(),
                seen.eye,
                (seen.yaw, seen.pitch),
                &self.world.ground.level,
            )
        });
        // A script's shot takes the place of the player's own view.
        let said = self.world.game.host().camera();
        let own = crate::shot::Pose {
            position: camera,
            yaw: seen.yaw,
            pitch: seen.pitch,
            lens: (0.0, 0.0),
        };
        let clock = (f64::from(now) + f64::from(frame.alpha())) / f64::from(tick_rate);
        let feet = self.feet_at(frame.alpha());
        let host = self.world.game.host();
        let shot = self.shot.pose(&said, own, clock, |who| {
            if who == CharacterId::PLAYER {
                Some(feet)
            } else {
                host.character_position(who)
            }
        });
        let own_fov = frame.settings().map_or(
            crate::settings::Settings::default().field_of_view,
            |settings| settings.field_of_view,
        );
        self.world.game.host().set_view(lore_script::ViewTold {
            position: shot.position,
            yaw: shot.yaw,
            pitch: shot.pitch,
            fov_degrees: own_fov + (shot.lens.0 - own_fov) * shot.lens.1,
        });
        // First: this reads the game's sound files, if it has not yet.
        self.world.frame(frame);
        for (nth, noise) in (0..).zip(self.heard.drain(..)) {
            if !fresh(&noise, now, tick_rate) {
                continue;
            }
            let names = &self.world.names;
            let tweak = self.world.game.host().sound_tweak(noise.sound_name(names));
            let sounds = self.world.game.sounds_mut();
            if let Some(play) = noise::play(&noise, nth, names, sounds, tweak) {
                frame.sound(SoundAsk::Once(play));
            }
        }
        frame.look_from_through(shot.position, shot.yaw, shot.pitch, shot.lens);
        // From the eyes: in first person, with no shot of the script's.
        let first = self
            .character
            .as_ref()
            .is_some_and(|it| it.view == View::First);
        let eyes = first && (0..3).all(|i| (shot.position[i] - seen.eye[i]).abs() < 0.05);
        self.show_body(frame, shot.position, (seen.yaw, seen.pitch), eyes);
        self.npc_figures.show(frame);
        self.show_water(frame, seen.eye);
        for tread in self.treads.drain(..) {
            frame.tread(tread);
        }
    }
}

mod things;
mod using;

#[cfg(test)]
mod heard;
#[cfg(test)]
mod rates;
#[cfg(test)]
mod third_person;
#[cfg(test)]
mod tools;
#[cfg(test)]
mod wetted;

/// Whom the lock-on holds next, for a player whose eyes are at `eye`
/// looking along `yaw`: the nearest character in front of them and in
/// reach, then the next nearest each time it is asked, and after the last,
/// nobody; the player's enemies (`game.sides`) before anyone else. The dead
/// and those out of reach are passed over.
fn lock_after(host: &ScriptHost, eye: [f32; 3], yaw: f32) -> Option<CharacterId> {
    let (sin, cos) = yaw.sin_cos();
    let sides = host.sides();
    let mut near: Vec<(bool, f32, CharacterId)> = Vec::new();
    host.each_character(|who, feet, health| {
        if who == CharacterId::PLAYER || health <= 0.0 {
            return;
        }
        let (x, z) = (feet[0] - eye[0], feet[2] - eye[2]);
        let far = x.hypot(z);
        let up = feet[1] - eye[1];
        // In reach, and in the half of the world they are looking at.
        if far > 0.2 && far.hypot(up) <= LOCK_REACH && (x * -sin + z * -cos) / far > 0.25 {
            // Whom the player counts as an enemy, or who counts them one.
            let (me, them) = (CharacterId::PLAYER.number(), who.number());
            let friendly = !(sides.is_enemy(me, them) || sides.is_enemy(them, me));
            near.push((friendly, far, who));
        }
    });
    near.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    match host.locked_on() {
        Some(now) => (near.iter())
            .position(|(_, _, who)| *who == now)
            .and_then(|at| near.get(at + 1))
            .map(|(_, _, who)| *who),
        None => near.first().map(|(_, _, who)| *who),
    }
}

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
    fn with_fall_damage_off_the_player_lands_unhurt() {
        use crate::settings::Settings;
        let actions = ActionMap::standard();
        let health_after_a_drop = |fall_damage: bool| {
            let scripts = [(
                "main.luau".to_owned(),
                "game.onStart(function() game.player:teleport(vector.create(0, 25, 0)) end)"
                    .to_owned(),
            )];
            let settings = Settings {
                fall_damage,
                ..Settings::default()
            };
            let script = ScriptGame::from_sources_with_settings(scripts, &actions, settings);
            let mut world = World::new(script, 60);
            for _ in 0..240 {
                world.tick(&InputFrame::default(), 1.0 / 60.0);
            }
            assert!(world.player().grounded);
            world
                .game()
                .host()
                .health(CharacterId::PLAYER)
                .map(|(now, _)| now)
        };
        assert!(health_after_a_drop(true).is_some_and(|health| health < 100.0));
        assert_eq!(health_after_a_drop(false), Some(100.0));
    }

    #[test]
    fn a_turn_of_the_difficulty_is_the_same_from_the_same_input_at_any_frame_rate() {
        use crate::playback::{Did, TICK_RATE, first_difference, paces, play, recording, steady};
        use lore_input::Key;
        // Each press of jump turns the dial a step harder, round to the
        // easiest after the hardest: as a player would in a menu.
        const RULES: &str = r#"
            game.onTick(function()
                if game.input.justPressed("jump") then
                    local steps = game.difficulty.steps
                    local at = table.find(steps, game.difficulty.name) or 1
                    game.difficulty.name = steps[at % #steps + 1]
                end
            end)
        "#;
        let mut presses = recording(240);
        for tick in [20, 70, 150] {
            presses[tick].push(Did::Press(Key::Space.into()));
            presses[tick + 3].push(Did::Release(Key::Space.into()));
        }
        let state = |game: &Playing| {
            let scales = game.world.difficulty();
            let done = game.world.game().host().rules_fingerprint();
            (
                scales.damage.to_bits(),
                scales.notice.to_bits(),
                scales.aim.to_bits(),
                done,
            )
        };
        let start = || {
            let actions = ActionMap::standard();
            Playing::start(
                ScriptGame::from_source(RULES, &actions),
                None,
                &actions,
                TICK_RATE,
            )
        };
        let expected = play(&mut start(), &presses, steady(TICK_RATE), state);
        // Standard, Hard, Story, Standard.
        let damage = |tick: usize| f32::from_bits(expected.states[tick].0);
        assert_eq!(
            [damage(10), damage(30), damage(100), damage(200)],
            [1.0, 1.5, 0.5, 1.0]
        );
        for (pace, frame_times) in paces() {
            let found = play(&mut start(), &presses, frame_times, state);
            if let Some(difference) = first_difference(&expected.states, &found.states) {
                panic!("At {pace}, {difference}");
            }
        }
    }

    #[test]
    fn a_hard_landing_hurts_and_a_short_drop_does_not() {
        let from = |metres: f32| fall_damage((2.0 * 9.81 * metres).sqrt());
        assert_eq!(from(3.0), 0.0);
        assert_eq!(from(6.0), 0.0);
        assert!((10.0..30.0).contains(&from(10.0)), "{}", from(10.0));
        assert!(from(20.0) > 100.0, "{}", from(20.0));
        assert_eq!(fall_damage(f32::NAN), 0.0);
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

    #[test]
    fn a_swing_hits_what_is_in_front_once_and_not_what_is_behind() {
        let actions = ActionMap::standard();
        let script = ScriptGame::from_source(
            r#"local ticks = 0
            game.onTick(function()
                ticks += 1
                if ticks == 40 then
                    game.player:swing("sword_attack", { damage = 30, reach = 1.0 })
                end
            end)"#,
            &actions,
        );
        let mut game = Playing::start(script, None, &actions, 60);
        let mut input = lore_input::Input::new(actions);
        // Let the player settle on their feet first.
        for _ in 0..30 {
            game.tick(input.tick(), STEP);
        }
        let feet = game.world.player().position;
        // Facing -Z: one in reach in front, one behind.
        let front = game.world.add_dummy([feet[0], feet[1], feet[2] - 1.1]);
        let back = game.world.add_dummy([feet[0], feet[1], feet[2] + 1.1]);
        let who = |game: &Playing, id: DummyId| {
            game.world.dummies.iter().find(|d| d.id == id).unwrap().name
        };
        let (front, back) = (who(&game, front), who(&game, back));
        for _ in 0..90 {
            game.tick(input.tick(), STEP);
        }
        let host = game.world.game.host();
        assert_eq!(host.health(front), Some((70.0, 100.0)), "hit once");
        assert_eq!(host.health(back), Some((100.0, 100.0)), "not hit");
    }

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
    fn a_level_saved_again_is_walked_from_where_the_player_stands() {
        use lore_assets::{LoadOptions, Scene};

        let level = Scene::from_bytes(
            "level.glb",
            crate::scaffold::STARTER_LEVEL,
            None,
            LoadOptions::default(),
        )
        .unwrap();
        let actions = ActionMap::standard();
        let script = ScriptGame::from_source("", &actions);
        let mut game = Playing::start(script, Some(&level), &actions, 60);
        let mut input = lore_input::Input::new(actions);
        for _ in 0..30 {
            game.tick(input.tick(), STEP);
        }
        let stood = game.world.player().position;

        // The same level with its floor four metres higher: the player is
        // not sent back to a spawn point, and falls to nothing new.
        let mut raised = level.clone();
        for instance in &mut raised.instances {
            instance.transform[3][1] += 4.0;
        }
        Game::level_saved(&mut game, &raised);
        assert_eq!(game.world.player().position, stood);
        assert_ne!(
            game.world.ground.level.triangle_count(),
            0,
            "the new shapes are the ground"
        );
        for _ in 0..30 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);
    }

    #[test]
    fn a_script_changes_how_fast_the_player_walks_and_how_high_they_jump() {
        use lore_input::Key;

        /// The top speed and the highest the feet got, over three seconds
        /// of running forward with a jump one second in.
        fn played(script: &str) -> (f32, f32, bool) {
            let actions = ActionMap::standard();
            let script = ScriptGame::from_source(script, &actions);
            let mut game = Playing::start(script, None, &actions, 60);
            let mut input = lore_input::Input::new(actions);
            input.press(Key::W);
            let (mut fastest, mut highest, mut sprinted) = (0.0f32, 0.0f32, false);
            for tick in 0..180 {
                if tick == 60 {
                    input.press(Key::Space);
                }
                game.tick(input.tick(), STEP);
                let player = game.world.player();
                fastest = fastest.max(player.velocity[0].hypot(player.velocity[2]));
                highest = highest.max(player.position[1]);
                sprinted |= player.sprinting;
            }
            assert_eq!(game.world.game.host().last_error(), None);
            (fastest, highest, sprinted)
        }

        let (speed, height, _) = played("");
        assert!((speed - 4.5).abs() < 0.1, "{speed}");
        assert!((height - 1.1).abs() < 0.1, "{height}");

        let (speed, height, _) = played(
            "game.onStart(function()\n\
             game.player:setWalkSpeed(8)\n\
             game.player:setJumpHeight(3)\n\
             game.player:setGravity(10)\n\
             end)",
        );
        assert!((speed - 8.0).abs() < 0.15, "{speed}");
        assert!((height - 3.0).abs() < 0.15, "{height}");

        // A script reads what the player is doing.
        let (_, _, sprinted) = played(
            "game.onTick(function()\n\
             assert(not game.player.crouched and not game.player.swimming)\n\
             assert(game.player.walkSpeed == 4.5)\n\
             end)",
        );
        assert!(!sprinted);
    }

    #[test]
    fn the_player_uses_the_thing_in_front_of_them_and_sees_its_prompt() {
        use lore_input::Key;
        use lore_ui::Draw;

        // A lever two metres ahead of where the player starts (they face
        // -Z), a chest ten metres off, and a door behind them.
        let map = crate::map::Map::parse(
            "map.toml",
            "version = 1\n\
             [[entity]]\nid = \"lever\"\ntype = \"lever\"\nname = \"Gate lever\"\n\
             position = [0, 1.4, -2]\n\
             [[entity]]\nid = \"chest\"\ntype = \"chest\"\nname = \"Chest\"\n\
             position = [0, 1.4, -10]\n\
             [[entity]]\nid = \"door\"\ntype = \"door\"\nname = \"Door\"\n\
             position = [0, 1.4, 2]\n",
        )
        .unwrap();
        let actions = ActionMap::standard();
        let script = ScriptGame::from_pack_parts(
            [(
                "main.luau".to_owned(),
                r#"local pulls = 0
                local lever = game.thing("Gate lever")
                lever.useText = "Pull"
                lever:onUse(function() pulls += 1 end)
                for _, other in { game.thing("Chest"), game.thing("Door") } do
                    other:onUse(function() error("out of reach or behind") end)
                end
                game.onTick(function()
                    local meant = game.player.lookingAt
                    assert(meant == nil or meant == lever)
                    if pulls == 1 then lever.useSeconds = 0.5 end
                    if pulls == 2 then lever.usable = false end
                end)"#
                    .to_owned(),
            )],
            Vec::new(),
            map,
            &actions,
            crate::settings::Settings::default(),
        );
        let mut game = Playing::start(script, None, &actions, 60);
        let mut input = lore_input::Input::new(actions);
        let words = |game: &mut Playing, input: &lore_input::Input| -> Vec<String> {
            let mut frame = Frame::between_ticks(input, 1.0, STEP);
            game.frame(&mut frame);
            frame
                .take_ui()
                .draw()
                .iter()
                .filter_map(|draw| match draw {
                    Draw::Text { text, .. } => Some(text.to_owned()),
                    Draw::Rect { .. }
                    | Draw::Turned { .. }
                    | Draw::Box { .. }
                    | Draw::Shadow { .. }
                    | Draw::Group { .. }
                    | Draw::EndGroup => None,
                })
                .collect()
        };
        let pulls = |game: &Playing| -> usize {
            let mut usable = 0;
            game.world.game.host().each_usable(|_, _, _| usable += 1);
            usable
        };
        for _ in 0..3 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);
        // The key and the words are on the screen with no HUD code.
        assert_eq!(words(&mut game, &input), ["E", "Pull"]);

        // A press pulls it once, however long the key stays down.
        input.press(Key::E);
        for _ in 0..10 {
            game.tick(input.tick(), STEP);
        }
        input.release(Key::E);
        game.tick(input.tick(), STEP);
        assert_eq!(game.world.game.host().last_error(), None);
        assert_eq!(pulls(&game), 3, "still usable after one pull");

        // Now it wants holding for half a second: a tap does nothing...
        input.press(Key::E);
        for _ in 0..10 {
            game.tick(input.tick(), STEP);
        }
        input.release(Key::E);
        game.tick(input.tick(), STEP);
        assert_eq!(pulls(&game), 3);
        // ...and a hold does, after which the script switches it off.
        input.press(Key::E);
        for _ in 0..40 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);
        assert_eq!(pulls(&game), 2, "the lever switched itself off");
        assert!(
            words(&mut game, &input).is_empty(),
            "no prompt for what cannot be used"
        );
    }

    #[test]
    fn the_player_pushes_a_crate_is_stopped_by_a_heavy_one_and_stands_on_a_fixed_one() {
        use lore_input::Key;

        /// Walks forward for four seconds toward a thing two metres ahead
        /// with these map properties, and says where the thing and the
        /// player ended (how far along -Z), how many things are awake, and
        /// what the script read of it.
        fn walked_into(properties: &str) -> (f32, f32, usize) {
            let map = crate::map::Map::parse(
                "map.toml",
                &format!(
                    "version = 1\n[[entity]]\nid = \"box\"\ntype = \"prop\"\nname = \"Box\"\n\
                     position = [0, 0, -2]\n[entity.properties]\nmodel = \"crate\"\n{properties}"
                ),
            )
            .unwrap();
            let actions = ActionMap::standard();
            let script = ScriptGame::from_pack_parts(
                [(
                    "main.luau".to_owned(),
                    r#"local box = game.thing("Box")
                    local started = box.position
                    game.onTick(function()
                        -- A script reads where it has got to and how it moves.
                        assert(box.moves == (box.weight > 0))
                        if not box.moves then assert(box.position == started and box.resting) end
                    end)"#
                        .to_owned(),
                )],
                Vec::new(),
                map,
                &actions,
                crate::settings::Settings::default(),
            );
            let mut game = Playing::start(script, None, &actions, 60);
            let mut input = lore_input::Input::new(actions);
            input.press(Key::W);
            for _ in 0..240 {
                game.tick(input.tick(), STEP);
            }
            assert_eq!(game.world.game.host().last_error(), None);
            let mut at = [0.0; 3];
            game.world.things.each(|_, told| at = told.position);
            (
                -at[2],
                -game.world.player().position[2],
                game.world.things.awake(),
            )
        }

        // A 20 kg crate goes along in front of the player.
        let (crate_at, player_at, _) = walked_into("moves = true\n");
        assert!(crate_at > 6.0, "pushed only to {crate_at}");
        // The player is behind it, touching: half a crate and their own
        // half-width back.
        assert!(
            (crate_at - player_at - 0.85).abs() < 0.2,
            "crate {crate_at}, player {player_at}"
        );

        // Two tonnes does not move, and the player does not walk through it.
        let (heavy_at, player_at, _) = walked_into("moves = true\nweight = 2000\n");
        assert!((heavy_at - 2.0).abs() < 0.05, "{heavy_at}");
        assert!(player_at < 1.2 && player_at > 1.0, "{player_at}");

        // One that does not move is as solid, and costs nothing: none awake.
        let (fixed_at, player_at, awake) = walked_into("");
        assert_eq!(fixed_at, 2.0);
        assert!(player_at < 1.2 && player_at > 1.0, "{player_at}");
        assert_eq!(awake, 0);

        // The same walk twice leaves the crate in the very same place.
        assert_eq!(walked_into("moves = true\n"), walked_into("moves = true\n"));
    }

    #[test]
    fn a_script_shoves_and_moves_a_thing_and_a_fixed_one_says_why_not() {
        let map = crate::map::Map::parse(
            "map.toml",
            "version = 1\n\
             [[entity]]\nid = \"a\"\ntype = \"prop\"\nname = \"Barrel\"\nposition = [5, 0, 0]\n\
             [entity.properties]\nmodel = \"barrel\"\nmoves = true\n\
             [[entity]]\nid = \"b\"\ntype = \"prop\"\nname = \"Wall crate\"\nposition = [9, 0, 0]\n\
             [entity.properties]\nmodel = \"crate\"\n",
        )
        .unwrap();
        let actions = ActionMap::standard();
        let script = ScriptGame::from_pack_parts(
            [(
                "main.luau".to_owned(),
                r#"local barrel = game.thing("Barrel")
                local ticks = 0
                game.onTick(function()
                    ticks += 1
                    if ticks == 90 then
                        assert(barrel.weight == 30 and barrel.resting and barrel.moves)
                        barrel:push(vector.create(0, 0, -150))
                    elseif ticks == 94 then
                        assert(barrel.velocity.z < -2 and not barrel.resting)
                    elseif ticks == 260 then
                        assert(barrel.position.z < -0.5, tostring(barrel.position))
                        barrel:teleport(vector.create(-4, 3, 0))
                    elseif ticks == 262 then
                        assert(math.abs(barrel.position.x + 4) < 0.01, tostring(barrel.position))
                        assert(barrel.position.y > 2.5)
                    elseif ticks == 270 then
                        game.thing("Wall crate"):push(vector.create(0, 0, 9))
                    end
                end)"#
                    .to_owned(),
            )],
            Vec::new(),
            map,
            &actions,
            crate::settings::Settings::default(),
        );
        let mut game = Playing::start(script, None, &actions, 60);
        let mut input = lore_input::Input::new(actions);
        for _ in 0..269 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);
        game.tick(input.tick(), STEP);
        let message = &game.world.game.host().last_error().unwrap().message;
        assert!(
            message.contains("\"Wall crate\" cannot be pushed"),
            "{message}"
        );
        assert!(message.contains("moves = true"), "{message}");
    }

    #[test]
    fn the_player_picks_a_crate_up_carries_it_and_throws_it() {
        use lore_input::{Key, MouseButton};
        use lore_ui::Draw;

        /// A game with a thing two metres ahead with these map properties
        /// and this script, the player looking down at it.
        fn game_with(properties: &str, script: &str) -> (Playing, lore_input::Input) {
            let map = crate::map::Map::parse(
                "map.toml",
                &format!(
                    "version = 1\n[[entity]]\nid = \"box\"\ntype = \"prop\"\nname = \"Box\"\n\
                     position = [0, 0, -2]\n[entity.properties]\nmodel = \"crate\"\n{properties}"
                ),
            )
            .unwrap();
            let actions = ActionMap::standard();
            let script = ScriptGame::from_pack_parts(
                [("main.luau".to_owned(), script.to_owned())],
                Vec::new(),
                map,
                &actions,
                crate::settings::Settings::default(),
            );
            let mut game = Playing::start(script, None, &actions, 60);
            game.pitch = -0.5;
            let mut input = lore_input::Input::new(actions);
            for _ in 0..20 {
                game.tick(input.tick(), STEP);
            }
            (game, input)
        }
        fn words(game: &mut Playing, input: &lore_input::Input) -> Vec<String> {
            let mut frame = Frame::between_ticks(input, 1.0, STEP);
            game.frame(&mut frame);
            frame
                .take_ui()
                .draw()
                .iter()
                .filter_map(|draw| match draw {
                    Draw::Text { text, .. } => Some(text.to_owned()),
                    Draw::Rect { .. }
                    | Draw::Turned { .. }
                    | Draw::Box { .. }
                    | Draw::Shadow { .. }
                    | Draw::Group { .. }
                    | Draw::EndGroup => None,
                })
                .collect()
        }
        fn crate_at(game: &Playing) -> ([f32; 3], [f32; 3]) {
            let mut at = ([0.0; 3], [0.0; 3]);
            game.world
                .things
                .each(|_, told| at = (told.middle, told.velocity));
            at
        }

        let (mut game, mut input) = game_with(
            "moves = true\n",
            r#"local box = game.thing("Box")
            local picked, dropped, thrown = 0, 0, 0
            game.onPickUp(function(thing) assert(thing == box) picked += 1 end)
            game.onDrop(function(thing, wasThrown)
                assert(thing == box)
                dropped += 1
                if wasThrown then thrown += 1 end
            end)
            game.onTick(function()
                assert(box.carryable and game.player.carryWeight == 40)
                assert((game.player.carrying == box) == (picked > dropped))
                if thrown == 1 then assert(dropped == 2) end
            end)"#,
        );
        // Light enough to carry, so it says so with no line of script.
        assert_eq!(words(&mut game, &input), ["E", "Pick up"]);
        input.press(Key::E);
        game.tick(input.tick(), STEP);
        input.release(Key::E);
        for _ in 0..40 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);
        assert_eq!(game.world.things.carried(), Some("box"));
        assert_eq!(words(&mut game, &input), ["E", "Drop"]);
        // It is held out in front of the eyes and a little under them.
        let eyes = game.world.player().eye_position();
        let (held, _) = crate_at(&game);
        assert!(
            (held[2] - (eyes[2] - 1.35 * 0.5f32.cos())).abs() < 0.15,
            "{held:?}"
        );
        assert!(held[1] > 0.4 && held[1] < eyes[1], "{held:?}");
        // Walking backward, it comes along and does not stop the player.
        input.press(Key::S);
        for _ in 0..90 {
            game.tick(input.tick(), STEP);
        }
        input.release(Key::S);
        let (held, _) = crate_at(&game);
        let player = game.world.player().position;
        assert!(
            player[2] > 4.0,
            "the crate held the player back: {player:?}"
        );
        assert!(
            (held[2] - player[2] + 1.18).abs() < 0.3,
            "{held:?} {player:?}"
        );
        // Put down with the same button, and picked up again.
        input.press(Key::E);
        game.tick(input.tick(), STEP);
        input.release(Key::E);
        game.tick(input.tick(), STEP);
        assert_eq!(game.world.things.carried(), None);
        for _ in 0..60 {
            game.tick(input.tick(), STEP);
        }
        input.press(Key::E);
        game.tick(input.tick(), STEP);
        input.release(Key::E);
        for _ in 0..40 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.things.carried(), Some("box"));
        // Thrown with the fire button, toward what the crosshair is on.
        game.pitch = 0.2;
        for _ in 0..30 {
            game.tick(input.tick(), STEP);
        }
        input.press(MouseButton::MouseLeft);
        game.tick(input.tick(), STEP);
        input.release(MouseButton::MouseLeft);
        game.tick(input.tick(), STEP);
        assert_eq!(game.world.things.carried(), None);
        let (_, flying) = crate_at(&game);
        assert!(flying[2] < -8.0 && flying[1] > 1.0, "{flying:?}");
        for _ in 0..20 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);

        // Too heavy: nothing offers to pick it up, and E does nothing.
        let (mut heavy, mut input) = game_with("moves = true\nweight = 90\n", "");
        assert!(words(&mut heavy, &input).is_empty());
        input.press(Key::E);
        heavy.tick(input.tick(), STEP);
        assert_eq!(heavy.world.things.carried(), None);
        // Unless the game says the player is that strong, or that it may be.
        let (mut strong, input) = game_with(
            "moves = true\nweight = 90\n",
            "game.onStart(function() game.player:setCarryWeight(100) end)",
        );
        assert_eq!(words(&mut strong, &input), ["E", "Pick up"]);
        // And a script says what may not be carried.
        let (mut fixed, input) =
            game_with("moves = true\n", "game.thing(\"Box\").carryable = false");
        assert!(words(&mut fixed, &input).is_empty());
    }

    /// A game on the flat floor with this map and script.
    fn game_on_map(things: &str, script: &str) -> (Playing, lore_input::Input) {
        let map = crate::map::Map::parse("map.toml", &format!("version = 1\n{things}")).unwrap();
        let actions = ActionMap::standard();
        let script = ScriptGame::from_pack_parts(
            [("main.luau".to_owned(), script.to_owned())],
            Vec::new(),
            map,
            &actions,
            crate::settings::Settings::default(),
        );
        let game = Playing::start(script, None, &actions, 60);
        (game, lore_input::Input::new(actions))
    }

    /// A map entry: a solid thing with these properties.
    fn solid_thing(name: &str, at: [f32; 3], properties: &str) -> String {
        format!(
            "[[entity]]\nid = \"{name}\"\ntype = \"prop\"\nname = \"{name}\"\n\
             position = [{}, {}, {}]\n[entity.properties]\n{properties}\n",
            at[0], at[1], at[2]
        )
    }

    #[test]
    fn a_lift_carries_whoever_stands_on_it_without_a_jolt() {
        // A crate whose top is just over the floor, under the player.
        let (mut game, mut input) = game_on_map(
            &solid_thing("Lift", [0.0, -0.9, 0.0], "model = \"crate\""),
            r#"local lift = game.thing("Lift")
            local ticks, arrived = 0, 0
            lift:onArrive(function(thing) assert(thing == lift) arrived += 1 end)
            game.onTick(function()
                ticks += 1
                if ticks == 30 then
                    assert(not lift.sliding)
                    lift:moveTo(vector.create(0, 3, 0), 2)
                elseif ticks == 60 then
                    assert(lift.sliding and arrived == 0)
                elseif ticks == 170 then
                    assert(not lift.sliding and arrived == 1)
                    assert(math.abs(lift.position.y - 3) < 0.001)
                    -- And along, to another thing's place would do as well.
                    lift:moveTo(vector.create(5, 3, 0), 2)
                elseif ticks == 320 then
                    assert(arrived == 2)
                end
            end)"#,
        );
        let mut widest: f32 = 0.0;
        for tick in 0..330u16 {
            game.tick(input.tick(), STEP);
            if tick > 20 {
                // The feet stay on its top the whole way, up and along.
                let mut top = 0.0;
                game.world
                    .things
                    .each(|_, told| top = told.position[1] + 1.0);
                let player = game.world.player();
                widest = widest.max((player.position[1] - top).abs());
                assert!(player.grounded, "in the air on tick {tick}");
            }
        }
        assert_eq!(game.world.game.host().last_error(), None);
        assert!(widest < 0.04, "the feet left the lift by {widest} m");
        let player = game.world.player().position;
        assert!(
            (player[1] - 4.0).abs() < 0.04 && (player[0] - 5.0).abs() < 0.05,
            "{player:?}"
        );
    }

    #[test]
    fn a_turning_platform_turns_whoever_stands_on_it() {
        let (mut game, mut input) = game_on_map(
            &solid_thing("Table", [0.0, -0.9, 0.4], "model = \"crate\""),
            r#"local table, ticks = game.thing("Table"), 0
            game.onTick(function()
                ticks += 1
                if ticks == 20 then table:turnTo(90, 1) end
            end)"#,
        );
        for _ in 0..15 {
            game.tick(input.tick(), STEP);
        }
        let (yaw, was) = (game.yaw(), game.world.player().position);
        for _ in 0..90 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);
        // A quarter turn to the left, and so is the view.
        let turned = (game.yaw() - yaw).rem_euclid(TAU);
        assert!(
            (turned - std::f32::consts::FRAC_PI_2).abs() < 1e-3,
            "{turned}"
        );
        // They stood 0.4 m in front of its middle (-Z of it): a quarter
        // turn to the left puts them 0.4 m to its left (-X).
        let now = game.world.player().position;
        assert!((was[2] - 0.0).abs() < 0.01, "{was:?}");
        assert!(
            (now[0] + 0.4).abs() < 0.03 && (now[2] - 0.4).abs() < 0.03,
            "{now:?}"
        );
    }

    #[test]
    fn a_door_opens_when_it_is_used_and_shuts_again() {
        use lore_input::Key;
        use lore_ui::Draw;

        fn words(game: &mut Playing, input: &lore_input::Input) -> Vec<String> {
            let mut frame = Frame::between_ticks(input, 1.0, STEP);
            game.frame(&mut frame);
            frame
                .take_ui()
                .draw()
                .iter()
                .filter_map(|draw| match draw {
                    Draw::Text { text, .. } => Some(text.to_owned()),
                    Draw::Rect { .. }
                    | Draw::Turned { .. }
                    | Draw::Box { .. }
                    | Draw::Shadow { .. }
                    | Draw::Group { .. }
                    | Draw::EndGroup => None,
                })
                .collect()
        }
        // A door across the way ahead, hinged half a metre to the left
        // (-X) of the player and shut along +X, and walls either side.
        let things = [
            solid_thing("Door", [-0.5, 0.0, -1.5], "model = \"door\"\nopens = true"),
            solid_thing("Left", [-1.0, 0.0, -1.5], "model = \"crate\""),
            solid_thing("Right", [1.0, 0.0, -1.5], "model = \"crate\""),
        ]
        .concat();
        let (mut game, mut input) = game_on_map(
            &things,
            r#"local door, opened = game.thing("Door"), 0
            door:onArrive(function() opened += 1 end)
            game.onTick(function() assert(door.isOpen == (opened % 2 == 1) or door.sliding) end)"#,
        );
        for _ in 0..10 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(words(&mut game, &input), ["E", "Open"]);
        // Shut, it is in the way.
        input.press(Key::W);
        for _ in 0..60 {
            game.tick(input.tick(), STEP);
        }
        input.release(Key::W);
        let stopped = game.world.player().position[2];
        assert!(stopped > -1.3, "walked through a shut door to {stopped}");
        // Used, it swings open in under a second, and the way is clear.
        input.press(Key::E);
        game.tick(input.tick(), STEP);
        input.release(Key::E);
        for _ in 0..60 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);
        assert_eq!(game.world.things.door("Door"), Some(true));
        // It has swung out of the way, so it is no longer what is looked at.
        assert!(words(&mut game, &input).is_empty());
        input.press(Key::W);
        for _ in 0..90 {
            game.tick(input.tick(), STEP);
        }
        input.release(Key::W);
        let through = game.world.player().position[2];
        assert!(through < -3.0, "still stopped at {through}");
        assert_eq!(game.world.game.host().last_error(), None);
    }

    #[test]
    fn a_thing_sliding_into_someone_pushes_them_and_waits_when_they_are_caught() {
        // A crate slides at the player, who has a fixed one behind them.
        let things = [
            solid_thing("Ram", [0.0, 0.0, -3.0], "model = \"crate\""),
            solid_thing("Wall", [0.0, 0.0, 2.0], "model = \"crate\""),
        ]
        .concat();
        let (mut game, mut input) = game_on_map(
            &things,
            r#"local ram, ticks, blocks = game.thing("Ram"), 0, 0
            ram:onBlocked(function(thing, who)
                assert(thing == ram and who == game.player)
                blocks += 1
            end)
            game.onTick(function()
                ticks += 1
                if ticks == 10 then ram:moveTo(vector.create(0, 0, 0.6), 3) end
                if ticks == 400 then
                    assert(blocks == 1, "told " .. blocks .. " times")
                    assert(ram.sliding and ram.position.z < 0.5)
                end
            end)"#,
        );
        let mut furthest: f32 = 0.0;
        for _ in 0..400 {
            game.tick(input.tick(), STEP);
            furthest = furthest.max(game.world.player().position[2]);
        }
        assert_eq!(game.world.game.host().last_error(), None);
        // Pushed back toward the wall, and never through it.
        assert!(furthest > 0.6 && furthest < 1.2, "{furthest}");
    }

    #[test]
    fn what_cannot_be_slid_or_opened_says_why() {
        let things = [
            solid_thing("Loose", [4.0, 0.0, 0.0], "model = \"crate\"\nmoves = true"),
            solid_thing("Fixed", [8.0, 0.0, 0.0], "model = \"crate\""),
        ]
        .concat();
        let said = |line: &str| {
            let (mut game, mut input) =
                game_on_map(&things, &format!("game.onStart(function() {line} end)"));
            game.tick(input.tick(), STEP);
            game.world.game.host().last_error().unwrap().message.clone()
        };
        let message = said("game.thing(\"Loose\"):moveTo(vector.create(0, 0, 0))");
        assert!(
            message.contains("it falls and is pushed about"),
            "{message}"
        );
        let message = said("game.thing(\"Fixed\"):open()");
        assert!(message.contains("is not a door"), "{message}");
        assert!(message.contains("opens = true"), "{message}");
        let message = said("game.thing(\"Fixed\"):moveTo(\"up\")");
        assert!(
            message.contains("a place as a vector, or another thing"),
            "{message}"
        );
        let message = said("game.thing(\"Fixed\"):turnTo(90, \"slowly\")");
        assert!(message.contains("how many seconds it takes"), "{message}");
    }

    #[test]
    fn climbing_and_ladders_are_each_one_switch() {
        use lore_input::Key;

        /// Holds forward and jump (or forward alone) for four seconds and
        /// says how high the player got, where they ended, and whether the
        /// script ever read them as climbing or on a ladder.
        fn played(things: &str, script: &str, jump: bool) -> (f32, [f32; 3]) {
            let (mut game, mut input) = game_on_map(things, script);
            input.press(Key::W);
            if jump {
                input.press(Key::Space);
            }
            let mut highest: f32 = 0.0;
            for _ in 0..240 {
                game.tick(input.tick(), STEP);
                highest = highest.max(game.world.player().position[1]);
            }
            assert_eq!(game.world.game.host().last_error(), None);
            (highest, game.world.player().position)
        }

        // Two fixed crates, one on the other: a wall two metres high.
        let wall = [
            solid_thing("Low", [0.0, 0.0, -2.5], "model = \"crate\""),
            solid_thing("High", [0.0, 1.0, -2.5], "model = \"crate\""),
        ]
        .concat();
        // By the usual rules a jump does not get up it.
        let (highest, _) = played(&wall, "", true);
        assert!(highest < 1.3, "{highest}");
        // One line, and the player pulls themselves up onto it.
        let (_, at) = played(
            &wall,
            r#"local climbed = false
            game.onStart(function()
                assert(not game.player:can("climb") and game.player:can("swim"))
                game.player:setAbility("climb", true)
                assert(game.player:can("climb") and game.player.climbHeight == 2.2)
            end)
            game.onTick(function()
                if game.player.climbing then climbed = true end
                if game.player.position.y > 1.9 then assert(climbed) end
            end)"#,
            true,
        );
        assert!(at[1] > 1.9 || at[2] < -3.2, "never got up: {at:?}");

        // A ladder in the way: walked past until the game says ladders
        // are climbed, then climbed by holding forward.
        // It stands against a wall five crates high, as a ladder does.
        let mut ladder = "[[entity]]\nid = \"l\"\ntype = \"ladder\"\nposition = [0, 0, -2]\n\
                          [entity.properties]\nheight = 6\n"
            .to_owned();
        for row in 0..5u8 {
            ladder.push_str(&solid_thing(
                &format!("Wall{row}"),
                [0.0, f32::from(row), -3.0],
                "model = \"crate\"",
            ));
        }
        let ladder = ladder.as_str();
        let (highest, at) = played(ladder, "", false);
        assert!(highest < 0.1 && at[2] > -2.3, "{highest} {at:?}");
        let (highest, _) = played(
            ladder,
            r#"local seen = false
            game.onStart(function() game.player:setAbility("ladders", true) end)
            game.onTick(function()
                if game.player.onLadder then seen = true end
                if game.player.position.y > 3 then assert(seen) end
            end)"#,
            false,
        );
        // Up the wall and onto its top.
        assert!(highest > 4.9, "climbed only to {highest}");
    }

    #[test]
    fn a_dodge_throws_the_player_the_way_they_push_where_the_game_has_dodges() {
        use lore_input::Key;

        /// Walks forward for a second, taps the dodge key, and says how far
        /// the next 0.35 s took them and whether the script saw a dodge.
        fn dodged(script: &str, held: Option<Key>) -> (f32, [f32; 3]) {
            let (mut game, mut input) = game_on_map("", script);
            if let Some(key) = held {
                input.press(key);
            }
            for _ in 0..60 {
                game.tick(input.tick(), STEP);
            }
            let from = game.world.player().position;
            input.press(Key::LeftAlt);
            game.tick(input.tick(), STEP);
            input.release(Key::LeftAlt);
            for _ in 0..20 {
                game.tick(input.tick(), STEP);
            }
            let to = game.world.player().position;
            assert_eq!(game.world.game.host().last_error(), None);
            (
                (to[0] - from[0]).hypot(to[2] - from[2]),
                [to[0] - from[0], 0.0, to[2] - from[2]],
            )
        }
        let on = r#"local saw = false
            game.onStart(function()
                game.player:setAbility("dodge", true)
                assert(game.player.dodgeSeconds == 0.35 and game.player.dodgeDistance == 3.2)
            end)
            game.onTick(function() if game.player.dodging then saw = true end end)"#;
        // Without the ability the key does nothing: a third of a second's walk.
        let (walked, _) = dodged("", Some(Key::W));
        assert!((walked - 1.57).abs() < 0.2, "{walked}");
        // With it, 3.2 metres in the 0.35 s, the way they were going.
        let (far, way) = dodged(on, Some(Key::W));
        assert!((far - 3.2).abs() < 0.35, "{far}");
        assert!(way[2] < -3.0, "{way:?}");
        // To the side while strafing, and backward from standing still.
        let (_, way) = dodged(on, Some(Key::D));
        assert!(way[0] > 2.8, "{way:?}");
        let (far, way) = dodged(on, None);
        assert!(far > 2.5 && way[2] > 2.5, "{way:?}");
        // A game sets how far and how long.
        let (far, _) = dodged(
            "game.onStart(function()\n\
             game.player:setAbility(\"dodge\", true)\n\
             game.player:setDodgeDistance(1.5)\n\
             game.player:setDodgeSeconds(0.2)\n\
             end)",
            None,
        );
        // The dodge's own metre and a half, and a step to stop in.
        assert!(far > 1.4 && far < 2.0, "{far}");
    }

    #[test]
    fn the_item_buttons_change_weapon_and_throw_a_grenade_past_the_rifle() {
        use lore_input::{GamepadButton, Key, MouseWheel};

        let (mut game, mut input) = game_on_map(
            "",
            r#"game.onSpawn(function()
                game.player:giveWeapon("pistol")
                game.player:giveWeapon("rifle")
                game.player:giveWeapon("grenade")
            end)"#,
        );
        let held = |game: &Playing| {
            let host = game.world.game.host();
            host.held_weapon(CharacterId::PLAYER)
                .map(|held| held.name.to_string())
        };
        let mut tap = |game: &mut Playing, button: lore_input::Button| {
            input.press(button);
            game.tick(input.tick(), STEP);
            input.release(button);
            game.tick(input.tick(), STEP);
        };
        tap(&mut game, Key::Z.into());
        assert_eq!(held(&game).as_deref(), Some("rifle"));
        // The wheel and the d-pad go round the two, past the grenade.
        tap(&mut game, MouseWheel::MouseWheelDown.into());
        assert_eq!(held(&game).as_deref(), Some("pistol"));
        tap(&mut game, GamepadButton::DpadRight.into());
        assert_eq!(held(&game).as_deref(), Some("rifle"));
        tap(&mut game, GamepadButton::DpadLeft.into());
        assert_eq!(held(&game).as_deref(), Some("pistol"));
        // The item button throws the grenade, and the pistol stays up.
        let grenades = |game: &Playing| {
            let host = game.world.game.host();
            host.thrown_weapon(CharacterId::PLAYER)
                .map(|it| it.state.magazine)
        };
        assert_eq!(grenades(&game), Some(1));
        tap(&mut game, Key::G.into());
        assert_eq!(held(&game).as_deref(), Some("pistol"));
        assert_eq!(grenades(&game), Some(0));
        assert_eq!(game.world.game.host().last_error(), None);
    }

    #[test]
    fn a_game_can_turn_weapon_changing_off() {
        use lore_input::MouseWheel;

        let (mut game, mut input) = game_on_map(
            "",
            r#"game.onSpawn(function()
                game.player:setAbility("switchWeapons", false)
                game.player:giveWeapon("pistol")
                game.player:giveWeapon("rifle")
            end)"#,
        );
        game.tick(input.tick(), STEP);
        input.press(MouseWheel::MouseWheelDown);
        game.tick(input.tick(), STEP);
        let host = game.world.game.host();
        let held = host.held_weapon(CharacterId::PLAYER).map(|held| held.name);
        assert_eq!(held.as_deref(), Some("rifle"));
    }

    #[test]
    fn the_block_button_raises_the_guard_and_the_lock_button_goes_round_who_is_in_front() {
        use lore_input::{Key, MouseButton};

        let (mut game, mut input) = game_on_map(
            "",
            r#"game.onStart(function()
                game.player:setAbility("block", true)
                game.player:setAbility("lockOn", true)
                assert(game.player:can("block") and game.player:can("lock_on"))
            end)
            game.onTick(function()
                local who = game.player.lockedOn
                print(game.player.blocking, who and who.name or "nobody")
            end)"#,
        );
        // Two dummies ahead (-Z), the nearer on the left, and one behind.
        game.world.add_dummy([-1.0, 0.0, -4.0]);
        game.world.add_dummy([2.0, 0.0, -9.0]);
        game.world.add_dummy([0.0, 0.0, 5.0]);
        let (near, far) = (game.world.dummies[0].name, game.world.dummies[1].name);
        game.tick(input.tick(), STEP);
        assert!(!game.world.game.host().blocking());
        assert_eq!(game.world.game.host().locked_on(), None);

        // The guard is up for as long as the button is held.
        input.press(MouseButton::MouseRight);
        game.tick(input.tick(), STEP);
        assert!(game.world.game.host().blocking());
        input.release(MouseButton::MouseRight);
        game.tick(input.tick(), STEP);
        assert!(!game.world.game.host().blocking());

        // One press holds the view on the nearest in front, the next on
        // the one after, and the third lets go. The one behind is never it.
        let mut press = |game: &mut Playing| {
            input.press(Key::Q);
            game.tick(input.tick(), STEP);
            input.release(Key::Q);
            game.tick(input.tick(), STEP);
            game.world.game.host().locked_on()
        };
        let before = game.yaw();
        assert_eq!(press(&mut game), Some(near));
        // Held on them, the view turns their way: to the left.
        for _ in 0..30 {
            game.tick(&lore_input::InputFrame::default(), STEP);
        }
        let turned =
            (game.yaw() - before + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
        assert!((turned - 0.245).abs() < 0.02, "{turned}");
        assert_eq!(press(&mut game), Some(far));
        assert_eq!(press(&mut game), None);
        assert_eq!(game.world.game.host().last_error(), None);

        // Without the abilities the buttons do nothing.
        let (mut plain, mut input) = game_on_map("", "");
        plain.world.add_dummy([0.0, 0.0, -4.0]);
        input.press(MouseButton::MouseRight);
        input.press(Key::Q);
        plain.tick(input.tick(), STEP);
        assert!(!plain.world.game.host().blocking());
        assert_eq!(plain.world.game.host().locked_on(), None);
    }

    #[test]
    fn a_paused_game_stops_the_world_and_slow_motion_changes_nothing_but_the_pace() {
        // A crate falling and a lift rising, paused part way.
        let things = [
            solid_thing(
                "Falling",
                [3.0, 6.0, -3.0],
                "model = \"crate\"\nmoves = true",
            ),
            solid_thing("Lift", [-3.0, 0.0, -3.0], "model = \"crate\""),
        ]
        .concat();
        let script = |extra: &str| {
            format!(
                r#"local ticks = 0
                game.onStart(function() game.thing("Lift"):moveTo(vector.create(-3, 4, -3), 2) end)
                game.onTick(function()
                    ticks += 1
                    {extra}
                end)"#
            )
        };
        let places = |game: &Playing| {
            let mut at = Vec::new();
            game.world.things.each(|_, told| at.push(told.position));
            at
        };
        let (mut game, mut input) =
            game_on_map(&things, &script("if ticks == 20 then game.pause() end"));
        for _ in 0..20 {
            game.tick(input.tick(), STEP);
        }
        let stopped = places(&game);
        assert!(stopped[0][1] < 5.9 && stopped[1][1] > 0.01, "{stopped:?}");
        // Paused: tick after tick, nothing in the world moves.
        for _ in 0..60 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(places(&game), stopped);
        assert_eq!(game.world.game.host().time_speed(), 0.0);

        // Slow motion is the same ticks at another pace: the world after
        // 150 of them is the same to the last digit at any speed.
        let after = |extra: &str| {
            let (mut game, mut input) = game_on_map(&things, &script(extra));
            for _ in 0..150 {
                game.tick(input.tick(), STEP);
            }
            assert_eq!(game.world.game.host().last_error(), None);
            (places(&game), game.world.player().position)
        };
        assert_eq!(
            after(""),
            after("if ticks == 5 then game.setTimeSpeed(0.25) end")
        );
    }

    #[test]
    fn the_player_switches_view_with_one_button_where_the_game_allows() {
        use lore_input::Key;

        /// Which view the script sees after F is pressed once.
        fn after_f(script: &str) -> &'static str {
            let actions = ActionMap::standard();
            let script = ScriptGame::from_source(script, &actions);
            let mut game = Playing::start(script, None, &actions, 60);
            let mut input = lore_input::Input::new(actions);
            for _ in 0..5 {
                game.tick(input.tick(), STEP);
            }
            input.press(Key::F);
            game.tick(input.tick(), STEP);
            input.release(Key::F);
            for _ in 0..5 {
                game.tick(input.tick(), STEP);
            }
            assert_eq!(game.world.game.host().last_error(), None);
            if game.world.game.host().camera_view().third {
                "third"
            } else {
                "first"
            }
        }

        // One line chooses the view; nothing else is needed.
        assert_eq!(after_f(""), "first");
        assert_eq!(after_f("game.player:setView(\"third\")"), "third");
        // One more gives the player the button, and the script hears of it.
        assert_eq!(
            after_f(
                "game.player:setView(\"third\")\n\
                 game.player:allowViewSwitch(true)\n\
                 local since = nil\n\
                 game.onTick(function()\n\
                 if since then since += 1 end\n\
                 if game.input.justPressed(\"switch_view\") then since = 0 end\n\
                 -- The tick after the press, the script reads the new view.\n\
                 if since and since >= 1 then assert(game.player.view == \"first\") end\n\
                 end)"
            ),
            "first"
        );
        assert_eq!(after_f("game.player:allowViewSwitch(true)"), "third");
    }

    #[test]
    fn under_a_shot_that_steers_forward_is_away_from_the_camera() {
        use lore_input::Key;

        /// Where two seconds of holding forward takes the player.
        fn walked(script: &str) -> [f32; 3] {
            let (mut game, mut input) = game_on_map("", script);
            input.press(Key::W);
            for _ in 0..120 {
                game.tick(input.tick(), STEP);
            }
            assert_eq!(game.world.game.host().last_error(), None);
            game.world.player().position
        }
        // Their own view faces north (-Z), and so they walk.
        let north = walked("");
        assert!(north[2] < -7.0 && north[0].abs() < 0.1, "{north:?}");
        // Seen side-on by a camera that looks west, forward is west.
        let west = walked(
            "game.camera.sideOn(game.player, { angle = 90, steer = true })\n\
             assert(game.camera.shot() == \"sideOn\")",
        );
        assert!(west[0] < -7.0 && west[2].abs() < 0.1, "{west:?}");
        // The same shot without it leaves the walking alone.
        let plain = walked("game.camera.sideOn(game.player, { angle = 90 })");
        assert!(plain[2] < -7.0, "{plain:?}");
        // From above, forward is up the screen: north.
        let above = walked("game.camera.topDown(game.player, { height = 12, steer = true })");
        assert!(above[2] < -7.0 && above[0].abs() < 0.1, "{above:?}");
    }

    #[test]
    fn a_script_puts_the_camera_somewhere_and_gives_it_back() {
        let actions = ActionMap::standard();
        let script = ScriptGame::from_source(
            r#"local ticks = 0
            game.onTick(function()
                ticks += 1
                if ticks == 10 then
                    game.camera.fixed(vector.create(0, 8, 20), vector.create(0, 8, 0), { fov = 40 })
                elseif ticks == 20 then
                    game.camera.reset(1)
                end
            end)"#,
            &actions,
        );
        let mut game = Playing::start(script, None, &actions, 60);
        let mut input = lore_input::Input::new(actions);
        let own = drawn(&mut game, &input, 1.0);
        assert_eq!(own.lens, None);
        for _ in 0..10 {
            game.tick(input.tick(), STEP);
        }
        assert_eq!(game.world.game.host().last_error(), None);
        let shot = drawn(&mut game, &input, 1.0);
        assert_eq!(shot.position, [0.0, 8.0, 20.0]);
        assert!(near(shot.yaw, 0.0) && near(shot.pitch, 0.0));
        assert_eq!(shot.lens, Some((40.0, 1.0)));

        // Given back over a second: half a second on it is part of the way.
        // The travel starts on the frame that first sees it asked for.
        let mut between = shot;
        for _ in 0..40 {
            game.tick(input.tick(), STEP);
            between = drawn(&mut game, &input, 1.0);
        }
        assert!(between.position[2] > 1.0 && between.position[2] < 19.0);
        for _ in 0..40 {
            game.tick(input.tick(), STEP);
            drawn(&mut game, &input, 1.0);
        }
        let back = drawn(&mut game, &input, 1.0);
        assert_eq!(back.lens, None);
        assert!(back.position[2].abs() < 6.0, "{:?}", back.position);
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
