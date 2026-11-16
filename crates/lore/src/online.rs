//! The game and its player, each on its own side of the netcode, in one
//! program.
//!
//! [`HostedGame`] is the game's side. It gives whoever joins a player,
//! moves the players with the commands they send and tells them where
//! everybody is, sixty times a second. The game's script hears about each
//! player joining and leaving.
//!
//! [`JoinedGame`] is a player's side. It moves its own player the moment a
//! key is pressed, without waiting for the game's side, and corrects itself
//! when the two disagree.
//!
//! [`LocalGame`] is both, handing each other their packets directly, which
//! is what `lore run` plays. Nothing here opens a port or touches a
//! network.

use std::f32::consts::TAU;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use lore_assets::{Marker, Scene};
use lore_input::{ActionId, ActionMap, InputFrame};
use lore_mode::{SpawnPoint, Stander, choose_spawn};
use lore_net::{
    Client, ClientSettings, Closed, HostEvent, HostSettings, LinkEvent, LinkSettings, MAX_PLAYERS,
    PlayerId, PlayerInput, Refusal, RemotePlayer, Server, ServerSettings, UdpHost, UdpLink,
};
use lore_physics::{Level, MoveInput, MoveSettings, Player};

use crate::blend;
use crate::game::{Frame, Game};
use crate::look;
use crate::map::{MAP_FILE, Map};
use crate::noise::{self, Noise, Walking};
use crate::scaffold::LEVEL_FILE;
use crate::script_game::ScriptGame;
use crate::shooting::{Names, Shooting};
use crate::sound_output::SoundAsk;

/// Half the width of the floor players stand on until a game has a level.
const FLOOR_REACH: f32 = 500.0;

/// What a marker in a level is called to be a place players start: `spawn`,
/// or anything that begins with it, such as `spawn.001` or `Spawn Red`.
const SPAWN_NAME: &str = "spawn";

/// How long a player started at a spawn point is given to land when a level
/// is tried out, in ticks: three seconds, a fall of some 40 m.
const LANDING_TICKS: u32 = 180;

/// A player started where the editor's camera is stands at least this far
/// above the floor under it, in metres, so the first step settles them on it.
#[cfg(feature = "editor")]
const STEP_CLEAR: f32 = 0.01;

/// A player's first snapshot puts them this near a spawn point, in metres
/// across the ground, if that is where the server started them.
const AT_SPAWN: f32 = 0.5;

/// What a server and its players must agree on to work out the same moves.
///
/// Cheap to copy: the level's shapes are shared, not copied.
#[derive(Debug, Clone)]
pub(crate) struct World {
    level: Arc<Level>,
    /// Where players may start. Never empty.
    spawns: Arc<[SpawnPoint]>,
    /// Which way a player starting at each of `spawns` faces.
    facing: Arc<[f32]>,
    rules: MoveSettings,
    /// Ticks in one second.
    tick_rate: u32,
    /// Seconds in one tick.
    step: f32,
}

impl World {
    /// A flat floor, 1 km on a side, at height zero, with players starting
    /// at its middle.
    fn flat(tick_rate: u32) -> Self {
        let r = FLOOR_REACH;
        let corners = [[-r, 0.0, -r], [r, 0.0, -r], [r, 0.0, r], [-r, 0.0, r]];
        let [a, b, c, d] = corners;
        Self::new(
            Level::from_triangles(&[[a, b, c], [a, c, d]]),
            &[],
            tick_rate,
        )
    }

    /// The world of a game's level: every shape in it is solid, and players
    /// start at its spawn markers, or at its middle if it has none.
    fn from_level(level: &Scene, tick_rate: u32) -> Self {
        let spawns: Vec<&Marker> = spawn_markers(level).collect();
        Self::new(Level::from_scene(level), &spawns, tick_rate)
    }

    fn new(level: Level, markers: &[&Marker], tick_rate: u32) -> Self {
        let mut spawns: Vec<SpawnPoint> = markers
            .iter()
            .map(|marker| SpawnPoint {
                position: position_of(marker),
                team: None,
            })
            .collect();
        let mut facing: Vec<f32> = markers.iter().map(|marker| yaw_of(marker)).collect();
        if spawns.is_empty() {
            spawns.push(SpawnPoint {
                position: [0.0; 3],
                team: None,
            });
            facing.push(0.0);
        }
        Self {
            level: Arc::new(level),
            spawns: spawns.into(),
            facing: facing.into(),
            rules: MoveSettings::default(),
            tick_rate,
            step: 1.0 / tick_rate.max(1) as f32,
        }
    }

    /// The world of a game with `level`, or the flat floor of a game with
    /// none. What is wrong for walking with the level and the spawn points
    /// in `map` is logged.
    fn of(level: Option<&Scene>, map: &Map, tick_rate: u32) -> Self {
        let Some(level) = level else {
            return Self::flat(tick_rate);
        };
        let world = Self::from_level(level, tick_rate);
        for problem in world.problems(level, map) {
            log::warn!("{problem}");
        }
        world
    }

