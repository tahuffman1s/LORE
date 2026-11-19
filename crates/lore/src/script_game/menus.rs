//! The pause and settings menus a game has from `lore new`, which are the
//! game's own script (`pause_menu.luau`, made of `menu.luau`): each is
//! played the way the window plays a game, with input in, the action set
//! switched when the script asks, and what the script asks of the frame
//! taken from it.

use lore_input::{Button, GamepadButton, Input, Key, MouseButton};
use lore_ui::Draw;

use super::*;
use crate::settings::{self, Settings};
use crate::world::World;

const STEP: f32 = 1.0 / 60.0;

const MAIN: &str = include_str!("../../template/main.luau");
const PAUSE_MENU: &str = include_str!("../../template/pause_menu.luau");
const MENU_KIT: &str = include_str!("../../template/menu.luau");

/// One text the game drew: its words and where its top-left corner is.
type Words = (String, [f32; 2]);

/// A game being played with nobody at the window.
struct Played {
    world: World,
    input: Input,
    /// The settings in effect: the last the game asked the frame for.
    settings: Settings,
    /// How many times the game has changed them.
    changes: u32,
    /// Whether the game has asked to end.
    closed: bool,
    /// The texts of the last frame.
    drawn: Vec<Words>,
    /// Whether a mistake in the game's script fails the test.
    strict: bool,
}

impl Played {
    /// A new game as `lore new` writes it, with `main` as its `main.luau`.
    fn with_main(main: &str, settings: Settings) -> Self {
        Self::opened_with(main, settings, settings)
    }

    /// A new game as `lore new` writes it, with `main` as its `main.luau`,
    /// opened with the settings `opened` and played with the settings
    /// `in_force`: as the editor played a game before it said them, or as
    /// a game opened before the player's file was read is played.
    fn opened_with(main: &str, opened: Settings, in_force: Settings) -> Self {
        let scripts = [
            ("main.luau", main),
            ("pause_menu.luau", PAUSE_MENU),
            ("menu.luau", MENU_KIT),
        ]
        .map(|(name, text)| (name.to_owned(), text.to_owned()));
        let game = ScriptGame::from_sources_with_settings(scripts, &ActionMap::standard(), opened);
        Self::of(game, in_force, true)
    }

    /// The game `lore new` writes.
    fn new() -> Self {
        Self::with_main(&MAIN.replace("{name}", "test"), Settings::default())
    }

    /// One of the example games. The jungle's sounds are made by its
    /// build.py and are not in every checkout, so with `strict` false a
    /// script that asks for one is let be.
    fn example(name: &str, strict: bool) -> Self {
        let folder = format!("{}/../../examples/{name}", env!("CARGO_MANIFEST_DIR"));
        let game = ScriptGame::open(folder, &ActionMap::standard()).unwrap();
        Self::of(game, Settings::default(), strict)
    }

    fn of(game: ScriptGame, settings: Settings, strict: bool) -> Self {
        assert_eq!(game.host().last_error(), None);
        let input = Input::new(game.host().actions());
        let mut played = Self {
            world: World::new(game, 60),
            input,
            settings,
            changes: 0,
            closed: false,
            drawn: Vec::new(),
            strict,
        };
        played.step();
        played
    }

    /// One tick and the frame after it, which says the settings in force,
    /// as the window does.
    fn step(&mut self) {
        self.world.tick(self.input.tick(), STEP);
        let mut frame = Frame::new(1.0).with_settings(self.settings);
        self.world.frame(&mut frame);
        if self.strict {
            assert_eq!(self.host().last_error(), None);
        }
        if let Some(set) = frame.action_set() {
            self.input.set_active_set(set);
        }
        if let Some(settings) = frame.take_settings() {
            self.settings = settings;
            self.changes += 1;
        }
        self.closed |= frame.closing();
        self.drawn = frame
            .take_ui()
            .draw()
            .iter()
            .filter_map(|draw| match draw {
                Draw::Text { text, position, .. } => Some((text.to_owned(), position)),
                Draw::Rect { .. } => None,
            })
            .collect();
    }

    /// Presses and lets go of a button, with a tick for each.
    fn tap(&mut self, button: impl Into<Button> + Copy) {
        self.input.press(button);
        self.step();
        self.input.release(button);
        self.step();
    }

