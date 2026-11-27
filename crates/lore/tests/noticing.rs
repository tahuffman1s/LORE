//! NPCs notice bit by bit in a running world on the starter level: the
//! player stepping into view makes a guard curious and then alarmed at the
//! stated rates, ducking out of sight leaves it searching and then calm, a
//! shot behind it turns it round to see who fired, a friend alarms nobody,
//! and the same input gives the same changes on the same ticks.

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

/// One place to start in the middle of the starter level, facing -Z.
const MAP: &str = "version = 1\n\n\
    [[entity]]\nid = \"start\"\ntype = \"spawn\"\nposition = [0.0, 0.0, 0.0]\nfacing = 0.0\n";

/// The world of a game with `script` as its rules, on the starter level:
/// a walled yard 28 metres across with a tower three metres square whose
/// middle is 11 metres along -Z.
fn world(script: &str) -> World {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    let map = Map::parse("map.toml", MAP).unwrap();
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
    let level = lore::level::from_bytes("level.glb", lore::scaffold::STARTER_LEVEL).unwrap();
    World::new(game, TICK_RATE).with_level(&level)
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

fn ask(world: &mut World, change: impl FnOnce(&mut PlayerInput)) {
    let mut asked = world.asked();
    change(&mut asked);
    world.ask(asked);
}

use lore::script::CharacterId;

/// The tick each `onAwareness` change came on: "<tick> <who> <before>><now>".
const TOLD: &str = r#"
local ticks = 0
game.onTick(function() ticks += 1 end)
for _, npc in game.npcs() do
    npc:onAwareness(function(who, now, before, place)
        print(ticks .. " " .. (if who then who.name else "place") .. " " .. before .. ">" .. now)
    end)
end
"#;

/// A guard south of the tower looking north at it; the player is put
/// behind the tower on the first tick, steps out on the sixtieth and, if
/// `back` is set, ducks behind it again on that tick.
fn behind_the_tower(back: u32) -> String {
    format!(
        r#"
local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 0 }}
local steps = 0
game.onTick(function()
    steps += 1
    if steps == 1 then game.player:teleport(vector.create(0, 0.5, -13.3)) end
    if steps == 60 then game.player:teleport(vector.create(4, 0.2, -13)) end
    if steps == {back} then game.player:teleport(vector.create(0, 0.5, -13.3)) end
end)
{TOLD}"#
    )
}

/// Each change told, as (tick, the rest of the line).
fn changes(said: &[String]) -> Vec<(u32, String)> {
    said.iter()
        .filter_map(|line| {
            let (tick, rest) = line.split_once(' ')?;
            Some((tick.parse().ok()?, rest.to_owned()))
        })
        .collect()
}

fn tick_of(changes: &[(u32, String)], what: &str) -> u32 {
    changes
        .iter()
        .find(|(_, rest)| rest == what)
        .map(|(tick, _)| *tick)
        .unwrap_or_else(|| panic!("no {what} in {changes:?}"))
}