    /// A player about to come into the world. Where players start is one
    /// rule everywhere: at the spawn points in `map` if it has any, else
    /// at the level's spawn markers, else in the middle of the level. Of
    /// those the one furthest from `others` is taken, or one nearly as
    /// good; `seed` settles the choice and should differ from one call to
    /// the next.
    ///
    /// Every spawn point is for anyone, and everyone else counts as an
    /// enemy to keep away from, until teams reach the running game (#206).
    fn spawn_in<'a>(
        &self,
        map: &Map,
        others: impl Iterator<Item = &'a Player>,
        seed: u32,
    ) -> Player {
        let mut standers = [Stander {
            position: [0.0; 3],
            enemy: true,
        }; MAX_PLAYERS];
        let mut count = 0;
        for (stander, other) in standers.iter_mut().zip(others) {
            stander.position = other.position;
            count += 1;
        }
        let placed: Vec<SpawnPoint> = map
            .spawns()
            .map(|spawn| SpawnPoint {
                position: spawn.position,
                team: None,
            })
            .collect();
        let points: &[SpawnPoint] = if placed.is_empty() {
            &self.spawns
        } else {
            &placed
        };
        let chosen = choose_spawn(points, None, &standers[..count], seed)
            .and_then(|point| points.get(point))
            .map_or([0.0; 3], |point| point.position);
        Player::at(chosen, &self.rules)
    }

    /// The world the editor plays a game in: `level`'s, or the flat floor of
    /// a game with none. Made once and used for every play, with whatever
    /// map the editor then holds, so nothing is said here about its spawn
    /// points.
    #[cfg(feature = "editor")]
    pub(crate) fn for_editor(level: Option<&Scene>, tick_rate: u32) -> Self {
        level.map_or_else(
            || Self::flat(tick_rate),
            |level| Self::from_level(level, tick_rate),
        )
    }

    /// Where the feet of a player go whose eyes are at `eye`, if there is
    /// anything under them to land on: a standing player's height below
    /// the eyes, and never under the floor.
    #[cfg(feature = "editor")]
    fn feet_under(&self, eye: [f32; 3]) -> Option<[f32; 3]> {
        let floor = self.level.raycast(eye, [0.0, -1.0, 0.0], f32::MAX)?.point[1];
        let feet = (eye[1] - self.rules.standing_eye_height).max(floor + STEP_CLEAR);
        Some([eye[0], feet, eye[2]])
    }

    /// How long one tick is, for sides in one program that count time
    /// in ticks. A sixtieth of a second when the rate is not a usable
    /// number, so that time still goes by.
    fn tick(&self) -> Duration {
        Duration::try_from_secs_f32(self.step)
            .ok()
            .filter(|tick| !tick.is_zero())
            .unwrap_or(Duration::from_nanos(16_666_667))
    }

    /// A player where one may start, before the server has said where this
    /// one does.
    fn first(&self) -> Player {
        let position = self.spawns.first().map_or([0.0; 3], |point| point.position);
        Player::at(position, &self.rules)
    }

    /// Which way a player who has just started at `position` faces, if that
    /// is one of the spawn points: `map`'s if it has any, else the level's.
    fn facing_at(&self, map: &Map, position: [f32; 3]) -> Option<f32> {
        let near =
            |point: [f32; 3]| (point[0] - position[0]).hypot(point[2] - position[2]) <= AT_SPAWN;
        if map.spawns().next().is_some() {
            return map
                .spawns()
                .find(|spawn| near(spawn.position))
                .map(|spawn| spawn.facing.to_radians().rem_euclid(TAU))
                .filter(|yaw| yaw.is_finite());
        }
        self.spawns
            .iter()
            .zip(self.facing.iter())
            .find(|(point, _)| near(point.position))
            .map(|(_, yaw)| *yaw)
    }

    /// What is wrong for walking with `level`, which this world was made
    /// from, and the spawn points in `map`: nothing to stand on, nowhere to
    /// start, or a start that a player falls from. Each is a sentence for
    /// the game's maker.
    fn problems(&self, level: &Scene, map: &Map) -> Vec<String> {
        if self.level.triangle_count() == 0 {
            return vec![format!(
                "{LEVEL_FILE} has no shapes in it, so there is nothing to stand on and players \
                 fall for ever. Add a floor in your 3D tool and export the level again."
            )];
        }
        let mut problems = Vec::new();
        let starts = starts(level, map);
        if starts.is_empty() {
            problems.push(format!(
                "This game has no spawn points, so players start at the level's middle \
                 (0, 0, 0). To choose where they start, run `lore edit` and use \"Place a \
                 spawn here\", which writes {MAP_FILE}; or add an empty object named `spawn` \
                 to the level in your 3D tool, turned the way players should look, and \
                 export it again."
            ));
        }
        let middle = [("the level's middle".to_owned(), [0.0; 3])];
        let starts = if starts.is_empty() {
            &middle[..]
        } else {
            &starts[..]
        };
        let still = MoveInput::default();
        for (place, position) in starts {
            let mut player = Player::at(*position, &self.rules);
            for _ in 0..LANDING_TICKS {
                player.step(&self.level, &self.rules, &still, self.step);
                if player.grounded {
                    break;
                }
            }
            if player.grounded {
                continue;
            }
            let [x, y, z] = position;
            let seconds = LANDING_TICKS as f32 * self.step;
            problems.push(format!(
                "A player who starts at {place} ({x:.1}, {y:.1}, {z:.1}) falls and has not \
                 landed after {seconds:.0} seconds: there is no floor under it. Move it to \
                 just above a floor, or put a floor under it."
            ));
        }
        problems
    }
}

