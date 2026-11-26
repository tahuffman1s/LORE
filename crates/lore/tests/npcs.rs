//! NPCs in a running world: one call puts a person in it, who stands on the
//! floor, is in the player's way, can be shot, used and found by name, and
//! goes the same way on every run.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore::input::{ActionMap, Input};
use lore::map::Map;
use lore::world::{PlayerInput, World};
use lore::{Game, ScriptGame};

const TICK_RATE: u32 = 60;
const STEP: f32 = 1.0 / 60.0;

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

/// One place to start in the middle of the flat floor, facing -Z, a gate
/// ten metres ahead of it, and a smith placed as an NPC.
const MAP: &str = "version = 1\n\n\
    [[entity]]\nid = \"south\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n\n\
    [[entity]]\nid = \"gate\"\ntype = \"place\"\nname = \"Gate\"\nposition = [0.0, 0.0, -10.0]\nfacing = 180.0\n\n\
    [[entity]]\nid = \"smith\"\ntype = \"npc\"\nname = \"Smith\"\nposition = [6.0, 0.0, -6.0]\nfacing = 90.0\n";

/// The world of a game with `script` as its rules, on the flat floor.
fn world(script: &str) -> World {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    let map = Map::parse("map.toml", MAP).unwrap();
    // The map is there before the script's first line, as in a real game.
    let game = ScriptGame::from_pack_parts(
        [("main.luau".to_owned(), script.to_owned())],
        Vec::new(),
        map,
        &ActionMap::standard(),
        lore::settings::Settings::default(),
    );
    assert!(
        game.host().last_error().is_none(),
        "{:?}",
        game.host().last_error()
    );
    World::new(game, TICK_RATE)
}

/// Runs `ticks` ticks, the player doing what was last asked of them.
fn play(world: &mut World, ticks: u32) {
    let nothing = Input::new(ActionMap::standard());
    for _ in 0..ticks {
        world.tick(nothing.frame(), STEP);
    }
    assert!(
        world.game().host().last_error().is_none(),
        "{:?}",
        world.game().host().last_error()
    );
}

/// Changes what the player is asking for.
fn ask(world: &mut World, change: impl FnOnce(&mut PlayerInput)) {
    let mut asked = world.asked();
    change(&mut asked);
    world.ask(asked);
}

#[test]
fn an_npc_made_in_one_line_stands_on_the_floor_at_its_place() {
    let mut world = world(
        r#"local guard = game.npc { name = "Guard", at = "Gate" }
        print(guard.name, guard.health, guard.position.z)"#,
    );
    assert_eq!(printed(), ["Guard 100 -10"]);
    // The guard and the smith placed in the map, in the order they came.
    assert_eq!(world.npcs().len(), 2);
    play(&mut world, 30);
    let guard = world.npcs()[1];
    assert!(guard.body.grounded);
    assert!((guard.body.position[1]).abs() < 0.05, "{:?}", guard.body);
    // The gate faces back the way the player came, and so does the guard.
    assert!((guard.facing() - std::f32::consts::PI).abs() < 1e-4);
}

