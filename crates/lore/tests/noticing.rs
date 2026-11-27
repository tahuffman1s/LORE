//! NPCs notice bit by bit in a running world on the starter level: the
//! player stepping into view makes a guard that counts them an enemy
//! curious and then alarmed at the stated rates, ducking out of sight
//! leaves it searching and then calm, a shot behind it turns it round to
//! see who fired, a friend alarms nobody, a guard in a game with no sides
//! only looks, and the same input gives the same changes on the same ticks.

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

/// A table of sides in which the guards count the player an enemy: give
/// a guard `side = "Guards"`.
const OUTLAW: &str = r#"
game.sides { Guards = { enemies = { "Outlaws" } }, Outlaws = {} }
game.player.side = "Outlaws"
"#;

/// A guard, who counts the player an enemy, south of the tower looking
/// north at it; the player is put
/// behind the tower on the first tick, steps out on the sixtieth and, if
/// `back` is set, ducks behind it again on that tick.
fn behind_the_tower(back: u32) -> String {
    format!(
        r#"
{OUTLAW}
local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 0, side = "Guards" }}
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
            // The first look's gap counts half.
            let first = if seen.seen == seen.since {
                lore_npc::FIRST_LOOK
            } else {
                1.0
            };
            let want = lore_npc::SIGHT_FILL * seen.strength * seen.strength * first / 60.0;
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
    // From the first look that saw them: about a second and a half, ten
    // metres off in plain day, standing at the edge of the tower (a
    // little more while they land and are seen less of).
    let want = (0.1 + 1.0 / (lore_npc::SIGHT_FILL * seen.strength * seen.strength)) * 60.0;
    let took = (alarmed - seen.since) as f32;
    assert!(
        curious > seen.since && took > want - 3.0 && took < want + 20.0,
        "{told:?} {seen:?} {want}"
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
    let mut world = world(&behind_the_tower(200));
    play(&mut world, 199);
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
        r#"{OUTLAW}
        game.weapon("gun", {{ from = "pistol" }})
        game.onSpawn(function() game.player:giveWeapon("gun") end)
        local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 0, side = "Guards" }}
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
    // No mark over a friend.
    let notice = guard.awareness.of(CharacterId::PLAYER).unwrap();
    assert!(!notice.may_alarm());
}

/// The angle from `a` to `b`, the short way round, in radians.
fn apart(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d)
}

#[test]
fn with_no_sides_a_guard_only_looks_at_the_player_and_forgets_them() {
    // A guard looking south at the player, who stands off to one side of
    // its look, close, then goes behind the tower.
    let mut world = world(&format!(
        r#"local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 180 }}
        local steps = 0
        game.onTick(function()
            steps += 1
            if steps == 1 then game.player:teleport(vector.create(2, 0.2, -2.5)) end
            if steps == 300 then game.player:teleport(vector.create(0, 0.5, -13.3)) end
        end)
        {TOLD}"#
    ));
    let facing = world.npcs()[0].facing();
    let mut most = lore_npc::Aware::Unaware;
    for _ in 0..299 {
        play(&mut world, 1);
        let guard = &world.npcs()[0];
        most = most.max(guard.awareness.level_of(CharacterId::PLAYER));
        assert!(apart(guard.facing(), facing) < 0.01, "the body turned");
    }
    assert_eq!(most, lore_npc::Aware::Curious);
    let guard = &world.npcs()[0];
    // It looks at them, head and chest; no mark is shown over it.
    assert_eq!(
        guard.awareness.interest().unwrap().about,
        Some(CharacterId::PLAYER)
    );
    assert!(guard.awareness.turns_to().is_none());
    assert!(!guard.awareness.of(CharacterId::PLAYER).unwrap().may_alarm());
    // Gone: calm in a few seconds, not twenty.
    play(&mut world, 5 * 60);
    let told = changes(&printed());
    assert_eq!(
        told.iter()
            .map(|(_, what)| what.as_str())
            .collect::<Vec<_>>(),
        ["Player unaware>curious", "Player curious>unaware"]
    );
    let calm = tick_of(&told, "Player curious>unaware");
    assert!((300..300 + 5 * 60).contains(&calm), "{told:?}");
    assert!(apart(world.npcs()[0].facing(), facing) < 0.01);
}

#[test]
fn a_guard_that_sees_the_player_hurt_a_friend_goes_from_curious_to_alarmed() {
    let mut world = world(&format!(
        r#"game.sides {{ Town = {{ friends = {{ "Guards" }} }}, Guards = {{}} }}
        local villager = game.npc {{ name = "Villager", at = vector.create(0, 0, -2), side = "Town" }}
        local guard = game.npc {{ name = "Guard", at = vector.create(3, 0, 0), side = "Guards", facing = 90 }}
        local steps = 0
        game.onStart(function() game.player:teleport(vector.create(0, 0, 0)) end)
        game.onTick(function()
            steps += 1
            if steps == 120 then villager:damage(10, {{ from = game.player }}) end
        end)
        {TOLD}"#
    ));
    play(&mut world, 119);
    let guard = &world.npcs()[1];
    assert_eq!(
        guard.awareness.level_of(CharacterId::PLAYER),
        lore_npc::Aware::Curious
    );
    play(&mut world, 120);
    let told = changes(&printed());
    let alarmed = told
        .iter()
        .find(|(_, what)| what == "Player searching>alarmed" || what == "Player curious>alarmed")
        .map(|(tick, _)| *tick);
    assert!(alarmed.is_some_and(|t| t > 120 && t < 200), "{told:?}");
    let guard = &world.npcs()[1];
    assert!(guard.awareness.of(CharacterId::PLAYER).unwrap().may_alarm());
}

