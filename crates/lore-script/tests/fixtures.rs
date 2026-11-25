//! The example scripts in `tests/fixtures`: each is run the way the engine
//! runs a game (input in, the live action set switched when the script asks)
//! and has to do what its comment says.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use lore_input::{ActionMap, Button, GamepadButton, Input, Key, MouseButton};
use lore_script::{CharacterId, HudPiece, HudShape, Motion, Movement, ScriptHost};

const WEATHER_MENU: &str = include_str!("fixtures/weather_menu.luau");
const DEBUG_MENU: &str = include_str!("fixtures/debug_menu.luau");
const FRAME_METER: &str = include_str!("fixtures/frame_meter.luau");
const CLEAR_KEY: &str = include_str!("fixtures/clear_key.luau");
const SPRAY_AIM: &str = include_str!("fixtures/spray_aim.luau");

const STEP: f32 = 1.0 / 60.0;
const SCREEN: [f32; 2] = [1920.0, 1080.0];
const PLAYER: CharacterId = CharacterId::PLAYER;

thread_local! {
    /// What scripts on this test's thread printed.
    static PRINTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
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

fn printed() -> Vec<String> {
    PRINTED.with_borrow_mut(std::mem::take)
}

/// A game and the input that drives it, the way the engine joins them.
struct Engine {
    host: ScriptHost,
    input: Input,
}

impl Engine {
    fn new(script: &str) -> Self {
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Info);
        printed();
        let host = ScriptHost::from_source(script, ActionMap::standard());
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        let input = Input::new(host.actions());
        Self { host, input }
    }

    /// Runs one tick, and puts the input in the action set the script chose.
    fn tick(&mut self) {
        self.host.tick(self.input.tick(), STEP);
        assert!(
            self.host.last_error().is_none(),
            "{:?}",
            self.host.last_error()
        );
        if let Some(set) = self.host.take_action_set() {
            self.input.set_active_set(set);
        }
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Presses and lets go of a button, with a tick for each.
    fn tap(&mut self, button: impl Into<Button> + Copy) {
        self.input.press(button);
        self.tick();
        self.input.release(button);
        self.tick();
    }

    fn point(&mut self, x: f32, y: f32) {
        self.input.set_pointer(Some([x, y]), SCREEN);
    }

    /// The set of actions the game is in, by name.
    fn set(&self) -> String {
        let map = self.host.actions();
        map.set_name(self.host.action_set()).to_owned()
    }

    fn pieces(&self) -> Vec<HudPiece> {
        let mut pieces = Vec::new();
        self.host
            .each_hud_piece(|_, piece| pieces.push(piece.clone()));
        pieces
    }

    /// Whether every piece of the HUD is shown (or none is).
    fn hud_shown(&self) -> Option<bool> {
        let pieces = self.pieces();
        let shown = pieces.first()?.visible;
        pieces
            .iter()
            .all(|piece| piece.visible == shown)
            .then_some(shown)
    }

    fn wind(&self) -> f32 {
        let [x, z] = self.host.weather().wind;
        x.hypot(z)
    }
}

/// Where the bar of weather menu row `row` (1 for rain) is when the pointer
/// is `across` of the way along it: the bars are 300 wide, 24 high, start
/// 100 right of the middle of the screen and are 50 apart.
fn bar_at(row: u16, across: f32) -> (f32, f32) {
    (910.0 + 300.0 * across, 428.0 + f32::from(row) * 50.0)
}

#[test]
fn f1_and_the_gamepad_select_button_open_and_close_the_weather_menu() {
    let mut game = Engine::new(WEATHER_MENU);
    game.tick();
    assert_eq!(game.set(), "gameplay");
    assert_eq!(game.hud_shown(), Some(false));

    game.tap(Key::F1);
    assert_eq!(game.set(), "menu");
    assert_eq!(game.hud_shown(), Some(true));
    game.tap(Key::F1);
    assert_eq!(game.set(), "gameplay");
    assert_eq!(game.hud_shown(), Some(false));

    let select: Button = "GamepadSelect".parse().unwrap();
    game.tap(select);
    assert_eq!(game.set(), "menu");
    game.tap(select);
    assert_eq!(game.set(), "gameplay");
}

#[test]
fn a_drag_on_a_bar_sets_that_weather_and_moves_its_bar() {
    let mut game = Engine::new(WEATHER_MENU);
    game.tap(Key::F1);

    let (x, y) = bar_at(1, 0.5);
    game.point(x, y);
    game.tick();
    game.input.press(MouseButton::MouseLeft);
    game.tick();
    assert!((game.host.weather().rain - 0.5).abs() < 0.01);
    // The drag goes on past the end of the bar, and off it sideways.
    let (x, _) = bar_at(1, 2.0);
    game.point(x, y + 200.0);
    game.tick();
    assert!((game.host.weather().rain - 1.0).abs() < 0.01);
    let (x, _) = bar_at(1, 0.25);
    game.point(x, y);
    game.tick();
    assert!((game.host.weather().rain - 0.25).abs() < 0.01);
    game.input.release(MouseButton::MouseLeft);
    // Let go, the bar is left where it was.
    let (x, _) = bar_at(1, 0.9);
    game.point(x, y);
    game.ticks(3);
    assert!((game.host.weather().rain - 0.25).abs() < 0.01);

    // The wind bar goes to 20 metres a second.
    let (x, y) = bar_at(2, 0.5);
    game.point(x, y);
    game.input.press(MouseButton::MouseLeft);
    game.ticks(2);
    assert!((game.wind() - 10.0).abs() < 0.1, "{}", game.wind());
}

#[test]
fn the_wheel_and_the_keys_change_the_chosen_row() {
    let mut game = Engine::new(WEATHER_MENU);
    game.tap(Key::F1);
    game.input.roll_wheel(2.0);
    game.tick();
    assert!((game.host.weather().rain - 0.1).abs() < 0.01);
    game.input.roll_wheel(-1.0);
    game.tick();
    assert!((game.host.weather().rain - 0.05).abs() < 0.01);

    // Down chooses the wind, and right makes it blow harder by one.
    let before = game.wind();
    game.tap(Key::ArrowDown);
    game.tap(Key::ArrowRight);
    assert!(
        (game.wind() - (before + 1.0)).abs() < 0.1,
        "{}",
        game.wind()
    );
    // Up goes back to the rain, and past the top comes round to the cloud.
    game.tap(Key::ArrowUp);
    game.tap(Key::ArrowUp);
    let cloud = game.host.weather().cloud_cover;
    game.tap(Key::ArrowLeft);
    assert!((game.host.weather().cloud_cover - (cloud - 0.05)).abs() < 0.01);
}

#[test]
fn the_mouse_chooses_the_row_it_is_over() {
    let mut game = Engine::new(WEATHER_MENU);
    game.tap(Key::F1);
    let (x, y) = bar_at(3, 0.5);
    game.point(x, y);
    game.tick();
    game.input.roll_wheel(1.0);
    game.tick();
    assert!((game.host.weather().cloud_cover - 0.55).abs() < 0.01);
}

#[test]
fn the_weather_menu_given_the_same_input_twice_gives_the_same_fingerprint() {
    let run = |open_it: bool| {
        let mut game = Engine::new(WEATHER_MENU);
        if open_it {
            game.tap(Key::F1);
            let (x, y) = bar_at(2, 0.3);
            game.point(x, y);
            game.input.press(MouseButton::MouseLeft);
            game.ticks(4);
            game.input.release(MouseButton::MouseLeft);
            game.input.roll_wheel(3.0);
            game.ticks(4);
            game.tap(Key::F1);
        } else {
            game.ticks(14);
        }
        (game.host.rules_fingerprint(), game.host.weather())
    };
    assert_eq!(run(true), run(true));
    // The fingerprint is of what the rules did, so it is not one and the
    // same whatever happened.
    assert_ne!(run(true).0, run(false).0);
    assert_ne!(run(true).1, run(false).1);
}

/// Steps the debug menu's cursor with the arrow keys and confirms with Enter.
fn choose(game: &mut Engine, downs: u32) {
    for _ in 0..downs {
        game.tap(Key::ArrowDown);
    }
    game.tap(Key::Enter);
}

#[test]
fn the_debug_menu_changes_how_the_player_moves_and_how_fast_time_goes() {
    let mut game = Engine::new(DEBUG_MENU);
    game.host.set_position(PLAYER, [3.0, 0.0, 4.0]);
    game.tick();
    assert_eq!(game.set(), "gameplay");
    assert_eq!(game.hud_shown(), Some(false));

    game.tap(Key::F7);
    assert_eq!(game.set(), "menu");
    // The first row is Walk, and the second Fly.
    choose(&mut game, 1);
    assert_eq!(game.host.movement(), Movement::Fly);
    choose(&mut game, 1);
    assert_eq!(game.host.movement(), Movement::Noclip);
    choose(&mut game, 0);
    assert_eq!(game.host.movement(), Movement::Noclip);

    // Teleport to start, facing north.
    game.host.set_position(PLAYER, [50.0, 0.0, 50.0]);
    choose(&mut game, 1);
    let teleport = game.host.take_teleport().expect("the player was sent back");
    assert_eq!(teleport.position, [3.0, 0.0, 4.0]);
    assert_eq!(teleport.facing, Some([0.0, 0.0, -1.0]));

    // Time 0.25, with the keys, and Time 4 with a click on its row.
    choose(&mut game, 2);
    assert_eq!(game.host.time_speed(), 0.25);
    game.point(560.0, 540.0 + 3.5 * 36.0);
    game.input.press(MouseButton::MouseLeft);
    game.tick();
    assert_eq!(game.host.time_speed(), 4.0);
    game.input.release(MouseButton::MouseLeft);
    game.tick();

    game.tap(Key::F7);
    assert_eq!(game.set(), "gameplay");
    assert_eq!(game.hud_shown(), Some(false));
}

#[test]
fn the_debug_menu_shows_where_the_player_is_how_fast_and_which_way() {
    let mut game = Engine::new(DEBUG_MENU);
    game.host.set_position(PLAYER, [3.0, 1.0, 4.0]);
    game.host.set_motion(
        PLAYER,
        Motion {
            velocity: [3.0, 0.0, 4.0],
            eye: [3.0, 2.6, 4.0],
            aim: [0.0, 0.0, -1.0],
            grounded: false,
            crouched: false,
            sprinting: false,
            swimming: false,
            on_ladder: false,
            climbing: false,
            dodging: false,
            aim_point: [0.0, 1.0, -1000.0],
            aim_blocked: false,
        },
    );
    game.tick();
    game.host.draw(1.0);
    let words = game
        .pieces()
        .into_iter()
        .find_map(|piece| match piece.shape {
            HudShape::Text { text, .. } if text.starts_with("Position") => Some(text),
            _ => None,
        })
        .expect("the readout is on the HUD");
    assert_eq!(
        words,
        "Position 3.0 1.0 4.0, speed 5.0, facing 0.00 0.00 -1.00, grounded false"
    );
}

#[test]
fn f12_shows_and_hides_the_frame_meter() {
    let mut game = Engine::new(FRAME_METER);
    game.tick();
    assert_eq!(game.hud_shown(), Some(false));
    game.tap(Key::F12);
    assert_eq!(game.hud_shown(), Some(true));
    let pieces = game.pieces();
    assert!(
        pieces
            .iter()
            .any(|piece| matches!(piece.shape, HudShape::Graph { .. }))
    );
    game.tap(Key::F12);
    assert_eq!(game.hud_shown(), Some(false));

    for _ in 0..30 {
        game.host.frame_drawn(0.02);
    }
    game.host.draw(1.0);
    let said = game
        .pieces()
        .into_iter()
        .find_map(|piece| match piece.shape {
            HudShape::Text { text, .. } => Some(text),
            _ => None,
        });
    assert_eq!(said.as_deref(), Some("50 fps (low 50), 20.0 ms"));
}

#[test]
fn f8_clears_once_for_each_press() {
    let mut game = Engine::new(CLEAR_KEY);
    game.ticks(3);
    assert!(printed().is_empty());
    game.input.press(Key::F8);
    game.ticks(5);
    assert_eq!(printed(), ["cleared"]);
    game.input.release(Key::F8);
    game.tap(Key::F8);
    assert_eq!(printed(), ["cleared"]);
    game.tap(GamepadButton::LeftBumper);
    assert_eq!(printed(), ["cleared"]);
}

#[test]
fn a_spray_starts_at_the_eyes_and_goes_where_the_player_looks_while_fire_is_held() {
    let mut game = Engine::new(SPRAY_AIM);
    game.host.set_motion(
        PLAYER,
        Motion {
            velocity: [0.0; 3],
            eye: [1.0, 1.6, 2.0],
            aim: [0.0, 0.0, -1.0],
            grounded: true,
            crouched: false,
            sprinting: false,
            swimming: false,
            on_ladder: false,
            climbing: false,
            dodging: false,
            aim_point: [0.0, 1.0, -1000.0],
            aim_blocked: false,
        },
    );
    game.ticks(2);
    assert!(printed().is_empty());
    game.input.press(MouseButton::MouseLeft);
    game.ticks(3);
    let lines = printed();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines
            .iter()
            .all(|line| line.contains("1, 1.6, 2") && line.contains("0, 0, -1")),
        "{lines:?}"
    );
    game.input.release(MouseButton::MouseLeft);
    game.ticks(2);
    assert!(printed().is_empty());
}
