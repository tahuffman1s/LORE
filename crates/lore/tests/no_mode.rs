//! A game whose script never calls `game.mode` has no match rules at all
//! (#259): no warm-up, no rounds, no teams and no score. A player comes
//! into the world once, when they join, and nothing the script did not ask
//! for moves them, kills them, scores them or starts them again.
//!
//! The server and its players are in this one program and hand each other
//! their packets directly, and time is counted in ticks, so every run plays
//! the same.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore::input::{ActionMap, Input, Key};
use lore::map::Map;
use lore::online::{HostedGame, JoinedGame, LocalGame};
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

/// How far apart two places are over the ground. Height is left out: being
/// put back at a spawn point is a move across the floor, and the flat floor
/// has a fault of its own that drops a walker a few centimetres into it for
/// a tick now and then (#261).
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// Where the server has its only player.
fn on_server(game: &LocalGame) -> [f32; 3] {
    let (_, player) = game.hosted().players().next().unwrap();
    player.position
}

/// Plays `ticks` ticks alone, walking forward and back by turns so the
/// player is on the move through every second of it, and gives the furthest
/// the server moved the player in any one tick.
fn walk_about(game: &mut LocalGame, input: &mut Input, ticks: u32) -> f32 {
    let mut furthest: f32 = 0.0;
    let mut before = on_server(game);
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
        let now = on_server(game);
        furthest = furthest.max(distance(before, now));
        before = now;
    }
    furthest
}

#[test]
fn a_player_alone_in_a_game_with_no_mode_is_spawned_once_and_left_alone() {
    let actions = ActionMap::default_fps();
    let mut game = LocalGame::start(game(TELLS_ALL), None, &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);

    // Settled at a spawn point.
    for _ in 0..30 {
        game.tick(input.tick(), STEP);
    }
    let start = on_server(&game);
    assert_eq!(printed(), ["join Player 1", "spawn Player 1"]);

    // What the issue saw: forward for five seconds, then ten ticks more.
    input.press(Key::W);
    for _ in 0..5 * SECOND - 30 {
        game.tick(input.tick(), STEP);
    }
    let at_five_seconds = on_server(&game);
    assert!(
        distance(start, at_five_seconds) > 10.0,
        "walked from {start:?} to {at_five_seconds:?}"
    );
    for _ in 0..10 {
        game.tick(input.tick(), STEP);
    }
    let just_after = on_server(&game);
    assert!(
        distance(at_five_seconds, just_after) < 10.0 * A_STEP,
        "put somewhere else: {at_five_seconds:?} then {just_after:?}"
    );
    assert!(distance(start, just_after) > distance(start, at_five_seconds));
    input.release(Key::W);

    // Eleven minutes: past the warm-up, the ten-minute round and the
    // intermission a deathmatch would have had.
    let furthest = walk_about(&mut game, &mut input, 11 * MINUTE);
    assert!(furthest > 0.01, "the player walked: {furthest}");
    assert!(
        furthest < A_STEP,
        "the player was moved {furthest} m in one tick"
    );

    // Standing still again, the player's own picture of where they are
    // agrees with the server's: nothing was put right on the way.
    input.release(Key::W);
    input.release(Key::S);
    for _ in 0..SECOND {
        game.tick(input.tick(), STEP);
    }
    let here = game.joined().player().position;
    assert!(
        distance(here, on_server(&game)) < 0.001,
        "player {here:?}, server {:?}",
        on_server(&game)
    );

    // Nothing else happened to them, and nothing was said to the script.
    assert_eq!(printed(), Vec::<String>::new());
    let host = game.hosted().game().host();
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
    let actions = ActionMap::default_fps();
    let mut hosted = HostedGame::in_program(game(TELLS_ALL), TICK_RATE);
    let nothing = Input::new(actions.clone());
    let mut ann = JoinedGame::in_program(&mut hosted, &actions).unwrap();
    hosted.tick(nothing.frame(), STEP);
    let mut ann_input = Input::new(actions.clone());
    let mut play = |hosted: &mut HostedGame, players: &mut [&mut JoinedGame], ticks: u32| {
        for _ in 0..ticks {
            for joined in players.iter_mut() {
                joined.tick(ann_input.tick(), STEP);
            }
            hosted.tick(nothing.frame(), STEP);
        }
    };
    play(&mut hosted, &mut [&mut ann], MINUTE);
    let ann_id = ann.id().unwrap();
    let where_is = |hosted: &HostedGame, id| {
        let (_, player) = hosted.players().find(|(other, _)| *other == id).unwrap();
        player.position
    };
    let before = where_is(&hosted, ann_id);
    assert_eq!(printed(), ["join Player 1", "spawn Player 1"]);

    // A second player arrives a minute in: only they are spawned.
    let mut bo = JoinedGame::in_program(&mut hosted, &actions).unwrap();
    hosted.tick(nothing.frame(), STEP);
    play(&mut hosted, &mut [&mut ann, &mut bo], 3 * MINUTE);

    assert_eq!(printed(), ["join Player 2", "spawn Player 2"]);
    assert!(distance(before, where_is(&hosted, ann_id)) < 0.001);
    let mode = hosted.game().host().mode();
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
    let mut game = LocalGame::start(game(WITH_A_MODE), None, &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);
    for _ in 0..30 {
        game.tick(input.tick(), STEP);
    }
    assert_eq!(printed(), ["spawn Player 1"]);
    assert_eq!(game.hosted().game().host().mode().phase(), Phase::Warmup);

    // The round starts five seconds in and everyone starts it from a spawn
    // point, as before.
    let furthest = walk_about(&mut game, &mut input, 6 * SECOND);
    assert!(furthest > 5.0, "put back at a spawn point: {furthest}");
    assert_eq!(printed(), ["round", "spawn Player 1"]);
    let mode = game.hosted().game().host().mode();
    assert_eq!(mode.phase(), Phase::Playing);
    assert!(mode.time_left().is_some());
}