/// Where players start in a game with `level` and `map`, each with what to
/// call it: the spawn points in the map if it has any, else the level's
/// spawn markers. Empty when the game has neither.
fn starts(level: &Scene, map: &Map) -> Vec<(String, [f32; 3])> {
    let placed: Vec<_> = map
        .spawns()
        .map(|spawn| {
            let name = if spawn.name.is_empty() {
                &spawn.id
            } else {
                &spawn.name
            };
            (
                format!("the spawn point \"{name}\" in {MAP_FILE}"),
                spawn.position,
            )
        })
        .collect();
    if !placed.is_empty() {
        return placed;
    }
    spawn_markers(level)
        .map(|marker| {
            (
                format!(
                    "the spawn point \"{}\" in {LEVEL_FILE}",
                    marker.name.as_deref().unwrap_or_default()
                ),
                position_of(marker),
            )
        })
        .collect()
}

/// The markers in `level` that are places for players to start.
fn spawn_markers(level: &Scene) -> impl Iterator<Item = &Marker> {
    level.markers.iter().filter(|marker| {
        marker.name.as_deref().is_some_and(|name| {
            name.trim_start()
                .get(..SPAWN_NAME.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(SPAWN_NAME))
        })
    })
}

/// Where `marker` stands.
fn position_of(marker: &Marker) -> [f32; 3] {
    let [x, y, z, _] = marker.transform[3];
    [x, y, z]
}

/// Which way `marker` faces across the ground, as a player's yaw: 0 faces
/// -Z and it grows turning left. A marker looks along its own -Z, as a
/// camera in a glTF file does; one pointing straight up or down faces -Z.
fn yaw_of(marker: &Marker) -> f32 {
    let [x, _, z, _] = marker.transform[2];
    // The marker's -Z is (-x, -z); yaw is the turn left from -Z to it.
    if x.is_finite() && z.is_finite() && x.hypot(z) > 1.0e-6 {
        x.atan2(z).rem_euclid(TAU)
    } else {
        0.0
    }
}

/// What trying a level out for walking found: see [`try_walking`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkCheck {
    /// How many places for players to start the game has: the `spawn`
    /// entities in its map file or, when that has none, the level's
    /// markers whose names begin with `spawn`.
    pub spawn_points: usize,
    /// What its maker should change, a sentence each. Empty when a player
    /// can start at every spawn point and land on something.
    pub problems: Vec<String>,
}

/// Tries `level` out for walking, with no window: starts a
/// player at each spawn point, lets them stand for three seconds and says
/// which ones never landed. `lore check` prints what it finds.
///
/// The spawn points are the `spawn` entities in `map`
/// ([`ScriptGame::map`]) if it has any, else the level's markers whose
/// names begin with `spawn`, else the middle of the level.
#[must_use]
pub fn try_walking(level: &Scene, map: &Map) -> WalkCheck {
    let world = World::from_level(level, 60);
    WalkCheck {
        spawn_points: starts(level, map).len(),
        problems: world.problems(level, map),
    }
}

/// The game's side: its script and its players.
///
/// Each tick it reads what the players sent, moves them, fires the weapons the
/// script has handed them, sends everyone the result and then runs the
/// script's tick.
#[derive(Debug)]
pub struct HostedGame {
    game: ScriptGame,
    host: UdpHost,
    server: Server,
    world: World,
    /// Who each place on the server is to the script.
    names: Names,
    /// The weapons players hold, and what they have launched.
    shooting: Shooting,
    /// When each player's feet come down.
    walking: Walking,
    /// The noises players made in the newest tick.
    noises: Vec<Noise>,
    /// How many times a player has been put at a spawn point.
    spawned: u32,
    /// Where the first player starts in place of a spawn point: the
    /// editor's "play from here". Used until the first tick is over.
    from_here: Option<[f32; 3]>,
    packet: Vec<u8>,
}

impl HostedGame {
    /// A server for `game` that is on no network and opens no port: only
    /// players in this program join it, with [`JoinedGame::in_program`].
    /// It is what a game played alone runs on, and what a test of a game's
    /// rules should use: packets are handed over directly, so nothing is
    /// late or lost and the same game goes the same way every time.
    ///
    /// `tick_rate` must be the rate the game is then ticked at.
    #[must_use]
    pub fn in_program(game: ScriptGame, tick_rate: u32) -> Self {
        Self::in_program_in(game, World::flat(tick_rate))
    }

    fn in_program_in(game: ScriptGame, world: World) -> Self {
        let host = UdpHost::in_program(HostSettings::default(), world.tick());
        Self::hosting(game, host, world)
    }

