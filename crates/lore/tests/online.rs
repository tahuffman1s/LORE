//! A game and the players in it, on the two sides of the netcode.
//!
//! The players are in the game's own program and are handed its packets
//! directly (`inside`, `join_inside`, `play`), so every run goes the same
//! way.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use lore::input::{ActionMap, Input, Key};
use lore::online::{HostedGame, JoinedGame, LocalGame};
use lore::{Game, ScriptGame, scaffold};

const TICK_RATE: u32 = 60;
const STEP: f32 = 1.0 / 60.0;

/// A new game in a folder of its own.
fn new_game(test: &str) -> ScriptGame {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("online-{test}"));
    let _ = std::fs::remove_dir_all(&folder);
    scaffold::new_game(&folder).unwrap();
    ScriptGame::open(&folder, &ActionMap::default_fps()).unwrap()
}

/// The game's side of a new game.
fn inside(test: &str) -> HostedGame {
    HostedGame::in_program(new_game(test), TICK_RATE)
}

/// A player joins, and the game runs on for a tick, which puts the newcomer
/// in the world.
fn join_inside(hosted: &mut HostedGame) -> JoinedGame {
    let joined = JoinedGame::in_program(hosted, &ActionMap::default_fps()).unwrap();
    hosted.tick(Input::new(ActionMap::default_fps()).frame(), STEP);
    joined
}