#[test]
fn a_guard_calmed_while_it_sees_its_enemy_is_alarmed_again_in_about_a_second() {
    let mut world = world(&format!(
        r#"{OUTLAW}
        local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 180, side = "Guards" }}
        local steps = 0
        game.onTick(function()
            steps += 1
            if steps == 120 then guard:calm() end
        end)
        {TOLD}"#
    ));
    play(&mut world, 200);
    let told = changes(&printed());
    let calmed = tick_of(&told, "Player alarmed>unaware");
    let again = told
        .iter()
        .filter(|(tick, what)| *tick > calmed && what.ends_with(">alarmed"))
        .map(|(tick, _)| *tick)
        .next();
    // Standing still four metres off: about a second.
    assert!(again.is_some_and(|t| t - calmed <= 70), "{told:?}");
}

#[test]
fn a_body_passed_twice_is_told_once() {
    // The guard finds a fallen villager, is calmed, walks off round the
    // yard and comes back to it.
    let mut world = world(&format!(
        r#"game.sides {{ Town = {{ friends = {{ "Guards" }} }}, Guards = {{}} }}
        local villager = game.npc {{ name = "Villager", at = vector.create(2, 0, -1), side = "Town" }}
        local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 180, side = "Guards" }}
        local steps = 0
        game.onTick(function()
            steps += 1
            if steps == 1 then game.player:teleport(vector.create(-10, 0.2, -12)) end
            if steps == 2 then villager:damage(1000) end
            if steps == 200 then guard:calm() end
            if steps == 210 then guard:goTo(vector.create(8, 0, 6)) end
        end)
        local laps = 0
        guard:onArrive(function()
            laps += 1
            if laps == 1 then guard:goTo(vector.create(0, 0, -4), {{ facing = 180 }}) end
        end)
        {TOLD}"#
    ));
    play(&mut world, 900);
    let villager = world.npcs()[0].who;
    assert!(
        world.npcs()[1].senses.sees(villager),
        "it sees the body again"
    );
    let told = changes(&printed());
    let found = told
        .iter()
        .filter(|(_, what)| what == "place unaware>alarmed")
        .count();
    assert_eq!(found, 1, "{told:?}");
}

#[test]
fn a_walk_that_ends_facing_a_way_turns_back_to_it_after_a_notice() {
    let mut world = world(&format!(
        r#"{OUTLAW}
        local guard = game.npc {{ name = "Guard", at = vector.create(-6, 0, 4), facing = 0, side = "Guards" }}
        guard:onArrive(function()
            guard:alert(vector.create(-6, 0, 8))
        end)
        local steps = 0
        game.onTick(function()
            steps += 1
            if steps == 1 then game.player:teleport(vector.create(10, 0.2, 10)) end
            if steps == 2 then guard:goTo(vector.create(-6, 0, 2), {{ facing = 90 }}) end
            if steps == 300 then guard:calm() end
        end)
        {TOLD}"#
    ));
    play(&mut world, 299);
    let told = changes(&printed());
    tick_of(&told, "place unaware>alarmed");
    // Turned round to the place it was alarmed about.
    let guard = &world.npcs()[0];
    assert!(
        apart(guard.facing(), std::f32::consts::PI) < 0.1,
        "{}",
        guard.facing()
    );
    play(&mut world, 180);
    let guard = &world.npcs()[0];
    assert!(
        apart(guard.facing(), std::f32::consts::FRAC_PI_2) < 0.05,
        "{}",
        guard.facing()
    );
    assert!(apart(guard.mind.wants(), std::f32::consts::FRAC_PI_2) < 0.05);
}

#[test]
fn a_saved_script_still_hears_of_awareness() {
    let dir = std::env::temp_dir().join(format!("lore-noticing-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script = format!(
        r#"{OUTLAW}
        local guard = game.npc {{ name = "Guard", at = vector.create(0, 0, -4), facing = 0, side = "Guards" }}
        guard:onAwareness(function(who, now, before) print("told " .. now) end)
        local steps = 0
        game.onTick(function()
            steps += 1
            if steps == 30 then guard:alert(game.player) end
        end)
        "#
    );
    std::fs::write(dir.join("main.luau"), &script).unwrap();
    std::fs::write(dir.join("map.toml"), MAP).unwrap();
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    let game = ScriptGame::open(&dir, &ActionMap::standard()).unwrap();
    let level = lore::level::from_bytes("level.glb", lore::scaffold::STARTER_LEVEL).unwrap();
    let mut world = World::new(game, TICK_RATE).with_level(&level);
    play(&mut world, 10);
    std::fs::write(dir.join("main.luau"), format!("{script}\n-- saved again\n")).unwrap();
    world.game_mut().host_mut().reload().unwrap();
    play(&mut world, 40);
    let said = printed();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(said, ["told alarmed"], "{said:?}");
    assert_eq!(world.npcs().len(), 1);
}