    fn hosting(game: ScriptGame, host: UdpHost, world: World) -> Self {
        Self {
            game,
            host,
            server: Server::new(world.step, ServerSettings::default()),
            world,
            names: [None; MAX_PLAYERS],
            shooting: Shooting::default(),
            walking: Walking::default(),
            noises: Vec::new(),
            spawned: 0,
            from_here: None,
            packet: Vec::new(),
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
    /// Call it before anyone joins. What is wrong with the level for
    /// walking, such as a spawn point with no floor under it, is logged as
    /// a warning ([`try_walking`] gives the same list).
    #[must_use]
    pub fn with_level(mut self, level: &Scene) -> Self {
        self.world = World::of(Some(level), self.game.map(), self.world.tick_rate);
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

    /// Everyone joined: their number on the server and where they are.
    pub fn players(&self) -> impl Iterator<Item = (PlayerId, &Player)> + '_ {
        self.server.players()
    }

    /// The noises players made in the newest tick, in the order they were
    /// made: every footstep and landing, every shot, the click of an empty
    /// weapon and each reload, with where it was made and how loud. The
    /// player at this computer hears what is in this list
    /// ([`LocalGame`]), and so can anything else that listens.
    #[must_use]
    pub fn noises(&self) -> &[Noise] {
        &self.noises
    }

    /// Deals with everything players have sent since the last call: lets
    /// in who wants to join, takes commands, and lets go of who left.
    fn listen(&mut self) {
        let Self {
            game,
            host,
            server,
            world,
            names,
            spawned,
            from_here,
            walking,
            ..
        } = self;
        while let Some(event) = host.poll() {
            match event {
                HostEvent::Join(request) => {
                    *spawned = spawned.wrapping_add(1);
                    let player = match *from_here {
                        Some(here) => Player::at(here, &world.rules),
                        None => {
                            let standing = server.players().map(|(_, player)| player);
                            world.spawn_in(game.map(), standing, *spawned)
                        }
                    };
                    match server.join(player) {
                        Ok(id) => {
                            host.accept(request, id);
                            walking.forget(id);
                            let name = format!("Player {}", u32::from(id) + 1);
                            log::info!("{name} joined.");
                            if let Some(slot) = names.get_mut(usize::from(id)) {
                                *slot = Some(game.host_mut().player_joined(&name));
                            }
                        }
                        Err(_) => host.refuse(request, Refusal::Full),
                    }
                }
                // A packet that cannot be read is one more lost packet.
                HostEvent::Packet(id, commands) => drop(server.receive(id, commands)),
                HostEvent::Left(id, why) => {
                    server.leave(id);
                    walking.forget(id);
                    log::info!("Player {} left ({why:?}).", u32::from(id) + 1);
                    if let Some(name) = names.get_mut(usize::from(id)).and_then(Option::take) {
                        game.host_mut().player_left(name);
                    }
                }
            }
        }
    }

    /// Puts everyone the match has just brought into the world, at the
    /// start of a round or after a death, at one of the map's spawn points.
    fn place_spawned(&mut self) {
        // Taken out first: placing reads the map, which the game also holds.
        let waiting: Vec<lore_script::PlayerId> = self.game.host_mut().take_spawns().collect();
        // Only the match's first word on the first player is overruled:
        // the next round, or the next life, starts at a spawn point.
        let mut from_here = self.from_here.take();
        for name in waiting {
            let place = (0..)
                .zip(self.names.iter())
                .find(|(_, held)| **held == Some(name));
            let Some((id, _)) = place else {
                continue;
            };
            self.spawned = self.spawned.wrapping_add(1);
            let placed = match from_here.take() {
                Some(here) => Player::at(here, &self.world.rules),
                None => {
                    // Everyone but the one being placed is to be kept away from.
                    let standing = self
                        .server
                        .players()
                        .filter(|(other, _)| *other != id)
                        .map(|(_, player)| player);
                    self.world.spawn_in(self.game.map(), standing, self.spawned)
                }
            };
            if let Some(player) = self.server.player_mut(id) {
                *player = placed;
                // Being put somewhere is not a step taken or a fall ended.
                self.walking.forget(id);
            }
        }
    }
}

impl Game for HostedGame {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        self.listen();
        // Before anyone is told where they are: a player the match brought
        // into the world, on joining or in the script's last tick, is at
        // their spawn point in the first snapshot that shows them there.
        self.place_spawned();
        self.server.tick(&self.world.level, &self.world.rules);
        // What was heard of the tick just run: whose feet came down, and
        // below, whose weapon did what.
        self.noises.clear();
        self.walking.tick(
            &self.server,
            &self.names,
            self.game.host(),
            self.world.step,
            &mut self.noises,
        );
        // The weapons players hold, by what each pressed in the tick just
        // run. The script hears of every shot, hit and reload here.
        self.shooting.tick(
            &self.server,
            &self.names,
            &self.world.level,
            &self.world.rules,
            self.world.step,
            self.game.host_mut(),
            &mut self.noises,
        );
        for id in self.host.players() {
            if self.server.write_snapshot(id, &mut self.packet).is_ok() {
                self.host.send(id, &self.packet);
            }
        }
        self.game.tick(input, step);
    }

    /// The script's own presenting, such as its rumble, which is for the
    /// player at this computer.
    fn frame(&mut self, frame: &mut Frame) {
        self.game.frame(frame);
    }
}

/// The actions that move a player, looked up once. An action the map does
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

/// Why a player could not join a game, or is no longer in it.
#[derive(Debug)]
pub struct JoinError(Closed);

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "The game ended: {}", self.0)
    }
}

impl std::error::Error for JoinError {}

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

/// A player's side of a game on a server.
///
/// Each tick it reads the server's news, moves its own player with what is
/// pressed without waiting for the server, and sends the server what it did.
#[derive(Debug)]
pub struct JoinedGame {
    link: UdpLink,
    client: Client,
    world: World,
    controls: Controls,
    yaw: f32,
    pitch: f32,
    closed: Option<Closed>,
    packet: Vec<u8>,
    /// The game's map as this side has it, for which way spawn points face.
    map: Map,
    /// The player has been turned the way their spawn point faces.
    faced: bool,
    /// Which way the player looks to begin with (yaw, then pitch) in place
    /// of the way their spawn point faces: the editor's "play from here".
    start_looking: Option<[f32; 2]>,
    /// Where the eyes were on the tick before the newest, to draw frames
    /// between the two from. `None` when the newest tick is to be drawn as
    /// it is: before the first tick, and when the eyes were put somewhere
    /// new.
    before: Option<[f32; 3]>,
}

