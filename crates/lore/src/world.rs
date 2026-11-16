//! The world of a game and the player in it.
//!
//! [`World`] is everything the game's rules know: the level, who is where,
//! what they hold and the game's script. It is stepped on the fixed tick,
//! and the same input always gives the same world.
//!
//! [`Playing`] is a world with the player at this computer in it. Each tick
//! it reads the controls and steps the world; each frame it looks from the
//! player's eyes. It is what `lore run` plays.

use std::fmt;

use lore_assets::Scene;
use lore_input::{ActionId, ActionMap, InputFrame};
use lore_physics::{MoveInput, Player};

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

pub(crate) use bodies::{Bodies, Body};
pub use bodies::{MAX_PLAYERS, PlayerId, PlayerInput};
pub(crate) use ground::Ground;
pub use ground::{WalkCheck, try_walking};

/// A world has as many players as it holds ([`MAX_PLAYERS`]), so one more
/// cannot come into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

impl fmt::Display for Full {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The world already has {MAX_PLAYERS} players in it, which is as many as it holds."
        )
    }
}

impl std::error::Error for Full {}

/// A game's world: its script, its level and the players in it.
///
/// Each tick ([`Game::tick`]) it moves every player by what they are asking
/// for ([`ask`](Self::ask)), fires the weapons the script has handed them
/// and then runs the script's tick. Nothing in it reads a clock or rolls a
/// die of its own, so a world given the same input tick after tick goes
/// the same way every time.
#[derive(Debug)]
pub struct World {
    game: ScriptGame,
    ground: Ground,
    bodies: Bodies,
    /// How many ticks have been run.
    tick: u32,
    /// The weapons players hold, and what they have launched.
    shooting: Shooting,
    /// When each player's feet come down.
    walking: Walking,
    /// The noises players made in the newest tick.
    noises: Vec<Noise>,
    /// How many times a player has been put at a spawn point.
    spawned: u32,
}

impl World {
    /// The world of `game` with nobody in it yet, on a flat floor at
    /// height zero until it is given a level
    /// ([`with_level`](Self::with_level)).
    ///
    /// `tick_rate` must be the rate the world is then ticked at.
    #[must_use]
    pub fn new(game: ScriptGame, tick_rate: u32) -> Self {
        Self::on(game, Ground::flat(tick_rate))
    }

    fn on(game: ScriptGame, ground: Ground) -> Self {
        Self {
            game,
            ground,
            bodies: Bodies::default(),
            tick: 0,
            shooting: Shooting::default(),
            walking: Walking::default(),
            noises: Vec::new(),
            spawned: 0,
        }
    }

    /// Plays the game in `level`, usually the game's `level.glb` as
    /// [`level::load`](crate::level::load) reads it: its shapes are solid.
    /// Without this, players walk a flat floor at height zero.
    ///
    /// Players start at the `spawn` entities of the game's map file
    /// ([`ScriptGame::map`]). When it has none they start at the level's
    /// markers whose names begin with `spawn` (`spawn`, `spawn.001`,
    /// `Spawn Red`), and with none of those either, at the level's middle.
    ///
    /// Call it before anyone is in the world. What is wrong with the level
    /// for walking, such as a spawn point with no floor under it, is
    /// logged as a warning ([`try_walking`] gives the same list).
    #[must_use]
    pub fn with_level(mut self, level: &Scene) -> Self {
        self.ground = Ground::of(Some(level), self.game.map(), self.ground.tick_rate);
        self
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

    /// Everyone in the world: their number and where they are.
    pub fn players(&self) -> impl Iterator<Item = (PlayerId, &Player)> + '_ {
        self.bodies.iter().map(|(id, body)| (id, &body.player))
    }