#[test]
fn an_npc_dropped_in_the_air_falls_to_the_floor() {
    let mut world = world(r#"game.npc { name = "Guard", at = vector.create(0, 3, -5) }"#);
    play(&mut world, 120);
    let guard = world.npcs()[1];
    assert!(guard.body.grounded, "{:?}", guard.body);
    assert!(guard.body.position[1].abs() < 0.05, "{:?}", guard.body);
}

#[test]
fn the_same_input_gives_the_same_npcs_tick_for_tick() {
    let script = r#"
        game.npc { name = "Guard", at = vector.create(0, 2, -3) }
        game.npc { name = "Cook", at = vector.create(0.2, 4, -3) }
        game.onSpawn(function() game.player:giveWeapon("rifle") end)"#;
    let run = || {
        let mut world = world(script);
        let mut seen = Vec::new();
        for tick in 0..240 {
            if tick == 60 {
                ask(&mut world, |asked| {
                    asked.fire = true;
                    asked.movement.forward = 1.0;
                });
            }
            play(&mut world, 1);
            seen.push(world.npcs().to_vec());
        }
        seen
    };
    assert_eq!(run(), run());
}

#[test]
fn the_player_walks_into_an_npc_and_stops() {
    let mut world = world(r#"game.npc { name = "Guard", at = vector.create(0, 0, -3) }"#);
    play(&mut world, 20);
    ask(&mut world, |asked| asked.movement.forward = 1.0);
    play(&mut world, 120);
    let z = world.player().position[2];
    // Feet a body's width short of the guard's.
    assert!(z > -3.0 + 0.6 && z < -3.0 + 0.8, "{z}");
}

#[test]
fn a_shot_npc_is_hurt_dies_falls_and_is_shot_no_more() {
    let mut world = world(
        r#"local guard = game.npc { name = "Guard", at = "Gate" }
        game.onSpawn(function() game.player:giveWeapon("rifle") end)
        game.onDamage(function(hurt)
            if hurt.victim == guard then print("hurt", hurt.how) end
        end)
        game.onDeath(function(victim, killer)
            print("died", victim.name, killer.name, guard.dead)
        end)"#,
    );
    play(&mut world, 30);
    ask(&mut world, |asked| {
        asked.aim = true;
        asked.fire = true;
    });
    play(&mut world, 120);
    let said = printed();
    assert!(said.iter().any(|line| line == "hurt rifle"), "{said:#?}");
    assert_eq!(
        said.iter()
            .filter(|line| line.starts_with("died"))
            .collect::<Vec<_>>(),
        ["died Guard Player true"]
    );
    let guard = world.npcs()[1];
    assert!(guard.down);
    // Fallen, it is walked over rather than into.
    ask(&mut world, |asked| {
        asked.fire = false;
        asked.movement.forward = 1.0;
    });
    play(&mut world, 200);
    assert!(world.player().position[2] < -11.0, "{:?}", world.player());
}

#[test]
fn the_player_uses_an_npc_like_a_thing_and_the_prompt_says_its_name() {
    let mut world = world(
        r#"local guard = game.npc { name = "Guard", at = vector.create(0, 0, -1.5) }
        guard:onUse(function(who) print("used", who.name, who == guard) end)"#,
    );
    play(&mut world, 20);
    assert_eq!(
        world.game().host().use_prompt(),
        Some(("Talk to Guard".to_owned(), 0.0))
    );
    ask(&mut world, |asked| asked.interact = true);
    play(&mut world, 2);
    assert_eq!(printed(), ["used Guard true"]);
}

#[test]
fn an_npc_placed_in_the_map_is_found_by_its_name_and_walks_into_zones() {
    let mut world = world(
        r#"local smith = game.npc("Smith")
        print(smith.name, smith.position.x)
        local ok, why = pcall(game.npc, "Smtih")
        print(why)"#,
    );
    let said = printed();
    assert_eq!(said[0], "Smith 6");
    assert!(
        said[1].contains("No NPC is called \"Smtih\". Did you mean \"Smith\"?"),
        "{said:#?}"
    );
    play(&mut world, 5);
    assert_eq!(world.npcs()[0].body.position[0], 6.0);
}

/// The map with the smith somewhere else, or (with `None`) not placed.
fn map_with_smith_at(at: Option<[f32; 3]>) -> Map {
    let text = MAP.split("[[entity]]\nid = \"smith\"").next().unwrap();
    let text = match at {
        Some([x, y, z]) => format!(
            "{text}[[entity]]\nid = \"smith\"\ntype = \"npc\"\nname = \"Smith\"\n\
             position = [{x:.1}, {y:.1}, {z:.1}]\nfacing = 0.0\n"
        ),
        None => text.to_owned(),
    };
    Map::parse("map.toml", &text).unwrap()
}

/// The world of a game opened with the map on disk, then given `map` as
/// the editor gives the one it holds when play is pressed.
fn world_with_editor_map(script: &str, map: Map) -> World {
    let _ = log::set_logger(&Capture);
    let game = ScriptGame::from_pack_parts(
        [("main.luau".to_owned(), script.to_owned())],
        Vec::new(),
        Map::parse("map.toml", MAP).unwrap(),
        &ActionMap::standard(),
        lore::settings::Settings::default(),
    )
    .with_map(map);
    World::new(game, TICK_RATE)
}

#[test]
fn play_from_the_editor_has_the_npcs_where_the_editor_moved_them() {
    let mut world = world_with_editor_map(
        r#"game.onTick(function() print(game.npc("Smith").position.x) end)"#,
        map_with_smith_at(Some([-4.0, 0.0, 2.0])),
    );
    let smith = world.npcs()[0];
    assert_eq!(smith.body.position, [-4.0, 0.0, 2.0]);
    assert_eq!(smith.facing(), 0.0);
    printed();
    play(&mut world, 1);
    assert_eq!(printed(), ["-4"]);
}

#[test]
fn play_from_the_editor_has_no_npc_the_editor_deleted() {
    let world = world_with_editor_map("", map_with_smith_at(None));
    assert!(world.npcs().is_empty(), "{:?}", world.npcs());
    let mut names = 0;
    world.game().host().each_npc(|_| names += 1);
    assert_eq!(names, 0);
}