impl JoinedGame {
    /// Joins `hosted`, which is in this same program: what either side
    /// sends is with the other at its next tick, whole and in order, and
    /// no port is used.
    ///
    /// The player is moved by the actions in `actions` called `move`,
    /// `look`, `jump`, `crouch`, `sprint`, `fire`, `aim` and `reload`,
    /// walks the level the game has and knows its map, and is in the game
    /// when this comes back.
    ///
    /// # Errors
    ///
    /// When the game is full.
    pub fn in_program(hosted: &mut HostedGame, actions: &ActionMap) -> Result<Self, JoinError> {
        let world = hosted.world.clone();
        let link = hosted
            .host
            .link_in_program(LinkSettings::default(), world.tick());
        let mut joined = Self {
            link,
            client: Client::new(world.first(), world.step, ClientSettings::default()),
            world,
            controls: Controls::new(actions),
            yaw: 0.0,
            pitch: 0.0,
            closed: None,
            packet: Vec::new(),
            map: hosted.game.map().clone(),
            faced: false,
            start_looking: None,
            before: None,
        };
        // Each side in turn until the server has answered. It always
        // does: were it not to, the link would give up after its five
        // seconds of ticks, which are counted here and not waited for.
        loop {
            joined.listen();
            if let Some(error) = joined.closed() {
                return Err(error);
            }
            if joined.link.id().is_some() {
                return Ok(joined);
            }
            hosted.listen();
        }
    }

    /// Why this player is no longer on the server, once that is so. The
    /// game goes on being ticked but nothing moves.
    #[must_use]
    pub fn closed(&self) -> Option<JoinError> {
        self.closed.map(JoinError)
    }

    /// This player's number on the server, once joined.
    #[must_use]
    pub fn id(&self) -> Option<PlayerId> {
        self.link.id()
    }

    /// The player as this side has worked out, ahead of the server's word.
    #[must_use]
    pub fn player(&self) -> &Player {
        self.client.player()
    }

    /// Where to put the camera: the player's eyes, with corrections from
    /// the server eased in rather than jumped to.
    #[must_use]
    pub fn eye_position(&self) -> [f32; 3] {
        let [x, y, z] = self.client.view_position();
        [x, y + self.client.player().eye_height, z]
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
            eye: self.eye_position(),
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

    /// Whether this player is in the game and is moved by what is pressed:
    /// the server has given them a place and said where they start.
    fn playing(&self) -> bool {
        self.closed.is_none() && self.link.id().is_some() && self.faced
    }

    /// The other players as the server last told of them.
    pub fn others(&self) -> impl Iterator<Item = RemotePlayer> + '_ {
        self.client.others()
    }

    /// Takes in everything the server has sent since the last call.
    fn listen(&mut self) {
        while let Some(event) = self.link.poll() {
            match event {
                LinkEvent::Joined(_) => {}
                // A snapshot that cannot be read is one more lost packet.
                LinkEvent::Packet(snapshot) => {
                    let _ = self
                        .client
                        .receive(&self.world.level, &self.world.rules, snapshot);
                }
                LinkEvent::Closed(why) => {
                    self.closed.get_or_insert(why);
                }
            }
        }
        // The server's first word says where this player starts: look the
        // way that spawn point faces.
        if !self.faced && self.client.server_tick() > 0 {
            self.faced = true;
            let at = self.client.player().position;
            if let Some([yaw, pitch]) = self.start_looking {
                (self.yaw, self.pitch) = (yaw, pitch);
            } else if let Some(yaw) = self.world.facing_at(&self.map, at) {
                self.yaw = yaw;
            }
        }
    }

    /// What the player is asking for this tick. Turns the view as well.
    fn read(&mut self, input: &InputFrame, step: f32) -> PlayerInput {
        self.controls
            .asked(input, step, &mut self.yaw, &mut self.pitch)
    }
}

impl Game for JoinedGame {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        let before = self.eye_position();
        // Until the server has said where this player starts, what is seen
        // is a guess, and the first real view is not glided to from it.
        let placed = self.faced;
        self.listen();
        // Nothing moves until the server has said where this player starts:
        // a step taken from a guessed place would be taken back.
        if self.playing() {
            let asked = self.read(input, step);
            self.client.predict(
                &self.world.level,
                &self.world.rules,
                &asked,
                &mut self.packet,
            );
            self.link.send(&self.packet);
        }
        self.before = (placed && !cut(before, self.eye_position())).then_some(before);
    }

    /// Looks from the player's eyes. A frame drawn between two ticks sees
    /// from between where the eyes were on the one and on the other, so
    /// walking is smooth however many frames are drawn to a tick. Which
    /// way the eyes face is as of this very frame: the newest tick's
    /// angles and what the mouse and the stick have done since.
    fn frame(&mut self, frame: &mut Frame) {
        if self.closed.is_some() {
            frame.close();
            return;
        }
        let turn = match self.controls.look {
            Some(look) if self.playing() => look::asked(frame.input(), look, frame.since_tick()),
            _ => [0.0; 2],
        };
        let seen = self.seen_at(frame.alpha(), turn);
        frame.look_from(seen.eye, seen.yaw, seen.pitch);
    }
}

