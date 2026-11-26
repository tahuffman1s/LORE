//! A player walking, jumping and looking around is the same game at any
//! frame rate: a recording of what they did is played with no window
//! ([`playback`](crate::playback)) and where they are after every tick
//! must be the same to the last bit.
//!
//! The recording is played through [`Playing`] itself, the game `lore run`
//! plays: its controls, its world and its view.

use std::f32::consts::TAU;
use std::time::Duration;

use lore_input::{ActionMap, GamepadButton, InputFrame, Key, Stick, StickAxis};

use super::{Ground, Playing};
use crate::game::{Frame, Game};
use crate::look;
use crate::playback::{
    Did, Played, Recording, TICK_RATE, first_difference, paces, play, recording, steady,
};
use crate::script_game::ScriptGame;

/// The yaw every walk starts with.
const START_YAW: f32 = 1.0;

/// A game played alone, and the yaw and pitch its newest frame was drawn
/// with.
struct Walker {
    game: Playing,
    drawn: (f32, f32),
}

impl Walker {
    /// A game with no rules on the flat floor, the player facing
    /// [`START_YAW`].
    fn new() -> Self {
        Self::playing("")
    }

    /// A game with these rules on the flat floor.
    fn playing(rules: &str) -> Self {
        let actions = ActionMap::standard();
        let script = ScriptGame::from_source(rules, &actions);
        let mut game = Playing::start(script, None, &actions, TICK_RATE);
        game.yaw = START_YAW;
        Self {
            game,
            drawn: (START_YAW, 0.0),
        }
    }

    fn player(&self) -> &lore_physics::Player {
        self.game.world.player()
    }
}

impl Game for Walker {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        self.game.tick(input, step);
    }

    fn frame(&mut self, frame: &mut Frame) {
        self.game.frame(frame);
        let view = frame.view().unwrap();
        self.drawn = (view.yaw, view.pitch);
    }
}

/// Everything that is the game's state, as bits: the player as the world
/// has them, and the view.
fn state(walker: &Walker) -> Vec<u32> {
    let (game, player) = (&walker.game, walker.player());
    let mut state: Vec<u32> = [
        player.position,
        player.velocity,
        player.ground_normal,
        [player.eye_height, 0.0, 0.0],
    ]
    .iter()
    .flatten()
    .map(|part| part.to_bits())
    .collect();
    state.extend(
        [
            player.grounded,
            player.crouched,
            player.sprinting,
            player.jump_held,
        ]
        .map(u32::from),
    );
    state.extend(game.eye.map(f32::to_bits));
    state.extend([game.yaw.to_bits(), game.pitch.to_bits()]);
    state
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
    let played = play(&mut walker, recording, frame_times, state);
    (walker, played)
}

/// How high the feet were after each tick of `played`.
fn heights(played: &Played<Vec<u32>>) -> Vec<f32> {
    // The player's place comes first, and height is its second part.
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
    let player = walker.player();
    let [x, _, z] = player.position;
    assert!(x.hypot(z) > 10.0, "walked {} m", x.hypot(z));
    let heights = heights(&expected);
    let jumps = heights
        .windows(2)
        .filter(|pair| pair[0] < 0.05 && pair[1] >= 0.05)
        .count();
    assert_eq!(jumps, 4, "jumps");
    assert!(player.grounded && player.position[1].abs() < 0.01);
    assert!((walker.game.yaw - START_YAW).abs() > 0.5);
    // A key moves the player in the very tick that sees it: W goes down
    // before tick 30 and the player is under way when it is over.
    let speed = |tick: usize| {
        let velocity = |axis: usize| f32::from_bits(expected.states[tick][3 + axis]);
        velocity(0).hypot(velocity(2))
    };
    assert_eq!(speed(29), 0.0);
    assert!(speed(30) > 0.0, "{}", speed(30));

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
        assert!(walker.player().grounded, "landed again");
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
    Ground::flat(TICK_RATE).rules.jump_height
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
            (walker.game.yaw - turned.0).abs() < 1e-4
                && (walker.game.pitch - turned.1).abs() < 1e-4,
            "{pace}: turned to {} and {}, not {turned:?}",
            walker.game.yaw,
            walker.game.pitch
        );
        // The picture ends where the game's rules have the view.
        assert_eq!(walker.drawn, (walker.game.yaw, walker.game.pitch), "{pace}");
        if let Some(difference) = first_difference(&expected.states, &found.states) {
            panic!("At {pace}, {difference}");
        }
    }
}

/// The world's clock after each tick, as bits: the hour, the day, and what
/// the rules did (each hour told prints a line).
fn clock_state(walker: &Walker) -> Vec<u32> {
    let world = &walker.game.world;
    let hour = world.time_of_day().unwrap_or(-1.0).to_bits();
    let done = world.game().host().rules_fingerprint();
    // Split in two, as the states are kept as u32s.
    #[allow(clippy::cast_possible_truncation)]
    let parts = [
        hour as u32,
        (hour >> 32) as u32,
        world.day().unwrap_or(0),
        done as u32,
        (done >> 32) as u32,
    ];
    parts.to_vec()
}

#[test]
fn the_hour_is_the_same_from_the_same_input_at_any_frame_rate() {
    // A day of a minute: an hour every 2.5 seconds, so ten seconds pass
    // midnight and four hours.
    const RULES: &str = r#"
        game.clock.dayLength = 1
        game.clock.time = "22:00"
        game.clock.onHour(function(hour) print("hour", hour, game.clock.day) end)
    "#;
    let recording = a_walk();
    let mut walker = Walker::playing(RULES);
    let expected = play(&mut walker, &recording, steady(TICK_RATE), clock_state);
    assert_eq!(walker.game.world.day(), Some(2));
    let hour = walker.game.world.time_of_day().unwrap_or_default();
    assert!((hour - 2.0).abs() < 0.01, "{hour}");
    for (pace, frame_times) in paces() {
        let mut walker = Walker::playing(RULES);
        let found = play(&mut walker, &recording, frame_times, clock_state);
        if let Some(difference) = first_difference(&expected.states, &found.states) {
            panic!("At {pace}, {difference}");
        }
    }
}
