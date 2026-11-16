//! A player walking, jumping and looking around is the same game at any
//! frame rate: a recording of what they did is played with no window
//! ([`playback`](crate::playback)) and where they are after every tick
//! must be the same to the last bit.
//!
//! The player is moved as a game played alone moves them: predicted on
//! their own side by the controls that [`JoinedGame`](super::JoinedGame)
//! reads, and by the server from the commands that sends. The two sides
//! are handed each other's packets directly here. [`LocalGame`] sends them
//! through this computer's network, where the moment a packet arrives is
//! not the game's to decide, so its state cannot be compared bit for bit
//! until a game played alone opens no port (#47).

use std::f32::consts::TAU;
use std::time::Duration;

use lore_input::{ActionMap, GamepadButton, InputFrame, Key, Stick, StickAxis};
use lore_net::{Client, ClientSettings, PlayerId, Server, ServerSettings};
use lore_physics::Player;

use super::{Controls, World};
use crate::game::{Frame, Game};
use crate::look;
use crate::playback::{
    Did, Played, Recording, TICK_RATE, first_difference, paces, play, recording, steady,
};

/// A server and the one player on it, with no network between them.
struct Walker {
    world: World,
    server: Server,
    client: Client,
    id: PlayerId,
    controls: Controls,
    yaw: f32,
    pitch: f32,
    /// The player's commands on their way to the server.
    commands: Vec<u8>,
    /// The server's word on its way to the player.
    snapshot: Vec<u8>,
    /// The yaw and pitch the newest frame was drawn with.
    drawn: (f32, f32),
}

/// The yaw every walk starts with.
const START_YAW: f32 = 1.0;

impl Walker {
    fn new() -> Self {
        let world = World::flat(TICK_RATE);
        let mut server = Server::new(world.step, ServerSettings::default());
        let id = server.join(world.first()).unwrap();
        Self {
            client: Client::new(world.first(), world.step, ClientSettings::default()),
            server,
            id,
            controls: Controls::new(&ActionMap::default_fps()),
            yaw: START_YAW,
            pitch: 0.0,
            commands: Vec::new(),
            snapshot: Vec::new(),
            drawn: (START_YAW, 0.0),
            world,
        }
    }

    /// Everything that is the game's state, as bits: the player as the
    /// server has them, as their own side has them, and the view.
    fn state(&self) -> Vec<u32> {
        let bits = |player: &Player| {
            let mut bits: Vec<u32> = [
                player.position,
                player.velocity,
                player.ground_normal,
                [player.eye_height, 0.0, 0.0],
            ]
            .iter()
            .flatten()
            .map(|part| part.to_bits())
            .collect();
            bits.extend(
                [
                    player.grounded,
                    player.crouched,
                    player.sprinting,
                    player.jump_held,
                ]
                .map(u32::from),
            );
            bits
        };
        let mut state = bits(self.server.player(self.id).unwrap());
        state.extend(bits(self.client.player()));
        state.extend([self.yaw.to_bits(), self.pitch.to_bits()]);
        state
    }
}

impl Game for Walker {
    /// The server's side and then the player's, as [`LocalGame`] ticks.
    ///
    /// [`LocalGame`]: super::LocalGame
    fn tick(&mut self, input: &InputFrame, step: f32) {
        let (level, rules) = (&self.world.level, &self.world.rules);
        if !self.commands.is_empty() {
            self.server.receive(self.id, &self.commands).unwrap();
        }
        self.server.tick(level, rules);
        self.server
            .write_snapshot(self.id, &mut self.snapshot)
            .unwrap();

        self.client.receive(level, rules, &self.snapshot).unwrap();
        let asked = self
            .controls
            .asked(input, step, &mut self.yaw, &mut self.pitch);
        self.client
            .predict(level, rules, &asked, &mut self.commands);
    }

    /// The view as [`JoinedGame`](super::JoinedGame) draws it: the newest
    /// tick's angles and what no tick has seen yet.
    fn frame(&mut self, frame: &mut Frame) {
        let look = self.controls.look.unwrap();
        let turn = look::asked(frame.input(), look, frame.since_tick());
        self.drawn = look::turned(self.yaw, self.pitch, turn);
        frame.look_from(self.client.view_position(), self.drawn.0, self.drawn.1);
    }
}

fn key(key: Key) -> lore_input::Button {
    key.into()
}

/// Ten seconds of a player walking about: forwards, turning with the
/// mouse, jumping standing and running, sprinting, crouching, turning
/// and walking with the sticks, and a flick of the mouse.
fn a_walk() -> Recording {
    let mut recording = recording(600);
    let mut at = |tick: usize, did: Did| recording[tick].push(did);
    at(30, Did::Press(key(Key::W)));
    at(90, Did::Press(key(Key::Space)));
    at(92, Did::Release(key(Key::Space)));
    at(150, Did::Press(key(Key::LeftShift)));
    at(180, Did::Press(key(Key::Space)));
    at(181, Did::Release(key(Key::Space)));
    at(200, Did::Release(key(Key::W)));
    at(200, Did::Release(key(Key::LeftShift)));
    at(205, Did::Press(key(Key::A)));
    at(240, Did::Stick(Stick::RightStick, StickAxis::X, 0.6));
    at(240, Did::Stick(Stick::RightStick, StickAxis::Y, -0.3));
    at(290, Did::Release(key(Key::A)));
    at(300, Did::Stick(Stick::RightStick, StickAxis::X, 0.0));
    at(300, Did::Stick(Stick::RightStick, StickAxis::Y, 0.0));
    at(320, Did::Stick(Stick::LeftStick, StickAxis::Y, 1.0));
    at(320, Did::Stick(Stick::LeftStick, StickAxis::X, -0.35));
    at(360, Did::Press(GamepadButton::GamepadSouth.into()));
    at(361, Did::Release(GamepadButton::GamepadSouth.into()));
    at(400, Did::Press(key(Key::LeftControl)));
    at(430, Did::Release(key(Key::LeftControl)));
    // A tap that is down and up again before a tick has seen it.
    at(450, Did::Press(key(Key::Space)));
    at(450, Did::Release(key(Key::Space)));
    at(500, Did::Mouse(-800.0, 300.0));
    at(540, Did::Stick(Stick::LeftStick, StickAxis::Y, 0.0));
    at(540, Did::Stick(Stick::LeftStick, StickAxis::X, 0.0));
    for (tick, done) in recording.iter_mut().enumerate().skip(60).take(120) {
        #[allow(clippy::cast_precision_loss)]
        done.push(Did::Mouse(7.0 - (tick % 3) as f32, -2.0));
    }
    recording
}