/// A game and the one player in it, in one program. The two sides hand
/// each other their packets directly, with no network in between, so
/// nothing is late or lost and no port is opened.
#[derive(Debug)]
pub struct LocalGame {
    hosted: HostedGame,
    joined: JoinedGame,
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

/// Whether `noise` is new enough to play on the server's tick `now`.
#[allow(clippy::cast_precision_loss)]
fn fresh(noise: &Noise, now: u32, tick_rate: u32) -> bool {
    now.wrapping_sub(noise.tick) as f32 <= HEARD_FOR * tick_rate as f32
}

impl LocalGame {
    /// Starts `game` and joins it. The player walks `level`, usually the
    /// game's `level.glb` as [`level::load`](crate::level::load) reads it
    /// (see [`HostedGame::with_level`]), or a flat floor with `None`.
    /// `actions` and `tick_rate` must be the ones the game is then run with.
    ///
    /// # Errors
    ///
    /// When the game cannot be joined, which is not expected.
    pub fn start(
        game: ScriptGame,
        level: Option<&Scene>,
        actions: &ActionMap,
        tick_rate: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let world = World::of(level, game.map(), tick_rate);
        Self::join_own(HostedGame::in_program_in(game, world), actions)
    }

    /// Starts `game` in `world` and joins it, as the editor does to play
    /// from where its camera is: the player's eyes start at `eye`, looking
    /// the way `yaw` and `pitch` say ([`Frame::look_from`]), and the player
    /// drops to the floor from there.
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
        world: &World,
        actions: &ActionMap,
        eye: [f32; 3],
        [yaw, pitch]: [f32; 2],
    ) -> Result<(Self, bool), Box<dyn std::error::Error>> {
        let mut hosted = HostedGame::in_program_in(game, world.clone());
        let feet = world
            .feet_under(eye)
            .filter(|_| yaw.is_finite() && pitch.is_finite());
        hosted.from_here = feet;
        let mut local = Self::join_own(hosted, actions)?;
        if let Some(feet) = feet {
            // The player's side starts out there as well, so not one frame
            // is seen from a spawn point before the server's first word.
            let joined = &mut local.joined;
            let player = Player::at(feet, &world.rules);
            joined.client = Client::new(player, world.step, ClientSettings::default());
            (joined.yaw, joined.pitch) = look::turned(yaw, pitch, [0.0; 2]);
            joined.start_looking = Some([joined.yaw, joined.pitch]);
        }
        Ok((local, feet.is_some()))
    }

    /// Joins `hosted` from this program, walking the very level the
    /// server has.
    fn join_own(
        mut hosted: HostedGame,
        actions: &ActionMap,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let joined = JoinedGame::in_program(&mut hosted, actions)?;
        Ok(Self {
            hosted,
            joined,
            heard: Vec::with_capacity(MOST_HEARD),
        })
    }

    /// The server's side.
    #[must_use]
    pub fn hosted(&self) -> &HostedGame {
        &self.hosted
    }

    /// The player's side.
    #[must_use]
    pub fn joined(&self) -> &JoinedGame {
        &self.joined
    }
}

impl Game for LocalGame {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        self.hosted.tick(input, step);
        // What no frame came for in time is let go, so the newest noises
        // always have room.
        let now = self.hosted.server.tick_number();
        let tick_rate = self.hosted.world.tick_rate;
        self.heard.retain(|noise| fresh(noise, now, tick_rate));
        let room = MOST_HEARD.saturating_sub(self.heard.len());
        let noises = self.hosted.noises();
        self.heard
            .extend_from_slice(&noises[..noises.len().min(room)]);
        self.joined.tick(input, step);
    }

    /// The player hears what was heard in the game since the last frame:
    /// their own feet and weapon equally in both ears, everyone else's
    /// from where they are.
    fn frame(&mut self, frame: &mut Frame) {
        // First: this reads the game's sound files, if it has not yet.
        self.hosted.frame(frame);
        let me = self.joined.id();
        let now = self.hosted.server.tick_number();
        let tick_rate = self.hosted.world.tick_rate;
        for (nth, noise) in (0..).zip(self.heard.drain(..)) {
            if !fresh(&noise, now, tick_rate) {
                continue;
            }
            let own = me == Some(noise.by);
            let sounds = self.hosted.game.sounds_mut();
            if let Some(play) = noise::play(&noise, own, nth, sounds) {
                frame.sound(SoundAsk::Once(play));
            }
        }
        self.joined.frame(frame);
    }
}