    /// Puts the pointer on the text that says `words`, a little way in.
    fn point_at(&mut self, words: &str) {
        let (_, [x, y]) = self.find(words);
        // A frame with no window is 1920 by 1080: pixels are HUD units.
        self.input
            .set_pointer(Some([x + 6.0, y + 8.0]), [1920.0, 1080.0]);
        self.step();
    }

    /// Clicks the text that says `words`.
    fn click(&mut self, words: &str) {
        self.point_at(words);
        self.tap(MouseButton::MouseLeft);
    }

    fn find(&self, words: &str) -> Words {
        (self.drawn.iter())
            .find(|(text, _)| text == words)
            .unwrap_or_else(|| panic!("\"{words}\" is not among {:?}", self.words()))
            .clone()
    }

    fn host(&self) -> &ScriptHost {
        self.world.game().host()
    }

    /// The texts of the last frame.
    fn words(&self) -> Vec<&str> {
        self.drawn.iter().map(|(text, _)| text.as_str()).collect()
    }

    fn shows(&self, words: &str) -> bool {
        self.words().contains(&words)
    }

    /// The set of actions the window is in, by name.
    fn set(&self) -> &str {
        self.input.map().set_name(self.input.active_set())
    }

    /// What the menu says beside the row called `label`.
    fn value_of(&self, label: &str) -> String {
        let at = (self.drawn.iter())
            .position(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("\"{label}\" is not among {:?}", self.words()));
        self.drawn[at + 1].0.clone()
    }
}

#[test]
fn escape_pauses_the_world_and_shows_the_menu_and_resume_lets_it_go_on() {
    let mut played = Played::new();
    assert_eq!(played.set(), "gameplay");
    assert_eq!(played.words(), Vec::<&str>::new(), "no menu yet");
    played.step();
    let before = played.world.tick_number();
    assert!(before > 0);

    played.tap(Key::Escape);
    assert!(played.host().paused());
    assert_eq!(played.set(), "menu");
    for words in ["Paused", "Resume", "Settings", "Quit"] {
        assert!(played.shows(words), "{:?}", played.words());
    }
    // The world stands still for as long as the menu is open.
    let paused_at = played.world.tick_number();
    let time = played.host().time();
    for _ in 0..30 {
        played.step();
    }
    assert_eq!(played.world.tick_number(), paused_at);
    assert_eq!(played.host().time(), time);
    assert!(paused_at <= before + 2);

    // Resume is where the menu opens: Enter presses it.
    played.tap(Key::Enter);
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");
    assert_eq!(played.words(), Vec::<&str>::new());
    played.step();
    assert!(played.world.tick_number() > paused_at);
    assert!(!played.closed);

    // Escape closes it from its first screen too, and Start from anywhere.
    played.tap(Key::Escape);
    assert!(played.host().paused());
    played.tap(Key::Escape);
    assert!(!played.host().paused());
    played.tap(GamepadButton::GamepadStart);
    played.tap(GamepadButton::DpadDown);
    played.tap(GamepadButton::GamepadSouth);
    assert!(played.shows("Sound"), "{:?}", played.words());
    played.tap(GamepadButton::GamepadStart);
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");
}

