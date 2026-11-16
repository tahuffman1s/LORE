//! A game's world and the players in it: walking, the level's walls,
//! where players start and which way they face.
//!
//! Time is counted in ticks, so every run goes the same way.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use lore::input::{ActionMap, Input, Key};
use lore::world::{PlayerId, Playing, World};
use lore::{Game, ScriptGame, scaffold};

const TICK_RATE: u32 = 60;
const STEP: f32 = 1.0 / 60.0;

/// A new game in a folder of its own.
fn new_game(test: &str) -> ScriptGame {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("world-{test}"));
    let _ = std::fs::remove_dir_all(&folder);
    scaffold::new_game(&folder).unwrap();
    ScriptGame::open(&folder, &ActionMap::default_fps()).unwrap()
}

/// Runs `ticks` ticks of a world, each player doing what was last asked of
/// them.
fn run(world: &mut World, ticks: u32) {
    let nothing = Input::new(ActionMap::default_fps());
    for _ in 0..ticks {
        world.tick(nothing.frame(), STEP);
    }
}

/// Runs `ticks` ticks of a game played with `input`.
fn play(game: &mut Playing, input: &mut Input, ticks: u32) {
    for _ in 0..ticks {
        game.tick(input.tick(), STEP);
    }
}

/// Where the feet of the player numbered `id` are.
fn feet(world: &World, id: PlayerId) -> [f32; 3] {
    world.player(id).unwrap().position
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

#[test]
fn a_player_walks_in_the_tick_the_key_goes_down() {
    let actions = ActionMap::default_fps();
    let mut game = Playing::start(new_game("walk"), None, &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);
    assert_eq!(game.world().game().host().players().len(), 1);

    // Settle onto the floor, then hold W.
    play(&mut game, &mut input, 30);
    let before = feet(game.world(), game.id());
    input.press(Key::W);
    play(&mut game, &mut input, 1);
    assert!(
        feet(game.world(), game.id())[2] < before[2],
        "the player moves in the tick the key goes down"
    );

    play(&mut game, &mut input, 60);
    input.release(Key::W);
    play(&mut game, &mut input, 60);

    let here = feet(game.world(), game.id());
    assert!(here[2] < -2.0, "walked forward, along -Z: {here:?}");
    assert!(here[1].abs() < 0.01, "on the floor: {here:?}");
    assert_eq!(game.world().players().count(), 1);
    // The picture is drawn from the player's eyes, right over their feet.
    let eye = game.eye_position();
    assert_eq!([eye[0], eye[2]], [here[0], here[2]]);
    assert!(eye[1] > here[1] + 1.0, "{eye:?}");
}

#[test]
fn the_script_hears_players_come_and_go() {
    let mut world = World::new(new_game("two"), TICK_RATE);
    let first = world.add_player().unwrap();
    let second = world.add_player().unwrap();
    assert_ne!(first, second);
    assert_eq!(world.game().host().players().len(), 2);

    // The second walks off to their right while the first stands.
    let mut asked = world.asked(second).unwrap();
    asked.movement.right = 1.0;
    world.ask(second, asked);
    let (stood, started) = (feet(&world, first), feet(&world, second));
    run(&mut world, 90);
    assert!(distance(stood, feet(&world, first)) < 0.02);
    assert!(distance(started, feet(&world, second)) > 1.0);

    world.remove_player(second);
    run(&mut world, 10);
    assert_eq!(world.players().count(), 1);
    assert_eq!(world.player(second), None);
    assert_eq!(world.game().host().players().len(), 1);
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
    let mut game = Playing::start(new_game("level"), Some(&level), &actions, TICK_RATE).unwrap();
    let mut input = Input::new(actions);

    // Settled at one of the six spawn points, looking at the other end.
    play(&mut game, &mut input, 30);
    let start = feet(game.world(), game.id());
    assert!(start[1].abs() < 0.02, "on the floor: {start:?}");
    assert!((start[2].abs() - 14.0).abs() < 0.01, "{start:?}");
    assert!([0.0, 6.0].contains(&start[0].abs().round()), "{start:?}");
    let yaw = game.yaw();
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

    let here = feet(game.world(), game.id());
    let out = here[2].abs();
    assert!(out > 15.5 && out < 15.7, "stopped by the wall: {here:?}");
    assert!((here[0] - start[0]).abs() < 0.01, "{here:?}");
    assert!(here[1].abs() < 0.02, "on the floor: {here:?}");
}

#[test]
fn players_in_a_level_start_apart() {
    let level = starter_level();
    let mut world = World::new(new_game("apart"), TICK_RATE).with_level(&level);
    let first = world.add_player().unwrap();
    let second = world.add_player().unwrap();
    run(&mut world, 30);

    // The second player is put at the other end from the first.
    let (a, b) = (feet(&world, first), feet(&world, second));
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
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("world-map");
    let _ = std::fs::remove_dir_all(&folder);
    scaffold::new_game(&folder).unwrap();
    let map_file = folder.join(lore::map::MAP_FILE);
    let first = [10.0, 0.0, -6.0];
    std::fs::write(&map_file, map_with_spawn(first)).unwrap();

    let game = ScriptGame::open(&folder, &ActionMap::default_fps()).unwrap();
    assert_eq!(game.map().spawns().count(), 1);
    let mut world = World::new(game, TICK_RATE);

    // The player is put at the spawn point.
    let one = world.add_player().unwrap();
    run(&mut world, 30);
    assert!(distance(feet(&world, one), first) < 0.01);

    // The map is saved with the spawn point somewhere else while the game
    // runs. The game notices, and the next player comes in there.
    let second = [-20.0, 0.0, 8.0];
    std::fs::write(&map_file, map_with_spawn(second)).unwrap();
    let moved = |world: &World| {
        world
            .game()
            .map()
            .spawns()
            .any(|spawn| spawn.position == second)
    };
    for _ in 0..2000 {
        if moved(&world) {
            break;
        }
        run(&mut world, 1);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(moved(&world), "the saved map was never picked up");
    let two = world.add_player().unwrap();
    run(&mut world, 30);
    assert!(distance(feet(&world, two), second) < 0.01);
    // Whoever was already playing stays where they are.
    assert!(distance(feet(&world, one), first) < 0.01);

    // A save with a mistake in it changes nothing: the map that worked stays.
    std::fs::write(&map_file, "version = 1\n[[entity]]\nid = 3\n").unwrap();
    for _ in 0..2000 {
        if world.game().map_error().is_some() {
            break;
        }
        run(&mut world, 1);
        std::thread::sleep(Duration::from_millis(5));
    }
    let said = world.game().map_error().unwrap().to_string();
    assert!(said.contains("line 3: An id has to be text"), "{said}");
    assert!(moved(&world));
}

#[test]
fn a_player_faces_the_way_the_maps_spawn_point_does() {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("world-facing");
    let _ = std::fs::remove_dir_all(&folder);
    scaffold::new_game(&folder).unwrap();
    let at = [3.0, 0.0, -2.0];
    let map = format!("{}facing = 90.0\n", map_with_spawn(at));
    std::fs::write(folder.join(lore::map::MAP_FILE), map).unwrap();
    let actions = ActionMap::default_fps();
    let game = ScriptGame::open(&folder, &actions).unwrap();
    assert_eq!(game.map().spawns().count(), 1);

    // Facing that way from the first frame, before any tick.
    let mut game = Playing::start(game, None, &actions, TICK_RATE).unwrap();
    let west = std::f32::consts::FRAC_PI_2;
    assert!((game.yaw() - west).abs() < 0.001, "facing {}", game.yaw());

    let mut still = Input::new(actions);
    play(&mut game, &mut still, 30);
    let here = feet(game.world(), game.id());
    // Started with their feet on the floor, a player is lifted the
    // centimetre they keep clear of it and moved no other way.
    assert!(distance(here, [at[0], 0.01, at[2]]) < 0.0001, "{here:?}");
    assert!((game.yaw() - west).abs() < 0.001, "facing {}", game.yaw());
}
