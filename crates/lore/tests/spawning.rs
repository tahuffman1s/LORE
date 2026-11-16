//! A player is put at a spawn point when they come into the world and
//! when they die, and at no other time: nothing is counted down, started
//! again or moved behind the script's back.
//!
//! Time is counted in ticks, so every run plays the same.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore::input::{ActionMap, Input, Key};
use lore::map::Map;
use lore::world::{Playing, World};
use lore::{Game, ScriptGame};

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

/// Says everything the engine does to a player, and hands each a pistol
/// as they join.
const TELLS_ALL: &str = r#"
game.onPlayerJoin(function(player) print("join " .. player.name) player:giveWeapon("pistol") end)
game.onPlayerLeave(function(player) print("leave " .. player.name) end)
game.onSpawn(function(player) print("spawn " .. player.name .. " " .. tostring(player.weapon)) end)
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
        // Seven seconds each way.
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
fn a_player_is_put_at_a_spawn_point_once_and_then_left_alone() {
    let actions = ActionMap::default_fps();
    let mut game = Playing::start(game(TELLS_ALL), None, &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);

    // Settled at a spawn point.
    for _ in 0..30 {
        game.tick(input.tick(), STEP);
    }
    let start = feet(&game);
    assert_eq!(printed(), ["join Player 1", "spawn Player 1 pistol"]);

    // Forward for five seconds, then ten ticks more: a game with rounds
    // used to start one here and put everyone back (#259).
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

    // Eleven minutes, which is longer than anything was ever counted down.
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
    assert_eq!(host.players().len(), 1);
    assert_eq!(game.world().players().count(), 1);
}

#[test]
fn someone_joining_moves_nobody_else() {
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
    assert_eq!(printed(), ["join Player 1", "spawn Player 1 pistol"]);

    // A second player arrives a minute in: only they are spawned.
    world.add_player().unwrap();
    play(&mut world, 3 * MINUTE);

    assert_eq!(printed(), ["join Player 2", "spawn Player 2 pistol"]);
    assert!(distance(before, world.player(ann).unwrap().position) < 0.001);
    assert_eq!(world.game().host().players().len(), 2);
}

#[test]
fn a_player_who_dies_is_put_back_at_a_spawn_point_with_what_they_held() {
    let mut world = World::new(game(TELLS_ALL), TICK_RATE);
    let nothing = Input::new(ActionMap::default_fps());
    let play = |world: &mut World, ticks: u32| {
        for _ in 0..ticks {
            world.tick(nothing.frame(), STEP);
        }
    };
    let ann = world.add_player().unwrap();
    play(&mut world, 30);
    let start = world.player(ann).unwrap().position;
    printed();

    // Three seconds' walk to the side: metres from both spawn points.
    let mut asked = world.asked(ann).unwrap();
    asked.movement.right = 1.0;
    world.ask(ann, asked);
    play(&mut world, 3 * SECOND);
    asked.movement.right = 0.0;
    world.ask(ann, asked);
    let spawns = [[0.0, 0.0, 0.0], [0.0, 0.0, -10.0]];
    let nearest = |here: [f32; 3]| {
        let to = spawns.map(|spawn| distance(spawn, here));
        to[0].min(to[1])
    };
    let away = world.player(ann).unwrap().position;
    assert!(nearest(away) > 3.0, "walked from {start:?} to {away:?}");
    assert_eq!(printed(), Vec::<String>::new());

    // Nothing has health yet, so dying is the engine being told so.
    let name = world.game().host().players()[0];
    world.game_mut().host_mut().player_died(name);
    assert_eq!(printed(), ["spawn Player 1 pistol"]);
    play(&mut world, 1);
    let back = world.player(ann).unwrap().position;
    assert!(nearest(back) < 0.01, "put at a spawn point: {back:?}");

    // Once: they are left alone again, still holding the pistol.
    play(&mut world, MINUTE);
    assert!(distance(back, world.player(ann).unwrap().position) < 0.01);
    assert_eq!(printed(), Vec::<String>::new());
    let host = world.game().host();
    assert_eq!(&*host.held_weapon(name).unwrap().name, "pistol");
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}