#[test]
fn a_gamepad_alone_changes_every_setting_and_the_file_keeps_them() {
    let mut played = Played::new();
    played.tap(GamepadButton::GamepadStart);
    played.tap(GamepadButton::DpadDown);
    played.tap(GamepadButton::GamepadSouth);
    // One tab for each heading, the first in front.
    for heading in ["Sound", "Controls", "Graphics"] {
        assert!(played.shows(heading), "{:?}", played.words());
    }
    assert!(played.shows("Music volume") && !played.shows("Vsync"));

    // Left on every row of every tab: a number goes down a step, a choice
    // back a word, and a toggle over.
    let usual = Settings::default();
    let told = usual.described();
    let mut heading = told[0].group.clone();
    for setting in &told {
        if setting.group != heading {
            heading.clone_from(&setting.group);
            played.tap(GamepadButton::RightBumper);
        }
        assert!(played.shows(&setting.label), "{:?}", played.words());
        played.tap(GamepadButton::DpadLeft);
        played.tap(GamepadButton::DpadDown);
    }
    let chosen = played.settings;
    assert_eq!(
        chosen,
        Settings {
            volume: 95.0,
            music_volume: 95.0,
            effects_volume: 95.0,
            sensitivity: chosen.sensitivity,
            invert_look: true,
            rumble: 95.0,
            ui_size: 95.0,
            field_of_view: 73.0,
            brightness: 45.0,
            upscaling: lore_render::Upscaling::Native,
            vsync: false,
            frame_rate_limit: 390.0,
            reflections: false,
            light_shafts: false,
            lamp_shadows: lore_render::LampShadows::Low,
            bounce: lore_render::Bounce::Low,
            ambient_occlusion: lore_render::AmbientOcclusion::Low,
        }
    );
    assert!((chosen.sensitivity - 0.9).abs() < 1e-6);
    assert_eq!(
        played.changes as usize,
        told.len(),
        "one change for each setting"
    );
    // The menu shows what is in force.
    assert_eq!(played.value_of("Field of view"), "73");
    assert_eq!(played.value_of("Upscaling"), "<  native  >");
    assert_eq!(played.value_of("Vsync"), "off");
    // Changing the rumble lets the hand feel how strong it now is.
    assert!(played.host().last_error().is_none());

    // What the engine saves of it is read back the same.
    let folder = std::env::temp_dir().join(format!("lore-menu-settings-{}", std::process::id()));
    let path = folder.join("settings.toml");
    settings::save(&path, &chosen).unwrap();
    assert_eq!(settings::load(&path), (chosen, Vec::new()));
    let _ = std::fs::remove_dir_all(&folder);

    // The row after the last setting puts every one of them back.
    assert!(played.shows("Reset all to defaults"));
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.settings, usual);
    assert_eq!(played.value_of("Vsync"), "on");

    // B goes back to the first screen, and B again to the game.
    played.tap(GamepadButton::GamepadEast);
    assert!(played.shows("Resume") && played.host().paused());
    played.tap(GamepadButton::GamepadEast);
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");
}

#[test]
fn the_menu_starts_with_the_settings_the_player_has() {
    let theirs = Settings {
        volume: 40.0,
        upscaling: lore_render::Upscaling::Performance,
        ..Settings::default()
    };
    let main = "print(game.settings.get(\"volume\"))\nrequire(\"./pause_menu\")\n";
    let mut played = Played::with_main(main, theirs);
    played.tap(Key::Escape);
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    assert_eq!(played.value_of("Volume"), "40");
    played.tap(Key::E);
    played.tap(Key::E);
    assert_eq!(played.value_of("Upscaling"), "<  performance  >");
    // Nothing was changed, so nothing was asked of the engine.
    assert_eq!(played.changes, 0);
    assert_eq!(played.settings, theirs);
}

#[test]
fn a_game_opened_with_other_settings_changes_only_what_its_menu_changes() {
    // As the editor played a game: opened with a new player's settings,
    // while the player had widened the field of view in a game played
    // from the editor before it.
    let theirs = Settings {
        field_of_view: 110.0,
        ..Settings::default()
    };
    let main = MAIN.replace("{name}", "test");
    let mut played = Played::opened_with(&main, Settings::default(), theirs);
    played.tap(Key::Escape);
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    played.tap(Key::ArrowLeft);
    assert_eq!(played.value_of("Volume"), "95");
    // The volume changed, and nothing else went back to what the game was
    // opened with.
    assert_eq!(played.changes, 1);
    assert_eq!(
        played.settings,
        Settings {
            volume: 95.0,
            ..theirs
        }
    );
    // The menu shows the settings in force.
    played.tap(Key::E);
    played.tap(Key::E);
    assert_eq!(played.value_of("Field of view"), "110");
}

#[test]
fn a_frame_rate_limit_of_0_is_shown_as_no_limit() {
    let mut played = Played::with_main(
        &MAIN.replace("{name}", "test"),
        Settings {
            frame_rate_limit: 30.0,
            reflections: false,
            ..Settings::default()
        },
    );
    played.tap(Key::Escape);
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    played.tap(Key::E);
    played.tap(Key::E);
    assert_eq!(played.value_of("Frame rate limit"), "30");
    // Down past the size, the field of view, the brightness, the upscaling
    // and vsync.
    for _ in 0..5 {
        played.tap(Key::ArrowDown);
    }
    played.tap(Key::ArrowLeft);
    assert_eq!(played.settings.frame_rate_limit, 0.0);
    assert_eq!(played.value_of("Frame rate limit"), "no limit");
    played.tap(Key::ArrowRight);
    assert_eq!(played.settings.frame_rate_limit, 30.0);
}

