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
use lore_physics::{MoveInput, Player};
use lore_script::CharacterId;

use crate::blend;
use crate::game::{Frame, Game};
use crate::look;
use crate::noise::{self, Noise, Walking};
use crate::script_game::ScriptGame;
use crate::shooting::Shooting;
use crate::sound_output::SoundAsk;

mod bodies;
mod ground;
mod spawn;
mod wet;

pub(crate) use bodies::{Body, Dummy};
pub use bodies::{DummyId, PlayerInput};
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
    /// How many times the player has been put at a spawn point.
    spawned: u32,
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
            spawned,
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
        let ground = Ground::of(Some(level), self.game.map(), self.ground.tick_rate);
        Self::on(self.game, ground, None)
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
    /// any dummy standing on one, if they have died since the last tick.
    fn respawn(&mut self) {
        if !self.game.host_mut().take_respawn() {
            return;
        }
        self.spawned = self.spawned.wrapping_add(1);
        let standing = self.dummies.iter().map(|dummy| dummy.feet);
        self.player.player = self
            .ground
            .spawn_in(self.game.map(), standing, self.spawned);
        // The script is told this tick, and reads where they now are.
        // Being put somewhere is not a step taken or a fall ended.
        self.walking = Walking::default();
    }
}

impl Game for World {
    /// Steps the world by one tick. `input` is what the game's script reads
    /// (`game.input`); the player moves by what was asked of them
    /// ([`World::ask`]).
    fn tick(&mut self, input: &InputFrame, step: f32) {
        // A player who died since the last tick is back at a spawn point
        // before they move.
        self.respawn();
        self.tick = self.tick.saturating_add(1);
        let Ground {
            level,
            rules,
            step: tick,
            ..
        } = &self.ground;
        let Body { player, asked } = &mut self.player;
        let before = *player;
        player.step(level, rules, &asked.movement, *tick);
        // The script reads where the player is as of this tick, in what it
        // is told of below and in its own tick.
        self.game
            .host_mut()
            .set_position(CharacterId::PLAYER, player.position);
        // What was heard of the tick just run: when a foot came down, and
        // below, what the weapon did.
        self.noises.clear();
        self.walking
            .tick(player, self.tick, *tick, &mut self.noises);
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
        self.game.tick(input, step);
    }

    /// The script's own presenting, such as its HUD and its rumble.
    fn frame(&mut self, frame: &mut Frame) {
        self.game.frame(frame);
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
    /// Whether the rumble of being under water is playing.
    under: bool,
}

/// The most marks on the water kept for a frame to show.
const MOST_MARKS: usize = 64;

/// What the rumble heard with the head under water is known by, so that
/// it can be stopped: no script's sound has this number.
const UNDER_WATER: lore_script::SoundId = lore_script::SoundId(u64::MAX);

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
        game: ScriptGame,
        level: Option<&Scene>,
        actions: &ActionMap,
        tick_rate: u32,
    ) -> Self {
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
            under: false,
        }
    }

    /// The world being played in.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
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
            .water_at(eye[0], eye[2])
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

impl Game for Playing {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        let before = self.eye;
        let asked = self
            .controls
            .asked(input, step, &mut self.yaw, &mut self.pitch);
        self.world.ask(asked);
        self.world.tick(input, step);
        self.eye = self.world.player().eye_position();
        self.before = (!cut(before, self.eye)).then_some(before);
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
    }
}

#[cfg(test)]
mod heard;
#[cfg(test)]
mod rates;
#[cfg(test)]
mod wetted;

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    #[test]
    fn a_script_played_alone_rumbles_the_players_gamepad() {
        use lore_input::{GamepadButton, Input, Rumble};

        let actions = ActionMap::default_fps();
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

        let actions = ActionMap::default_fps();
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
        let actions = ActionMap::default_fps();
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
