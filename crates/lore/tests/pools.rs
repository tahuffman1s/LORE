//! Pools in a running game: the level's still fluid is what its sheet says,
//! the player wades and swims in it by what it is, pushes it up and over
//! its brim, and the script reads and changes it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore::assets::{Instance, Mesh, MeshPart, Scene};
use lore::input::{ActionMap, Input};
use lore::map::Map;
use lore::world::{PlayerInput, World};
use lore::{FluidFrame, Frame, Game, ScriptGame};

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

type Triangle = [[f32; 3]; 3];

/// A closed box from `min` to `max`.
fn block(min: [f32; 3], max: [f32; 3]) -> Vec<Triangle> {
    let corner = |x: usize, y: usize, z: usize| {
        let pick = |axis: usize, high: usize| if high == 1 { max[axis] } else { min[axis] };
        [pick(0, x), pick(1, y), pick(2, z)]
    };
    let face = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| [[a, b, c], [a, c, d]];
    [
        face(
            corner(0, 0, 0),
            corner(0, 0, 1),
            corner(0, 1, 1),
            corner(0, 1, 0),
        ),
        face(
            corner(1, 0, 1),
            corner(1, 0, 0),
            corner(1, 1, 0),
            corner(1, 1, 1),
        ),
        face(
            corner(0, 0, 0),
            corner(1, 0, 0),
            corner(1, 0, 1),
            corner(0, 0, 1),
        ),
        face(
            corner(0, 1, 1),
            corner(1, 1, 1),
            corner(1, 1, 0),
            corner(0, 1, 0),
        ),
        face(
            corner(1, 0, 0),
            corner(0, 0, 0),
            corner(0, 1, 0),
            corner(1, 1, 0),
        ),
        face(
            corner(0, 0, 1),
            corner(1, 0, 1),
            corner(1, 1, 1),
            corner(0, 1, 1),
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A level that is a tank on a wide floor: a hollow six metres square and
/// three deep, its wall towards +x a quarter of a metre lower than the
/// rest, with a sheet named "Tank" two metres over its floor that says
/// what `extras` says.
fn tank(extras: &str) -> Scene {
    tank_and(extras, Vec::new())
}

/// The same level with `more` standing in it.
fn tank_and(extras: &str, more: Vec<Triangle>) -> Scene {
    let mut solid = block([-30.0, -0.5, -30.0], [30.0, 0.0, 30.0]);
    solid.extend(more);
    solid.extend(block([-3.5, 0.0, -3.5], [-3.0, 3.0, 3.5]));
    solid.extend(block([3.0, 0.0, -3.5], [3.5, 2.75, 3.5]));
    solid.extend(block([-3.0, 0.0, -3.5], [3.0, 3.0, -3.0]));
    solid.extend(block([-3.0, 0.0, 3.0], [3.0, 3.0, 3.5]));
    let part = |positions: Vec<[f32; 3]>| MeshPart {
        indices: (0..u32::try_from(positions.len()).unwrap()).collect(),
        positions,
        ..MeshPart::default()
    };
    let stood = |mesh: u32, name: &str, extras: Option<String>| Instance {
        mesh,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        mirrored: false,
        name: Some(name.to_owned()),
        extras,
    };
    let sheet = vec![
        [-0.5, 2.0, 0.5],
        [0.5, 2.0, 0.5],
        [0.5, 2.0, -0.5],
        [-0.5, 2.0, 0.5],
        [0.5, 2.0, -0.5],
        [-0.5, 2.0, -0.5],
    ];
    let mut scene = Scene {
        meshes: vec![
            Mesh {
                name: None,
                parts: vec![part(solid.into_iter().flatten().collect())],
            },
            Mesh {
                name: None,
                parts: vec![part(sheet)],
            },
        ],
        instances: vec![
            stood(0, "land", None),
            stood(1, "Tank", Some(extras.to_owned())),
        ],
        ..Scene::default()
    };
    lore_physics::fill(&mut scene);
    scene
}

/// A place to start in the middle of the tank, on its floor.
const IN_THE_TANK: &str = "version = 1\n\n\
    [[entity]]\nid = \"tank\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n";

/// The world of a game with `script` as its rules in a tank that says
/// `extras`, the player on the tank's floor.
fn world(script: &str, extras: &str) -> World {
    world_in(script, &tank(extras))
}

/// The world of a game with `script` as its rules in `scene`.
fn world_in(script: &str, scene: &Scene) -> World {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let map = Map::parse("map.toml", IN_THE_TANK).unwrap();
    let game = ScriptGame::from_source(script, &ActionMap::standard()).with_map(map);
    assert!(
        game.host().last_error().is_none(),
        "{:?}",
        game.host().last_error()
    );
    World::new(game, TICK_RATE).with_level(scene)
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

/// What the world says of its fluid in the next frame.
fn fluid_frame(world: &mut World) -> FluidFrame {
    let mut frame = Frame::default();
    world.frame(&mut frame);
    frame.take_fluid()
}

/// Changes what the player is asking for.
fn ask(world: &mut World, change: impl FnOnce(&mut PlayerInput)) {
    let mut asked = world.asked();
    change(&mut asked);
    world.ask(asked);
}

/// How far the player swims in two seconds of holding forward, once
/// they have come to rest in the tank: not so far as to meet its wall.
fn swum(world: &mut World) -> f32 {
    play(world, 300);
    assert!(world.player().swimming, "{:?}", world.player());
    let from = world.player().position;
    ask(world, |asked| asked.movement.forward = 1.0);
    play(world, 120);
    let to = world.player().position;
    (to[0] - from[0]).hypot(to[2] - from[2])
}

const WATER: &str = r#"{"water": true}"#;
const HONEY: &str = r#"{"water": true, "fluid": "honey"}"#;
const LAVA: &str = r#"{"water": true, "fluid": "lava"}"#;

#[test]
fn a_levels_pool_is_what_its_sheet_says_and_is_drawn_as_that() {
    let mut water = world("", WATER);
    play(&mut water, 1);
    let frame = fluid_frame(&mut water);
    assert!(frame.pools_said);
    assert_eq!(frame.pools.len(), 1);
    assert_eq!(frame.pools[0].look, None, "the level's own water");
    assert!(frame.pools[0].freezes);

    let mut honey = world(
        r#"
        local tank = game.fluids.pool("Tank")
        game.onStart(function()
            print(tank.fluid.name, tank.fluid == game.fluid("honey"), tank.level, tank.depth)
            print(tank.litres > 72000 and tank.litres < 74000, tank.brim > 2.6 and tank.brim < 2.75)
        end)
        "#,
        HONEY,
    );
    play(&mut honey, 1);
    assert_eq!(printed(), ["honey true 2 2", "true true"]);
    let frame = fluid_frame(&mut honey);
    let look = frame.pools[0].look.expect("honey has a look");
    // Golden: it takes far more blue than red.
    assert!(look.absorbs[2] > 3.0 * look.absorbs[0], "{look:?}");
    assert!(!frame.pools[0].freezes);
}

#[test]
fn honey_is_swum_slower_than_water_and_carries_the_swimmer_higher() {
    let mut water = world("", WATER);
    let mut honey = world("", HONEY);
    let (in_water, in_honey) = (swum(&mut water), swum(&mut honey));
    assert!(in_water > 1.0, "{in_water} m through water");
    assert!(
        in_honey < 0.65 * in_water,
        "{in_honey} m through honey, {in_water} through water"
    );
    assert!(
        honey.player().submerged < water.player().submerged - 0.05,
        "{} in honey, {} in water",
        honey.player().submerged,
        water.player().submerged
    );
}

#[test]
fn whoever_is_in_a_pool_pushes_its_surface_up() {
    let mut world = world(
        r#"
        local tank = game.fluids.pool("Tank")
        game.onTick(function() print(tank.level, tank.litres) end)
        "#,
        WATER,
    );
    play(&mut world, 300);
    let lines = printed();
    let last: Vec<f32> = lines
        .last()
        .unwrap()
        .split(' ')
        .map(|number| number.parse().unwrap())
        .collect();
    // A swimmer has about seventy litres of themselves under, in a tank
    // of thirty-six square metres: two millimetres.
    let under = world.player().submerged * 87.0;
    let expected = 2.0 + under / 1000.0 / 36.5;
    assert!(
        (last[0] - expected).abs() < 3.0e-4,
        "{} against {expected}",
        last[0]
    );
    let first: f32 = lines[0].split(' ').nth(1).unwrap().parse().unwrap();
    assert!(
        (last[1] - first).abs() < 1.0,
        "nothing was added: {first} then {}",
        last[1]
    );
    assert!(fluid_frame(&mut world).pools[0].shift > 0.001);
}

#[test]
fn a_script_hears_who_is_in_a_pool_and_what_it_does_to_them() {
    let mut world = world(
        r#"
        local harm = 0
        game.fluids.onTouch(function(touch)
            harm += touch.harm
            if touch.seconds > 0.99 and touch.seconds < 1.01 then
                print(touch.pool.name, touch.fluid.name, touch.cover, touch.submerged > 0.5, touch.depth > 1)
                print(harm > 0.4 and harm <= 1.01)
            end
        end)
        "#,
        LAVA,
    );
    play(&mut world, 90);
    assert_eq!(printed(), ["Tank lava 1 true true", "true"]);
}

#[test]
fn a_tank_filled_past_its_brim_runs_out_over_its_low_wall() {
    let mut world = world(
        r#"
        local tank = game.fluids.pool("Tank")
        local ran = false
        game.onStart(function() game.player:teleport(vector.create(10, 0.1, 10)) end)
        game.onTick(function(step)
            if game.time() < 1 then
                tank:fill(40000 * step, "honey")
            end
            if tank.overflowing and not ran then
                ran = true
                print("over", tank.level >= tank.brim - 0.001)
            end
        end)
        game.onTick(function()
            if game.time() > 5.99 and game.time() < 6.01 then
                print(tank:share("honey") > 0.3, tank.fluid.name, game.fluids.amount() > 50)
            end
        end)
        "#,
        WATER,
    );
    play(&mut world, 6 * 60 + 2);
    assert_eq!(printed(), ["over true", "true water true"]);
    // What ran out is drawn outside the low wall, and the tank at its brim
    // as water with honey in it.
    let frame = fluid_frame(&mut world);
    assert!(frame.pools[0].look.is_some(), "a mix has a look of its own");
    assert!(frame.pools[0].shift > 0.7);
    assert!(
        frame.drops.iter().all(|drop| drop.place[0] > 3.4),
        "it ran out another way"
    );
    assert!(frame.drops.len() > 50, "{} drops", frame.drops.len());
}

#[test]
fn a_script_moves_a_pools_level_over_time_and_changes_what_it_holds() {
    let mut world = world(
        r#"
        local tank = game.fluids.pool("tank")
        game.onStart(function()
            game.player:teleport(vector.create(10, 0.1, 10))
            tank:setLevel(1, 2)
        end)
        game.onTick(function()
            local now = game.time()
            if math.abs(now - 1) < 0.005 or math.abs(now - 2.5) < 0.005 then
                print(string.format("%.2f", tank.level))
            end
            if math.abs(now - 3) < 0.005 then
                tank.fluid = "mercury"
                tank.litres = 9000
            end
            if math.abs(now - 3.5) < 0.005 then
                print(tank.fluid.name, string.format("%.2f %.0f", tank.level, tank.litres))
            end
        end)
        "#,
        WATER,
    );
    play(&mut world, 4 * 60);
    assert_eq!(printed(), ["1.50", "1.00", "mercury 0.25 9000"]);
    let frame = fluid_frame(&mut world);
    let look = frame.pools[0].look.expect("mercury has a look");
    assert!(look.metal > 0.9, "{look:?}");
}

#[test]
fn falling_into_a_pool_throws_drops_of_what_it_holds() {
    let mut world = world(
        r#"
        game.onStart(function() game.player:teleport(vector.create(0, 6, 0)) end)
        "#,
        HONEY,
    );
    let mut most = 0;
    for _ in 0..120 {
        play(&mut world, 1);
        most = most.max(fluid_frame(&mut world).drops.len());
    }
    assert!(most >= 3, "{most} drops thrown");
    assert!(world.player().submerged > 0.0, "{:?}", world.player());
}

#[test]
fn a_tub_nobody_marked_becomes_a_pool_the_script_and_the_picture_are_told_of() {
    // A tub two metres square on the floor far from the tank, its wall
    // towards +x lower than the rest. No sheet says it is a pool.
    let tub = [
        block([-13.2, 0.0, -13.2], [-13.0, 1.0, -10.8]),
        block([-11.0, 0.0, -13.2], [-10.8, 0.8, -10.8]),
        block([-13.0, 0.0, -13.2], [-11.0, 1.0, -13.0]),
        block([-13.0, 0.0, -11.0], [-11.0, 1.0, -10.8]),
    ];
    let scene = tank_and(WATER, tub.into_iter().flatten().collect());
    let mut world = world_in(
        r#"
        local tap = vector.create(-12, 1.5, -12)
        local told = false
        game.onStart(function() game.player:teleport(vector.create(10, 0.1, 10)) end)
        game.onTick(function()
            if game.time() < 8 then
                game.fluids.spray("honey", tap, vector.create(0, -2, 0), { litresPerSecond = 60, spread = 0 })
            end
            local pools = game.fluids.pools()
            if #pools == 2 and not told then
                told = true
                print("found", pools[2].name, pools[2].fluid.name)
            end
            if game.time() > 9.99 and game.time() < 10.01 then
                local tub = game.fluids.pool("pool 2")
                print(tub.litres > 300, tub.depth > 0.08, tub.brim > 0.7, tub.overflowing)
                tub:fill(20, "water")
            end
        end)
        "#,
        &scene,
    );
    // Before anything is poured the picture is told there are none found.
    let frame = fluid_frame(&mut world);
    assert_eq!(frame.found.as_ref().map(Vec::len), Some(0));
    assert_eq!(frame.pools.len(), 1);
    play(&mut world, 10 * 60 + 2);
    assert_eq!(printed(), ["found pool 2 honey", "true true true false"]);
    // The picture is given the tub's shape once, and how it stands with
    // the tank from then on: honey, with a look of its own.
    let frame = fluid_frame(&mut world);
    let found = frame.found.expect("the found hollows are said");
    assert_eq!(found.len(), 1);
    assert!(found[0].depth_at(-12.0, -12.0) > 0.0, "the tub's own shape");
    assert_eq!(frame.pools.len(), 2);
    assert!(frame.pools[1].look.is_some());
    let frame = fluid_frame(&mut world);
    assert!(frame.found.is_none(), "said once, until it changes");
    assert_eq!(frame.pools.len(), 2);
}