#[test]
fn a_mouse_alone_works_the_menu() {
    let mut played = Played::new();
    played.tap(Key::Escape);
    played.click("Settings");
    assert!(played.shows("Effects volume"), "{:?}", played.words());
    // A click on a tab brings it to the front.
    played.click("Graphics");
    assert!(played.shows("Vsync"), "{:?}", played.words());
    // A click on a toggle turns it over, and one on the right half of a
    // choice goes on a word.
    played.click("Vsync");
    assert!(!played.settings.vsync);
    let (_, [x, y]) = played.find("<  quality  >");
    played
        .input
        .set_pointer(Some([x + 140.0, y + 8.0]), [1920.0, 1080.0]);
    played.step();
    played.tap(MouseButton::MouseLeft);
    assert_eq!(played.settings.upscaling, lore_render::Upscaling::Balanced);
    // The wheel changes the row the pointer chose.
    played.point_at("Field of view");
    played.input.roll_wheel(1.0);
    played.step();
    assert_eq!(played.settings.field_of_view, 75.0);
    played.click("Back");
    played.click("Resume");
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");
}

#[test]
fn quitting_asks_first_and_starts_on_the_safe_answer() {
    let mut played = Played::new();
    played.tap(Key::Escape);
    played.tap(Key::ArrowUp);
    played.tap(Key::Enter);
    assert!(played.shows("Quit the game?"), "{:?}", played.words());
    assert!(played.shows("Keep playing"));
    assert!(!played.closed);
    // Enter again, as a hurried player presses it, keeps playing.
    played.tap(Key::Enter);
    assert!(!played.closed);
    assert!(played.shows("Resume") && played.host().paused());

    // Back on the first screen Quit is still the row chosen. Asked again,
    // the safe answer is again where it starts, whatever was chosen the
    // last time.
    played.tap(Key::Enter);
    played.tap(Key::ArrowDown);
    played.tap(Key::Escape);
    assert!(played.shows("Resume") && !played.closed);
    played.tap(Key::Enter);
    assert!(played.shows("Quit the game?"));
    played.tap(Key::Enter);
    assert!(played.shows("Resume") && !played.closed);

    played.tap(Key::Enter);
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    assert!(played.closed, "the game ends when the player says so");
}

#[test]
fn the_menu_goes_back_to_the_action_set_and_the_pause_the_game_had() {
    // A game with a shop of its own, in the "menu" set, that pauses the
    // game itself and opens the pause menu from in there.
    let main = r#"
        game.input.bind("shop", { "F3" })
        game.input.bind("help", { "F4" })
        local pauseMenu = require("./pause_menu")
        game.onTick(function()
            if game.input.justPressed("shop") then
                game.input.useSet("menu")
                game.pause()
            elseif game.input.justPressed("help") then
                pauseMenu.open()
            end
        end)
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::F3);
    assert_eq!(played.set(), "menu");
    assert!(played.host().paused());
    played.tap(Key::F4);
    assert!(played.shows("Resume"));
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    played.tap(Key::Escape);
    played.tap(Key::Escape);
    assert!(!played.shows("Resume"));
    assert_eq!(played.set(), "menu", "the shop's set, not gameplay");
    assert!(
        played.host().paused(),
        "the shop's pause is the shop's to end"
    );
}

#[test]
fn the_alley_pauses_under_its_pause_menu_and_keeps_its_own_menus_apart() {
    let mut played = Played::example("alley", true);
    played.tap(Key::F1);
    assert!(played.shows("Weather") && !played.host().paused());
    // Escape closes the alley's own menu; it does not pause from in there.
    played.tap(Key::Escape);
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");

    played.tap(Key::Escape);
    assert!(played.host().paused() && played.shows("Resume"));
    // The alley's keys wait while the game is paused.
    played.tap(Key::F1);
    played.tap(Key::F12);
    assert!(
        played.shows("Resume") && !played.shows("Weather"),
        "{:?}",
        played.words()
    );
    played.tap(Key::Escape);
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");
    assert!(!played.closed);
}

#[test]
fn the_jungle_has_the_pause_menu_beside_its_weather_panel() {
    let mut played = Played::example("jungle", false);
    played.tap(Key::Escape);
    assert!(played.host().paused() && played.shows("Resume"));
    assert_eq!(played.set(), "menu");
    played.tap(Key::Escape);
    assert!(!played.host().paused());
    // The weather panel opens and closes as it did, without pausing.
    played.tap(Key::E);
    assert_eq!(played.set(), "menu");
    played.tap(Key::Escape);
    assert_eq!(played.set(), "gameplay");
    assert!(!played.host().paused() && !played.shows("Resume"));
}