/// Runs `ticks` ticks of the game and each player, with each player's
/// own keys.
fn play(hosted: &mut HostedGame, players: &mut [(&mut JoinedGame, &mut Input)], ticks: u32) {
    let nothing = Input::new(ActionMap::default_fps());
    for _ in 0..ticks {
        for (joined, input) in players.iter_mut() {
            joined.tick(input.tick(), STEP);
        }
        hosted.tick(nothing.frame(), STEP);
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

#[test]
fn a_joined_player_walks_at_once_and_the_server_agrees() {
    let mut hosted = inside("walk");
    let mut joined = join_inside(&mut hosted);
    let mut input = Input::new(ActionMap::default_fps());
    assert_eq!(hosted.game().host().mode().players().len(), 1);

    // Settle onto the floor, then hold W.
    play(&mut hosted, &mut [(&mut joined, &mut input)], 30);
    let before = joined.player().position;
    input.press(Key::W);
    joined.tick(input.tick(), STEP);
    assert!(
        joined.player().position[2] < before[2],
        "the player's own side moves them in the tick the key goes down"
    );

    play(&mut hosted, &mut [(&mut joined, &mut input)], 60);
    input.release(Key::W);
    play(&mut hosted, &mut [(&mut joined, &mut input)], 60);

    let here = joined.player().position;
    assert!(here[2] < -2.0, "walked forward, along -Z: {here:?}");
    assert!(here[1].abs() < 0.01, "on the floor: {here:?}");
    let (_, on_server) = hosted.players().next().unwrap();
    assert!(
        distance(on_server.position, here) < 0.001,
        "server {:?}, player {here:?}",
        on_server.position
    );
    assert!(joined.closed().is_none());
}

#[test]
fn players_see_each_other_and_the_script_hears_them_come_and_go() {
    let mut hosted = inside("two");
    let mut first = join_inside(&mut hosted);
    let mut second = join_inside(&mut hosted);
    let (mut still, mut walking) = (
        Input::new(ActionMap::default_fps()),
        Input::new(ActionMap::default_fps()),
    );
    assert_ne!(first.id(), second.id());
    assert_eq!(hosted.game().host().mode().players().len(), 2);

    walking.press(Key::D);
    play(
        &mut hosted,
        &mut [(&mut first, &mut still), (&mut second, &mut walking)],
        90,
    );

    let seen: Vec<_> = first.others().collect();
    assert_eq!(seen.len(), 1);
    assert_eq!(Some(seen[0].id), second.id());
    assert!(
        distance(seen[0].position, second.player().position) < 1.0,
        "the first player sees the second near where they are: {:?} and {:?}",
        seen[0].position,
        second.player().position
    );
    assert!(seen[0].position[0] > 1.0, "{:?}", seen[0].position);

    drop(second);
    play(&mut hosted, &mut [(&mut first, &mut still)], 10);
    assert_eq!(hosted.players().count(), 1);
    assert_eq!(hosted.game().host().mode().players().len(), 1);
    assert_eq!(first.others().count(), 0);
}

#[test]
fn a_game_played_alone_goes_through_the_same_netcode() {
    let actions = ActionMap::default_fps();
    let mut game = LocalGame::start(new_game("alone"), None, &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);

    input.press(Key::W);
    for _ in 0..120 {
        game.tick(input.tick(), STEP);
    }

    let here = game.joined().player().position;
    assert!(here[2] < -3.0, "{here:?}");
    assert_eq!(game.hosted().players().count(), 1);
    assert_eq!(game.hosted().game().host().mode().players().len(), 1);
    assert!(game.joined().closed().is_none());
}

/// The level `lore new` writes: a 32 m floor with a wall around it at
/// 16 m, and three places to start at each end, 14 m from the middle.
fn starter_level() -> lore::assets::Scene {
    lore::level::from_bytes("level.glb", scaffold::STARTER_LEVEL).unwrap()
}

#[test]
fn a_game_with_a_level_starts_at_a_spawn_point_and_its_walls_are_solid() {
    let actions = ActionMap::default_fps();
    let level = starter_level();
    let mut game = LocalGame::start(new_game("level"), Some(&level), &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);
    let play = |game: &mut LocalGame, input: &mut Input, ticks: u32| {
        for _ in 0..ticks {
            game.tick(input.tick(), STEP);
        }
    };

    // Settled at one of the six spawn points, looking at the other end.
    play(&mut game, &mut input, 30);
    let start = game.joined().player().position;
    assert!(start[1].abs() < 0.02, "on the floor: {start:?}");
    assert!((start[2].abs() - 14.0).abs() < 0.01, "{start:?}");
    assert!([0.0, 6.0].contains(&start[0].abs().round()), "{start:?}");
    let yaw = game.joined().yaw();
    let faces_south = (yaw - std::f32::consts::PI).abs() < 0.001;
    assert!(
        if start[2] < 0.0 {
            faces_south
        } else {
            yaw == 0.0
        },
        "at {start:?} facing {yaw}"
    );

    // Backing away from the middle, the wall around the level is 2 m behind.
    // Two seconds of walking would be 9 m on a flat floor.
    input.press(Key::S);
    play(&mut game, &mut input, 120);
    input.release(Key::S);
    play(&mut game, &mut input, 60);

    let here = game.joined().player().position;
    let out = here[2].abs();
    assert!(out > 15.5 && out < 15.7, "stopped by the wall: {here:?}");
    assert!((here[0] - start[0]).abs() < 0.01, "{here:?}");
    assert!(here[1].abs() < 0.02, "on the floor: {here:?}");
    let (_, on_server) = game.hosted().players().next().unwrap();
    assert!(
        distance(on_server.position, here) < 0.001,
        "server {:?}, player {here:?}",
        on_server.position
    );
}

#[test]
fn players_in_a_level_start_apart() {
    let level = starter_level();
    let mut hosted = inside("apart").with_level(&level);
    let mut first = join_inside(&mut hosted);
    let mut second = join_inside(&mut hosted);
    let (mut still, mut also_still) = (
        Input::new(ActionMap::default_fps()),
        Input::new(ActionMap::default_fps()),
    );
    play(
        &mut hosted,
        &mut [(&mut first, &mut still), (&mut second, &mut also_still)],
        30,
    );

    // The second player is put at the other end from the first.
    let (a, b) = (first.player().position, second.player().position);
    assert!((a[2].abs() - 14.0).abs() < 0.01, "{a:?}");
    assert!((b[2] + a[2]).abs() < 0.01, "{a:?} and {b:?}");
}

/// A map file with one spawn point at `position`.
fn map_with_spawn(position: [f32; 3]) -> String {
    let [x, y, z] = position;
    format!(
        "version = 1\n\n[[entity]]\nid = \"only\"\ntype = \"spawn\"\n\
         position = [{x:?}, {y:?}, {z:?}]\n"
    )
}

#[test]
fn players_start_at_the_maps_spawn_point_and_a_saved_map_is_picked_up() {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("online-map");
    let _ = std::fs::remove_dir_all(&folder);
    scaffold::new_game(&folder).unwrap();
    let map_file = folder.join(lore::map::MAP_FILE);
    let first = [10.0, 0.0, -6.0];
    std::fs::write(&map_file, map_with_spawn(first)).unwrap();

    let game = ScriptGame::open(&folder, &ActionMap::default_fps()).unwrap();
    assert_eq!(game.map().spawns().count(), 1);
    let mut hosted = HostedGame::in_program(game, TICK_RATE);

    // The player is put at the spawn point, on both sides.
    let mut one = join_inside(&mut hosted);
    let mut still = Input::new(ActionMap::default_fps());
    play(&mut hosted, &mut [(&mut one, &mut still)], 30);
    let (_, on_server) = hosted.players().next().unwrap();
    assert!(distance(on_server.position, first) < 0.01, "{on_server:?}");
    assert!(
        distance(one.player().position, first) < 0.01,
        "{:?}",
        one.player().position
    );

    // The map is saved with the spawn point somewhere else while the game
    // runs. The game notices, and the next player comes in there.
    let second = [-20.0, 0.0, 8.0];
    std::fs::write(&map_file, map_with_spawn(second)).unwrap();
    let moved = |hosted: &HostedGame| {
        hosted
            .game()
            .map()
            .spawns()
            .any(|spawn| spawn.position == second)
    };
    for _ in 0..2000 {
        if moved(&hosted) {
            break;
        }
        play(&mut hosted, &mut [(&mut one, &mut still)], 1);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(moved(&hosted), "the saved map was never picked up");
    let mut two = join_inside(&mut hosted);
    let mut also_still = Input::new(ActionMap::default_fps());
    play(
        &mut hosted,
        &mut [(&mut one, &mut still), (&mut two, &mut also_still)],
        30,
    );
    let id = two.id().unwrap();
    let (_, on_server) = hosted.players().find(|(other, _)| *other == id).unwrap();
    assert!(distance(on_server.position, second) < 0.01, "{on_server:?}");
    // Whoever was already playing stays where they are.
    assert!(distance(one.player().position, first) < 0.01);

    // A save with a mistake in it changes nothing: the map that worked stays.
    std::fs::write(&map_file, "version = 1\n[[entity]]\nid = 3\n").unwrap();
    for _ in 0..2000 {
        if hosted.game().map_error().is_some() {
            break;
        }
        play(&mut hosted, &mut [], 1);
        std::thread::sleep(Duration::from_millis(5));
    }
    let said = hosted.game().map_error().unwrap().to_string();
    assert!(said.contains("line 3: An id has to be text"), "{said}");
    assert!(moved(&hosted));
}

#[test]
fn a_player_faces_the_way_the_maps_spawn_point_does() {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("online-facing");
    let _ = std::fs::remove_dir_all(&folder);
    scaffold::new_game(&folder).unwrap();
    let at = [3.0, 0.0, -2.0];
    let map = format!("{}facing = 90.0\n", map_with_spawn(at));
    std::fs::write(folder.join(lore::map::MAP_FILE), map).unwrap();
    let game = ScriptGame::open(&folder, &ActionMap::default_fps()).unwrap();
    assert_eq!(game.map().spawns().count(), 1);
    let mut hosted = HostedGame::in_program(game, TICK_RATE);

    let mut joined = join_inside(&mut hosted);
    let mut still = Input::new(ActionMap::default_fps());
    play(&mut hosted, &mut [(&mut joined, &mut still)], 30);
    let here = joined.player().position;
    let (_, on_server) = hosted.players().next().unwrap();
    // Started with their feet on the floor, a player is lifted the
    // centimetre they keep clear of it and moved no other way.
    assert!(distance(here, [at[0], 0.01, at[2]]) < 0.0001, "{here:?}");
    assert!(distance(here, on_server.position) < 0.001, "{here:?}");
    let yaw = joined.yaw();
    assert!(
        (yaw - std::f32::consts::FRAC_PI_2).abs() < 0.001,
        "facing {yaw}"
    );
}
