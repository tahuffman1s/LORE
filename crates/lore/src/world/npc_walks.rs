//! NPCs walking in a world: to places, through doors, past each other.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_input::{ActionMap, InputFrame};

use lore_assets::{Instance, Mesh, Scene};

use crate::ScriptGame;
use crate::game::Game;
use crate::scaffold;
use crate::world::World;

const STEP: f32 = 1.0 / 60.0;

thread_local! {
    /// What scripts on this test's thread printed, and what was warned of.
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static WARNED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
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
        } else if record.level() == log::Level::Warn {
            WARNED.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

thread_local! {
    /// Held for the rest of the test, so `lore check`'s pacing, which
    /// turns the log off, does not run meanwhile.
    static QUIET_HELD: RefCell<Option<std::sync::MutexGuard<'static, ()>>> =
        const { RefCell::new(None) };
}

/// Starts collecting what is printed on this thread, afresh.
fn capture() {
    QUIET_HELD.with_borrow_mut(|held| {
        if held.is_none() {
            *held = Some(
                crate::cli::LOG_QUIET
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }
    });
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    PRINTED.with_borrow_mut(Vec::clear);
    WARNED.with_borrow_mut(Vec::clear);
}

/// A world of the starter level with `script` and `map` (map.toml's
/// entities, without the version line).
fn starter(script: &str, map: &str) -> World {
    capture();
    let level = crate::level::from_bytes("level.glb", scaffold::STARTER_LEVEL).unwrap();
    let map = crate::map::Map::parse("map.toml", &format!("version = 1\n{map}")).unwrap();
    World::new(game_of(script, map), 60).with_level(&level)
}

/// A game of `script` with `map` there before its first line.
fn game_of(script: &str, map: crate::map::Map) -> ScriptGame {
    let game = ScriptGame::from_pack_parts(
        [("main.luau".to_owned(), script.to_owned())],
        Vec::new(),
        map,
        &ActionMap::standard(),
        crate::settings::Settings::default(),
    );
    assert_eq!(game.host().last_error(), None);
    game
}

fn run(world: &mut World, ticks: u32) {
    let still = InputFrame::default();
    for _ in 0..ticks {
        world.tick(&still, STEP);
    }
}

/// Where the NPC called `name` stands.
fn feet_of(world: &World, name: &str) -> [f32; 3] {
    let host = world.game.host();
    world
        .npcs()
        .iter()
        .find(|npc| host.character_name(npc.who).as_deref() == Some(name))
        .map(|npc| npc.body.position)
        .unwrap()
}

fn across(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

#[test]
fn an_npc_walks_to_the_far_side_and_arrives_without_a_jump() {
    let mut world = starter(
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(1.8, 0, 5.5) }
        guard:goTo(vector.create(13, 0, -13))
        guard:onArrive(function(npc, where)
            assert(npc == guard and where.x == 13)
            assert(guard.destination == nil)
            print("arrived")
        end)
        game.onTick(function()
            if game.time() < 0.05 then assert(guard.destination.z == -13) end
        end)
        "#,
        "",
    );
    let mut was = feet_of(&world, "Guard");
    let mut most: f32 = 0.0;
    let mut ticks = 0;
    while ticks < 60 * 40 {
        run(&mut world, 1);
        ticks += 1;
        let now = feet_of(&world, "Guard");
        most = most.max(across(was, now));
        was = now;
        if world.npcs()[0].mind.walk.goal().is_none() {
            break;
        }
    }
    assert_eq!(world.game.host().last_error(), None);
    let end = feet_of(&world, "Guard");
    assert!(
        across(end, [13.0, 0.0, -13.0]) < 0.6,
        "{end:?} after {ticks} ticks: {:?}",
        world.npcs()[0].mind.walk
    );
    // A walk, never a jump along the way: no more than a sprint's tick.
    assert!(most < 0.12, "{most}");
    // At an unhurried walk, some 26 m takes a while.
    assert!(ticks > 60 * 8, "{ticks}");
    assert!(world.npcs()[0].mind.walk.trouble().is_none());
}

/// A level of blocks, each from its west-north-bottom corner to its
/// east-south-top one.
fn blocks(boxes: &[([f32; 3], [f32; 3])]) -> Scene {
    let mut scene = Scene::default();
    scene.meshes.push(Mesh {
        name: None,
        parts: vec![crate::standin::block()],
    });
    for &(low, high) in boxes {
        let size: [f32; 3] = std::array::from_fn(|i| high[i] - low[i]);
        let foot = [(low[0] + high[0]) / 2.0, low[1], (low[2] + high[2]) / 2.0];
        scene.instances.push(Instance {
            mesh: 0,
            transform: crate::standin::at(foot, size),
            mirrored: false,
            name: None,
            extras: None,
        });
    }
    scene
}

/// A floor 20 m on a side with a wall across it along z = -2, and a
/// doorway `wide` metres wide in the wall from x = 0.
fn walled(wide: f32) -> Scene {
    blocks(&[
        ([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0]),
        ([-10.0, 0.0, -2.1], [0.0, 3.0, -1.9]),
        ([wide, 0.0, -2.1], [10.0, 3.0, -1.9]),
    ])
}

/// A door across the 1 m doorway of [`walled`], hinged at its west side.
const CELLAR_DOOR: &str = "[[entity]]\nid = \"door\"\ntype = \"prop\"\nname = \"Cellar door\"\n\
                           position = [0.0, 0.0, -2.0]\n[entity.properties]\nmodel = \"door\"\n\
                           opens = true\n";

/// A world of `level` with `script` and `map`.
fn world_of(level: &Scene, script: &str, map: &str) -> World {
    capture();
    let map = crate::map::Map::parse("map.toml", &format!("version = 1\n{map}")).unwrap();
    World::new(game_of(script, map), 60).with_level(level)
}

/// Runs `world` until nobody is walking anywhere, or `most` ticks: how
/// many it took.
fn until_still(world: &mut World, most: u32) -> u32 {
    for tick in 0..most {
        run(world, 1);
        if world
            .npcs()
            .iter()
            .all(|npc| npc.mind.walk.goal().is_none())
        {
            return tick + 1;
        }
    }
    most
}

/// What the script printed since the world was made, in order.
fn printed(_: &World) -> Vec<String> {
    PRINTED.with_borrow(Clone::clone)
}

/// What was warned of since the world was made, in order.
fn warned() -> Vec<String> {
    WARNED.with_borrow(Clone::clone)
}

#[test]
fn a_shut_door_on_the_way_is_opened_as_the_player_opens_it_and_walked_through() {
    let mut world = world_of(
        &walled(1.0),
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(0.5, 0, 3) }
        guard:goTo(vector.create(0.5, 0, -6))
        game.thing("Cellar door"):onUse(function(door, who)
            print(who.name .. " opened " .. door.name)
        end)
        guard:onArrive(function() print("through") end)
        "#,
        CELLAR_DOOR,
    );
    let took = until_still(&mut world, 60 * 20);
    assert_eq!(world.game.host().last_error(), None);
    assert_eq!(
        printed(&world),
        ["Guard opened Cellar door", "through"],
        "after {took} ticks"
    );
    assert!(world.things.door("door") == Some(true));
    let end = feet_of(&world, "Guard");
    assert!(across(end, [0.5, 0.0, -6.0]) < 0.6, "{end:?}");
}

#[test]
fn a_locked_door_is_said_to_be_why_it_cannot_get_there() {
    let mut world = world_of(
        &walled(1.0),
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(0.5, 0, 3) }
        game.thing("Cellar door").usable = false
        guard:goTo(vector.create(0.5, 0, -6))
        guard:onCannotReach(function(npc, why) print(why) end)
        guard:onArrive(function() print("through?") end)
        "#,
        CELLAR_DOOR,
    );
    until_still(&mut world, 60 * 5);
    assert_eq!(world.game.host().last_error(), None);
    let said = printed(&world);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with(
            "Guard cannot reach (0.5, 0.0, -6.0): the door \"Cellar door\" is locked (it has usable = false"
        ),
        "{said:?}"
    );
    assert!(said[0].contains("the nearest it can get is"), "{said:?}");
    assert_eq!(world.things.door("door"), Some(false));
    match world.npcs()[0].mind.walk.trouble() {
        Some(lore_npc::Trouble::NoWay(lore_nav::NoWay::Shut { gates, .. })) => {
            assert_eq!(gates, &[lore_nav::GateId(0)]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn two_npcs_swap_sides_through_a_doorway_without_standing_in_each_other() {
    let mut world = world_of(
        &walled(1.2),
        r#"
        local a = game.npc { name = "A", at = vector.create(0.6, 0, 3) }
        local b = game.npc { name = "B", at = vector.create(0.6, 0, -7) }
        a:goTo(vector.create(0.6, 0, -7))
        b:goTo(vector.create(0.6, 0, 3))
        for _, npc in { a, b } do
            npc:onArrive(function() print(npc.name .. " arrived") end)
            npc:onCannotReach(function(_, why) print(why) end)
        end
        "#,
        "",
    );
    let mut closest = f32::MAX;
    let mut took = 0;
    for tick in 0..60 * 30 {
        run(&mut world, 1);
        let [a, b] = [feet_of(&world, "A"), feet_of(&world, "B")];
        closest = closest.min(across(a, b));
        took = tick;
        if world
            .npcs()
            .iter()
            .all(|npc| npc.mind.walk.goal().is_none())
        {
            break;
        }
    }
    let mut said = printed(&world);
    said.sort();
    assert_eq!(said, ["A arrived", "B arrived"], "after {took} ticks");
    // Two bodies 0.35 m round never closer than touching.
    assert!(closest > 0.69, "{closest}");
}

#[test]
fn the_same_walks_twice_go_the_same_way_tick_for_tick() {
    let script = r#"
        for i = 1, 6 do
            local npc = game.npc { name = "Walker " .. i, at = vector.create(-6 + i * 2, 0, 8) }
            npc:goTo(vector.create(12 - i * 3, 0, -12), { run = i % 2 == 0 })
        end
        game.npc("Walker 3"):follow(game.player)
        "#;
    let mut one = starter(script, "");
    let mut two = starter(script, "");
    for tick in 0..60 * 12 {
        run(&mut one, 1);
        run(&mut two, 1);
        let here: Vec<_> = one
            .npcs()
            .iter()
            .map(|npc| (npc.body.position, npc.facing()))
            .collect();
        let there: Vec<_> = two
            .npcs()
            .iter()
            .map(|npc| (npc.body.position, npc.facing()))
            .collect();
        assert_eq!(here, there, "on tick {tick}");
    }
}

/// The player, walking: `forward` pushed, facing `yaw`.
fn player_walks(world: &mut World, forward: f32, yaw: f32) {
    let mut asked = world.asked();
    asked.movement.forward = forward;
    asked.movement.yaw = yaw;
    world.ask(asked);
}

#[test]
fn a_follower_keeps_near_the_player_along_a_walk_and_never_walks_into_them() {
    let mut world = starter(
        r#"
        local friend = game.npc { name = "Friend", at = vector.create(2, 0, 11) }
        friend:follow(game.player, { distance = 3 })
        friend:onCannotReach(function(_, why) print(why) end)
        "#,
        "",
    );
    // Ahead (-Z) for three seconds, to the right for two, then still.
    let walk = [
        (180, 1.0, 0.0),
        (120, 1.0, -std::f32::consts::FRAC_PI_2),
        (240, 0.0, 0.0),
    ];
    let mut nearest = f32::MAX;
    let mut furthest: f32 = 0.0;
    for (ticks, forward, yaw) in walk {
        player_walks(&mut world, forward, yaw);
        for _ in 0..ticks {
            run(&mut world, 1);
            let gap = across(feet_of(&world, "Friend"), world.player().position);
            nearest = nearest.min(gap);
            furthest = furthest.max(gap);
        }
    }
    assert_eq!(printed(&world), Vec::<String>::new());
    let gap = across(feet_of(&world, "Friend"), world.player().position);
    // Standing by, within its distance once the player stops.
    assert!(gap <= 3.05, "{gap}");
    assert!(nearest > 0.69, "{nearest}");
    // The player walks at 4.5 m/s; the follower runs to keep up.
    assert!(furthest < 9.0, "{furthest}");
    // And it faces the player as it stands.
    let friend = &world.npcs()[0];
    let to = world.player().position;
    let want = (-(to[0] - friend.body.position[0])).atan2(-(to[2] - friend.body.position[2]));
    let off = (friend.facing() - want + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    assert!(off.abs() < 0.1, "{off}");
}

#[test]
fn someone_in_the_way_for_good_is_given_up_on_with_where() {
    let mut world = world_of(
        &walled(1.0),
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(0.5, 0, 3) }
        guard:goTo(vector.create(0.5, 0, -6))
        guard:onCannotReach(function(_, why) print(why) end)
        "#,
        "",
    );
    // The player stands in the doorway, and does not move.
    world.player.player.position = [0.5, 0.0, -2.0];
    let took = until_still(&mut world, 60 * 30);
    let said = printed(&world);
    assert_eq!(said.len(), 1, "{said:?} after {took} ticks");
    assert!(
        said[0].starts_with("Guard cannot reach (0.5, 0.0, -6.0): the player is in the way at ("),
        "{said:?}"
    );
    // It tried for a while first, and never walked through them.
    assert!(took > 60 * 8, "{took}");
    assert!(across(feet_of(&world, "Guard"), [0.5, 0.0, -2.0]) > 0.69);
    assert!(matches!(
        world.npcs()[0].mind.walk.trouble(),
        Some(lore_npc::Trouble::Stuck {
            by: Some(lore_npc::Blocker::Player),
            ..
        })
    ));
}

#[test]
fn an_npc_with_nobody_listening_has_its_trouble_in_the_log() {
    let mut world = world_of(
        &walled(1.0),
        r#"game.npc { name = "Guard", at = vector.create(0.5, 0, 3) }:goTo(vector.create(0, 50, 0))"#,
        "",
    );
    until_still(&mut world, 60);
    let warned = warned();
    assert!(
        warned.iter().any(|line| line.starts_with(
            "Guard cannot reach (0.0, 50.0, 0.0): there is no floor a person can stand on"
        ) && line.contains("guard:onCannotReach")),
        "{warned:?}"
    );
}

#[test]
fn walking_npcs_are_heard_stepping_and_crouch_where_there_is_no_room_to_stand() {
    let mut world = starter(
        r#"game.npc { name = "Guard", at = vector.create(1.8, 0, 5.5) }:goTo(vector.create(13, 0, -13))"#,
        "",
    );
    let guard = world.npcs()[0].who;
    let mut steps = 0;
    let mut crouched = false;
    for _ in 0..60 * 30 {
        run(&mut world, 1);
        steps += (world.noises().iter())
            .filter(|noise| noise.by == Some(guard) && noise.kind.is_footfall())
            .count();
        crouched |= world.npcs()[0].body.crouched;
        if world.npcs()[0].mind.walk.goal().is_none() {
            break;
        }
    }
    assert!(steps > 10, "{steps}");
    assert!(crouched);
}

#[test]
fn a_hundred_walking_npcs_take_a_tick_in_a_known_time() {
    let mut world = starter(
        r#"
        for i = 0, 99 do
            local npc = game.npc { name = "Walker " .. i, at = vector.create(-12 + (i % 10) * 2.6, 0, -12 + (i // 10) * 2.6) }
            npc:goTo(vector.create(12 - (i % 10) * 2.6, 0, 12 - (i // 10) * 2.6))
        end
        "#,
        "",
    );
    // The ways are all found within the first few ticks, a few at a time.
    run(&mut world, 30);
    let started = std::time::Instant::now();
    let ticks = 120;
    run(&mut world, ticks);
    let each = started.elapsed() / ticks;
    let walking = world
        .npcs()
        .iter()
        .filter(|npc| npc.mind.walk.walking())
        .count();
    // The same hundred standing, and nobody: what walking itself costs.
    let timed = |script: &str| {
        let mut world = starter(script, "");
        run(&mut world, 30);
        let started = std::time::Instant::now();
        run(&mut world, ticks);
        started.elapsed() / ticks
    };
    let standing = timed(
        "for i = 0, 99 do game.npc { name = \"Stander \" .. i, at = vector.create(-12 + (i % 10) * 2.6, 0, -12 + (i // 10) * 2.6) } end",
    );
    let empty = timed("");
    println!("100 NPCs, {walking} walking: {each:?} a tick; standing {standing:?}; none {empty:?}");
    assert!(walking > 50, "{walking}");
    // Some 1.2 ms in a debug build: 8 ms leaves room for a machine busy
    // with other builds and still catches walking grown several times
    // dearer. `lore bench` will hold the real number.
    assert!(each < std::time::Duration::from_millis(8), "{each:?}");
}

#[test]
fn a_walk_ended_by_the_world_is_forgotten_by_the_script() {
    let mut world = starter(
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(1.8, 0, 5.5) }
        local friend = game.npc { name = "Friend", at = vector.create(-2, 0, 8) }
        guard:goTo(vector.create(13, 0, -13))
        friend:follow(guard)
        friend:onCannotReach(function(_, why) print(why) end)
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 30 then
                assert(guard.destination ~= nil and friend.destination == guard)
                guard:setHealth(0)
            elseif ticks == 32 then
                assert(guard.destination == nil, "down, it walks nowhere")
                guard:remove()
            elseif ticks == 34 then
                assert(friend.destination == nil, "whoever it followed left")
                print("checked")
            end
        end)
        "#,
        "",
    );
    run(&mut world, 40);
    assert_eq!(world.game.host().last_error(), None);
    let said = printed(&world);
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[0].starts_with("Friend cannot reach "), "{said:?}");
    assert!(said[0].contains("has left the world"), "{said:?}");
    assert_eq!(said[1], "checked");
    // A world made again does not send them off again.
    assert!(
        world
            .npcs()
            .iter()
            .all(|npc| npc.mind.walk.goal().is_none())
    );
}

#[test]
fn going_every_tick_where_it_cannot_reach_is_told_once_and_searched_seldom() {
    let mut world = world_of(
        &walled(1.0),
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(0.5, 0, 3) }
        guard:onCannotReach(function(_, why) print(why) end)
        game.onTick(function() guard:goTo(vector.create(0, 50, 0)) end)
        "#,
        "",
    );
    run(&mut world, 60 * 5);
    assert_eq!(world.game.host().last_error(), None);
    let said = printed(&world);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("no floor a person can stand on"),
        "{said:?}"
    );
}

#[test]
fn going_every_tick_where_it_can_reach_simply_walks_there() {
    let mut world = starter(
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(1.8, 0, 5.5) }
        game.onTick(function() guard:goTo(vector.create(1.8, 0, -4)) end)
        "#,
        "",
    );
    run(&mut world, 60 * 12);
    assert_eq!(world.game.host().last_error(), None);
    let end = feet_of(&world, "Guard");
    assert!(across(end, [1.8, 0.0, -4.0]) < 0.6, "{end:?}");
}

#[test]
fn a_follower_held_up_in_a_doorway_says_so() {
    let mut world = world_of(
        &walled(1.0),
        r#"
        local friend = game.npc { name = "Friend", at = vector.create(0.5, 0, 3) }
        game.npc { name = "Doorman", at = vector.create(0.5, 0, -2) }
        friend:follow(game.player, { distance = 2 })
        friend:onCannotReach(function(_, why) print(why) end)
        "#,
        "",
    );
    world.player.player.position = [0.5, 0.0, -8.0];
    // The player wanders a little on the far side, so new ways keep coming.
    for tick in 0..60 * 30 {
        let x = if (tick / 60) % 2 == 0 { 1.0 } else { -1.0 };
        world.player.player.position = [0.5 + x, 0.0, -8.0];
        run(&mut world, 1);
    }
    let said = printed(&world);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("Friend cannot reach the player: Doorman is in the way"),
        "{said:?}"
    );
}

#[test]
fn a_door_the_script_keeps_shut_is_given_up_on_for_that_reason() {
    let mut world = world_of(
        &walled(1.0),
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(0.5, 0, 3) }
        local door = game.thing("Cellar door")
        local used = 0
        door:onUse(function() used += 1; print("used " .. used) end)
        game.onTick(function() if door.isOpen then door:close() end end)
        guard:goTo(vector.create(0.5, 0, -6))
        guard:onCannotReach(function(_, why) print(why) end)
        "#,
        CELLAR_DOOR,
    );
    until_still(&mut world, 60 * 10);
    assert_eq!(world.game.host().last_error(), None);
    let said = printed(&world);
    assert_eq!(said.len(), 2, "{said:?}");
    assert_eq!(said[0], "used 1");
    assert!(said[1].contains("stayed shut"), "{said:?}");
}

/// A floor 20 m on a side whose east edge, at x = 2, is a drop of 3 m to
/// a floor below, with a wall along z = -3 from x = -10 to 2.
fn ledge() -> Scene {
    blocks(&[
        ([-10.0, -0.2, -10.0], [2.0, 0.0, 10.0]),
        ([2.0, -3.2, -10.0], [12.0, -3.0, 10.0]),
        ([-10.0, 0.0, -3.4], [2.0, 3.0, -3.0]),
    ])
}

#[test]
fn the_player_cannot_push_a_walker_off_a_ledge_or_into_a_wall() {
    let mut world = world_of(
        &ledge(),
        r#"
        local a = game.npc { name = "A", at = vector.create(1.55, 0, 6) }
        a:goTo(vector.create(1.55, 0, -2))
        local b = game.npc { name = "B", at = vector.create(-4, 0, -2.5) }
        b:goTo(vector.create(-8, 0, -2.5))
        "#,
        "",
    );
    let mut lowest: f32 = 0.0;
    let mut deepest: f32 = f32::MAX;
    for _ in 0..60 * 4 {
        // The player leans on each from the inland or open side.
        let a = feet_of(&world, "A");
        let b = feet_of(&world, "B");
        world.player.player.position = if world.tick % 2 == 0 {
            [a[0] - 0.4, 0.0, a[2]]
        } else {
            [b[0], 0.0, b[2] + 0.4]
        };
        run(&mut world, 1);
        lowest = lowest.min(feet_of(&world, "A")[1]);
        deepest = deepest.min(feet_of(&world, "B")[2]);
    }
    assert!(lowest > -0.5, "A went over the edge: {lowest}");
    // The wall's face is at z = -3; B's body is 0.35 round.
    assert!(deepest > -3.0 + 0.3, "B went into the wall: {deepest}");
}

#[test]
fn a_walk_under_way_when_the_level_changes_finds_its_way_on_the_new_one() {
    let open = blocks(&[([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0])]);
    let mut world = world_of(
        &open,
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(-3, 0, 3) }
        guard:goTo(vector.create(-3, 0, -6))
        guard:onArrive(function() print("arrived") end)
        guard:onCannotReach(function(_, why) print(why) end)
        "#,
        "",
    );
    run(&mut world, 20);
    world.change_level(&walled(1.0));
    run(&mut world, 3);
    // The new way goes through the doorway at x 0 to 1, not the wall.
    let way = world.npcs()[0].mind.walk.way();
    assert!(
        way.points
            .iter()
            .any(|p| (p.point[2] + 2.0).abs() < 0.6 && (0.0..1.0).contains(&p.point[0])),
        "{way:?}"
    );
    until_still(&mut world, 60 * 20);
    assert_eq!(printed(&world), ["arrived"]);
}