fn walk_at(
    recording: &Recording,
    frame_times: impl IntoIterator<Item = Duration>,
) -> (Walker, Played<Vec<u32>>) {
    let mut walker = Walker::new();
    let played = play(&mut walker, recording, frame_times, Walker::state);
    (walker, played)
}

/// How high the feet were after each tick of `played`.
fn heights(played: &Played<Vec<u32>>) -> Vec<f32> {
    // The server's player comes first, and height is the second part.
    played
        .states
        .iter()
        .map(|state| f32::from_bits(state[1]))
        .collect()
}

#[test]
fn a_walk_is_the_same_to_the_last_bit_at_any_frame_rate() {
    let recording = a_walk();
    let (walker, expected) = walk_at(&recording, steady(TICK_RATE));
    assert_eq!(expected.states.len(), 600);

    // The recording does what it is meant to: the player went somewhere,
    // left the ground four times and came down, and turned.
    let player = walker.client.player();
    let [x, _, z] = player.position;
    assert!(x.hypot(z) > 10.0, "walked {} m", x.hypot(z));
    let heights = heights(&expected);
    let jumps = heights
        .windows(2)
        .filter(|pair| pair[0] < 0.05 && pair[1] >= 0.05)
        .count();
    assert_eq!(jumps, 4, "jumps");
    assert!(player.grounded && player.position[1].abs() < 0.01);
    assert!((walker.yaw - START_YAW).abs() > 0.5);
    // What the player's own side worked out is what the server did, a
    // fixed few ticks later: nothing was ever put right.
    let half = expected.states[0].len() / 2 - 1;
    let server = |tick: usize| &expected.states[tick][..half];
    let own = |tick: usize| &expected.states[tick][half..2 * half];
    let late = (1..8)
        .find(|late| server(100) == own(100 - late))
        .expect("the server runs what the player sent");
    for tick in 10..600 {
        assert_eq!(server(tick), own(tick - late), "tick {tick}, {late} late");
    }

    for (pace, frame_times) in paces() {
        let (_, found) = walk_at(&recording, frame_times);
        assert_eq!(found.ticks().1, 0, "{pace}: no tick is skipped");
        if let Some(difference) = first_difference(&expected.states, &found.states) {
            panic!("At {pace}, {difference}");
        }
    }
}

#[test]
fn a_jump_is_as_high_at_any_frame_rate() {
    let mut recording = recording(120);
    recording[10].push(Did::Press(key(Key::Space)));
    recording[40].push(Did::Release(key(Key::Space)));
    let top = |frame_times: Box<dyn Iterator<Item = Duration>>| {
        let (walker, played) = walk_at(&recording, frame_times);
        assert!(walker.client.player().grounded, "landed again");
        heights(&played).into_iter().fold(0.0_f32, f32::max)
    };
    let expected = top(Box::new(steady(TICK_RATE)));
    // As high as the movement settings say, to within a tick's rise.
    let asked = walker_jump_height();
    assert!(
        (expected - asked).abs() < 0.1,
        "{expected} m, not {asked} m"
    );
    for (pace, frame_times) in paces() {
        let found = top(frame_times);
        assert_eq!(found.to_bits(), expected.to_bits(), "{pace}: {found} m");
    }
}

fn walker_jump_height() -> f32 {
    World::flat(TICK_RATE).rules.jump_height
}

#[test]
fn the_same_mouse_movement_turns_the_view_by_the_same_angle_at_any_frame_rate() {
    // 1000 counts to the right and 250 up, in sweeps of different speeds
    // and one flick, with ticks in between in which the mouse is still.
    let mut recording = recording(200);
    for done in recording.iter_mut().skip(10).take(50) {
        done.push(Did::Mouse(8.0, 2.0));
    }
    for done in recording.iter_mut().skip(80).step_by(4).take(25) {
        done.push(Did::Mouse(12.0, 3.0));
    }
    recording[190].push(Did::Mouse(300.0, 75.0));
    let turned = (
        (START_YAW - 1000.0 * look::MOUSE_TURN).rem_euclid(TAU),
        250.0 * look::MOUSE_TURN,
    );

    let (_, expected) = walk_at(&recording, steady(TICK_RATE));
    for (pace, frame_times) in paces() {
        let (walker, found) = walk_at(&recording, frame_times);
        assert!(
            (walker.yaw - turned.0).abs() < 1e-4 && (walker.pitch - turned.1).abs() < 1e-4,
            "{pace}: turned to {} and {}, not {turned:?}",
            walker.yaw,
            walker.pitch
        );
        // The picture ends where the game's rules have the view.
        assert_eq!(walker.drawn, (walker.yaw, walker.pitch), "{pace}");
        if let Some(difference) = first_difference(&expected.states, &found.states) {
            panic!("At {pace}, {difference}");
        }
    }
}
