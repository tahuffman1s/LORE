//! A game's world and the player in it: walking, the level's walls,
//! where the player starts and which way they face.
//!
//! Time is counted in ticks, so every run goes the same way.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use lore::input::{ActionMap, Input, Key};
use lore::world::{Playing, World};
use lore::{Game, ScriptGame, scaffold};

const TICK_RATE: u32 = 60;
const STEP: f32 = 1.0 / 60.0;

/// A new game in a folder of its own.
fn new_game(test: &str) -> ScriptGame {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("world-{test}"));
    let _ = std::fs::remove_dir_all(&folder);
    scaffold::new_game(&folder).unwrap();
    ScriptGame::open(&folder, &ActionMap::standard()).unwrap()
}

/// Runs `ticks` ticks of a world, the player doing what was last asked of
/// them.
fn run(world: &mut World, ticks: u32) {
    let nothing = Input::new(ActionMap::standard());
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

/// Where the player's feet are.
fn feet(world: &World) -> [f32; 3] {
    world.player().position
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

#[test]
fn a_player_walks_in_the_tick_the_key_goes_down() {
    let actions = ActionMap::standard();
    let mut game = Playing::start(new_game("walk"), None, &actions, TICK_RATE);
    let mut input = Input::new(actions);

    // Settle onto the floor, then hold W.
    play(&mut game, &mut input, 30);
    let before = feet(game.world());
    input.press(Key::W);
    play(&mut game, &mut input, 1);
    assert!(
        feet(game.world())[2] < before[2],
        "the player moves in the tick the key goes down"
    );

    play(&mut game, &mut input, 60);
    input.release(Key::W);
    play(&mut game, &mut input, 60);

    let here = feet(game.world());
    assert!(here[2] < -2.0, "walked forward, along -Z: {here:?}");
    assert!(here[1].abs() < 0.01, "on the floor: {here:?}");
    // The picture is drawn from the player's eyes, right over their feet.
    let eye = game.eye_position();
    assert_eq!([eye[0], eye[2]], [here[0], here[2]]);
    assert!(eye[1] > here[1] + 1.0, "{eye:?}");
}

#[test]
fn a_dummy_stands_where_it_is_put_while_the_player_walks() {
    let mut world = World::new(new_game("dummy"), TICK_RATE);
    let started = feet(&world);
    let stood = [started[0] + 5.0, started[1], started[2]];
    let dummy = world.add_dummy(stood);

    // The player walks off to their right; the dummy does nothing.
    let mut asked = world.asked();
    asked.movement.right = 1.0;
    world.ask(asked);
    run(&mut world, 90);
    assert!(distance(started, feet(&world)) > 1.0);
    assert_eq!(world.dummy(dummy), Some(stood));

    world.remove_dummy(dummy);
    run(&mut world, 10);
    assert_eq!(world.dummy(dummy), None);
    let host = world.game().host();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

/// The level `lore new` writes: a walled garden, 28 m across, with one
/// place to start, 10 m south of the middle, facing the tower to the north.
fn starter_level() -> lore::assets::Scene {
    lore::level::from_bytes("level.glb", scaffold::STARTER_LEVEL).unwrap()
}

#[test]
fn a_game_with_a_level_starts_at_its_spawn_point_and_its_walls_are_solid() {
    let actions = ActionMap::standard();
    let level = starter_level();
    let mut game = Playing::start(new_game("level"), Some(&level), &actions, TICK_RATE);
    let mut input = Input::new(actions);

    // Settled at the one start, on the path, looking north at the tower.
    play(&mut game, &mut input, 30);
    let start = feet(game.world());
    assert!(start[1].abs() < 0.02, "on the path: {start:?}");
    assert!(
        start[0].abs() < 0.01 && (start[2] - 10.0).abs() < 0.01,
        "{start:?}"
    );
    assert_eq!(game.yaw(), 0.0);

    // Walking forward for a second goes a few metres up the path, and
    // stays on it.
    input.press(Key::W);
    play(&mut game, &mut input, 60);
    input.release(Key::W);
    play(&mut game, &mut input, 30);
    let ahead = feet(game.world());
    assert!(ahead[2] < start[2] - 3.0, "walked north: {ahead:?}");
    assert!(ahead[1].abs() < 0.3, "{ahead:?}");

    // Backing away, the cliff at the south edge stops the player a body's
    // width short of 14 m. Five seconds of walking would be 20 m.
    input.press(Key::S);
    play(&mut game, &mut input, 300);
    input.release(Key::S);
    play(&mut game, &mut input, 60);
    let here = feet(game.world());
    assert!(
        here[2] > 13.3 && here[2] < 14.0,
        "stopped by the cliff: {here:?}"
    );
    assert!((here[0] - start[0]).abs() < 0.01, "{here:?}");
    assert!(here[1].abs() < 0.3, "{here:?}");
}

#[test]
fn a_player_who_dies_is_put_back_at_the_start() {
    let level = starter_level();
    let mut world = World::new(new_game("apart"), TICK_RATE).with_level(&level);
    run(&mut world, 30);
    let first = feet(&world);
    assert!((first[2] - 10.0).abs() < 0.01, "{first:?}");

    for _ in 0..5 {
        world.game_mut().host_mut().player_died();
        run(&mut world, 30);
        let back = feet(&world);
        assert!(distance(first, back) < 0.01, "{first:?} and {back:?}");
    }
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
fn the_player_starts_at_the_maps_spawn_point_and_a_saved_map_is_picked_up() {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("world-map");
    let _ = std::fs::remove_dir_all(&folder);
    scaffold::new_game(&folder).unwrap();
    let map_file = folder.join(lore::map::MAP_FILE);
    let first = [10.0, 0.0, -6.0];
    std::fs::write(&map_file, map_with_spawn(first)).unwrap();

    let game = ScriptGame::open(&folder, &ActionMap::standard()).unwrap();
    assert_eq!(game.map().spawns().count(), 1);
    let mut world = World::new(game, TICK_RATE);

    // The player is put at the spawn point.
    run(&mut world, 30);
    assert!(distance(feet(&world), first) < 0.01);

    // The map is saved with the spawn point somewhere else while the game
    // runs. The game notices, and the player is next put there.
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
    // Until they die, the player stays where they are.
    run(&mut world, 30);
    assert!(distance(feet(&world), first) < 0.01);
    world.game_mut().host_mut().player_died();
    run(&mut world, 30);
    assert!(distance(feet(&world), second) < 0.01);

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
    let actions = ActionMap::standard();
    let game = ScriptGame::open(&folder, &actions).unwrap();
    assert_eq!(game.map().spawns().count(), 1);

    // Facing that way from the first frame, before any tick.
    let mut game = Playing::start(game, None, &actions, TICK_RATE);
    let west = std::f32::consts::FRAC_PI_2;
    assert!((game.yaw() - west).abs() < 0.001, "facing {}", game.yaw());

    let mut still = Input::new(actions);
    play(&mut game, &mut still, 30);
    let here = feet(game.world());
    // Started with their feet on the floor, the player is lifted the
    // centimetre they keep clear of it and moved no other way.
    assert!(distance(here, [at[0], 0.01, at[2]]) < 0.0001, "{here:?}");
    assert!((game.yaw() - west).abs() < 0.001, "facing {}", game.yaw());
}
