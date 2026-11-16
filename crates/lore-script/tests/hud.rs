//! `game.hud`: the pieces a script makes are the pieces the engine is
//! handed, a change is handed over once, and a mistake is explained.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_input::{ActionMap, InputFrame};
use lore_script::{HudPiece, HudShape, ScriptHost};
use lore_ui::{Anchor, Color};

fn host(script: &str) -> ScriptHost {
    ScriptHost::from_source(script, ActionMap::default_fps())
}

/// A game running `script`, which has no mistake in it.
fn running(script: &str) -> ScriptHost {
    let host = host(script);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

fn tick(host: &mut ScriptHost) {
    host.tick(&InputFrame::default(), 1.0 / 60.0);
}

/// Every piece of the HUD, in order.
fn pieces(host: &ScriptHost) -> Vec<HudPiece> {
    let mut pieces = Vec::new();
    host.each_hud_piece(|place, piece| {
        assert_eq!(place, pieces.len());
        pieces.push(piece.clone());
    });
    pieces
}

/// The places of the pieces changed since this was last asked.
fn changed(host: &mut ScriptHost) -> Vec<usize> {
    let mut places = Vec::new();
    host.take_hud_changes(|place, _| places.push(place));
    places
}

/// The message of the mistake on the first line of `script`.
fn mistake(script: &str) -> String {
    let host = host(script);
    let error = host.last_error().expect("the script has a mistake");
    assert_eq!(error.line, Some(1), "{error}");
    assert_eq!(pieces(&host), [], "a mistake makes no piece");
    error.message.clone()
}

fn text(piece: &HudPiece) -> &str {
    match &piece.shape {
        HudShape::Text { text, .. } => text,
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn pieces_are_handed_over_as_written_in_the_order_they_were_made() {
    let host = running(
        r##"game.hud.text("Hello")
        game.hud.text(30, { at = "bottom right", right = -20, down = -10, size = 48, color = "#ff8800" })
        game.hud.bar(0.5, { at = "bottom left", width = 400, height = 30, color = "red" })
        game.hud.bar(1)
        game.hud.box({ at = "center", width = 6, height = 6, color = "#00000080" })
        game.hud.box()"##,
    );
    let pieces = pieces(&host);
    assert_eq!(
        pieces[0],
        HudPiece {
            shape: HudShape::Text {
                text: "Hello".to_owned(),
                size: 28.0
            },
            anchor: Anchor::TopLeft,
            offset: [0.0, 0.0],
            color: Color::WHITE,
            visible: true,
        }
    );
    assert_eq!(
        pieces[1],
        HudPiece {
            shape: HudShape::Text {
                text: "30".to_owned(),
                size: 48.0
            },
            anchor: Anchor::BottomRight,
            offset: [-20.0, -10.0],
            color: Color::rgb(1.0, 136.0 / 255.0, 0.0),
            visible: true,
        }
    );
    assert_eq!(
        pieces[2].shape,
        HudShape::Bar {
            value: 0.5,
            size: [400.0, 30.0]
        }
    );
    assert_eq!(pieces[2].anchor, Anchor::BottomLeft);
    assert!(pieces[2].color.r > 0.8 && pieces[2].color.g < 0.4);
    assert_eq!(
        pieces[3].shape,
        HudShape::Bar {
            value: 1.0,
            size: [300.0, 24.0]
        }
    );
    assert_eq!(pieces[4].shape, HudShape::Box { size: [6.0, 6.0] });
    assert_eq!(pieces[4].anchor, Anchor::Center);
    assert_eq!(pieces[4].color, Color::rgba(0.0, 0.0, 0.0, 128.0 / 255.0));
    assert_eq!(
        pieces[5].shape,
        HudShape::Box {
            size: [100.0, 100.0]
        }
    );
    assert_eq!(pieces.len(), 6);
}

#[test]
fn every_place_and_colour_name_is_taken_however_it_is_cased() {
    let host = running(
        r##"for _, place in { "top left", "top", "top right", "left", "center", "right",
            "bottom left", "bottom", "bottom right", "Bottom-Right", "centre", " TOP_LEFT " } do
            game.hud.box({ at = place })
        end
        for _, color in { "white", "black", "gray", "grey", "red", "orange", "yellow",
            "green", "blue", "purple", "pink", "Red", "#FFF000" } do
            game.hud.box({ color = color })
        end"##,
    );
    let pieces = pieces(&host);
    let anchors: Vec<Anchor> = pieces[..12].iter().map(|piece| piece.anchor).collect();
    assert_eq!(
        anchors,
        [
            Anchor::TopLeft,
            Anchor::Top,
            Anchor::TopRight,
            Anchor::Left,
            Anchor::Center,
            Anchor::Right,
            Anchor::BottomLeft,
            Anchor::Bottom,
            Anchor::BottomRight,
            Anchor::BottomRight,
            Anchor::Center,
            Anchor::TopLeft,
        ]
    );
    assert_eq!(pieces[12].color, Color::WHITE);
    assert_eq!(pieces[13].color, Color::BLACK);
    assert_eq!(pieces[14].color, pieces[15].color, "gray and grey");
    assert_eq!(pieces[16].color, pieces[23].color, "red and Red");
    assert_eq!(pieces[24].color, Color::rgb(1.0, 240.0 / 255.0, 0.0));
}

#[test]
fn a_change_is_handed_over_once_and_only_when_something_changed() {
    let mut host = running(
        r#"local ammo = game.hud.text("30 / 90", { at = "bottom right" })
        local health = game.hud.bar(1)
        local rounds = 30
        game.onTick(function()
            if game.input.justPressed("jump") then rounds -= 1 end
            ammo.text = rounds .. " / 90"
            health.value = rounds / 30
            health.color = if rounds < 30 then "orange" else "white"
        end)"#,
    );
    let layout = host.hud_layout();
    assert_eq!(
        changed(&mut host),
        [0_usize; 0],
        "new pieces are not changes"
    );

    // Setting what a piece already has is no change, tick after tick.
    tick(&mut host);
    tick(&mut host);
    assert_eq!(changed(&mut host), [0_usize; 0]);

    let jump = host_jump();
    host.tick(&jump, 1.0 / 60.0);
    let mut seen = Vec::new();
    host.take_hud_changes(|place, piece| seen.push((place, piece.clone())));
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!((seen[0].0, text(&seen[0].1)), (0, "29 / 90"));
    assert_eq!(seen[1].0, 1);
    assert!(
        matches!(seen[1].1.shape, HudShape::Bar { value, .. } if (value - 29.0 / 30.0).abs() < 1e-6)
    );
    assert_ne!(seen[1].1.color, Color::WHITE);
    assert_eq!(changed(&mut host), [0_usize; 0], "handed over once");
    assert_eq!(host.hud_layout(), layout, "no piece was made or removed");
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

/// An input in which `jump` has just been pressed.
fn host_jump() -> InputFrame {
    let mut input = lore_input::Input::new(ActionMap::default_fps());
    input.press(lore_input::Key::Space);
    input.tick().clone()
}

#[test]
fn a_script_reads_back_what_it_wrote() {
    let mut host = running(
        r##"local ammo = game.hud.text("30 / 90", { color = "Red" })
        local health = game.hud.bar(7)
        local dot = game.hud.box()
        game.onStart(function()
            print(ammo.text, ammo.color, ammo.visible, health.value, health.color, dot.visible)
            print(tostring(ammo), tostring(health), tostring(dot))
            ammo.text = 12.5
            ammo.visible = false
            health.value = -3
            dot.color = "#11223344"
            assert(ammo.text == "12.5" and ammo.visible == false)
            assert(health.value == 0 and dot.color == "#11223344")
            assert(ammo.color == "Red")
        end)"##,
    );
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let pieces = pieces(&host);
    assert!(!pieces[0].visible);
    assert_eq!(
        pieces[1].shape,
        HudShape::Bar {
            value: 0.0,
            size: [300.0, 24.0]
        }
    );
    assert_eq!(changed(&mut host), [0, 1, 2]);
}

#[test]
fn a_removed_piece_is_gone_and_the_rest_keep_their_order() {
    let mut host = running(
        r#"local first = game.hud.text("first")
        local second = game.hud.text("second")
        local third = game.hud.text("third")
        game.onStart(function()
            second:remove()
            second:remove()
            third.text = "last"
        end)"#,
    );
    let before = host.hud_layout();
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_ne!(host.hud_layout(), before);
    let pieces = pieces(&host);
    assert_eq!(
        pieces.iter().map(text).collect::<Vec<_>>(),
        ["first", "last"]
    );
    // The change is told by the place the piece has now.
    assert_eq!(changed(&mut host), [1]);
}

#[test]
fn the_hud_can_be_changed_while_a_picture_is_drawn() {
    let mut host = running(
        r#"local health = game.hud.bar(0)
        local shown = game.tween(0, 1, 1)
        game.onDraw(function() health.value = shown.value end)"#,
    );
    tick(&mut host);
    tick(&mut host);
    let rules = host.rules_fingerprint();
    host.draw(0.5);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(changed(&mut host), [0]);
    let HudShape::Bar { value, .. } = pieces(&host)[0].shape else {
        panic!("a bar");
    };
    assert!((value - 1.5 / 60.0).abs() < 1e-6, "{value}");
    // The HUD is shown, and is no rule of the game.
    assert_eq!(host.rules_fingerprint(), rules);
}

#[test]
fn a_piece_made_on_every_tick_is_stopped_and_explained() {
    let mut host = running(r#"game.onTick(function() game.hud.text("Score: 0") end)"#);
    for _ in 0..300 {
        tick(&mut host);
    }
    let error = host.last_error().expect("too many pieces");
    assert!(
        error.message.contains("already has 256 pieces")
            && error.message.contains("at the top of the file"),
        "{error}"
    );
    assert_eq!(pieces(&host).len(), 256);
}

#[test]
fn long_text_is_cut_and_line_breaks_become_spaces() {
    let host = running(
        r#"game.hud.text("one\ntwo\tthree")
        game.hud.text(string.rep("é", 400))"#,
    );
    let pieces = pieces(&host);
    assert_eq!(text(&pieces[0]), "one two three");
    assert_eq!(text(&pieces[1]).chars().count(), 303);
    assert!(text(&pieces[1]).ends_with("é..."));
}

#[test]
fn mistakes_in_making_a_piece_say_what_to_write() {
    for (script, said) in [
        (
            "game.hud.text()",
            "game.hud.text needs the words to show, in quotes, or a number, but it was given nothing.",
        ),
        (
            r#"game.hud.text({ at = "top" })"#,
            "game.hud.text takes the words to show first and the options second.",
        ),
        (
            r#"game.hud.text("Hi", "top")"#,
            "game.hud.text takes its options in a table, but it was given text.",
        ),
        (
            r#"game.hud.text("Hi", { at = "bottom rihgt" })"#,
            "Did you mean \"bottom right\"?",
        ),
        (
            r#"game.hud.text("Hi", { at = "somewhere" })"#,
            "The places are: top left, top, top right, left, center, right, bottom left, bottom, bottom right.",
        ),
        (
            r#"game.hud.text("Hi", { at = 5 })"#,
            "needs a place on the screen in quotes for `at`, but it was given a number.",
        ),
        (
            r#"game.hud.text("Hi", { colour = "red" })"#,
            "game.hud.text has no option called `colour`. Did you mean `color`?",
        ),
        (
            r#"game.hud.text("Hi", { color = "redd" })"#,
            "Did you mean \"red\"?",
        ),
        (
            r#"game.hud.text("Hi", { color = "turquoise" })"#,
            "The colours are: white, black, gray, red, orange, yellow, green, blue, purple, pink. For any other, write a hex code such as \"#ff8800\".",
        ),
        (
            r##"game.hud.text("Hi", { color = "#12345" })"##,
            "is not a hex colour.",
        ),
        (
            r##"game.hud.text("Hi", { color = "#+12345" })"##,
            "is not a hex colour.",
        ),
        (
            r#"game.hud.text("Hi", { color = { 1, 0, 0 } })"#,
            "needs a colour in quotes, but it was given a table.",
        ),
        (
            r#"game.hud.text("Hi", { size = "big" })"#,
            "game.hud.text needs a number for `size`, the height of the text in pixels on a screen 1080 high, but it was given text.",
        ),
        (
            r#"game.hud.text("Hi", { width = 100 })"#,
            "game.hud.text has no `width` or `height`: text is as wide as its words.",
        ),
        (
            r#"game.hud.text("Hi", { down = 0 / 0 })"#,
            "but it was given a number that is infinite or not a number.",
        ),
        (
            r#"game.hud.bar("full")"#,
            "game.hud.bar needs a number from 0 (empty) to 1 (full), but it was given text.",
        ),
        (
            "game.hud.bar(0 / 0)",
            "was given something that is not a number",
        ),
        (
            "game.hud.bar({ width = 100 })",
            "game.hud.bar takes how full the bar is, from 0 to 1, first and the options second.",
        ),
        (
            "game.hud.bar(1, { size = 20 })",
            "game.hud.bar has no `size`: a HUD bar has a `width` and a `height`",
        ),
        (
            "game.hud.bar(1, { width = 0 })",
            "game.hud.bar needs a `width` above 0",
        ),
        (
            r#"game.hud.box("red")"#,
            "game.hud.box takes its options in a table, but it was given text.",
        ),
        (
            "game.hud.box({}, {})",
            "game.hud.box takes only its options, in one table.",
        ),
        (
            "game.hud.label(\"Hi\")",
            "game.hud.label does not exist. game.hud has: text, bar, box.",
        ),
        (
            "game.hud.txet(\"Hi\")",
            "game.hud.txet does not exist. Did you mean game.hud.text?",
        ),
    ] {
        let message = mistake(script);
        assert!(message.contains(said), "{script}\n{message}");
    }
}

#[test]
fn mistakes_in_changing_a_piece_say_what_to_write() {
    for (line, said) in [
        (
            "ammo.text = nil",
            "A HUD text's `text` needs the words to show, in quotes, or a number, but it was given nothing.",
        ),
        (
            "ammo.value = 1",
            "A HUD text has nothing called `value`. It has: text, color, visible, and ammo:remove() takes it off the screen. Only a bar has a value",
        ),
        (
            "ammo.txt = 1",
            "A HUD text has nothing called `txt`. Did you mean `text`?",
        ),
        (
            "ammo.size = 40",
            "Where a piece is and how big it is are chosen when it is made",
        ),
        (
            "print(ammo.colour)",
            "A HUD text has nothing called `colour`. Did you mean `color`?",
        ),
        (
            "ammo.removed = true",
            "Did you mean `remove`? Write it with a colon: ammo:remove()",
        ),
        (
            "ammo.visible = 1",
            "A HUD text's `visible` needs true or false, but it was given a number.",
        ),
        ("ammo.color = \"bleu\"", "Did you mean \"blue\"?"),
        (
            "health.text = \"Health\"",
            "A HUD bar has nothing called `text`. It has: value, color, visible, and health:remove() takes it off the screen. For words beside it, make a text",
        ),
        (
            "health.value = \"full\"",
            "A HUD bar's `value` needs a number from 0 (empty) to 1 (full), but it was given text.",
        ),
        ("ammo.remove()", "remove needs a colon, not a dot"),
        (
            "ammo:remove() ammo.text = \"gone\"",
            "This piece was taken off the HUD with :remove(), so there is nothing to read or change. To hide a HUD text",
        ),
        (
            "health:remove() print(health.value)",
            "To hide a HUD bar and show it again later, set `visible` instead, like this: health.visible = false.",
        ),
    ] {
        let script =
            format!("local ammo = game.hud.text(\"30\")\nlocal health = game.hud.bar(1)\n{line}");
        let host = host(&script);
        let error = host.last_error().expect("the script has a mistake");
        assert_eq!(error.line, Some(3), "{error}");
        assert!(error.message.contains(said), "{line}\n{}", error.message);
    }
}

#[test]
fn a_save_makes_the_hud_again_from_the_new_file() {
    let folder = std::env::temp_dir().join(format!("lore-script-hud-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("main.luau");
    std::fs::write(
        &file,
        "game.hud.text(\"old\")\ngame.onStart(function() game.hud.text(\"made later\") end)\n",
    )
    .unwrap();
    let mut host = ScriptHost::open(&folder, ActionMap::default_fps()).unwrap();
    tick(&mut host);
    assert_eq!(pieces(&host).len(), 2);
    let before = host.hud_layout();

    // A version with a mistake changes nothing: the old HUD stays up.
    std::fs::write(&file, "game.hud.text(\"new\")\ngame.hud.oops()\n").unwrap();
    assert!(host.reload().is_err());
    assert_eq!(host.hud_layout(), before);
    assert_eq!(text(&pieces(&host)[0]), "old");

    std::fs::write(&file, "game.hud.text(\"new\")\n").unwrap();
    host.reload().unwrap();
    assert_ne!(host.hud_layout(), before);
    let pieces = pieces(&host);
    assert_eq!(pieces.iter().map(text).collect::<Vec<_>>(), ["new"]);
    let _ = std::fs::remove_dir_all(&folder);
}
