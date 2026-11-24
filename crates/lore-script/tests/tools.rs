//! What a script can ask of the screen, the frames and the player: its own
//! buttons, the pointer, stepping through a menu, frame times, the pieces
//! under the pointer, seeing and moving the player, and the speed of time.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use lore_input::{
    ActionMap, Device, GamepadButton, GamepadStyle, Input, InputFrame, Key, MouseButton,
};
use lore_script::weapons::Zone;
use lore_script::{
    CharacterId, ENTRY_SCRIPT, HudShape, Motion, Movement, ScriptHost, Teleport, WeaponHit,
};

thread_local! {
    /// What scripts on this test's thread printed.
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The warnings the engine logged on this test's thread.
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The notes the engine logged on this test's thread.
    static INFOS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Collects `print` output and warnings per thread.
struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "script" {
            PRINTED.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        } else if record.level() == log::Level::Warn {
            WARNINGS.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        } else if record.level() == log::Level::Info {
            INFOS.with_borrow_mut(|lines| lines.push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn printed() -> Vec<String> {
    PRINTED.with_borrow_mut(std::mem::take)
}

fn warnings() -> Vec<String> {
    WARNINGS.with_borrow_mut(std::mem::take)
}

fn infos() -> Vec<String> {
    INFOS.with_borrow_mut(std::mem::take)
}

fn listen() {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Info);
    printed();
    warnings();
    infos();
}

const STEP: f32 = 1.0 / 60.0;
const PLAYER: CharacterId = CharacterId::PLAYER;

/// A game with this script, which has no mistake in it.
fn game(script: &str) -> ScriptHost {
    listen();
    let host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    host
}

/// The message of the mistake loading `script` makes.
fn load_mistake(script: &str) -> String {
    listen();
    let host = ScriptHost::from_source(script, ActionMap::standard());
    host.last_error()
        .expect("the script has a mistake")
        .message
        .clone()
}

/// The message of the mistake the running script just made.
fn mistake(host: &ScriptHost) -> String {
    host.last_error()
        .unwrap_or_else(|| panic!("the script made no mistake; it printed {:?}", printed()))
        .message
        .clone()
}

fn run(host: &mut ScriptHost, ticks: u32) {
    let idle = InputFrame::default();
    for _ in 0..ticks {
        host.tick(&idle, STEP);
    }
}

/// The words of the first text on the HUD.
fn hud_text(host: &ScriptHost) -> String {
    let mut found = None;
    host.each_hud_piece(|_, piece| {
        if let (None, HudShape::Text { text, .. }) = (&found, &piece.shape) {
            found = Some(text.clone());
        }
    });
    found.expect("the HUD has a text")
}

/// A game folder that is deleted when the test ends.
struct GameFolder(PathBuf);

impl GameFolder {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        listen();
        let folder = std::env::temp_dir().join(format!(
            "lore-tools-test-{}-{}",
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
        ScriptHost::open(&self.0, ActionMap::standard()).unwrap()
    }
}

impl Drop for GameFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const OPEN_MAP: &str = r#"
game.input.bind("open_map", { "F1", "LeftBumper" })
game.onTick(function()
    if game.input.justPressed("open_map") then print("opened") end
end)
"#;

#[test]
fn a_button_of_the_games_own_is_heard_through_the_map_the_host_gives() {
    let mut host = game(OPEN_MAP);
    let mut input = Input::new(host.actions());
    host.tick(input.tick(), STEP);
    assert!(printed().is_empty());
    input.press(Key::F1);
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["opened"]);
    // The left bumper also turns to the previous tab in a menu, and the
    // game's own actions are heard in menus too: said once, as a warning.
    let said = warnings();
    assert!(
        said.len() == 1 && said[0].contains("menu_previous_tab"),
        "{said:?}"
    );

    // The gamepad button works too, and in the menu set as well.
    input.release(Key::F1);
    host.tick(input.tick(), STEP);
    input.press(GamepadButton::LeftBumper);
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["opened"]);
    input.release(GamepadButton::LeftBumper);
    input.set_active_set(host.actions().set("menu").unwrap());
    host.tick(input.tick(), STEP);
    input.press(Key::F1);
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["opened"]);
}

