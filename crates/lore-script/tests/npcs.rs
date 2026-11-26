//! NPCs as a script makes and finds them: `game.npc`, what an NPC has
//! besides being a character, and what a saved script does to them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use lore_input::{ActionMap, InputFrame};
use lore_script::{
    CharacterId, ENTRY_SCRIPT, NpcAsk, Placed, PlacedValue, ScriptHost, Usable, UseSaid,
};

thread_local! {
    /// What scripts on this test's thread printed.
    static PRINTED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Collects `print` output per thread.
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

/// What the script printed since this was last asked.
fn printed() -> Vec<String> {
    PRINTED.with_borrow_mut(std::mem::take)
}

fn listen() {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
}

const STEP: f32 = 1.0 / 60.0;

/// A gate to stand at, a tower, a zone round the well, and a smith placed
/// as an NPC.
fn placed() -> Vec<Placed> {
    let thing = |id: &str, kind: &str, name: &str, position: [f32; 3], facing: f32| Placed {
        id: id.to_owned(),
        kind: kind.to_owned(),
        name: name.to_owned(),
        position,
        facing,
        properties: Vec::new(),
    };
    let mut well = thing("w", "zone", "Well", [20.0, 0.0, 0.0], 0.0);
    well.properties
        .push(("radius".to_owned(), PlacedValue::Number(2.0)));
    vec![
        thing("g", "place", "Gate", [0.0, 0.0, -10.0], 180.0),
        thing("t", "place", "Tower", [5.0, 4.0, -20.0], 0.0),
        well,
        thing("s", "npc", "Smith", [6.0, 0.0, -6.0], 90.0),
    ]
}

/// A game of `script` with those placed, as the engine opens a packaged one.
fn game(script: &str) -> ScriptHost {
    listen();
    ScriptHost::from_sources_with_placed(
        [(ENTRY_SCRIPT.to_owned(), script.to_owned())],
        Vec::new(),
        ActionMap::standard(),
        Vec::new(),
        placed(),
    )
}

/// The error a script stopped loading with.
fn load_error(script: &str) -> String {
    let host = game(script);
    host.last_error()
        .map(|error| error.message.clone())
        .unwrap_or_default()
}

/// A game in a folder, so it can be saved and loaded again.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        listen();
        let folder = std::env::temp_dir().join(format!(
            "lore-npcs-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let folder = Self(folder);
        folder.write(script);
        folder
    }

    fn write(&self, script: &str) {
        std::fs::write(self.0.join(ENTRY_SCRIPT), script).unwrap();
    }

    fn open(&self) -> ScriptHost {
        ScriptHost::open_with_placed(&self.0, ActionMap::standard(), Vec::new(), placed()).unwrap()
    }
}

impl Drop for GameFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn one_line_makes_an_npc_at_a_place_and_the_engine_is_asked_for_its_body() {
    let mut host = game(
        r#"local guard = game.npc { name = "Guard", at = "Gate" }
        print(guard.name, guard.health, guard.position, guard.dead, guard == game.npc("Guard"))
        print(guard.useText, guard.usable, guard.lookDistance)"#,
    );
    assert_eq!(host.last_error(), None);
    assert_eq!(
        printed(),
        [
            "Guard 100 vector(0, 0, -10) false true",
            "Talk to Guard false 5"
        ]
    );
    let asks = host.take_npc_asks();
    // The smith placed in the map first, then the guard.
    assert_eq!(asks.len(), 2, "{asks:?}");
    let NpcAsk::Made { feet, facing, .. } = asks[1] else {
        panic!("{asks:?}");
    };
    assert_eq!(feet, [0.0, 0.0, -10.0]);
    // The gate is turned half round, and so is the guard.
    assert!((facing - std::f32::consts::PI).abs() < 1e-5);
    let mut told = Vec::new();
    host.each_npc(|npc| told.push(npc.feet));
    assert_eq!(told, [[6.0, 0.0, -6.0], [0.0, 0.0, -10.0]]);
}

#[test]
fn an_npc_stands_at_a_position_a_thing_or_a_place_in_other_capitals() {
    let host = game(
        r#"local a = game.npc { name = "A", at = vector.create(1, 2, 3), facing = 90 }
        local b = game.npc { name = "B", at = game.thing("Tower") }
        local c = game.npc { name = "C", at = "tower" }
        print(a.position, b.position, c.position)"#,
    );
    assert_eq!(host.last_error(), None);
    assert_eq!(
        printed(),
        ["vector(1, 2, 3) vector(5, 4, -20) vector(5, 4, -20)"]
    );
    assert_eq!(host.place_of("Gate"), Some(([0.0, 0.0, -10.0], 180.0)));
    assert_eq!(host.place_of("Nowhere"), None);
}

#[test]
fn an_npc_placed_in_the_map_is_there_before_the_first_line() {
    let host = game(r#"local smith = game.npc("smith") print(smith.name, smith.position.x)"#);
    assert_eq!(host.last_error(), None);
    assert_eq!(printed(), ["Smith 6"]);
}

#[test]
fn a_mistake_in_making_or_finding_an_npc_says_what_to_write() {
    assert!(
        load_error(r#"game.npc { name = "Guard", at = "Gaet" }"#).contains(
            "game.npc cannot stand \"Guard\" at \"Gaet\": nothing in the map is called that. \
             Did you mean \"Gate\"? The map has \"Gate\", \"Tower\", \"Well\", \"Smith\"."
        )
    );
    assert!(
        load_error(r#"game.npc { name = "Guard" }"#)
            .contains("game.npc needs `at`: where \"Guard\" stands")
    );
    assert!(load_error(r#"game.npc { at = "Gate" }"#).contains("game.npc needs a `name`"));
    assert!(
        load_error(r#"game.npc { name = "Guard", at = "Gate", postion = 1 }"#)
            .contains("game.npc does not take `postion`.")
    );
    assert!(
        load_error(r#"game.npc { name = "Guard", at = "Gate", facng = 1 }"#)
            .contains("Did you mean `facing`?")
    );
    assert!(load_error(r#"game.npc { name = "Guard", at = 5 }"#).contains("it was given a number"));
    assert!(load_error("game.npc(5)").contains("game.npc takes the name of an NPC to find"));
    let unknown = load_error(r#"game.npc("Smtih")"#);
    assert!(
        unknown.contains(
            "No NPC is called \"Smtih\". Did you mean \"Smith\"? The NPCs are \"Smith\"."
        ),
        "{unknown}"
    );
    let not_npc = load_error(r#"game.npc("Gate")"#);
    assert!(
        not_npc.contains("\"Gate\" is placed in the map as type \"place\", not \"npc\""),
        "{not_npc}"
    );
    // What only an NPC has is no mistake to read on the player? It is.
    assert!(load_error("print(game.player.useText)").contains("Only an NPC has `useText`"));
    assert!(load_error("game.player:remove()").contains("Only an NPC has `remove`"));
    let typo = load_error(
        r#"local guard = game.npc { name = "Guard", at = "Gate" } print(guard.lookDistanse)"#,
    );
    assert!(
        typo.contains("npc.lookDistanse does not exist. Did you mean npc.lookDistance?"),
        "{typo}"
    );
    let moved = load_error(
        r#"local guard = game.npc { name = "Guard", at = "Gate" } guard.position = vector.zero"#,
    );
    assert!(moved.contains("change `at` there and save"), "{moved}");
}

#[test]
fn the_player_uses_an_npc_with_its_own_words_until_it_dies() {
    let mut host = game(
        r#"local guard = game.npc { name = "Guard", at = "Gate" }
        guard.useText = "Ask about the road"
        guard.useReach = 3
        guard:onUse(function(npc) print("used", npc.name, npc == guard) end)"#,
    );
    assert_eq!(host.last_error(), None);
    let seen = npcs_usable(&host);
    assert_eq!(seen.len(), 1);
    let (Usable::Npc(guard), at, said) = seen[0].clone() else {
        panic!("{seen:?}");
    };
    // Used at the chest.
    assert_eq!(at, [0.0, 1.3, -10.0]);
    assert_eq!(
        (said.text.as_str(), said.reach),
        ("Ask about the road", 3.0)
    );
    host.set_use_target(Some(Usable::Npc(guard)), 0.5);
    assert_eq!(
        host.use_prompt(),
        Some(("Ask about the road".to_owned(), 0.5))
    );
    host.use_npc(guard);
    assert_eq!(printed(), ["used Guard true"]);
    host.damage(guard, 500.0, None, "fall");
    assert!(npcs_usable(&host).is_empty(), "the dead are not used");
}

fn npcs_usable(host: &ScriptHost) -> Vec<(Usable<'static>, [f32; 3], UseSaid)> {
    let mut seen = Vec::new();
    host.each_usable(|meant, at, said| {
        if let Usable::Npc(id) = meant {
            seen.push((Usable::Npc(id), at, said.clone()));
        }
    });
    seen
}

#[test]
fn an_npc_is_hurt_dies_walks_into_zones_and_can_be_removed() {
    let mut host = game(
        r#"local guard = game.npc { name = "Guard", at = "Gate" }
        game.onDeath(function(victim, killer) print("died", victim.name, victim == guard) end)
        game.zone("Well"):onEnter(function(who) print("well", who.name) end)
        game.onTick(function()
            if guard.dead and #game.npcs() == 2 then guard:remove() print(#game.npcs()) end
        end)"#,
    );
    assert_eq!(host.last_error(), None);
    let mut ids = Vec::new();
    host.each_npc(|npc| ids.push(npc.id));
    let guard = ids[1];
    let _ = host.take_npc_asks();
    host.set_position(guard, [20.0, 0.0, 0.0]);
    host.step_zones();
    assert_eq!(printed(), ["well Guard"]);
    assert!(host.damage(guard, 150.0, Some(CharacterId::PLAYER), "sword"));
    host.tick(&InputFrame::default(), STEP);
    assert_eq!(printed(), ["died Guard true", "1"]);
    assert_eq!(host.take_npc_asks(), [NpcAsk::Removed { id: guard }]);
    assert!(!host.is_npc(guard));
}

#[test]
fn a_saved_script_keeps_its_npcs_moves_those_it_now_puts_elsewhere_and_lets_go_of_the_rest() {
    let game = GameFolder::new(
        r#"game.npc { name = "Guard", at = "Gate" }
        game.npc { name = "Cook", at = "Gate" }
        game.npc { name = "Guard 2", at = "Tower" }"#,
    );
    let mut host = game.open();
    assert_eq!(host.last_error(), None);
    let mut first = Vec::new();
    host.each_npc(|npc| first.push(npc.id));
    let [smith, guard, cook, second_guard] = first[..] else {
        panic!("{first:?}");
    };
    let _ = host.take_npc_asks();
    host.damage(guard, 30.0, None, "fall");

    // The first guard moves to the tower; the cook's line is gone; the
    // second guard's line is the same.
    game.write(
        r#"game.npc { name = "Guard", at = "Tower" }
        game.npc { name = "Guard 2", at = "Tower" }"#,
    );
    host.reload().unwrap();
    let mut now = Vec::new();
    host.each_npc(|npc| now.push(npc.id));
    assert_eq!(now, [smith, guard, second_guard]);
    assert_eq!(
        host.health(guard),
        Some((70.0, 100.0)),
        "the same guard, still hurt"
    );
    let asks = host.take_npc_asks();
    assert!(asks.contains(&NpcAsk::Removed { id: cook }), "{asks:?}");
    assert!(
        asks.iter()
            .any(|ask| matches!(ask, NpcAsk::Put { id, feet, .. } if *id == guard && *feet == [5.0, 4.0, -20.0])),
        "{asks:?}"
    );
    assert_eq!(asks.len(), 2, "{asks:?}");

    // A save with a mistake changes nobody.
    game.write(
        r#"game.npc { name = "Guard", at = "Gate" }
        game.npc { name = "Baker", at = "Gate" }
        game.npc { name = "Oops" }"#,
    );
    assert!(host.reload().is_err());
    let mut after = Vec::new();
    host.each_npc(|npc| after.push(npc.id));
    assert_eq!(after, now);
    let asks = host.take_npc_asks();
    // The baker that the failed save made is taken out again at once.
    assert!(
        matches!(asks[..], [NpcAsk::Made { id: made, .. }, NpcAsk::Removed { id: gone }] if made == gone),
        "{asks:?}"
    );
    assert_eq!(host.character_position(guard), Some([5.0, 4.0, -20.0]));
}

#[test]
fn an_npc_placed_while_the_game_runs_comes_into_it() {
    let mut host = game("");
    let _ = host.take_npc_asks();
    let mut more = placed();
    more.push(Placed {
        id: "b".to_owned(),
        kind: "npc".to_owned(),
        name: String::new(),
        position: [1.0, 0.0, 1.0],
        facing: 0.0,
        properties: Vec::new(),
    });
    host.set_placed(more);
    let asks = host.take_npc_asks();
    assert!(
        matches!(
            asks[..],
            [NpcAsk::Made {
                feet: [1.0, 0.0, 1.0],
                ..
            }]
        ),
        "{asks:?}"
    );
}

#[test]
fn a_map_saved_while_the_game_runs_moves_renames_and_takes_out_its_npcs() {
    let mut host = game(
        r#"local smith = game.npc("Smith")
        game.npc { name = "Guard", at = "Gate" }
        game.onTick(function() print(smith.name, smith.position.x) end)"#,
    );
    assert_eq!(host.last_error(), None);
    let mut ids = Vec::new();
    host.each_npc(|npc| ids.push(npc.id));
    let [smith, guard] = ids[..] else {
        panic!("{ids:?}");
    };
    let _ = host.take_npc_asks();

    // The smith is moved, turned and renamed in the map.
    let mut moved = placed();
    let entity = moved.iter_mut().find(|entity| entity.id == "s").unwrap();
    entity.position = [8.0, 0.0, -6.0];
    entity.facing = 0.0;
    entity.name = "Old Smith".to_owned();
    host.set_placed(moved.clone());
    assert_eq!(
        host.take_npc_asks(),
        [NpcAsk::Put {
            id: smith,
            feet: [8.0, 0.0, -6.0],
            facing: 0.0
        }]
    );
    host.tick(&InputFrame::default(), STEP);
    assert_eq!(
        printed(),
        ["Old Smith 8"],
        "the same handle, moved and renamed"
    );

    // Saved again unchanged, nobody is moved: one who walked stays.
    host.set_position(smith, [9.0, 0.0, -6.0]);
    host.set_placed(moved.clone());
    assert_eq!(host.take_npc_asks(), []);
    assert_eq!(host.character_position(smith), Some([9.0, 0.0, -6.0]));

    // Taken out of the map, the smith leaves; the guard the script made
    // stays.
    moved.retain(|entity| entity.id != "s");
    host.set_placed(moved);
    assert_eq!(host.take_npc_asks(), [NpcAsk::Removed { id: smith }]);
    assert!(!host.is_npc(smith));
    assert!(host.is_npc(guard));
    host.tick(&InputFrame::default(), STEP);
    let error = host.last_error().map(|error| error.message.clone());
    assert!(
        error
            .as_deref()
            .is_some_and(|said| said.contains("no longer in the world")),
        "{error:?}"
    );
}

#[test]
fn one_name_is_one_person_and_a_crowd_is_numbered() {
    // Made in game.onTick, a second guard is a mistake that says what to
    // write, not a new guard every tick.
    let mut host = game(
        r#"game.npc { name = "Guard", at = "Gate" }
        game.onTick(function() game.npc { name = "Guard", at = "Gate" } end)"#,
    );
    assert_eq!(host.last_error(), None);
    for _ in 0..120 {
        host.tick(&InputFrame::default(), STEP);
    }
    let mut count = 0;
    host.each_npc(|_| count += 1);
    assert_eq!(count, 2, "the smith and one guard");
    let error = host
        .last_error()
        .map(|error| error.message.clone())
        .unwrap();
    assert!(
        error.contains("There is already an NPC called \"Guard\"")
            && error.contains("in game.onTick it would make one every tick")
            && error.contains("find it later with game.npc(\"Guard\")")
            && error.contains("game.npc { name = \"Guard 2\", at = \"Gate\" }"),
        "{error}"
    );

    // Twice in the file, in any capitals, says how to make a second or
    // many.
    let twice = load_error(
        r#"game.npc { name = "Guard", at = "Gate" }
        game.npc { name = "guard", at = "Tower" }"#,
    );
    assert!(
        twice.contains("There is already an NPC called \"guard\"")
            && twice.contains("\"guard 2\"")
            && twice.contains("for i = 1, 5 do"),
        "{twice}"
    );
    // The smith is placed in the map.
    let smith = load_error(r#"game.npc { name = "Smith", at = "Gate" }"#);
    assert!(
        smith.contains("called \"Smith\", placed in the map"),
        "{smith}"
    );

    // A crowd made in a loop, each numbered, is found one by one.
    let host = game(
        r#"for i = 1, 5 do game.npc { name = "Villager " .. i, at = "Gate" } end
        print(#game.npcs(), game.npc("Villager 3").name)"#,
    );
    assert_eq!(host.last_error(), None);
    assert_eq!(printed(), ["6 Villager 3"]);

    // A removed NPC's name is free again.
    let host = game(
        r#"game.npc { name = "Guard", at = "Gate" }:remove()
        print(game.npc { name = "Guard", at = "Gate" }.name)"#,
    );
    assert_eq!(host.last_error(), None);
    assert_eq!(printed(), ["Guard"]);
}

#[test]
fn two_npcs_placed_in_the_map_with_one_name_are_not_found_by_it() {
    listen();
    let mut both = placed();
    let mut other = both.last().unwrap().clone();
    other.id = "s2".to_owned();
    both.push(other);
    let host = ScriptHost::from_sources_with_placed(
        [(ENTRY_SCRIPT.to_owned(), r#"game.npc("Smith")"#.to_owned())],
        Vec::new(),
        ActionMap::standard(),
        Vec::new(),
        both,
    );
    let error = host
        .last_error()
        .map(|error| error.message.clone())
        .unwrap();
    assert!(
        error.contains("2 NPCs placed in the map are called \"Smith\"")
            && error.contains("\"Smith 2\""),
        "{error}"
    );
}

#[test]
fn an_npc_is_not_usable_once_it_is_dead_and_is_again_when_stood_up() {
    let mut host = game(
        r#"local guard = game.npc { name = "Guard", at = "Gate" }
        guard:onUse(function() end)
        local ticks = 0
        game.onTick(function()
            ticks += 1
            print(guard.usable, guard.dead)
            if ticks == 2 then guard:setHealth(50) end
        end)"#,
    );
    assert_eq!(host.last_error(), None);
    let mut ids = Vec::new();
    host.each_npc(|npc| ids.push(npc.id));
    host.tick(&InputFrame::default(), STEP);
    host.damage(ids[1], 500.0, None, "fall");
    host.tick(&InputFrame::default(), STEP);
    host.tick(&InputFrame::default(), STEP);
    assert_eq!(printed(), ["true false", "false true", "true false"]);
}

/// An NPC as a test compares them: id, name, feet, facing and health.
type Seen = (CharacterId, String, [f32; 3], f32, Option<(f32, f32)>);

#[test]
fn a_reloaded_script_leaves_the_same_npcs_as_a_fresh_start() {
    let before = r#"game.npc { name = "Guard", at = "Gate" }
        game.npc { name = "Cook", at = "Gate" }
        game.npc { name = "Baker", at = "Tower" }"#;
    // The guard moved, the cook's line gone, one more at the end.
    let after = r#"game.npc { name = "Guard", at = "Tower", facing = 90 }
        game.npc { name = "Baker", at = "Tower" }
        game.npc { name = "Miller", at = "Gate" }"#;
    let seen = |host: &ScriptHost| {
        let mut seen = Vec::new();
        host.each_npc(|npc| {
            let name = host.character_name(npc.id).unwrap();
            seen.push((npc.id, name, npc.feet, npc.facing, host.health(npc.id)));
        });
        seen
    };
    let saved = GameFolder::new(before);
    let mut host = saved.open();
    saved.write(after);
    host.reload().unwrap();
    let fresh = GameFolder::new(after);
    let fresh = fresh.open();
    let (reloaded, fresh) = (seen(&host), seen(&fresh));
    // The same people, in the same order, in the same places, as well.
    let without_ids = |seen: &[Seen]| {
        seen.iter()
            .map(|(_, name, feet, facing, health)| (name.clone(), *feet, *facing, *health))
            .collect::<Vec<_>>()
    };
    assert_eq!(without_ids(&reloaded), without_ids(&fresh));
    // Their ids go up in that order in both.
    for seen in [&reloaded, &fresh] {
        assert!(
            seen.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "{seen:?}"
        );
    }
}