    /// The player numbered `id`, if there is one.
    #[must_use]
    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.bodies.get(id).map(|body| &body.player)
    }

    /// What the player numbered `id` is asking for, if there is one: what
    /// [`ask`](Self::ask) last said, or, for a player nothing has been
    /// asked of yet, standing still and facing the way their spawn point
    /// does.
    #[must_use]
    pub fn asked(&self, id: PlayerId) -> Option<PlayerInput> {
        self.bodies.get(id).map(|body| body.asked)
    }

    /// Says what the player numbered `id` is asking for. Every tick from
    /// the next on moves them by it, and works their weapon by it, until
    /// something else is asked.
    pub fn ask(&mut self, id: PlayerId, asked: PlayerInput) {
        if let Some(body) = self.bodies.get_mut(id) {
            body.asked = asked;
        }
    }

    /// The noises players made in the newest tick, in the order they were
    /// made: every footstep and landing, every shot, the click of an empty
    /// weapon and each reload, with where it was made and how loud. The
    /// player hears what is in this list ([`Playing`]), and so can anything
    /// else that listens.
    #[must_use]
    pub fn noises(&self) -> &[Noise] {
        &self.noises
    }

    /// Brings one more player into the world, at a spawn point away from
    /// everyone already in it and facing the way it does, and tells the
    /// game's script that they have come.
    ///
    /// # Errors
    ///
    /// [`Full`] when the world has no room.
    pub fn add_player(&mut self) -> Result<PlayerId, Full> {
        self.add(None)
    }

    /// Brings one more player in. With `here`, their feet start there, in
    /// place of a spawn point.
    fn add(&mut self, here: Option<[f32; 3]>) -> Result<PlayerId, Full> {
        let id = self.bodies.free().ok_or(Full)?;
        self.spawned = self.spawned.wrapping_add(1);
        let player = match here {
            Some(here) => Player::at(here, &self.ground.rules),
            None => {
                let standing = self.bodies.iter().map(|(_, body)| &body.player);
                self.ground
                    .spawn_in(self.game.map(), standing, self.spawned)
            }
        };
        self.walking.forget(id);
        let name = format!("Player {}", u32::from(id) + 1);
        let name = self.game.host_mut().player_joined(&name);
        self.bodies.put(
            id,
            Body {
                player,
                asked: PlayerInput::default(),
                name,
            },
        );
        // Joining asks for a spawn point; this settles where they start.
        self.place_spawned(here.map(|here| (name, here)));
        if let Some(body) = self.bodies.get_mut(id) {
            let facing = self.ground.facing_at(self.game.map(), body.player.position);
            body.asked.movement.yaw = facing.unwrap_or(0.0);
        }
        Ok(id)
    }

    /// Takes the player numbered `id` out of the world and tells the game's
    /// script that they have gone.
    pub fn remove_player(&mut self, id: PlayerId) {
        if let Some(body) = self.bodies.take(id) {
            self.walking.forget(id);
            self.game.host_mut().player_left(body.name);
        }
    }

    /// Puts everyone waiting for one, by having joined or died, at one of
    /// the map's spawn points. With `here`, the player of that name is put at that place instead.
    fn place_spawned(&mut self, here: Option<(lore_script::PlayerId, [f32; 3])>) {
        // Taken out first: placing reads the map, which the game also holds.
        let waiting: Vec<lore_script::PlayerId> = self.game.host_mut().take_spawns().collect();
        for name in waiting {
            let Some(id) = self.bodies.named(name) else {
                continue;
            };
            self.spawned = self.spawned.wrapping_add(1);
            let placed = match here {
                Some((whom, here)) if whom == name => Player::at(here, &self.ground.rules),
                _ => {
                    // Everyone but the one being placed is to be kept away from.
                    let standing = self
                        .bodies
                        .iter()
                        .filter(|(other, _)| *other != id)
                        .map(|(_, body)| &body.player);
                    self.ground
                        .spawn_in(self.game.map(), standing, self.spawned)
                }
            };
            if let Some(body) = self.bodies.get_mut(id) {
                body.player = placed;
                // Being put somewhere is not a step taken or a fall ended.
                self.walking.forget(id);
            }
        }
    }
}

