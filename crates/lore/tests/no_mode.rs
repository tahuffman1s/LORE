//! A game whose script never calls `game.mode` has no match rules at all
//! (#259): no warm-up, no rounds, no teams and no score. A player comes
//! into the world once, when they join, and nothing the script did not ask
//! for moves them, kills them, scores them or starts them again.
//!
//! Time is counted in ticks, so every run plays the same.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore::input::{ActionMap, Input, Key};
use lore::map::Map;
use lore::world::{Playing, World};
use lore::{Game, ScriptGame};
use lore_mode::Phase;

const TICK_RATE: u32 = 60;
const STEP: f32 = 1.0 / 60.0;
const SECOND: u32 = TICK_RATE;
const MINUTE: u32 = 60 * SECOND;

/// Further than a player on foot gets in one tick: 20 cm a tick is 12 m/s,
/// and walking is 7.5 cm a tick. Being put back at a spawn point is metres.
const A_STEP: f32 = 0.2;

thread_local! {
    /// What scripts on this test's thread printed.
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Collects `print` output per thread, so tests can run side by side.
struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "script" {
            PRINTED.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn printed() -> Vec<String> {
    PRINTED.with_borrow_mut(std::mem::take)
}

/// Says everything the match does to a player, and names no mode.
const TELLS_ALL: &str = r#"
game.onPlayerJoin(function(player) print("join " .. player.name) end)
game.onPlayerLeave(function(player) print("leave " .. player.name) end)
game.onSpawn(function(player) print("spawn " .. player.name) end)
game.onKill(function(killer, victim) print("kill " .. victim.name) end)
"#;

/// The same game with the one line that asks for rounds.
const WITH_A_MODE: &str = r#"
game.mode {}
game.onSpawn(function(player) print("spawn " .. player.name) end)
game.onRoundStart(function() print("round") end)
"#;

/// Two places to start, 10 m apart on the flat floor.
const TWO_SPAWNS: &str = "version = 1\n\n\
    [[entity]]\nid = \"south\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n\n\
    [[entity]]\nid = \"north\"\ntype = \"spawn\"\nposition = [0.0, 0.0, -10.0]\nfacing = 180.0\n";

fn game(script: &str) -> ScriptGame {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let map = Map::parse("map.toml", TWO_SPAWNS).unwrap();
    let game = ScriptGame::from_source(script, &ActionMap::default_fps()).with_map(map);
    assert!(
        game.host().last_error().is_none(),
        "{:?}",
        game.host().last_error()
    );
    game
}

/// How far apart two places are.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}

/// The most a walker's feet may rise or sink on the flat floor: a tenth of
/// a millimetre. They used to sink 18 cm for a tick now and then (#261).
const LEVEL: f32 = 0.0001;

/// Where the world has its only player.
fn feet(game: &Playing) -> [f32; 3] {
    let [(_, player)] = game.world().players().collect::<Vec<_>>()[..] else {
        panic!("one player");
    };
    player.position
}

/// What [`walk_about`] saw.
struct Walk {
    /// The furthest the player moved in any one tick.
    furthest: f32,
    /// The most the player's feet rose or sank from where they started.
    off_level: f32,
}

/// Plays `ticks` ticks alone, walking forward and back by turns so the
/// player is on the move through every second of it.
fn walk_about(game: &mut Playing, input: &mut Input, ticks: u32) -> Walk {
    let mut before = feet(game);
    let height = before[1];
    let mut walk = Walk {
        furthest: 0.0,
        off_level: 0.0,
    };
    for tick in 0..ticks {
        // Seven seconds each way: forward right through the fifth second,
        // when the round used to start.
        let (hold, release) = if (tick / (7 * SECOND)).is_multiple_of(2) {
            (Key::W, Key::S)
        } else {
            (Key::S, Key::W)
        };
        input.release(release);
        input.press(hold);
        game.tick(input.tick(), STEP);
        let now = feet(game);
        walk.furthest = walk.furthest.max(distance(before, now));
        walk.off_level = walk.off_level.max((now[1] - height).abs());
        before = now;
    }
    walk
}

#[test]
fn a_player_alone_in_a_game_with_no_mode_is_spawned_once_and_left_alone() {
    let actions = ActionMap::default_fps();
    let mut game = Playing::start(game(TELLS_ALL), None, &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);

    // Settled at a spawn point.
    for _ in 0..30 {
        game.tick(input.tick(), STEP);
    }
    let start = feet(&game);
    assert_eq!(printed(), ["join Player 1", "spawn Player 1"]);

    // What the issue saw: forward for five seconds, then ten ticks more.
    input.press(Key::W);
    for _ in 0..5 * SECOND - 30 {
        game.tick(input.tick(), STEP);
    }
    let at_five_seconds = feet(&game);
    assert!(
        distance(start, at_five_seconds) > 10.0,
        "walked from {start:?} to {at_five_seconds:?}"
    );
    for _ in 0..10 {
        game.tick(input.tick(), STEP);
    }
    let just_after = feet(&game);
    assert!(
        distance(at_five_seconds, just_after) < 10.0 * A_STEP,
        "put somewhere else: {at_five_seconds:?} then {just_after:?}"
    );
    assert!(distance(start, just_after) > distance(start, at_five_seconds));
    input.release(Key::W);

    // Eleven minutes: past the warm-up, the ten-minute round and the
    // intermission a deathmatch would have had.
    let Walk {
        furthest,
        off_level,
    } = walk_about(&mut game, &mut input, 11 * MINUTE);
    assert!(furthest > 0.01, "the player walked: {furthest}");
    assert!(
        furthest < A_STEP,
        "the player was moved {furthest} m in one tick"
    );
    // The floor is flat, so all of it was at one height (#261).
    assert!(
        off_level < LEVEL,
        "the player's feet left the floor by {off_level} m"
    );

    // Standing still again, the eyes the picture is drawn from are right
    // over the feet: the view was never left behind.
    input.release(Key::W);
    input.release(Key::S);
    for _ in 0..SECOND {
        game.tick(input.tick(), STEP);
    }
    let (eye, here) = (game.eye_position(), feet(&game));
    assert_eq!([eye[0], eye[2]], [here[0], here[2]]);

    // Nothing else happened to them, and nothing was said to the script.
    assert_eq!(printed(), Vec::<String>::new());
    let host = game.world().game().host();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let mode = host.mode();
    assert_eq!(mode.phase(), Phase::Playing);
    assert_eq!(mode.time_left(), None);
    assert_eq!(mode.round(), 0);
    assert!(mode.teams().is_empty());
    let [player] = mode.players() else {
        panic!("one player: {:?}", mode.players());
    };
    assert!(player.alive);
    assert_eq!(player.team, None);
    assert_eq!((player.score, player.kills, player.deaths), (0, 0, 0));
}

#[test]
fn someone_joining_a_game_with_no_mode_moves_nobody_else() {
    let mut world = World::new(game(TELLS_ALL), TICK_RATE);
    let nothing = Input::new(ActionMap::default_fps());
    let play = |world: &mut World, ticks: u32| {
        for _ in 0..ticks {
            world.tick(nothing.frame(), STEP);
        }
    };
    let ann = world.add_player().unwrap();
    play(&mut world, MINUTE);
    let before = world.player(ann).unwrap().position;
    assert_eq!(printed(), ["join Player 1", "spawn Player 1"]);

    // A second player arrives a minute in: only they are spawned.
    world.add_player().unwrap();
    play(&mut world, 3 * MINUTE);

    assert_eq!(printed(), ["join Player 2", "spawn Player 2"]);
    assert!(distance(before, world.player(ann).unwrap().position) < 0.001);
    let mode = world.game().host().mode();
    assert_eq!(mode.phase(), Phase::Playing);
    for player in mode.players() {
        assert!(player.alive);
        assert_eq!(player.team, None);
        assert_eq!((player.score, player.kills, player.deaths), (0, 0, 0));
    }
}

#[test]
fn a_game_that_asks_for_a_mode_still_starts_its_round_after_the_warm_up() {
    let actions = ActionMap::default_fps();
    let mut game = Playing::start(game(WITH_A_MODE), None, &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);
    for _ in 0..30 {
        game.tick(input.tick(), STEP);
    }
    assert_eq!(printed(), ["spawn Player 1"]);
    assert_eq!(game.world().game().host().mode().phase(), Phase::Warmup);

    // The round starts five seconds in and everyone starts it from a spawn
    // point, as before.
    let furthest = walk_about(&mut game, &mut input, 6 * SECOND).furthest;
    assert!(furthest > 5.0, "put back at a spawn point: {furthest}");
    assert_eq!(printed(), ["round", "spawn Player 1"]);
    let mode = game.world().game().host().mode();
    assert_eq!(mode.phase(), Phase::Playing);
    assert!(mode.time_left().is_some());
}