#[test]
fn the_player_stepping_into_view_makes_a_guard_curious_then_alarmed_at_the_stated_rates() {
    let mut world = world(&behind_the_tower(0));
    play(&mut world, 59);
    assert_eq!(printed(), Vec::<String>::new());
    // Each tick in view, the gauge rises by SIGHT_FILL times the square of
    // how strongly the player is seen, a sixtieth of it a tick.
    let mut checked = 0;
    for _ in 0..120 {
        let before = world.npcs()[0].awareness.gauge_of(CharacterId::PLAYER);
        play(&mut world, 1);
        let guard = &world.npcs()[0];
        let after = guard.awareness.gauge_of(CharacterId::PLAYER);
        if let Some(seen) = guard
            .senses
            .sighting(CharacterId::PLAYER)
            .filter(|s| s.in_view)
            && after < 1.0
        {
            let want = lore_npc::SIGHT_FILL * seen.strength * seen.strength / 60.0;
            assert!(
                (after - before - want).abs() < 1e-4,
                "{before} {after} {seen:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 5, "{checked}");
    let told = changes(&printed());
    let guard = &world.npcs()[0];
    let seen = guard.senses.sighting(CharacterId::PLAYER).copied().unwrap();
    let curious = tick_of(&told, "Player unaware>curious");
    let alarmed = tick_of(&told, "Player searching>alarmed");
    // From the first look that saw them: well under a second at ten metres
    // in plain day.
    assert!(
        curious > seen.since && alarmed - seen.since < 45,
        "{told:?} {seen:?}"
    );
    // Curious, it looks: what it is most aware of is the player, where
    // they stand.
    let interest = guard.awareness.interest().unwrap();
    assert_eq!(interest.about, Some(CharacterId::PLAYER));
    assert!((interest.at[0] - 4.0).abs() < 0.5, "{interest:?}");
    // The script reads the same.
    let host = world.game().host();
    assert!(host.last_error().is_none());
}

#[test]
fn ducking_behind_a_wall_leaves_the_guard_searching_then_calm() {
    let mut world = world(&behind_the_tower(150));
    play(&mut world, 149);
    let told = changes(&printed());
    tick_of(&told, "Player searching>alarmed");
    play(&mut world, 30);
    let last_seen = world.npcs()[0]
        .awareness
        .of(CharacterId::PLAYER)
        .and_then(|notice| notice.seen)
        .unwrap();
    play(&mut world, 30 * 60);
    let told = changes(&printed());
    let after =
        |what: &str| f32::from(u16::try_from(tick_of(&told, what) - last_seen).unwrap()) / 60.0;
    // Stated: alarmed 4.75 s after the last sight, searching till 16,
    // curious till 23.5.
    assert!(
        (after("Player alarmed>searching") - 4.75).abs() < 0.1,
        "{told:?}"
    );
    assert!(
        (after("Player searching>curious") - 16.0).abs() < 0.1,
        "{told:?}"
    );
    assert!(
        (after("Player curious>unaware") - 23.5).abs() < 0.1,
        "{told:?}"
    );
    assert_eq!(told.len(), 3, "{told:?}");
    assert_eq!(world.npcs()[0].awareness.level(), lore_npc::Aware::Unaware);
}

#[test]
fn a_shot_behind_a_guard_turns_it_round_and_then_it_sees_the_player() {
    let mut world = world(&format!(
        r#"game.weapon("gun", {{ from = "pistol" }})
        game.onSpawn(function() game.player:giveWeapon("gun") end)
        local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 0 }}
        {TOLD}"#
    ));
    // The player stands four metres behind the guard, unseen.
    play(&mut world, 60);
    assert_eq!(printed(), Vec::<String>::new());
    let before = world.npcs()[0].facing();
    ask(&mut world, |asked| asked.fire = true);
    play(&mut world, 2);
    ask(&mut world, |asked| asked.fire = false);
    play(&mut world, 150);
    let told = changes(&printed());
    let heard = tick_of(&told, "Player unaware>searching");
    let alarmed = tick_of(&told, "Player searching>alarmed");
    // Half a turn at half a turn a second, a look, and a moment to fill.
    assert!(alarmed > heard && alarmed - heard < 100, "{told:?}");
    let after = world.npcs()[0].facing();
    let turned = (after - before).rem_euclid(std::f32::consts::TAU);
    assert!(
        (turned - std::f32::consts::PI).abs() < 0.2,
        "{before} {after} {told:?}"
    );
}

#[test]
fn a_friend_walking_past_alarms_nobody() {
    let mut world = world(&format!(
        r#"game.sides {{ Guards = {{}} }}
        game.player.side = "Guards"
        local guard = game.npc {{ name = "Guard", at = vector.create(-6, 0, -4), facing = 270, side = "Guards" }}
        {TOLD}"#
    ));
    // The player walks across the guard's view, close by, and back.
    ask(&mut world, |asked| asked.movement.forward = 1.0);
    play(&mut world, 90);
    ask(&mut world, |asked| asked.movement.forward = -1.0);
    play(&mut world, 90);
    assert_eq!(printed(), Vec::<String>::new());
    let guard = &world.npcs()[0];
    assert!(
        guard.senses.sighting(CharacterId::PLAYER).is_some(),
        "the friend was seen"
    );
    let noticed = guard.awareness.gauge_of(CharacterId::PLAYER);
    assert!(noticed > 0.0 && noticed <= lore_npc::CALM_TOPS, "{noticed}");
    assert_eq!(guard.awareness.level(), lore_npc::Aware::Unaware);
}

#[test]
fn the_same_input_gives_the_same_changes_on_the_same_ticks() {
    let run = || {
        let mut world = world(&behind_the_tower(150));
        play(&mut world, 30);
        ask(&mut world, |asked| asked.movement.forward = 1.0);
        play(&mut world, 900);
        printed()
    };
    let first = run();
    assert!(first.len() >= 3, "{first:?}");
    assert_eq!(first, run());
}

#[test]
fn a_script_raises_the_alarm_and_settles_it_and_reads_it() {
    let mut world = world(&format!(
        r#"local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 0, alertness = 0.5 }}
        local steps = 0
        game.onTick(function()
            steps += 1
            if steps == 10 then guard:alert(game.player) end
            if steps == 12 then print("read " .. guard.awareness .. " " .. guard:awarenessOf(game.player) .. " " .. guard:noticeOf(game.player)) end
            if steps == 20 then guard:calm() end
            if steps == 30 then guard:alert(vector.create(3, 0, -8)) end
        end)
        {TOLD}"#
    ));
    play(&mut world, 40);
    let said = printed();
    assert_eq!(
        said,
        [
            "10 Player unaware>alarmed",
            "read alarmed alarmed 1",
            "20 Player alarmed>unaware",
            "30 place unaware>alarmed",
        ],
        "{said:?}"
    );
    // A place it is alarmed about is where it looks.
    let interest = world.npcs()[0].awareness.interest().copied().unwrap();
    assert!(interest.is_place() && interest.at == [3.0, 0.0, -8.0]);
}

/// A hundred NPCs standing about the starter level, the player walking
/// among them and noticed by many: how long a tick takes. Run with
/// `--release --nocapture` for the numbers.
#[test]
fn a_hundred_npcs_notice_cheaply() {
    let script = format!(
        r#"for i = 0, 99 do
            local x = -12 + (i % 10) * 2.6
            local z = -2 + (i // 10) * 1.5
            game.npc {{ name = "Villager " .. i, at = vector.create(x, 0, z), facing = (i * 37) % 360 }}
        end
        {TOLD}"#
    );
    let mut world = world(&script);
    play(&mut world, 30);
    ask(&mut world, |asked| asked.movement.forward = 0.5);
    let nothing = Input::new(ActionMap::standard());
    let ticks = 240;
    let started = std::time::Instant::now();
    for _ in 0..ticks {
        world.tick(nothing.frame(), STEP);
    }
    let each = started.elapsed() / ticks;
    let noticing = world
        .npcs()
        .iter()
        .filter(|npc| npc.awareness.of(CharacterId::PLAYER).is_some())
        .count();
    println!("100 NPCs, {noticing} noticing the player: {each:?} a tick");
    assert!(noticing > 10, "{noticing}");
    let _ = printed();
    // The same bound as the senses' own crowd test: awareness adds a few
    // microseconds to a tick of about 2 ms in a debug build.
    assert!(
        each < std::time::Duration::from_millis(25),
        "a tick of 100 NPCs took {each:?}"
    );
}
