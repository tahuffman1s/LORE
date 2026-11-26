//! `game.hud`: the pieces a script makes are the pieces the engine is
//! handed, a change is handed over once, and a mistake is explained.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_input::{ActionMap, InputFrame};
use lore_script::{HudPiece, HudShape, ScriptHost};
use lore_ui::{Anchor, Color};

fn host(script: &str) -> ScriptHost {
    ScriptHost::from_source(script, ActionMap::standard())
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
            id: 0,
            shape: HudShape::Text {
                text: "Hello".to_owned(),
                size: 28.0
            },
            anchor: Anchor::TopLeft,
            offset: [0.0, 0.0],
            color: Color::WHITE,
            visible: true,
            layer: 0,
            style: None,
        }
    );
    assert_eq!(
        pieces[1],
        HudPiece {
            id: 1,
            shape: HudShape::Text {
                text: "30".to_owned(),
                size: 48.0
            },
            anchor: Anchor::BottomRight,
            offset: [-20.0, -10.0],
            color: Color::rgb(1.0, 136.0 / 255.0, 0.0),
            visible: true,
            layer: 0,
            style: None,
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
    let mut input = lore_input::Input::new(ActionMap::standard());
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
            assert(dot.right == 0 and dot.down == 0)
            dot.right = 12
            dot.down = -1e9
            assert(dot.right == 12 and dot.down == -8000)
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
    assert_eq!(pieces[2].offset, [12.0, -8000.0]);
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
    for _ in 0..4200 {
        tick(&mut host);
    }
    let error = host.last_error().expect("too many pieces");
    assert!(
        error.message.contains("already has 4096 pieces")
            && error.message.contains("at the top of the file"),
        "{error}"
    );
    assert_eq!(pieces(&host).len(), 4096);
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
            "game.hud.label does not exist. game.hud has: text, bar, box, frameGraph, line, image, size, onScreen.",
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
            "A HUD text has nothing called `value`. It has: text, color, visible, layer, right, down, width, height, hovered, pressed, opacity, scale, rotation, transformOrigin, clipTo, fontFamily, fontWeight, fontStyle, letterSpacing, textShadow, ammo:pointerAt() says where the pointer is across it, and ammo:remove() takes it off the screen. Only a bar has a value",
        ),
        (
            "ammo.txt = 1",
            "A HUD text has nothing called `txt`. Did you mean `text`?",
        ),
        (
            "ammo.size = 40",
            "Where a piece is pinned and how big it is are chosen when it is made",
        ),
        (
            "ammo.right = \"far\"",
            "A HUD text's `right` needs a number, in pixels on a screen 1080 high, but it was given text. Write it like this: ammo.right = -40",
        ),
        (
            "health.down = 0 / 0",
            "A HUD bar's `down` needs a number, in pixels on a screen 1080 high, but it was given a number that is infinite or not a number.",
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
            "A HUD bar has nothing called `text`. It has: value, color, visible, layer, right, down, width, height, hovered, pressed, opacity, scale, rotation, transformOrigin, clipTo, health:pointerAt() says where the pointer is across it, and health:remove() takes it off the screen. For words beside it, make a text",
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
    let mut host = ScriptHost::open(&folder, ActionMap::standard()).unwrap();
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

#[test]
fn a_line_runs_between_its_two_ends_and_each_end_can_move() {
    let mut host = running(
        r#"
        local line = game.hud.line({ at = "center", right = -10, down = 5, toRight = 30, toDown = -20, thickness = 3, color = "red" })
        game.onTick(function()
            line.toRight = 50
            line.thickness = 1
            print(line.right, line.down, line.toRight, line.toDown, line.thickness)
        end)
        "#,
    );
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let piece = &pieces(&host)[0];
    assert_eq!(piece.offset, [-10.0, 5.0]);
    assert_eq!(
        piece.shape,
        HudShape::Line {
            to: [50.0, -20.0],
            thickness: 1.0
        }
    );
}

#[test]
fn a_box_can_be_resized_rounded_and_turned() {
    let mut host = running(
        r#"
        local card = game.hud.box({ width = 40, height = 20, corner = 6, rotation = 30 })
        game.onTick(function()
            card.width = 80
            card.rotation = 90
            print(card.width, card.height, card.corner, card.rotation)
        end)
        "#,
    );
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let piece = &pieces(&host)[0];
    assert_eq!(piece.shape, HudShape::Box { size: [80.0, 20.0] });
    let look = piece.style.as_ref().unwrap().look;
    assert_eq!(look.radii, [6.0; 4]);
    assert_eq!(look.rotation, 90.0);
}

#[test]
fn an_image_is_named_by_its_file_and_keeps_its_shape_when_one_side_is_said() {
    let folder = std::env::temp_dir().join(format!("lore-hud-images-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(folder.join("images/stars")).unwrap();
    // A PNG header is all the script reads: 200 by 100 pixels.
    let mut png = vec![
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13,
    ];
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&200_u32.to_be_bytes());
    png.extend_from_slice(&100_u32.to_be_bytes());
    std::fs::write(folder.join("images/stars/orion.png"), &png).unwrap();
    std::fs::write(folder.join("images/compass.png"), &png).unwrap();
    std::fs::write(
        folder.join("main.luau"),
        r#"
        local a = game.hud.image("stars/orion", { width = 50, rotation = 10 })
        local b = game.hud.image("compass")
        "#,
    )
    .unwrap();
    let host = ScriptHost::open(&folder, lore_input::ActionMap::standard()).unwrap();
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let names: Vec<&str> = host
        .hud_images()
        .iter()
        .map(|info| info.name.as_str())
        .collect();
    assert_eq!(names, ["compass", "stars/orion"]);
    let made = pieces(&host);
    assert_eq!(
        made[0].shape,
        HudShape::Image {
            image: 1,
            size: [50.0, 25.0],
        }
    );
    assert_eq!(made[0].style.as_ref().unwrap().look.rotation, 10.0);
    assert_eq!(
        made[1].shape,
        HudShape::Image {
            image: 0,
            size: [200.0, 100.0],
        }
    );

    std::fs::write(folder.join("main.luau"), r#"game.hud.image("stars/orian")"#).unwrap();
    let host = ScriptHost::open(&folder, lore_input::ActionMap::standard()).unwrap();
    let error = host.last_error().expect("no such image");
    assert!(
        error.message.contains("no image called \"stars/orian\"")
            && error.message.contains("Did you mean \"stars/orion\"?"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_text_says_how_big_its_words_are_and_that_cannot_be_set() {
    let mut host = running(
        r#"
        local short = game.hud.text("Hi", { size = 20 })
        local long = game.hud.text("Hello, world", { size = 20 })
        local tall = game.hud.text("Hi", { size = 40 })
        game.onTick(function()
            assert(long.width > short.width * 2, "longer words are wider")
            assert(tall.height > short.height * 1.5, "bigger words are taller")
            assert(short.width > 0)
        end)
        "#,
    );
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let error = mistake(r#"local t = game.hud.text("Hi"); t.width = 5"#);
    assert!(error.contains("cannot be set"), "{error}");
}

#[test]
fn the_hud_says_how_much_room_it_has() {
    let host = host(
        r#"
        local width, height = game.hud.size()
        print(width, height)
        assert(width > height and height > 900 and height < 1080, "a wide screen inside its margin")
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

// ---- Styles ----------------------------------------------------------

/// What `say` collects: the script's first piece is a text it writes to.
const SAY: &str = r##"
local __out = game.hud.text("")
local function say(...)
    local parts = {}
    for i = 1, select("#", ...) do parts[i] = tostring((select(i, ...))) end
    local line = table.concat(parts, ",")
    __out.text = __out.text == "" and line or (__out.text .. "|" .. line)
end
"##;

/// A game running `script` after `SAY`, which has no mistake in it.
fn saying(script: &str) -> ScriptHost {
    running(&format!("{SAY}{script}"))
}

/// What the script said so far, one entry for each `say`.
fn said(host: &ScriptHost) -> Vec<String> {
    text(&pieces(host)[0])
        .split('|')
        .map(str::to_owned)
        .collect()
}

/// Runs `script` for one tick and gives what it said.
fn printed(script: &str) -> Vec<String> {
    let mut host = saying(script);
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    said(&host)
}

/// The mistake `script` makes on its first tick, or while it loads.
fn fail_with(script: &str) -> String {
    let mut host = host(script);
    tick(&mut host);
    host.last_error().expect("a mistake").to_string()
}

/// A game on disk with a font in its `fonts` folder, running `script`.
fn with_fonts(script: &str) -> ScriptHost {
    let folder = std::env::temp_dir().join(format!(
        "lore-hud-fonts-{}-{}",
        std::process::id(),
        script.len()
    ));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(folder.join("fonts")).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../lore-render/fonts/AtkinsonHyperlegibleNext-Regular.ttf"
        ),
        folder.join("fonts/atkinson.ttf"),
    )
    .unwrap();
    std::fs::write(folder.join("main.luau"), format!("{SAY}{script}")).unwrap();
    ScriptHost::open(&folder, ActionMap::standard()).unwrap()
}

#[test]
fn a_plain_piece_has_no_style_and_every_style_field_has_its_default() {
    let mut host = saying(
        r#"
        local ammo = game.hud.text("30")
        game.onTick(function()
            say(ammo.opacity, ammo.scale, ammo.rotation, ammo.transformOrigin)
            say(ammo.fontFamily, ammo.fontWeight, ammo.fontStyle, ammo.letterSpacing, ammo.textShadow)
            say(ammo.clipTo)
        end)
        "#,
    );
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert!(pieces(&host)[1].style.is_none());
    assert_eq!(said(&host), ["1,1,0,center", "nil,400,normal,0,nil", "nil"]);
}

#[test]
fn every_style_field_is_set_when_made_and_later_and_read_back() {
    let log = printed(
        r##"
        local box = game.hud.box({
            opacity = 0.5, scale = 1.5, rotation = 30, transformOrigin = "top left",
            borderRadius = { 1, 2, 3, 4 }, borderWidth = 2, borderColor = "red",
            gradient = { colors = { "#3a2a10", "#120c04" }, angle = 90, stops = { 0, 0.5 } },
            slice = { 1, 2, 3, 4 },
            boxShadow = { right = 1, down = 4, blur = 12, spread = 2, color = "#000a", inset = true },
        })
        say(box.opacity, box.scale, box.rotation, box.transformOrigin)
        say(#box.borderRadius, box.borderRadius[3], box.borderWidth, box.borderColor)
        say(box.gradient.colors[1], box.gradient.angle, box.gradient.stops[2], box.gradient.radial)
        say(box.slice[4], box.boxShadow.down, box.boxShadow.color, box.boxShadow.inset)
        box.opacity = 0.25
        box.borderRadius = 6
        box.borderWidth = { 1, 2, 3, 4 }
        box.gradient = { colors = { "red", "blue", "green" }, radial = true, center = { 0.25, 0.75 } }
        box.boxShadow = nil
        say(box.opacity, box.borderRadius, box.corner, box.borderWidth[2])
        say(#box.gradient.colors, box.gradient.radial, box.gradient.center[2], box.gradient.stops[2], box.boxShadow)
        local text = game.hud.text("Hi", {
            fontWeight = "bold", fontStyle = "italic", letterSpacing = -1,
            textShadow = { right = 2, down = 3, blur = 4, color = "black" },
        })
        say(text.fontWeight, text.fontStyle, text.letterSpacing, text.textShadow.down, text.textShadow.color)
        text.fontWeight = 300
        say(text.fontWeight)
        "##,
    );
    assert_eq!(log[0], "0.5,1.5,30,top left");
    assert_eq!(log[1], "4,3,2,red");
    assert_eq!(log[2], "#3a2a10,90,0.5,nil");
    assert_eq!(log[3], "4,4,#000a,true");
    assert_eq!(log[4], "0.25,6,6,2");
    assert_eq!(log[5], "3,true,0.75,0.5,nil");
    assert_eq!(log[6], "700,italic,-1,3,black");
    assert_eq!(log[7], "300");
}

#[test]
fn nil_puts_a_style_field_back_to_its_default() {
    let mut host = saying(
        r#"
        local box = game.hud.box({ opacity = 0.5, scale = 2, rotation = 10, borderWidth = 3,
            borderColor = "red", transformOrigin = "bottom right", boxShadow = { blur = 4 } })
        game.onTick(function()
            box.opacity = nil box.scale = nil box.rotation = nil box.borderWidth = nil
            box.borderColor = nil box.transformOrigin = nil box.boxShadow = nil
        end)
        "#,
    );
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let look = pieces(&host)[1].style.as_ref().unwrap().look;
    assert_eq!(look, lore_ui::HudStyle::default());
}

#[test]
fn a_style_change_marks_the_piece_changed_once() {
    let mut host = running(
        r#"
        local ammo = game.hud.text("30")
        game.onTick(function() ammo.opacity = 0.5 end)
        "#,
    );
    host.take_hud_changes(|_, _| {});
    tick(&mut host);
    let mut changed = 0;
    host.take_hud_changes(|_, _| changed += 1);
    assert_eq!(changed, 1);
    tick(&mut host);
    host.take_hud_changes(|_, _| changed += 1);
    assert_eq!(changed, 1, "the same value is no change");
}

#[test]
fn corner_is_borderradius() {
    let log = printed(
        r#"
        local a = game.hud.box({ corner = 5 })
        local b = game.hud.box({ borderRadius = 7 })
        a.borderRadius = 9
        b.corner = 3
        say(a.corner, a.borderRadius, b.corner, b.borderRadius)
        "#,
    );
    assert_eq!(log[0], "9,9,3,3");
}

#[test]
fn a_piece_is_cut_to_a_box_and_clip_reads_back_the_box() {
    let log = printed(
        r#"
        local panel = game.hud.box({ width = 100, height = 50 })
        local label = game.hud.text("Hi", { clipTo = panel })
        say(label.clipTo ~= nil, label.clipTo.width)
        label.clipTo = nil
        say(label.clipTo)
        "#,
    );
    assert_eq!(log, ["true,100", "nil"]);
}

#[test]
fn style_mistakes_name_the_nearest_field_and_what_to_write() {
    let error = fail_with(r#"local t = game.hud.text("x") t.opacty = 1"#);
    assert!(error.contains("`opacity`"), "{error}");
    let error = fail_with(r#"game.hud.box({ boxShadows = {} })"#);
    assert!(error.contains("`boxShadow`"), "{error}");
    let error = fail_with(r#"local t = game.hud.text("x") t.opacity = "half""#);
    assert!(
        error.contains("needs a number") && error.contains("ammo.opacity = 0.5"),
        "{error}"
    );
    let error = fail_with(r#"local b = game.hud.box() b.borderWidth = { 1, 2 }"#);
    assert!(error.contains("list of 4"), "{error}");
    let error = fail_with(r#"local b = game.hud.box() b.gradient = { colors = { "red" } }"#);
    assert!(error.contains("2 to 8 colours"), "{error}");
    let error = fail_with(r#"local b = game.hud.box() b.gradient = { colour = {} }"#);
    assert!(error.contains("`colors`"), "{error}");
    let error = fail_with(r#"local b = game.hud.box() b.boxShadow = { blurr = 2 }"#);
    assert!(error.contains("`blur`"), "{error}");
    let error = fail_with(r#"local b = game.hud.box() b.transformOrigin = "upper left""#);
    assert!(error.contains("place"), "{error}");
    let error = fail_with(r#"local t = game.hud.text("x") t.fontWeight = "heavy""#);
    assert!(error.contains("\"bold\""), "{error}");
    let error = fail_with(r#"local t = game.hud.text("x") t.fontStyle = "slanted""#);
    assert!(error.contains("\"italic\""), "{error}");
    let error = fail_with(r#"local t = game.hud.text("x") t.borderWidth = 1"#);
    assert!(error.contains("nothing called `borderWidth`"), "{error}");
}

#[test]
fn a_bad_clip_target_is_explained() {
    let error = fail_with(r#"local t = game.hud.text("x") t.clipTo = t"#);
    assert!(error.contains("itself"), "{error}");
    let error =
        fail_with(r#"local a = game.hud.text("x") local t = game.hud.text("y", { clipTo = a })"#);
    assert!(error.contains("not a box or image"), "{error}");
    let error = fail_with(
        r#"local a = game.hud.box() a:remove() local t = game.hud.text("y") t.clipTo = a"#,
    );
    assert!(error.contains("remove"), "{error}");
}

#[test]
fn an_unknown_font_family_names_the_fonts_folder_and_the_closest_family() {
    let error = fail_with(r#"local t = game.hud.text("x") t.fontFamily = "Cinzel""#);
    assert!(
        error.contains("no font family called \"Cinzel\"") && error.contains("folder called fonts"),
        "{error}"
    );
    let host =
        with_fonts(r#"local t = game.hud.text("x", { fontFamily = "atkinson hyperlegible nex" })"#);
    let error = host.last_error().expect("unknown family").to_string();
    assert!(
        error.contains("Did you mean \"Atkinson Hyperlegible Next\"?"),
        "{error}"
    );
}

#[test]
fn a_known_font_family_is_found_whatever_its_capitals_and_reads_back_as_the_folder_has_it() {
    let mut host = with_fonts(
        r#"
        local t = game.hud.text("x", { fontFamily = "ATKINSON hyperlegible next" })
        game.onTick(function() say(t.fontFamily) t.fontFamily = nil say(t.fontFamily) end)
        "#,
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(pieces(&host)[1].style.as_ref().unwrap().look.font.family, 1);
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(said(&host), ["Atkinson Hyperlegible Next", "nil"]);
}

// ---- Animation -------------------------------------------------------

fn bar_value(host: &ScriptHost, place: usize) -> f32 {
    match &pieces(host)[place].shape {
        HudShape::Bar { value, .. } => *value,
        other => panic!("not a bar: {other:?}"),
    }
}

fn ticks(host: &mut ScriptHost, count: usize) {
    for _ in 0..count {
        tick(host);
    }
}

#[test]
fn half_a_second_of_ticks_lands_exactly() {
    let mut host = running(
        r#"local bar = game.hud.bar(0)
        game.onStart(function() bar:animate({ value = 1 }, { seconds = 0.5, ease = "linear" }) end)"#,
    );
    ticks(&mut host, 15);
    assert!((bar_value(&host, 0) - 0.5).abs() < 1e-6);
    ticks(&mut host, 14);
    assert!(bar_value(&host, 0) < 1.0);
    tick(&mut host);
    assert_eq!(bar_value(&host, 0), 1.0);
    ticks(&mut host, 5);
    assert_eq!(bar_value(&host, 0), 1.0);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}

#[test]
fn a_delay_waits_before_moving() {
    let mut host = running(
        r#"local bar = game.hud.bar(0)
        game.onStart(function() bar:animate({ value = 1 }, { seconds = 0.5, ease = "linear", delay = 0.1 }) end)"#,
    );
    ticks(&mut host, 6);
    assert_eq!(bar_value(&host, 0), 0.0);
    tick(&mut host);
    assert!(bar_value(&host, 0) > 0.0);
    ticks(&mut host, 29);
    assert_eq!(bar_value(&host, 0), 1.0);
}

#[test]
fn on_done_is_called_once_after_every_field_arrives() {
    let mut host = saying(
        r#"local box = game.hud.box()
        game.onStart(function()
            box:animate({ opacity = 0.5, right = 10 }, { seconds = 0.25, onDone = function()
                say(box.opacity, box.right)
            end })
        end)"#,
    );
    ticks(&mut host, 14);
    assert_eq!(said(&host), [""]);
    ticks(&mut host, 60);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(said(&host), ["0.5,10"]);
}

#[test]
fn a_new_animation_of_a_field_takes_over_from_where_it_is() {
    let mut host = saying(
        r#"local bar = game.hud.bar(0)
        local first = 0
        game.onStart(function()
            bar:animate({ value = 1 }, { seconds = 1, ease = "linear", onDone = function() say("first") end })
        end)
        game.after(0.5, function()
            bar:animate({ value = 0 }, { seconds = 1, ease = "linear" })
        end)"#,
    );
    // The timer is called on tick 30; the first animation has had 30 steps.
    ticks(&mut host, 31);
    let at = bar_value(&host, 1);
    // 0.5 at the takeover, then one step of the new one back down.
    assert!((at - (0.5 - 0.5 / 60.0)).abs() < 1e-4, "{at}");
    ticks(&mut host, 59);
    assert_eq!(bar_value(&host, 1), 0.0);
    assert_eq!(said(&host), [""]);
}

#[test]
fn an_animation_of_other_fields_leaves_the_first_running() {
    let mut host = running(
        r#"local box = game.hud.box()
        game.onStart(function()
            box:animate({ right = 60 }, { seconds = 1, ease = "linear" })
            box:animate({ down = 30 }, { seconds = 0.5, ease = "linear" })
        end)"#,
    );
    ticks(&mut host, 60);
    assert_eq!(pieces(&host)[0].offset, [60.0, 30.0]);
}

#[test]
fn removing_a_piece_drops_its_animations_without_calling_on_done() {
    let mut host = saying(
        r#"local box = game.hud.box()
        game.onStart(function()
            box:animate({ right = 60 }, { seconds = 0.2, onDone = function() say("done") end })
        end)
        game.after(0.05, function() box:remove() end)"#,
    );
    ticks(&mut host, 120);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(said(&host), [""]);
}

#[test]
fn only_the_pieces_that_animate_are_marked_changed() {
    let mut host = running(
        r#"local still = game.hud.bar(0.5)
        local moving = game.hud.bar(0)
        game.onStart(function() moving:animate({ value = 1 }, { seconds = 1 }) end)"#,
    );
    tick(&mut host);
    assert_eq!(changed(&mut host), [1]);
    ticks(&mut host, 100);
    let _ = changed(&mut host);
    tick(&mut host);
    assert!(changed(&mut host).is_empty());
}

#[test]
fn colours_move_a_channel_at_a_time() {
    let mut host = running(
        r##"local box = game.hud.box({ color = "#000000" })
        game.onStart(function() box:animate({ color = "#ff8000", borderColor = "#ffffff" }, { seconds = 1, ease = "linear" }) end)"##,
    );
    ticks(&mut host, 30);
    let piece = &pieces(&host)[0];
    assert!((piece.color.r - 0.5).abs() < 1e-5, "{:?}", piece.color);
    assert!((piece.color.g - 128.0 / 255.0 * 0.5).abs() < 1e-3);
    ticks(&mut host, 30);
    let piece = &pieces(&host)[0];
    assert_eq!(
        piece.color,
        Color {
            r: 1.0,
            g: 128.0 / 255.0,
            b: 0.0,
            a: 1.0
        }
    );
}

#[test]
fn two_games_given_the_same_input_have_the_same_values_every_tick() {
    let script = r##"local box = game.hud.box({ color = "#102030" })
        local bar = game.hud.bar(0)
        game.onStart(function()
            box:animate({ right = 91, opacity = 0.3, rotation = 33, color = "#ffcc00" }, { seconds = 0.7, ease = "ease" })
            bar:animate({ value = 1 }, { seconds = 0.9, ease = "ease-in-out", delay = 0.13 })
        end)
        game.after(0.4, function() box:animate({ right = -20 }, { seconds = 0.5, ease = "ease-out" }) end)"##;
    let (mut one, mut two) = (running(script), running(script));
    for _ in 0..120 {
        tick(&mut one);
        tick(&mut two);
        assert_eq!(pieces(&one), pieces(&two));
    }
    assert_eq!(bar_value(&one, 1), 1.0);
}

#[test]
fn animate_mistakes_name_the_nearest_word_and_what_to_write() {
    let error = mistake(r#"local b = game.hud.bar(1); b:animate({ opacty = 0 })"#);
    assert!(error.contains("Did you mean `opacity`?"), "{error}");
    let error = mistake(r#"local t = game.hud.text("x"); t:animate({ width = 10 })"#);
    assert!(error.contains("no `width` to animate"), "{error}");
    assert!(error.contains("opacity"), "{error}");
    let error = mistake(r#"local b = game.hud.bar(1); b:animate({ value = 0 }, { secconds = 1 })"#);
    assert!(error.contains("Did you mean `seconds`?"), "{error}");
    let error =
        mistake(r#"local b = game.hud.bar(1); b:animate({ value = 0 }, { ease = "ease-inout" })"#);
    assert!(error.contains("Did you mean \"ease-in-out\"?"), "{error}");
    let error = mistake(r#"local b = game.hud.bar(1); b:animate({ value = "full" })"#);
    assert!(error.contains("needs a number"), "{error}");
    let error = mistake(r#"local b = game.hud.bar(1); b:animate({ color = 5 })"#);
    assert!(error.contains("needs a colour"), "{error}");
    let error = mistake(r#"local b = game.hud.bar(1); b:animate({ value = 0 }, { seconds = -1 })"#);
    assert!(error.contains("needs a number of seconds"), "{error}");
    let error = mistake(r#"local b = game.hud.bar(1); b:animate({ value = 0 }, { onDone = 3 })"#);
    assert!(error.contains("needs a function"), "{error}");
    let error = mistake(r#"local b = game.hud.bar(1); b:animate({})"#);
    assert!(error.contains("no fields"), "{error}");
    let error = mistake(r#"local b = game.hud.bar(1); b.animate({ value = 0 })"#);
    assert!(error.contains("colon"), "{error}");
    let error = mistake(r#"local b = game.hud.bar(1); b:remove(); b:animate({ value = 0 })"#);
    assert!(error.contains("taken off the HUD"), "{error}");
}

#[test]
fn a_texts_width_counts_its_letter_spacing() {
    let mut host = running(
        r#"local t = game.hud.text("abcd", { size = 20 })
        local plain = t.width
        t.letterSpacing = 3
        assert(math.abs(t.width - plain - 12) < 0.01, "spacing adds to the width")"#,
    );
    tick(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
}
