//! Things placed in the level, as a script finds and uses them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_input::{ActionMap, InputFrame};
use lore_script::{DEFAULT_REACH, Placed, PlacedValue, ScriptHost, Usable, UseSaid};

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
    host.each_usable(|meant, _, said| {
        if let Usable::Thing(id) = meant {
            seen.push((id.to_owned(), said.clone()));
        }
    });
    seen
}

#[test]
fn a_thing_is_found_by_name_at_the_top_of_the_script_and_read() {
    let host = game(
        r#"local door = game.thing("Cellar door")
        assert(door.name == "Cellar door" and door.id == "a1" and door.kind == "door")
        assert(door.position == vector.create(0, 0, -2) and door.facing == 90)
        assert(door:property("locked") == true and door:property("gold") == 12)
        assert(door:property("owner") == "Mira" and door:property("colour") == nil)
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
        chest:onUse(function() end)
        -- Words alone do not make a thing usable: it needs a function.
        local other = game.things("chest")[2]
        other.useText = "Open"
        assert(not other.usable)
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
                handled: true,
            }
        )
    );
    assert_eq!((now[1].1.seconds, now[1].1.reach), (1.5, 4.0));
    assert_eq!(host.use_prompt(), None, "nothing is under the crosshair");

    // The engine says which thing the player means, and the prompt follows.
    host.set_use_target(Some(Usable::Thing("b2")), 0.25);
    assert_eq!(host.use_prompt(), Some(("Use".to_owned(), 0.25)));
    host.set_use_target(Some(Usable::Thing("a1")), 0.0);
    assert_eq!(host.use_prompt(), Some(("Open".to_owned(), 0.0)));
    host.use_thing("a1");
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    host.use_thing("a1");
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    assert_eq!(host.last_error(), None);
    // Switched off by its own function: no longer usable, no prompt.
    assert_eq!(usable(&host).len(), 1);
    assert_eq!(host.use_prompt(), None);
    // A thing with no function is not usable, and using it is no mistake.
    host.use_thing("c3");
    host.use_thing("nothing");
    assert_eq!(host.last_error(), None);
}

#[test]
fn a_game_can_draw_its_own_prompt() {
    let mut host =
        game("game.showUsePrompt(false)\ngame.thing(\"Old chest\"):onUse(function() end)");
    host.set_use_target(Some(Usable::Thing("b2")), 0.0);
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
    assert!(
        message.contains("change them with `lore edit`"),
        "{message}"
    );

    let message = said("print(game.thing(\"a1\").colour)");
    assert!(
        message.contains("A thing.colour does not exist"),
        "{message}"
    );

    let message = said("game.thing(\"a1\").useSeconds = \"long\"");
    assert!(message.contains("`useSeconds` needs a number"), "{message}");

    let message = said("game.showUsePrompt(0)");
    assert!(message.contains("game.showUsePrompt(false)"), "{message}");

    // With nothing placed, the way to place something is said.
    let empty = ScriptHost::from_source("game.thing(\"Door\")", ActionMap::standard());
    let message = &empty.last_error().unwrap().message;
    assert!(
        message.contains("Nothing is placed at all yet"),
        "{message}"
    );
}

fn zones() -> Vec<Placed> {
    let zone =
        |id: &str, kind: &str, name: &str, at: [f32; 3], properties: &[(&str, f64)]| Placed {
            id: id.to_owned(),
            kind: kind.to_owned(),
            name: name.to_owned(),
            position: at,
            facing: 0.0,
            properties: properties
                .iter()
                .map(|(name, number)| ((*name).to_owned(), PlacedValue::Number(*number)))
                .collect(),
        };
    vec![
        zone("z1", "zone", "Bridge", [10.0, 0.0, 0.0], &[]),
        zone(
            "z2",
            "zone",
            "Hall",
            [0.0, 0.0, -20.0],
            &[("width", 4.0), ("depth", 10.0), ("height", 5.0)],
        ),
        zone("d1", "door", "Gate", [30.0, 0.0, 0.0], &[("radius", 1.0)]),
    ]
}

#[test]
fn a_zones_shape_is_a_round_or_a_box_from_the_map() {
    let all = zones();
    let (bridge, hall) = (&all[0], &all[1]);
    // Two metres round, three high, and half a metre under.
    assert!(bridge.holds([11.9, 0.0, 0.0]) && !bridge.holds([12.1, 0.0, 0.0]));
    assert!(bridge.holds([10.0, 2.9, 0.0]) && !bridge.holds([10.0, 3.1, 0.0]));
    assert!(bridge.holds([10.0, -0.4, 0.0]) && !bridge.holds([10.0, -0.6, 0.0]));
    assert!(!bridge.holds([f32::NAN, 0.0, 0.0]));
    // Four wide (x) and ten deep (z).
    assert!(hall.holds([1.9, 0.0, -24.9]) && !hall.holds([2.1, 0.0, -20.0]));
    assert!(!hall.holds([0.0, 0.0, -25.1]) && hall.holds([0.0, 4.9, -20.0]));
    // Turned a quarter, it is ten wide and four deep.
    let turned = Placed {
        facing: 90.0,
        ..hall.clone()
    };
    assert!(turned.holds([4.9, 0.0, -20.0]) && !turned.holds([0.0, 0.0, -22.1]));
}

