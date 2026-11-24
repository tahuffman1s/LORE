//! Things placed in the level, as a script finds and uses them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_input::{ActionMap, InputFrame};
use lore_script::{DEFAULT_REACH, Placed, PlacedValue, ScriptHost, UseSaid};

fn placed() -> Vec<Placed> {
    let thing = |id: &str, kind: &str, name: &str, x: f32| Placed {
        id: id.to_owned(),
        kind: kind.to_owned(),
        name: name.to_owned(),
        position: [x, 0.0, -2.0],
        facing: 90.0,
        properties: vec![
            ("locked".to_owned(), PlacedValue::Flag(true)),
            ("gold".to_owned(), PlacedValue::Number(12.0)),
            ("owner".to_owned(), PlacedValue::Text("Mira".to_owned())),
        ],
    };
    vec![
        thing("a1", "door", "Cellar door", 0.0),
        thing("b2", "chest", "Old chest", 3.0),
        thing("c3", "chest", "", 6.0),
    ]
}

fn game(script: &str) -> ScriptHost {
    ScriptHost::from_sources_with_placed(
        [("main.luau".to_owned(), script.to_owned())],
        Vec::new(),
        ActionMap::standard(),
        Vec::new(),
        placed(),
    )
}

fn usable(host: &ScriptHost) -> Vec<(String, UseSaid)> {
    let mut seen = Vec::new();
    host.each_usable(|id, _, said| seen.push((id.to_owned(), said.clone())));
    seen
}

#[test]
fn a_thing_is_found_by_name_at_the_top_of_the_script_and_read() {
    let host = game(
        r#"local door = game.thing("Cellar door")
        assert(door.name == "Cellar door" and door.id == "a1" and door.kind == "door")
        assert(door.position == vector.create(0, 0, -2) and door.facing == 90)
        assert(door:get("locked") == true and door:get("gold") == 12)
        assert(door:get("owner") == "Mira" and door:get("colour") == nil)
        -- By id, and whatever the capitals; the same thing is the same value.
        assert(game.thing("a1") == door and game.thing("cellar DOOR") == door)
        assert(tostring(door) == "Cellar door")
        assert(#game.things() == 3 and #game.things("chest") == 2 and #game.things("lamp") == 0)
        assert(game.things("chest")[2].name == "" and tostring(game.things("chest")[2]) == "c3")
        assert(not door.usable and door.useText == "Use" and door.useReach == 2.5)"#,
    );
    assert_eq!(host.last_error(), None);
    assert!(usable(&host).is_empty(), "nothing was made usable");
}

#[test]
fn one_line_makes_a_thing_usable_and_using_it_calls_the_function() {
    let mut host = game(
        r#"local door = game.thing("Cellar door")
        local opened = 0
        door.useText = "Open"
        door:onUse(function(used)
            opened += 1
            assert(used == door)
            if opened == 2 then door.usable = false end
        end)
        local chest = game.thing("Old chest")
        chest.useSeconds = 1.5
        chest.useReach = 4
        game.onTick(function()
            local meant = game.player.lookingAt
            print(opened, meant and meant.name or "nothing", game.player.useProgress)
        end)"#,
    );
    assert_eq!(host.last_error(), None);
    let now = usable(&host);
    assert_eq!(now.len(), 2);
    assert_eq!(
        now[0],
        (
            "a1".to_owned(),
            UseSaid {
                text: "Open".to_owned(),
                reach: DEFAULT_REACH,
                seconds: 0.0,
                usable: true,
            }
        )
    );
    assert_eq!((now[1].1.seconds, now[1].1.reach), (1.5, 4.0));
    assert_eq!(host.use_prompt(), None, "nothing is under the crosshair");

    // The engine says which thing the player means, and the prompt follows.
    host.set_use_target(Some("b2"), 0.25);
    assert_eq!(host.use_prompt(), Some(("Use".to_owned(), 0.25)));
    host.set_use_target(Some("a1"), 0.0);
    assert_eq!(host.use_prompt(), Some(("Open".to_owned(), 0.0)));
    host.use_thing("a1");
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    host.use_thing("a1");
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    assert_eq!(host.last_error(), None);
    // Switched off by its own function: no longer usable, no prompt.
    assert_eq!(usable(&host).len(), 1);
    assert_eq!(host.use_prompt(), None);
    // A thing with no function is used without a word.
    host.use_thing("c3");
    host.use_thing("nothing");
    assert_eq!(host.last_error(), None);
}

#[test]
fn a_game_can_draw_its_own_prompt() {
    let mut host = game("game.usePrompt(false)\ngame.thing(\"Old chest\"):onUse(function() end)");
    host.set_use_target(Some("b2"), 0.0);
    assert_eq!(host.use_prompt(), None);
    assert_eq!(usable(&host).len(), 1);
}

#[test]
fn a_thing_taken_out_of_the_map_says_so_when_it_is_next_read() {
    let mut host = game(
        r#"local door = game.thing("Cellar door")
        door:onUse(function() end)
        game.onTick(function() print(door.name) end)"#,
    );
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    assert_eq!(host.last_error(), None);
    let mut fewer = placed();
    fewer.remove(0);
    host.set_placed(fewer);
    assert!(usable(&host).is_empty());
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    let message = &host.last_error().unwrap().message;
    assert!(message.contains("no longer placed in the map"), "{message}");
}

#[test]
fn mistakes_with_things_say_what_to_write() {
    let said = |script: &str| game(script).last_error().unwrap().message.clone();

    let message = said("game.thing(\"Celar door\")");
    assert!(
        message.contains("Nothing called \"Celar door\" is placed"),
        "{message}"
    );
    assert!(
        message.contains("Did you mean \"Cellar door\"?"),
        "{message}"
    );
    assert!(message.contains("\"Old chest\", \"c3\""), "{message}");

    let message = said("game.thing(3)");
    assert!(message.contains("game.thing(\"Cellar door\")"), "{message}");

    let message = said("game.things(true)");
    assert!(message.contains("game.things(\"door\")"), "{message}");

    let message = said("game.thing(\"a1\").onUse(function() end)");
    assert!(message.contains("needs a colon before it"), "{message}");

    let message = said("game.thing(\"a1\"):onUse(\"open\")");
    assert!(message.contains("needs the function to call"), "{message}");

    let message = said("game.thing(\"a1\").useTxt = \"Open\"");
    assert!(message.contains("Did you mean useText?"), "{message}");

    let message = said("game.thing(\"a1\").position = vector.create(0, 0, 0)");
    assert!(message.contains("move it with `lore edit`"), "{message}");

    let message = said("print(game.thing(\"a1\").colour)");
    assert!(
        message.contains("A thing.colour does not exist"),
        "{message}"
    );

    let message = said("game.thing(\"a1\").useSeconds = \"long\"");
    assert!(message.contains("`useSeconds` needs a number"), "{message}");

    let message = said("game.usePrompt(0)");
    assert!(message.contains("game.usePrompt(false)"), "{message}");

    // With nothing placed, the way to place something is said.
    let empty = ScriptHost::from_source("game.thing(\"Door\")", ActionMap::standard());
    let message = &empty.last_error().unwrap().message;
    assert!(
        message.contains("Nothing is placed at all yet"),
        "{message}"
    );
}