#[test]
fn binding_from_a_function_says_to_move_it_to_the_top_of_the_file() {
    let mut host = game(r#"game.onTick(function() game.input.bind("open_map", { "F1" }) end)"#);
    run(&mut host, 1);
    let message = mistake(&host);
    assert!(
        message.contains("top of the file, outside any function"),
        "{message}"
    );
}

#[test]
fn a_misspelt_button_gets_the_suggestion() {
    let message = load_mistake(r#"game.input.bind("open_map", { "Spce" })"#);
    assert!(message.contains("Space"), "{message}");
    assert!(message.contains("Write it like this"), "{message}");
}

#[test]
fn a_bad_name_or_list_says_what_to_write() {
    for script in [
        r#"game.input.bind("", { "F1" })"#,
        r#"game.input.bind("open/map", { "F1" })"#,
        r#"game.input.bind("open_map", {})"#,
        r#"game.input.bind("open_map", "F1")"#,
        r#"game.input.bind("open_map", { "WASD" })"#,
        r#"game.input.bind(5, { "F1" })"#,
    ] {
        let message = load_mistake(script);
        assert!(message.contains("game.input.bind"), "{script}: {message}");
    }
    let message = load_mistake(&format!(
        r#"game.input.bind("{}", {{ "F1" }})"#,
        "a".repeat(41)
    ));
    assert!(message.contains("1 to 40 characters"), "{message}");
}

#[test]
fn a_name_with_a_space_or_a_capital_is_fixed_for_the_script_writer() {
    let message = load_mistake(r#"game.input.bind("Weather menu", { "F1" })"#);
    assert!(message.contains("\"Weather menu\""), "{message}");
    assert!(
        message.contains(r#"Write it like this: game.input.bind("weather_menu", {"#),
        "{message}"
    );
    let message = load_mistake(r#"game.input.bind("open-map", { "F1" })"#);
    assert!(
        message.contains(r#"game.input.bind("open_map","#),
        "{message}"
    );
    // Lower case letters, digits and underscores are fine.
    let _host = game(r#"game.input.bind("map_2", { "F1", "LeftBumper" })"#);
}

#[test]
fn an_engine_action_cannot_be_bound_again() {
    let message = load_mistake(r#"game.input.bind("jump", { "F1" })"#);
    assert!(message.contains("engine's actions"), "{message}");
    assert!(
        message.contains("game.input.pressed(\"jump\")"),
        "{message}"
    );
}

#[test]
fn binding_one_name_twice_in_a_version_is_a_mistake() {
    let message = load_mistake(
        "game.input.bind(\"open_map\", { \"F1\" })\ngame.input.bind(\"open_map\", { \"F2\" })",
    );
    assert!(message.contains("twice"), "{message}");
}

#[test]
fn a_button_with_no_gamepad_button_or_a_clash_is_a_warning_not_a_failure() {
    let _host = game(r#"game.input.bind("open_map", { "F1" })"#);
    let said = warnings();
    assert!(said.iter().any(|w| w.contains("LeftBumper")), "{said:?}");

    let _host = game(r#"game.input.bind("heal", { "Space", "GamepadSelect" })"#);
    let said = warnings();
    assert!(
        said.iter()
            .any(|w| w.contains("\"Space\"") && w.contains("heal")),
        "{said:?}"
    );
}

#[test]
fn a_reload_keeps_the_players_bindings_and_warns_of_a_new_action() {
    let folder = GameFolder::new(OPEN_MAP);
    let mut host = folder.open();
    // The engine takes the map, and the player rebinds the action.
    let mut map = host.actions();
    let open = map.action("open_map").unwrap();
    map.replace_button(
        open,
        "F1".parse().map(|b: lore_input::Button| b).unwrap(),
        "F5".parse().unwrap(),
    );
    run(&mut host, 1);
    warnings();

    folder.write(&format!(
        "{OPEN_MAP}\ngame.input.bind(\"zoom\", {{ \"Z\", \"LeftBumper\" }})"
    ));
    host.reload().unwrap();
    let said = warnings();
    assert!(
        said.iter()
            .any(|w| w.contains("will not hear it until the game is started again")),
        "{said:?}"
    );
    // The name the map already had changed nothing, and the new one is in
    // the host's map for the next start of the game.
    let after = host.actions();
    assert!(after.action("zoom").is_ok());
    assert_eq!(
        after.bindings(after.action("open_map").unwrap()).len(),
        2,
        "its bindings were not added again"
    );
    // The engine's old map does not know it: reading it reads not pressed.
    let mut input = Input::new(map);
    host.tick(input.tick(), STEP);
    assert!(host.last_error().is_none());
}

#[test]
fn a_reload_with_other_keys_for_an_existing_action_says_they_wait() {
    let folder = GameFolder::new(OPEN_MAP);
    let mut host = folder.open();
    infos();
    // The same keys in another order say nothing.
    folder.write(&OPEN_MAP.replace(
        "{ \"F1\", \"GamepadSelect\" }",
        "{ \"GamepadSelect\", \"F1\" }",
    ));
    host.reload().unwrap();
    let said = infos();
    assert!(
        said.iter().all(|line| !line.contains("open_map")),
        "{said:?}"
    );
    folder.write(&OPEN_MAP.replace("\"F1\"", "\"F6\""));
    host.reload().unwrap();
    let said = infos();
    assert!(
        said.iter()
            .any(|line| line.contains("open_map") && line.contains("started again")),
        "{said:?}"
    );
}

#[test]
fn the_pointer_and_the_wheel_come_from_this_ticks_input() {
    let mut host = game(
        r#"game.onTick(function()
            local x, y = game.input.pointer()
            print(x, y, game.input.wheel())
        end)"#,
    );
    let mut input = Input::new(host.actions());
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["nil nil 0"]);
    input.set_pointer(Some([100.0, 250.5]), [1920.0, 1080.0]);
    input.roll_wheel(2.0);
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["100 250.5 2"]);
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["100 250.5 0"]);
}

/// The ticks on which `script`'s `game.onTick` printed a step, with the
/// action held from the first tick.
fn steps_while_held(script: &str, hold: impl FnOnce(&mut Input), ticks: u32) -> Vec<String> {
    let mut host = game(script);
    let mut input = Input::new(host.actions());
    hold(&mut input);
    for _ in 0..ticks {
        host.tick(input.tick(), STEP);
    }
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    printed()
}

#[test]
fn stepping_repeats_after_a_wait_counted_in_ticks() {
    let script = r#"
        local n = 0
        game.onTick(function()
            local x, y = game.input.stepped("move")
            if x ~= 0 or y ~= 0 then print(n, x, y) end
            n += 1
        end)"#;
    let steps = steps_while_held(script, |input| input.press(Key::W), 40);
    // At once, then after 0.4 s (24 ticks), then every 0.1 s (6 ticks).
    assert_eq!(steps, ["0 0 1", "24 0 1", "30 0 1", "36 0 1"]);

    let steps = steps_while_held(script, |input| input.press(Key::A), 3);
    assert_eq!(steps, ["0 -1 0"]);
}

#[test]
fn stepping_waits_in_real_seconds_whatever_the_speed_of_time() {
    let script = |speed: &str| {
        format!(
            r#"game.setTimeSpeed({speed})
            local n = 0
            game.onTick(function()
                local x, y = game.input.stepped("move")
                if y ~= 0 then print(n) end
                n += 1
            end)"#
        )
    };
    // Ticks at 8 times the speed take 8 times as many for the same wait.
    let steps = steps_while_held(&script("1"), |input| input.press(Key::W), 40);
    assert_eq!(steps, ["0", "24", "30", "36"]);
    let steps = steps_while_held(&script("4"), |input| input.press(Key::W), 150);
    assert_eq!(steps, ["0", "96", "120", "144"]);
    let steps = steps_while_held(&script("0.25"), |input| input.press(Key::W), 13);
    assert_eq!(steps, ["0", "6", "8", "10", "12"]);
    // Stopped time still has ticks at the usual rate.
    let steps = steps_while_held(&script("0"), |input| input.press(Key::W), 26);
    assert_eq!(steps, ["0", "24"]);
}

#[test]
fn stepping_works_for_a_button_and_for_nothing_held() {
    let script = r#"
        local n = 0
        game.onTick(function()
            local x, y = game.input.stepped("jump")
            if x ~= 0 or y ~= 0 then print(n, x, y) end
            n += 1
        end)"#;
    let steps = steps_while_held(script, |input| input.press(Key::Space), 26);
    assert_eq!(steps, ["0 1 0", "24 1 0"]);
    assert!(steps_while_held(script, |_| {}, 60).is_empty());
}

#[test]
fn stepping_counts_even_when_the_script_does_not_ask() {
    // Asked only from the 20th tick on: the key was pressed on the first,
    // so the repeat at tick 24 is still on time.
    let script = r#"
        local n = 0
        game.onTick(function()
            n += 1
            if n < 20 then return end
            local x, y = game.input.stepped("move")
            if y ~= 0 then print(n - 1) end
        end)"#;
    let steps = steps_while_held(script, |input| input.press(Key::W), 32);
    assert_eq!(steps, ["24", "30"]);
}

#[test]
fn stepping_an_unknown_action_lists_the_real_ones() {
    let mut host = game(r#"game.onTick(function() game.input.stepped("jmp") end)"#);
    run(&mut host, 1);
    assert!(mistake(&host).contains("jump"));
}

#[test]
fn a_label_names_what_the_player_presses_on_the_device_they_use() {
    let mut host = game(
        r#"local hint = game.hud.text("")
        game.onTick(function() end)
        game.onDraw(function()
            hint.text = game.input.label("jump") .. "|" .. game.input.label("nothing bound")
        end)"#,
    );
    // An action nobody bound has no name to show, and is an error to ask.
    run(&mut host, 1);
    host.draw(1.0);
    assert!(mistake(&host).contains("nothing bound"));

    let mut host = game(
        r#"local hint = game.hud.text("")
        game.onDraw(function()
            hint.text = game.input.label("jump") .. "|" .. game.input.label("move")
        end)"#,
    );
    run(&mut host, 1);
    host.draw(1.0);
    assert_eq!(hud_text(&host), "Space|W A S D");

    host.set_device(Device::Gamepad, GamepadStyle::PlayStation);
    host.draw(1.0);
    assert_eq!(hud_text(&host), "Cross|LeftStick");

    // What the player changed is what the label says.
    let mut controls = ActionMap::standard();
    let jump = controls.action("jump").unwrap();
    controls.replace_button(jump, "Space".parse().unwrap(), "F".parse().unwrap());
    host.set_controls(&controls);
    host.set_device(Device::KeyboardAndMouse, GamepadStyle::Xbox);
    host.draw(1.0);
    assert!(hud_text(&host).starts_with("F|"), "{}", hud_text(&host));
}

#[test]
fn a_label_is_not_for_the_rules() {
    let mut host = game(r#"game.onTick(function() print(game.input.label("jump")) end)"#);
    run(&mut host, 1);
    let message = mistake(&host);
    assert!(message.contains("game.onDraw"), "{message}");
    assert!(message.contains("the key can change"), "{message}");
}

#[test]
fn frame_times_are_read_while_drawing() {
    let mut host = game(
        r#"local shown = game.hud.text("")
        game.onDraw(function()
            shown.text = string.format("%.1f %.1f %.1f %.1f", game.frames.rate(),
                game.frames.lastTime() * 1000, game.frames.slowestTime() * 1000,
                game.frames.lowRate())
        end)"#,
    );
    run(&mut host, 1);
    host.draw(1.0);
    assert_eq!(hud_text(&host), "0.0 0.0 0.0 0.0");

    for _ in 0..99 {
        host.frame_drawn(0.01);
    }
    host.frame_drawn(0.1);
    host.draw(1.0);
    // Over the last second: 99 fast frames and the slow one come to 1.09 s,
    // so the second holds the slow one and 90 fast ones.
    // The slowest hundredth of 100 frames is the one slow frame: 10 a second.
    let text = hud_text(&host);
    let parts: Vec<f32> = text.split(' ').map(|n| n.parse().unwrap()).collect();
    assert!((parts[0] - 91.0 / 1.0).abs() < 2.0, "{text}");
    assert!((parts[1] - 100.0).abs() < 0.1, "{text}");
    assert!((parts[2] - 100.0).abs() < 0.1, "{text}");
    assert!((parts[3] - 10.0).abs() < 0.1, "{text}");

    // Nonsense is left out.
    host.frame_drawn(f32::NAN);
    host.frame_drawn(-1.0);
    let mut seen = Vec::new();
    host.each_frame_time(1000, |seconds| seen.push(seconds));
    assert_eq!(seen.len(), 100);
}

#[test]
fn the_frame_times_come_oldest_first_and_only_a_thousand_are_kept() {
    let mut host = game("game.onTick(function() end)");
    for frame in 0..1500_u16 {
        host.frame_drawn(f32::from(frame) * 0.001);
    }
    let mut seen = Vec::new();
    host.each_frame_time(3, |seconds| seen.push(seconds));
    for (got, want) in seen.iter().zip([1.497, 1.498, 1.499]) {
        assert!((got - want).abs() < 1e-5, "{seen:?}");
    }
    assert_eq!(seen.len(), 3);
    let mut count = 0;
    host.each_frame_time(5000, |_| count += 1);
    assert_eq!(count, 1000);
}

#[test]
fn the_frames_are_not_for_the_rules() {
    for read in ["rate", "lowRate", "lastTime", "slowestTime"] {
        let mut host = game(&format!(
            "game.onTick(function() print(game.frames.{read}()) end)"
        ));
        run(&mut host, 1);
        let message = mistake(&host);
        assert!(message.contains("game.onDraw"), "{read}: {message}");
        assert!(message.contains("game.hud.text"), "{read}: {message}");
    }
}

#[test]
fn a_frame_graph_is_a_piece_the_engine_fills_in() {
    let host = game(
        r#"game.hud.frameGraph()
        game.hud.frameGraph({ at = "top right", width = 300, height = 100, frames = 1000 })"#,
    );
    let mut shapes = Vec::new();
    host.each_hud_piece(|_, piece| shapes.push(piece.shape.clone()));
    assert_eq!(
        shapes,
        [
            HudShape::Graph {
                size: [240.0, 80.0],
                frames: 240
            },
            HudShape::Graph {
                size: [300.0, 100.0],
                frames: 1000
            },
        ]
    );
    for bad in [
        "{ frames = 5 }",
        "{ frames = 2000 }",
        "{ frames = 12.5 }",
        "{ frames = \"all\" }",
    ] {
        let message = load_mistake(&format!("game.hud.frameGraph({bad})"));
        assert!(message.contains("frames"), "{bad}: {message}");
    }
    let message = load_mistake("game.hud.frameGraph({ color = \"red\" })");
    assert!(message.contains("no option called `color`"), "{message}");
}

const CENTER_BOX: &str = r#"
local box = game.hud.box({ at = "center", width = 100, height = 100 })
local graph = game.hud.frameGraph({ at = "center", width = 100, height = 100 })
game.onTick(function()
    local x, y = box:pointerAt()
    local function r(v) return v and string.format("%.2f", v) or "nil" end
    print(box.hovered, graph.hovered, r(x), r(y))
end)
"#;

/// What `CENTER_BOX` prints with the pointer at `at` on a 1920 by 1080 screen.
fn over_center_box(at: Option<[f32; 2]>) -> String {
    let mut host = game(CENTER_BOX);
    let mut input = Input::new(host.actions());
    input.set_pointer(at, [1920.0, 1080.0]);
    host.tick(input.tick(), STEP);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    printed().remove(0)
}

#[test]
fn a_piece_knows_when_the_pointer_is_over_it_and_where() {
    assert_eq!(over_center_box(Some([960.0, 540.0])), "true true 0.50 0.50");
    assert_eq!(over_center_box(Some([910.0, 490.0])), "true true 0.00 0.00");
    // The right and bottom edges are outside, and the fractions go on past 1.
    assert_eq!(
        over_center_box(Some([1060.0, 540.0])),
        "false false 1.50 0.50"
    );
    assert_eq!(
        over_center_box(Some([100.0, 100.0])),
        "false false -8.10 -3.90"
    );
    assert_eq!(over_center_box(None), "false false 0.00 0.00");
}

/// Two boxes over each other at the middle of the screen and a third beside
/// them, which print which of them are pressed each tick.
const PRESS_BOXES: &str = r#"
local under = game.hud.box({ at = "center", width = 100, height = 100 })
local over = game.hud.box({ at = "center", width = 100, height = 100 })
local aside = game.hud.box({ at = "center", right = 300, width = 100, height = 100 })
game.onTick(function()
    print(under.pressed, over.pressed, aside.pressed)
end)
"#;

#[test]
fn a_piece_is_pressed_from_the_click_that_began_on_it_until_it_is_let_go() {
    let mut host = game(PRESS_BOXES);
    let mut input = Input::new(host.actions());
    input.set_active_set(host.actions().set("menu").unwrap());
    let tick = |host: &mut ScriptHost, input: &mut Input| {
        host.tick(input.tick(), STEP);
        printed().remove(0)
    };
    let screen = [1920.0, 1080.0];
    input.set_pointer(Some([960.0, 540.0]), screen);
    assert_eq!(tick(&mut host, &mut input), "false false false");
    // Only the piece on top is pressed.
    input.press(MouseButton::MouseLeft);
    assert_eq!(tick(&mut host, &mut input), "false true false");
    assert_eq!(tick(&mut host, &mut input), "false true false");
    // It stays pressed when the pointer leaves it, and nothing else is
    // pressed by the pointer reaching another piece.
    input.set_pointer(Some([1260.0, 540.0]), screen);
    assert_eq!(tick(&mut host, &mut input), "false true false");
    input.release(MouseButton::MouseLeft);
    assert_eq!(tick(&mut host, &mut input), "false false false");
    // A click that begins away from every piece presses none, even when the
    // pointer is dragged over one.
    input.set_pointer(Some([100.0, 100.0]), screen);
    input.press(MouseButton::MouseLeft);
    tick(&mut host, &mut input);
    input.set_pointer(Some([1260.0, 540.0]), screen);
    assert_eq!(tick(&mut host, &mut input), "false false false");
}

#[test]
fn a_game_with_no_click_action_never_presses_anything() {
    listen();
    // A map with no "menu_click" in it.
    let mut host = ScriptHost::from_source(PRESS_BOXES, ActionMap::new());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let mut input = Input::new(host.actions());
    input.set_pointer(Some([960.0, 540.0]), [1920.0, 1080.0]);
    for _ in 0..3 {
        host.tick(input.tick(), STEP);
    }
    assert!(printed().iter().all(|line| line == "false false false"));
}

#[test]
fn a_hidden_piece_is_never_hovered_and_hovered_cannot_be_set() {
    let mut host = game(
        r#"local box = game.hud.box({ at = "center", width = 100, height = 100 })
        box.visible = false
        game.onTick(function() print(box.hovered, box:pointerAt()) end)"#,
    );
    let mut input = Input::new(host.actions());
    input.set_pointer(Some([960.0, 540.0]), [1920.0, 1080.0]);
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["false 0.5 0.5"]);

    let message = load_mistake("local box = game.hud.box()\nbox.hovered = true");
    assert!(message.contains("hovered"), "{message}");
    assert!(message.contains("cannot be set"), "{message}");
    let message = load_mistake("local box = game.hud.box()\nbox.pressed = true");
    assert!(message.contains("pressed"), "{message}");
    assert!(message.contains("cannot be set"), "{message}");
}

#[test]
fn text_is_measured_with_the_font_the_engine_gave() {
    struct Wide;
    impl lore_ui::TextMeasure for Wide {
        fn width(&self, _: &str, _: f32) -> f32 {
            1000.0
        }
    }
    let script = r#"local words = game.hud.text("Hi", { at = "left", size = 40 })
        game.onTick(function() print(words.hovered) end)"#;
    // Pointer 500 pixels into the text: past a guessed two letters, inside
    // one a thousand wide.
    let at = Some([sa() + 500.0, 540.0]);
    let mut host = game(script);
    let mut input = Input::new(host.actions());
    input.set_pointer(at, [1920.0, 1080.0]);
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["false"]);
    host.set_text_measure(Box::new(Wide));
    host.tick(input.tick(), STEP);
    assert_eq!(printed(), ["true"]);
}

#[test]
fn what_the_picture_does_to_a_piece_is_not_what_the_rules_find_hovered() {
    let script = r#"local words = game.hud.text("Hi", { at = "left", size = 40 })
        game.onTick(function() print(words.hovered) end)
        game.onDraw(function() words.text = string.rep("long ", 40) end)"#;
    let play = |draws: bool| {
        let mut host = game(script);
        let mut input = Input::new(host.actions());
        input.set_pointer(Some([sa() + 500.0, 540.0]), [1920.0, 1080.0]);
        for _ in 0..5 {
            host.tick(input.tick(), STEP);
            if draws {
                host.draw(1.0);
                host.draw(1.0);
            }
        }
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        let lines = printed();
        assert!(lines.iter().all(|line| line == "false"), "{lines:?}");
        host.rules_fingerprint()
    };
    assert_eq!(play(false), play(true));
}

/// The left edge of the safe area on a 1920 wide screen, where a piece
/// pinned to the left begins.
fn sa() -> f32 {
    lore_ui::hud_area(
        lore_ui::Anchor::Left,
        [0.0, 0.0],
        [10.0, 10.0],
        [1920.0, 1080.0],
    )[0]
}

#[test]
fn pieces_under_the_pointer_work_in_the_picture_too() {
    let mut host = game(
        r#"local box = game.hud.box({ at = "center", width = 100, height = 100 })
        local label = game.hud.text("")
        game.onDraw(function() label.text = tostring(box.hovered) end)"#,
    );
    let mut input = Input::new(host.actions());
    input.set_pointer(Some([960.0, 540.0]), [1920.0, 1080.0]);
    host.tick(input.tick(), STEP);
    host.draw(1.0);
    let mut seen = None;
    host.each_hud_piece(|_, piece| {
        if let HudShape::Text { text, .. } = &piece.shape {
            seen = Some(text.clone());
        }
    });
    assert_eq!(seen.as_deref(), Some("true"));
}

fn motion(position_y: f32) -> Motion {
    Motion {
        velocity: [1.0, -2.0, 3.0],
        eye: [0.0, position_y + 1.7, 0.0],
        aim: [1.0, 0.0, 0.0],
        grounded: false,
    }
}

#[test]
fn the_player_can_be_seen_moving() {
    let mut host = game(
        r#"game.onTick(function()
            local p = game.player
            print(p.velocity, p.eye, p.aim, p.grounded, p.movement, p.flySpeed)
        end)"#,
    );
    host.set_position(PLAYER, [1.0, 2.0, 3.0]);
    // Nothing said yet: still, eyes a little above the feet, looking ahead.
    run(&mut host, 1);
    assert_eq!(
        printed(),
        ["vector(0, 0, 0) vector(1, 3.6, 3) vector(0, 0, -1) true walk 11"]
    );

    host.set_motion(PLAYER, motion(2.0));
    run(&mut host, 1);
    assert_eq!(
        printed(),
        ["vector(1, -2, 3) vector(0, 3.7, 0) vector(1, 0, 0) false walk 11"]
    );
}

#[test]
fn teleporting_puts_the_feet_there_at_once_and_hands_the_move_to_the_engine() {
    let mut host = game(
        r#"game.onStart(function()
            game.player:teleport(vector.create(5, 6, 7), vector.create(0, 0, -4))
            print(game.player.position)
        end)"#,
    );
    run(&mut host, 1);
    assert_eq!(printed(), ["vector(5, 6, 7)"]);
    let Teleport { position, facing } = host.take_teleport().unwrap();
    assert_eq!(position, [5.0, 6.0, 7.0]);
    assert_eq!(facing, Some([0.0, 0.0, -1.0]));
    assert_eq!(host.take_teleport(), None, "taken once");

    let mut host =
        game("game.onStart(function() game.player:teleport(vector.create(1, 2, 3)) end)");
    run(&mut host, 1);
    assert_eq!(host.take_teleport().unwrap().facing, None);
}

#[test]
fn teleporting_with_nothing_to_go_to_or_nowhere_to_face_is_a_mistake() {
    for (call, word) in [
        ("game.player:teleport(\"home\")", "needs a place"),
        ("game.player:teleport()", "needs a place"),
        (
            "game.player:teleport(vector.create(0, 0, 0), vector.create(0, 0, 0))",
            "no direction",
        ),
        (
            "game.player:teleport(vector.create(0, 0, 0), vector.create(1 / 0, 0, 0))",
            "no direction",
        ),
        (
            "game.player:teleport(vector.create(0 / 0, 0, 0))",
            "no size",
        ),
        (
            "game.player:teleport(vector.create(0, 1e38, 0))",
            "Keep it inside the level",
        ),
        (
            "game.player:teleport(vector.create(-2000000, 0, 0))",
            "Keep it inside the level",
        ),
        (
            "game.player:teleport(vector.create(0, 0, 0), \"north\")",
            "needs a vector",
        ),
    ] {
        let mut host = game(&format!("game.onTick(function() {call} end)"));
        run(&mut host, 1);
        let message = mistake(&host);
        assert!(message.contains(word), "{call}: {message}");
        assert!(message.contains("Write it like this"), "{call}: {message}");
        assert_eq!(host.take_teleport(), None);
    }
}

#[test]
fn only_the_player_can_be_moved_or_changed() {
    let mut host = game(
        r#"game.onWeaponHit(function(hit)
            hit.victim:teleport(vector.create(0, 0, 0))
        end)"#,
    );
    let dummy = host.add_character("Dummy");
    run(&mut host, 1);
    host.weapon_hit(&WeaponHit {
        shooter: PLAYER,
        victim: Some(dummy),
        weapon: "rifle".into(),
        damage: 30.0,
        part: Some(Zone::Body),
        position: [0.0, 1.2, -10.0],
        distance: 10.0,
        explosion: false,
    });
    let message = mistake(&host);
    assert!(
        message.contains("only game.player can be teleported so far"),
        "{message}"
    );
    assert_eq!(host.take_teleport(), None);
}

#[test]
fn the_way_the_player_moves_is_set_and_survives_a_save() {
    let folder = GameFolder::new(
        r#"game.onStart(function()
            game.player:setMovement("noclip")
            game.player:setFlySpeed(30)
        end)
        game.onTick(function() print(game.player.movement, game.player.flySpeed) end)"#,
    );
    let mut host = folder.open();
    assert_eq!((host.movement(), host.fly_speed()), (Movement::Walk, 11.0));
    run(&mut host, 1);
    assert_eq!(
        (host.movement(), host.fly_speed()),
        (Movement::Noclip, 30.0)
    );
    assert_eq!(printed(), ["noclip 30"]);

    folder.write("game.onTick(function() print(game.player.movement) end)");
    host.reload().unwrap();
    assert_eq!(
        (host.movement(), host.fly_speed()),
        (Movement::Noclip, 30.0)
    );
    run(&mut host, 1);
    assert_eq!(printed(), ["noclip"]);
}

#[test]
fn a_bad_way_to_move_or_flying_speed_says_what_is_allowed() {
    let mut host = game(r#"game.onTick(function() game.player:setMovement("flye") end)"#);
    run(&mut host, 1);
    let message = mistake(&host);
    assert!(message.contains("Did you mean \"fly\"?"), "{message}");
    assert!(
        message.contains("walk") && message.contains("noclip"),
        "{message}"
    );

    let mut host = game(r#"game.onTick(function() game.player:setMovement("swim") end)"#);
    run(&mut host, 1);
    let message = mistake(&host);
    assert!(
        message.contains("\"walk\", \"fly\" and \"noclip\""),
        "{message}"
    );

    for call in [
        "setMovement(3)",
        "setFlySpeed(\"fast\")",
        "setFlySpeed(0 / 0)",
        "setFlySpeed(1 / 0)",
    ] {
        let mut host = game(&format!("game.onTick(function() game.player:{call} end)"));
        run(&mut host, 1);
        assert!(mistake(&host).contains("Write it like this"), "{call}");
    }
    let mut host = game("game.onTick(function() game.player:setFlySpeed(200) end)");
    run(&mut host, 1);
    assert_eq!(host.fly_speed(), 200.0);
}

#[test]
fn a_fly_speed_or_time_speed_out_of_range_is_taken_as_the_nearest_end() {
    for (call, fly) in [("0.1", 0.5), ("201", 200.0), ("-5", 0.5), ("30", 30.0)] {
        let mut host = game(&format!(
            "game.onTick(function() game.player:setFlySpeed({call}) end)"
        ));
        run(&mut host, 1);
        assert!(host.last_error().is_none(), "{call}");
        assert_eq!(host.fly_speed(), fly, "{call}");
    }
    for (call, speed) in [("-1", 0.0), ("9", 8.0), ("100", 8.0), ("0.5", 0.5)] {
        let mut host = game(&format!(
            "game.onTick(function() game.setTimeSpeed({call}) end)"
        ));
        run(&mut host, 1);
        assert!(host.last_error().is_none(), "{call}");
        assert_eq!(host.time_speed(), speed, "{call}");
    }
}

#[test]
fn moving_the_player_is_for_the_rules_not_the_picture() {
    let mut host = game(
        r#"game.onDraw(function() game.player:setMovement("fly") game.player:teleport(vector.create(0, 0, 0)) end)"#,
    );
    run(&mut host, 1);
    host.draw(1.0);
    let message = mistake(&host);
    assert!(message.contains("game.onDraw"), "{message}");
    assert_eq!(host.movement(), Movement::Walk);
}

#[test]
fn time_speed_is_set_read_and_checked() {
    let mut host = game(
        r#"print(game.timeSpeed())
        game.onTick(function() game.setTimeSpeed(0.25) print(game.timeSpeed()) end)"#,
    );
    assert_eq!(printed(), ["1"]);
    assert_eq!(host.time_speed(), 1.0);
    run(&mut host, 1);
    assert_eq!(host.time_speed(), 0.25);
    assert_eq!(
        printed(),
        ["0.25"].map(String::from)[..],
        "ticks print once"
    );

    for call in [
        "setTimeSpeed(0 / 0)",
        "setTimeSpeed(1 / 0)",
        "setTimeSpeed(\"slow\")",
    ] {
        let mut host = game(&format!("game.onTick(function() game.{call} end)"));
        run(&mut host, 1);
        let message = mistake(&host);
        assert!(message.contains("Write it like this"), "{call}: {message}");
        assert_eq!(host.time_speed(), 1.0);
    }
}

#[test]
fn stopped_time_keeps_the_clock_still_but_the_script_still_hears_the_player() {
    let mut host = game(
        r#"local n = 0
        game.after(0.1, function() print("timer") end)
        game.onTick(function(step)
            n += 1
            if n == 3 then game.setTimeSpeed(0) end
            if game.input.justPressed("jump") then game.setTimeSpeed(1) end
            print(string.format("%d %.4f %.4f", n, game.time(), step))
        end)"#,
    );
    let mut input = Input::new(host.actions());
    for _ in 0..3 {
        host.tick(input.tick(), STEP);
    }
    assert_eq!(host.time_speed(), 0.0);
    let before = host.time();
    printed();
    for _ in 0..30 {
        host.tick(input.tick(), STEP);
    }
    assert_eq!(host.time(), before, "time did not move");
    let lines = printed();
    assert_eq!(lines.len(), 30, "the handler ran every tick");
    assert!(
        lines.iter().all(|line| line.ends_with("0.0000")),
        "{lines:?}"
    );
    assert!(lines.iter().all(|line| !line.contains("timer")));

    // The player presses jump while time is stopped: the script hears it and
    // starts time again, and the timer finishes its wait afterwards.
    input.press(Key::Space);
    host.tick(input.tick(), STEP);
    assert_eq!(host.time_speed(), 1.0);
    printed();
    run(&mut host, 10);
    let lines = printed();
    assert!(lines.iter().any(|line| line == "timer"), "{lines:?}");
    assert!(host.time() > before);
}

#[test]
fn the_same_script_and_inputs_give_the_same_fingerprint() {
    let script = r#"
        game.input.bind("open_map", { "F1", "GamepadSelect" })
        game.onTick(function()
            if game.input.justPressed("open_map") then
                game.player:teleport(vector.create(1, 2, 3))
                game.player:setMovement("fly")
                game.player:setFlySpeed(20)
                game.setTimeSpeed(0.5)
            end
            local x, y = game.input.stepped("move")
            if y ~= 0 then print("step", y) end
            print(game.input.wheel())
        end)
        game.onDraw(function() print(game.frames.rate()) end)"#;
    let play = |frame_pace: f32| {
        let mut host = game(script);
        let mut input = Input::new(host.actions());
        input.press(Key::W);
        for tick in 0..40 {
            if tick == 10 {
                input.press(Key::F1);
            }
            host.tick(input.tick(), STEP);
            // Pictures at any pace change nothing the rules did.
            let mut shown = 0.0;
            while shown < 1.0 {
                host.frame_drawn(frame_pace);
                host.draw(shown);
                shown += frame_pace * 60.0;
            }
        }
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        host.rules_fingerprint()
    };
    let first = play(0.01);
    assert_eq!(first, play(0.01));
    assert_eq!(first, play(0.004));
    assert_ne!(first, 0);
}