#[test]
fn three_npcs_through_one_doorway_all_get_there_without_standing_in_each_other() {
    let mut world = world_of(
        &walled(1.2),
        r#"
        for i = 1, 3 do
            local npc = game.npc { name = "N" .. i, at = vector.create(-1.5 + i, 0, 3) }
            npc:goTo(vector.create(-1.5 + i, 0, -6))
            npc:onArrive(function() print(npc.name) end)
            npc:onCannotReach(function(_, why) print(why) end)
        end
        "#,
        "",
    );
    let mut closest = f32::MAX;
    for _ in 0..60 * 30 {
        run(&mut world, 1);
        let at: Vec<_> = world.npcs().iter().map(|npc| npc.body.position).collect();
        for i in 0..at.len() {
            for j in i + 1..at.len() {
                closest = closest.min(across(at[i], at[j]));
            }
        }
        if world
            .npcs()
            .iter()
            .all(|npc| npc.mind.walk.goal().is_none())
        {
            break;
        }
    }
    let mut said = printed(&world);
    said.sort();
    assert_eq!(said, ["N1", "N2", "N3"]);
    assert!(closest > 0.65, "{closest}");
}

#[test]
fn an_npc_that_is_down_is_told_to_stand_first() {
    let mut world = starter(
        r#"
        local guard = game.npc { name = "Guard", at = vector.create(1.8, 0, 5.5) }
        guard:setHealth(0)
        local ok, why = pcall(function() guard:goTo("Gate") end)
        print(why)
        "#,
        "",
    );
    run(&mut world, 1);
    let said = printed(&world);
    assert!(
        said.iter()
            .any(|line| line.contains("Guard is down") && line.contains("setHealth")),
        "{said:?}"
    );
}
