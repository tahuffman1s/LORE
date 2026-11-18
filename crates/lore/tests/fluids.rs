//! Fluids in a running game: what the script lets out reaches the picture,
//! what lies underfoot changes how the player walks, and the script hears
//! how much there is and what the player stands in.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

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

/// One place to start, in the middle of the flat floor, facing -Z.
const ONE_SPAWN: &str = "version = 1\n\n\
    [[entity]]\nid = \"south\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n";

/// The world of a game with `script` as its rules, on the flat floor with
/// that one place to start.
fn world(script: &str) -> World {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    let map = Map::parse("map.toml", ONE_SPAWN).unwrap();
    let game = ScriptGame::from_source(script, &ActionMap::standard()).with_map(map);
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

/// Water sprayed down onto the player's feet, a little each tick.
const WATER_UNDERFOOT: &str = r#"
game.onTick(function()
    game.fluids.spray("water", game.player.position + vector.create(0, 0.5, 0),
        vector.create(0, -1, 0), { litresPerSecond = 0.6 })
end)
"#;

#[test]
fn water_sprayed_by_the_script_reaches_the_frame_with_the_looks_once() {
    let mut world = world(WATER_UNDERFOOT);
    play(&mut world, 60);
    let first = fluid_frame(&mut world);
    assert!(first.said);
    assert!(!first.drops.is_empty());
    assert_eq!(first.looks.as_ref().map(|looks| looks.len()), Some(32));
    let second = fluid_frame(&mut world);
    assert!(second.said, "the drops are said every frame");
    assert!(second.looks.is_none(), "nothing about the looks changed");
}

#[test]
fn the_script_can_read_how_many_litres_there_are_and_it_rises_while_it_sprays() {
    let script = format!(
        "{WATER_UNDERFOOT}\ngame.onTick(function() print(game.fluids.amount(\"water\")) end)"
    );
    let mut world = world(&script);
    play(&mut world, 10);
    let early: f32 = printed().last().unwrap().parse().unwrap();
    play(&mut world, 50);
    let late: f32 = printed().last().unwrap().parse().unwrap();
    assert!(late > early + 0.1, "{early} L then {late} L");
}

/// Oil sprayed on the floor in front of the player, over the first second
/// when `OIL` is true.
const OIL_AHEAD: &str = r#"
local sprayed = 0
game.onTick(function()
    if OIL and sprayed < 90 then
        sprayed += 1
        local z = -0.8 - (sprayed % 15) * 0.15
        game.fluids.spray("oil", vector.create(0, 0.3, z), vector.create(0, -1, 0), { litresPerSecond = 3 })
    end
end)
"#;

/// How far the player slides after letting go of forward, having walked
/// into the sprayed place, on a floor with oil on it or without.
fn slide(oiled: bool) -> f32 {
    let script = format!("OIL = {oiled}\n{OIL_AHEAD}");
    let mut world = world(&script);
    play(&mut world, 100);
    ask(&mut world, |asked| asked.movement.forward = 1.0);
    let mut ticks = 0;
    while world.player().position[2] > -1.5 && ticks < 300 {
        play(&mut world, 1);
        ticks += 1;
    }
    ask(&mut world, |asked| asked.movement.forward = 0.0);
    let letting_go = world.player().position;
    play(&mut world, 90);
    let stopped = world.player().position;
    (stopped[2] - letting_go[2]).hypot(stopped[0] - letting_go[0])
}

#[test]
fn a_player_slides_further_on_oil_than_on_a_dry_floor() {
    let dry = slide(false);
    let oily = slide(true);
    assert!(
        oily > dry + 0.2,
        "slid {oily} m on oil and {dry} m on the dry floor"
    );
}

#[test]
fn standing_in_a_sprayed_puddle_calls_on_touch() {
    let script = format!(
        "{WATER_UNDERFOOT}\n\
         game.fluids.onTouch(function(touch) print('wet ' .. touch.fluid.name) end)"
    );
    let mut world = world(&script);
    play(&mut world, 60);
    let said = printed();
    assert!(said.iter().any(|line| line == "wet water"), "{said:?}");
}

#[test]
fn nothing_is_said_of_a_touch_on_a_dry_floor() {
    let mut world = world("game.fluids.onTouch(function(touch) print('wet') end)");
    play(&mut world, 60);
    assert_eq!(printed(), Vec::<String>::new());
}

#[test]
fn a_water_with_a_colour_is_told_to_the_picture_and_a_fresh_one_says_as_the_level_does() {
    let mut fresh = world("");
    let first = fluid_frame(&mut fresh);
    assert!(
        matches!(first.water_look, Some(None)),
        "{:?}",
        first.water_look
    );
    assert!(fluid_frame(&mut fresh).water_look.is_none());

    let mut red = world("game.fluid(\"water\").color = \"red\"");
    play(&mut red, 1);
    let look = fluid_frame(&mut red).water_look;
    assert!(matches!(look, Some(Some(_))), "{look:?}");
}

#[test]
fn a_clear_empties_the_drops_while_time_is_stopped() {
    let script = r#"
local n = 0
game.onTick(function()
    n += 1
    if n < 30 then
        game.fluids.spray("water", game.player.position + vector.create(0, 0.5, 0),
            vector.create(0, -1, 0), { litresPerSecond = 0.6 })
    elseif n == 30 then
        game.setTimeSpeed(0)
    elseif n == 35 then
        game.fluids.clear()
    end
end)
"#;
    let mut world = world(script);
    play(&mut world, 32);
    assert!(!fluid_frame(&mut world).drops.is_empty());
    play(&mut world, 10);
    let after = fluid_frame(&mut world);
    assert!(after.said);
    assert!(after.drops.is_empty(), "{} drops left", after.drops.len());
}