#[test]
fn a_zone_tells_the_script_who_walked_in_and_who_left_once_each() {
    listen();
    let mut host = ScriptHost::from_sources_with_placed(
        [(
            "main.luau".to_owned(),
            r#"local bridge = game.zone("Bridge")
            assert(bridge == game.thing("Bridge") and game.zone("z1") == bridge)
            bridge:onEnter(function(who) print("in", who.name, bridge:has(who)) end)
            bridge:onLeave(function(who) print("out", who.name, bridge:has(who)) end)
            -- Anything placed can be a zone.
            game.thing("Gate"):onEnter(function(who) print("at the gate", who.name) end)"#
                .to_owned(),
        )],
        Vec::new(),
        ActionMap::standard(),
        Vec::new(),
        zones(),
    );
    assert_eq!(host.last_error(), None);
    let player = lore_script::CharacterId::PLAYER;
    let guard = host.add_character("Guard");
    host.set_position(guard, [10.5, 0.0, 0.5]);
    let step = |host: &mut ScriptHost| {
        host.tick(&InputFrame::default(), 1.0 / 60.0);
        host.step_zones();
    };
    // The guard starts on the bridge; the player is at the origin.
    step(&mut host);
    assert_eq!(printed(), ["in Guard true"]);
    // Nothing more while nobody crosses.
    step(&mut host);
    step(&mut host);
    assert_eq!(printed(), Vec::<String>::new());
    // The player walks on, then both leave: out is said before in.
    host.set_position(player, [9.0, 0.0, 0.0]);
    step(&mut host);
    assert_eq!(printed(), ["in Player true"]);
    host.set_position(player, [30.0, 0.0, 0.0]);
    host.set_position(guard, [50.0, 0.0, 0.0]);
    step(&mut host);
    assert_eq!(
        printed(),
        ["out Player false", "out Guard false", "at the gate Player"]
    );
    // Someone taken out of the world while inside is not told of.
    host.set_position(guard, [10.0, 0.0, 0.0]);
    step(&mut host);
    assert_eq!(printed(), ["in Guard true"]);
    host.remove_character(guard);
    step(&mut host);
    assert_eq!(host.last_error(), None);
    assert_eq!(printed(), Vec::<String>::new());

    // `lore check` says the player crossed each watched zone.
    assert_eq!(host.pretend_zone_crossings(), 2);
    assert_eq!(
        printed(),
        ["in Player false", "out Player false", "at the gate Player"]
    );
}

#[test]
fn a_zone_the_map_lacks_is_an_error_that_lists_the_zones() {
    let said = |script: &str| {
        ScriptHost::from_sources_with_placed(
            [("main.luau".to_owned(), script.to_owned())],
            Vec::new(),
            ActionMap::standard(),
            Vec::new(),
            zones(),
        )
        .last_error()
        .unwrap()
        .message
        .clone()
    };
    let message = said("game.zone(\"Brige\")");
    assert!(message.contains("No zone called \"Brige\""), "{message}");
    assert!(message.contains("Did you mean \"Bridge\"?"), "{message}");
    assert!(
        message.contains("The zones are \"Bridge\", \"Hall\"."),
        "{message}"
    );

    let message = said("game.zone(\"Gate\")");
    assert!(message.contains("was not placed as a zone"), "{message}");

    let message = said("game.zone(\"Bridge\"):onEnter(3)");
    assert!(
        message.contains("needs the function to call with whoever"),
        "{message}"
    );

    let message = said("game.zone(\"Bridge\"):has(\"me\")");
    assert!(message.contains("bridge:has(game.player)"), "{message}");

    let empty = ScriptHost::from_source("game.zone(\"Bridge\")", ActionMap::standard());
    let message = &empty.last_error().unwrap().message;
    assert!(message.contains("The map has no zones yet"), "{message}");
}

#[test]
fn what_the_map_says_moves_is_known_from_the_scripts_first_line() {
    let thing = |id: &str, properties: Vec<(&str, PlacedValue)>| Placed {
        id: id.to_owned(),
        kind: "prop".to_owned(),
        name: id.to_owned(),
        position: [1.0, 0.0, 2.0],
        facing: 0.0,
        properties: properties
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    };
    let text = |words: &str| PlacedValue::Text(words.to_owned());
    let mut host = ScriptHost::from_sources_with_placed(
        [(
            "main.luau".to_owned(),
            r#"local crate, heavy, fixed, marker =
                game.thing("crate"), game.thing("heavy"), game.thing("fixed"), game.thing("marker")
            assert(crate.moves and crate.weight == 20 and crate.resting)
            assert(crate.velocity == vector.create(0, 0, 0))
            assert(heavy.moves and heavy.weight == 300)
            assert(not fixed.moves and fixed.weight == 0 and fixed.resting)
            assert(not marker.moves and marker.weight == 0)
            game.onTick(function() crate:push(vector.create(0, 0, -100)) end)"#
                .to_owned(),
        )],
        Vec::new(),
        ActionMap::standard(),
        Vec::new(),
        vec![
            thing(
                "crate",
                vec![("model", text("crate")), ("moves", PlacedValue::Flag(true))],
            ),
            thing(
                "heavy",
                vec![
                    ("model", text("barrel")),
                    ("moves", PlacedValue::Flag(true)),
                    ("weight", PlacedValue::Number(300.0)),
                ],
            ),
            thing("fixed", vec![("model", text("crate"))]),
            thing("marker", Vec::new()),
        ],
    );
    assert_eq!(host.last_error(), None);
    host.tick(&InputFrame::default(), 1.0 / 60.0);
    assert_eq!(host.last_error(), None);
    assert_eq!(
        host.take_thing_asks(),
        [lore_script::ThingAsk::Push {
            id: "crate".to_owned(),
            impulse: [0.0, 0.0, -100.0],
        }]
    );
}