impl Game for World {
    /// Steps the world by one tick. `input` is what the game's script reads
    /// (`game.input`); the players move by what was asked of each
    /// ([`World::ask`]).
    fn tick(&mut self, input: &InputFrame, step: f32) {
        // A player who died since the last tick is back at a spawn point
        // before anyone moves.
        self.place_spawned(None);
        self.tick = self.tick.saturating_add(1);
        let Ground {
            level,
            rules,
            step: tick,
            ..
        } = &self.ground;
        for body in self.bodies.iter_mut() {
            body.player.step(level, rules, &body.asked.movement, *tick);
        }
        // What was heard of the tick just run: whose feet came down, and
        // below, whose weapon did what.
        self.noises.clear();
        self.walking
            .tick(&self.bodies, self.tick, *tick, &mut self.noises);
        // The weapons players hold, by what each is pressing. The script
        // hears of every shot, hit and reload here.
        self.shooting.tick(
            &self.bodies,
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

    /// What `input` asks of a player for one tick of `step` seconds. The
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

/// Where a player's eyes are and which way they face.
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

/// A world and the player at this computer in it.
///
/// Each tick it turns the view by what the mouse and the stick did, asks
/// the world for what is pressed and steps the world. Each frame it looks
/// from the player's eyes and plays what was heard.
#[derive(Debug)]
pub struct Playing {
    world: World,
    /// The player's number in the world.
    id: PlayerId,
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
}

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
    ///
    /// # Errors
    ///
    /// [`Full`] is not expected: a new world has room for its player.
    pub fn start(
        game: ScriptGame,
        level: Option<&Scene>,
        actions: &ActionMap,
        tick_rate: u32,
    ) -> Result<Self, Full> {
        let ground = Ground::of(level, game.map(), tick_rate);
        Self::enter(World::on(game, ground), actions, None)
    }

    /// Starts `game` on `ground` with its player in the world, as the
    /// editor does to play from where its camera is: the player's eyes
    /// start at `eye`, looking the way `yaw` and `pitch` say
    /// ([`Frame::look_from`]), and the player drops to the floor from
    /// there.
    ///
    /// With nothing under `eye` to land on, the player starts as in any
    /// game, at a spawn point, and `false` is given with the game.
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start).
    #[cfg(feature = "editor")]
    pub(crate) fn start_from(
        game: ScriptGame,
        ground: &Ground,
        actions: &ActionMap,
        eye: [f32; 3],
        [yaw, pitch]: [f32; 2],
    ) -> Result<(Self, bool), Full> {
        let feet = ground
            .feet_under(eye)
            .filter(|_| yaw.is_finite() && pitch.is_finite());
        let mut playing = Self::enter(World::on(game, ground.clone()), actions, feet)?;
        if feet.is_some() {
            (playing.yaw, playing.pitch) = look::turned(yaw, pitch, [0.0; 2]);
        }
        Ok((playing, feet.is_some()))
    }

    /// Brings the player into `world`: at `here` if that is given, else at
    /// a spawn point.
    fn enter(mut world: World, actions: &ActionMap, here: Option<[f32; 3]>) -> Result<Self, Full> {
        let id = world.add(here)?;
        let yaw = world.asked(id).map_or(0.0, |asked| asked.movement.yaw);
        let eye = world.player(id).map_or([0.0; 3], Player::eye_position);
        Ok(Self {
            world,
            id,
            controls: Controls::new(actions),
            yaw,
            pitch: 0.0,
            eye,
            before: None,
            heard: Vec::with_capacity(MOST_HEARD),
        })
    }

    /// The world being played in.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// The player's number in the world.
    #[must_use]
    pub fn id(&self) -> PlayerId {
        self.id
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

impl Game for Playing {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        let before = self.eye;
        let asked = self
            .controls
            .asked(input, step, &mut self.yaw, &mut self.pitch);
        self.world.ask(self.id, asked);
        self.world.tick(input, step);
        if let Some(player) = self.world.player(self.id) {
            self.eye = player.eye_position();
        }
        self.before = (!cut(before, self.eye)).then_some(before);
        // What no frame came for in time is let go, so the newest noises
        // always have room.
        let (now, tick_rate) = (self.world.tick, self.world.ground.tick_rate);
        self.heard.retain(|noise| fresh(noise, now, tick_rate));
        let room = MOST_HEARD.saturating_sub(self.heard.len());
        let noises = self.world.noises();
        self.heard
            .extend_from_slice(&noises[..noises.len().min(room)]);
    }

    /// Looks from the player's eyes and plays what was heard since the
    /// last frame: the player's own feet and weapon equally in both ears,
    /// anyone else's from where they are.
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
            let own = noise.by == self.id;
            let sounds = self.world.game.sounds_mut();
            if let Some(play) = noise::play(&noise, own, nth, sounds) {
                frame.sound(SoundAsk::Once(play));
            }
        }
        let turn = self.controls.look.map_or([0.0; 2], |look| {
            look::asked(frame.input(), look, frame.since_tick())
        });
        let seen = self.seen_at(frame.alpha(), turn);
        frame.look_from(seen.eye, seen.yaw, seen.pitch);
    }
}

#[cfg(test)]
mod heard;
#[cfg(test)]
mod rates;

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
        let mut game = Playing::start(script, None, &actions, 60).unwrap();
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
        let mut game = Playing::start(script, None, &actions, 60).unwrap();
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
        let game = Playing::start(script, None, &actions, 60).unwrap();
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
        let fired = game.world.asked(game.id()).unwrap();
        assert!(fired.fire, "{fired:?}");
        assert_eq!(
            (fired.movement.yaw, fired.pitch),
            (game.yaw(), game.pitch())
        );
    }
}