#[cfg(test)]
mod heard;
#[cfg(test)]
mod rates;

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

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
        let mut game = LocalGame::start(script, None, &actions, 60).unwrap();
        let mut input = Input::new(actions);
        let asked = |game: &mut LocalGame, input: &mut Input| {
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
        let mut game = LocalGame::start(script, None, &actions, 60).unwrap();
        let mut input = Input::new(actions);
        let view = |game: &mut LocalGame, alpha: f32| {
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
            let before = game.joined.seen();
            game.tick(input.tick(), 1.0 / 60.0);
            let after = game.joined.seen();
            assert_eq!(
                game.joined.before.is_some(),
                game.joined.before == Some(before.eye)
            );
            let moved = (after.eye[2] - before.eye[2]).abs();
            if game.joined.before.is_some() && moved > 0.01 {
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
        assert_eq!(game.joined.seen(), after);
    }

    #[test]
    fn eyes_put_somewhere_new_are_not_glided_there() {
        assert!(!cut([0.0, 1.6, 0.0], [0.0, 1.6, -0.1]));
        // A respawn across the level, and a place that is not a number.
        assert!(cut([0.0, 1.6, 0.0], [0.0, 1.6, -28.0]));
        assert!(cut([0.0, 1.6, 0.0], [f32::NAN, 1.6, 0.0]));
    }

    /// A game played alone, ticked until the player has been given a place
    /// and is moved by what is pressed.
    fn playing_alone() -> (LocalGame, lore_input::Input) {
        let actions = ActionMap::default_fps();
        let script = ScriptGame::from_source("", &actions);
        let mut game = LocalGame::start(script, None, &actions, 60).unwrap();
        let mut input = lore_input::Input::new(actions);
        for _ in 0..300 {
            if game.joined.playing() {
                break;
            }
            game.tick(input.tick(), STEP);
        }
        assert!(game.joined.playing(), "the player joins their own game");
        (game, input)
    }

    const STEP: f32 = 1.0 / 60.0;

    /// The view of a frame drawn `alpha` of the way to the next tick, with
    /// what `input` holds that no tick has seen.
    fn drawn(game: &mut LocalGame, input: &lore_input::Input, alpha: f32) -> crate::game::View {
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
        let (yaw, pitch) = (game.joined.yaw(), game.joined.pitch());

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
        assert_eq!((game.joined.yaw(), game.joined.pitch()), (yaw, pitch));

        // The tick takes in the same movement: the picture does not jump,
        // and nothing is turned twice.
        let before = drawn(&mut game, &input, 1.0);
        game.tick(input.tick(), STEP);
        let after = drawn(&mut game, &input, 0.0);
        assert!(near(after.yaw, before.yaw) && near(after.pitch, before.pitch));
        assert_eq!(
            (after.yaw, after.pitch),
            (game.joined.yaw(), game.joined.pitch())
        );
    }

    #[test]
    fn the_view_follows_the_stick_by_how_long_it_was_held() {
        use lore_input::{Stick, StickAxis};

        let (mut game, mut input) = playing_alone();
        let yaw = game.joined.yaw();
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

        // The tick that sees the click asks the server for a shot along
        // the view that was on the screen, not one from a tick earlier.
        game.tick(input.tick(), STEP);
        assert!(near(game.joined.yaw(), on_screen.yaw));
        assert!(near(game.joined.pitch(), on_screen.pitch));

        // And that is the direction the server fires along.
        let id = game.joined.id().unwrap();
        let mut fired = None;
        for _ in 0..300 {
            game.tick(input.tick(), STEP);
            let asked = game.hosted.server.input(id).unwrap_or_default();
            if asked.fire {
                fired = Some(asked);
                break;
            }
        }
        let fired = fired.expect("the server hears of the click");
        // As closely as a packet carries an angle.
        assert!(
            (fired.movement.yaw - on_screen.yaw).abs() < 1e-3,
            "{fired:?}"
        );
        assert!((fired.pitch - on_screen.pitch).abs() < 1e-3, "{fired:?}");
    }

    #[test]
    fn nobody_looks_around_before_they_are_in_the_game() {
        let actions = ActionMap::default_fps();
        let script = ScriptGame::from_source("", &actions);
        let mut game = LocalGame::start(script, None, &actions, 60).unwrap();
        let mut input = lore_input::Input::new(actions);
        assert!(!game.joined.playing());

        // A tick would not turn the view yet, so a frame does not either.
        let still = drawn(&mut game, &input, 0.5);
        input.move_mouse(300.0, 0.0);
        assert_eq!(drawn(&mut game, &input, 0.5), still);
    }

    /// A marker called `name` at `position`, turned `yaw` radians to the left.
    fn marker(name: &str, position: [f32; 3], yaw: f32) -> Marker {
        let (sin, cos) = yaw.sin_cos();
        let [x, y, z] = position;
        Marker {
            name: Some(name.to_owned()),
            transform: [
                [cos, 0.0, -sin, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [sin, 0.0, cos, 0.0],
                [x, y, z, 1.0],
            ],
            extras: None,
        }
    }

    /// A level that is a floor 20 m on a side at height zero, with `markers`.
    fn floor_with(markers: Vec<Marker>) -> Scene {
        use lore_assets::{Instance, Mesh, MeshPart};

        let mut scene = Scene::default();
        scene.meshes.push(Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: vec![
                    [-10.0, 0.0, -10.0],
                    [10.0, 0.0, -10.0],
                    [10.0, 0.0, 10.0],
                    [-10.0, 0.0, 10.0],
                ],
                indices: vec![0, 2, 1, 0, 3, 2],
                ..MeshPart::default()
            }],
        });
        scene.instances.push(Instance {
            mesh: 0,
            transform: marker("", [0.0; 3], 0.0).transform,
            mirrored: false,
            name: None,
            extras: None,
        });
        scene.markers = markers;
        scene
    }

    #[test]
    fn markers_whose_names_begin_with_spawn_are_where_players_start() {
        let level = floor_with(vec![
            marker("Camera Spot", [1.0, 0.0, 1.0], 0.0),
            marker("spawn", [2.0, 0.0, 3.0], 0.0),
            marker("Spawn Red", [-4.0, 0.5, 0.0], FRAC_PI_2),
            marker("respawn timer", [5.0, 0.0, 5.0], 0.0),
            marker("spawn.001", [0.0, 0.0, -6.0], TAU / 2.0),
        ]);
        let world = World::from_level(&level, 60);

        let places: Vec<_> = world.spawns.iter().map(|point| point.position).collect();
        assert_eq!(
            places,
            [[2.0, 0.0, 3.0], [-4.0, 0.5, 0.0], [0.0, 0.0, -6.0]]
        );
        // A player looks the way the marker is turned: left is west (-X).
        assert_eq!(
            world.facing_at(&Map::default(), [2.0, 0.01, 3.1]),
            Some(0.0)
        );
        let west = world.facing_at(&Map::default(), [-4.0, 0.5, 0.0]).unwrap();
        assert!((west - FRAC_PI_2).abs() < 1.0e-6, "{west}");
        let south = world.facing_at(&Map::default(), [0.0, 0.0, -6.0]).unwrap();
        assert!((south - TAU / 2.0).abs() < 1.0e-6, "{south}");
        assert_eq!(world.facing_at(&Map::default(), [7.0, 0.0, 7.0]), None);
        assert_eq!(try_walking(&level, &Map::default()).spawn_points, 3);
        assert_eq!(
            try_walking(&level, &Map::default()).problems,
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_new_player_is_put_at_a_spawn_point_away_from_the_others() {
        let level = floor_with(vec![
            marker("spawn", [-9.0, 0.0, 0.0], 0.0),
            marker("spawn.001", [9.0, 0.0, 0.0], 0.0),
        ]);
        let world = World::from_level(&level, 60);
        for seed in 0..20 {
            let first = world.spawn_in(&Map::default(), std::iter::empty(), seed);
            assert_eq!(first.position[0].abs(), 9.0);
            let second = world.spawn_in(&Map::default(), std::iter::once(&first), seed);
            assert_eq!(second.position[0], -first.position[0], "seed {seed}");
            // The same question has the same answer.
            assert_eq!(
                world.spawn_in(&Map::default(), std::iter::once(&first), seed),
                second
            );
        }
    }

    #[test]
    fn a_level_with_no_spawn_points_starts_players_at_its_middle_and_says_so() {
        let level = floor_with(vec![marker("Camera Spot", [1.0, 0.0, 1.0], 0.0)]);
        let world = World::from_level(&level, 60);
        assert_eq!(
            world
                .spawn_in(&Map::default(), std::iter::empty(), 7)
                .position,
            [0.0; 3]
        );

        let walked = try_walking(&level, &Map::default());
        assert_eq!(walked.spawn_points, 0);
        assert_eq!(walked.problems.len(), 1, "{:?}", walked.problems);
        let said = &walked.problems[0];
        assert!(
            said.starts_with(
                "This game has no spawn points, so players start at the level's middle"
            ),
            "{said}"
        );
        assert!(said.contains("an empty object named `spawn`"), "{said}");
    }

    #[test]
    fn a_spawn_point_with_no_floor_under_it_is_named() {
        let level = floor_with(vec![
            marker("spawn", [0.0, 2.0, 0.0], 0.0),
            marker("spawn.ledge", [30.0, 0.0, 0.5], 0.0),
        ]);
        let walked = try_walking(&level, &Map::default());
        assert_eq!(walked.problems.len(), 1, "{:?}", walked.problems);
        let said = &walked.problems[0];
        assert!(
            said.starts_with(
                "A player who starts at the spawn point \"spawn.ledge\" in level.glb \
                 (30.0, 0.0, 0.5) falls and has not landed after 3 seconds"
            ),
            "{said}"
        );
        assert!(said.contains("put a floor under it"), "{said}");
    }

    #[test]
    fn spawn_points_in_the_map_file_are_used_instead_of_the_levels_markers() {
        let level = floor_with(vec![marker("spawn", [-9.0, 0.0, 0.0], 0.0)]);
        let world = World::from_level(&level, 60);
        let map = Map::parse(
            "map.toml",
            "version = 1\n\n\
             [[entity]]\nid = \"aaaa\"\ntype = \"spawn\"\nname = \"Blue spawn\"\n\
             position = [5.0, 0.0, 5.0]\nfacing = 90.0\n\n\
             [[entity]]\nid = \"bbbb\"\ntype = \"spawn\"\nposition = [50.0, 0.0, 0.0]\n\n\
             [[entity]]\nid = \"cccc\"\ntype = \"zone\"\nposition = [1.0, 0.0, 1.0]\n",
        )
        .unwrap();

        // With an empty map the level's marker is where players start.
        let empty = Map::default();
        assert_eq!(
            world.spawn_in(&empty, std::iter::empty(), 3).position,
            [-9.0, 0.0, 0.0]
        );
        assert_eq!(world.facing_at(&empty, [-9.0, 0.0, 0.0]), Some(0.0));

        // With spawn points in the map, only they are.
        for seed in 0..20 {
            let x = world.spawn_in(&map, std::iter::empty(), seed).position[0];
            assert!(x == 5.0 || x == 50.0, "{x}");
        }
        let west = world.facing_at(&map, [5.0, 0.0, 5.2]).unwrap();
        assert!((west - FRAC_PI_2).abs() < 1.0e-6, "{west}");
        assert_eq!(world.facing_at(&map, [-9.0, 0.0, 0.0]), None);

        // The walk is tried from the map's points, and the one off the
        // floor is named by its file; one with no name goes by its id.
        let walked = try_walking(&level, &map);
        assert_eq!(walked.spawn_points, 2);
        assert_eq!(walked.problems.len(), 1, "{:?}", walked.problems);
        assert!(
            walked.problems[0].starts_with(
                "A player who starts at the spawn point \"bbbb\" in map.toml (50.0, 0.0, 0.0) \
                 falls"
            ),
            "{}",
            walked.problems[0]
        );
    }

    #[test]
    fn a_level_with_nothing_in_it_has_nothing_to_stand_on() {
        let mut level = Scene::default();
        level.markers.push(marker("spawn", [0.0; 3], 0.0));
        let walked = try_walking(&level, &Map::default());
        assert_eq!(walked.problems.len(), 1, "{:?}", walked.problems);
        assert!(
            walked.problems[0].starts_with("level.glb has no shapes in it"),
            "{}",
            walked.problems[0]
        );
    }
}
