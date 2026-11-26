//! The pause and settings menus every game has from the engine
//! (`game.pauseMenu`, made of `game.menu`): each is
//! played the way the window plays a game, with input in, the action set
//! switched when the script asks, and what the script asks of the frame
//! taken from it.

use lore_input::{Button, GamepadButton, Input, Key, MouseButton};
use lore_ui::Draw;

use super::*;
use crate::settings::{self, Settings};
use crate::world::World;

const STEP: f32 = 1.0 / 60.0;

const TEMPLATE: &str = include_str!("../../template/main.luau");

/// The template's `main.luau` without its title screen: these tests are
/// about the HUD and the engine's menus, which the title would cover. The
/// title has its own test below.
fn main_script() -> String {
    let from = TEMPLATE.find("-- The title screen").unwrap();
    let to = TEMPLATE.find("-- The welcome goes away").unwrap();
    format!("{}{}", &TEMPLATE[..from], &TEMPLATE[to..]).replace("    title:show()\n", "")
}

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
        let scripts = [("main.luau".to_owned(), main.to_owned())];
        let game = ScriptGame::from_sources_with_settings(scripts, &ActionMap::standard(), opened);
        Self::of(game, in_force, true)
    }

    /// The game `lore new` writes.
    fn new() -> Self {
        Self::with_main(
            &main_script().replace("{name}", "test"),
            Settings::default(),
        )
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
        let device = self.input.last_device();
        let style = lore_input::GamepadStyle::default();
        self.world.tick(self.input.tick(), STEP);
        let mut frame = Frame::new(1.0)
            .with_settings(self.settings)
            .with_device(device, style);
        self.world.frame(&mut frame);
        if self.strict {
            assert_eq!(self.host().last_error(), None);
        }
        if let Some(set) = frame.action_set() {
            self.input.set_active_set(set);
        }
        if let Some(on) = frame.typing() {
            self.input.set_typing(on);
        }
        if let Some(hears) = frame.listening() {
            self.input.set_listening(hears);
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
                Draw::Rect { .. }
                | Draw::Turned { .. }
                | Draw::Box { .. }
                | Draw::Shadow { .. } => None,
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
fn a_new_game_starts_under_its_title_screen_and_start_hides_it() {
    let folder = std::env::temp_dir().join(format!("lore-title-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    crate::scaffold::new_game(&folder).unwrap();
    let game = ScriptGame::open(&folder, &ActionMap::standard()).unwrap();
    let mut played = Played::of(game, Settings::default(), true);
    played.step();
    assert!(played.shows("Start"), "{:?}", played.words());
    assert!(played.shows("A new adventure"), "{:?}", played.words());
    assert!(played.host().paused(), "the title holds the game still");
    assert_eq!(played.set(), "menu");

    // Start has focus from the first moment: confirm presses it.
    played.tap(Key::Enter);
    played.step();
    assert!(!played.shows("Start"), "{:?}", played.words());
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");
    assert!(played.shows("Jumps: 0"), "{:?}", played.words());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_new_game_greets_and_counts_jumps_on_its_screen() {
    let mut played = Played::new();
    assert!(played.shows("Welcome to test!"), "{:?}", played.words());
    assert!(played.shows("Jumps: 0"), "{:?}", played.words());

    played.tap(Key::Space);
    assert!(played.shows("Jumps: 1"), "{:?}", played.words());
    assert!(!played.shows("Jumps: 0"));

    // The welcome goes away after eight seconds; the count stays.
    for _ in 0..8 * 60 {
        played.step();
    }
    assert!(!played.shows("Welcome to test!"), "{:?}", played.words());
    assert!(played.shows("Jumps: 1"), "{:?}", played.words());
}

#[test]
fn escape_pauses_the_world_and_shows_the_menu_and_resume_lets_it_go_on() {
    let mut played = Played::new();
    assert_eq!(played.set(), "gameplay");
    let hud = [
        "Welcome to test!",
        "Walk to the tower. Space or the bottom face button jumps.",
        "Jumps: 0",
    ];
    assert_eq!(
        played.words(),
        hud,
        "only the new game's own HUD, no menu yet"
    );
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
    assert_eq!(played.words(), hud, "the menu is gone, the HUD is back");
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
    // back a word, and a toggle over. Night brightness starts at its least,
    // so it goes right, up a step.
    let usual = Settings::default();
    let told = usual.described();
    let mut heading = told[0].group.clone();
    for setting in &told {
        if setting.group != heading {
            heading.clone_from(&setting.group);
            played.tap(GamepadButton::RightBumper);
        }
        assert!(played.shows(&setting.label), "{:?}", played.words());
        if setting.name == "night_brightness" {
            played.tap(GamepadButton::DpadRight);
        } else {
            played.tap(GamepadButton::DpadLeft);
        }
        played.tap(GamepadButton::DpadDown);
    }
    let chosen = played.settings;
    assert_eq!(
        chosen,
        Settings {
            volume: 95.0,
            music_volume: 95.0,
            effects_volume: 95.0,
            mouse_sensitivity: chosen.mouse_sensitivity,
            stick_sensitivity: chosen.stick_sensitivity,
            invert_look: true,
            rumble: 95.0,
            ui_size: 95.0,
            field_of_view: 73.0,
            brightness: 45.0,
            night_brightness: 5.0,
            upscaling: lore_render::Upscaling::Native,
            vsync: false,
            frame_rate_limit: 390.0,
            reflections: false,
            light_shafts: false,
            lamp_shadows: lore_render::LampShadows::Low,
            bounce: lore_render::Bounce::Low,
            ambient_occlusion: lore_render::AmbientOcclusion::Low,
            contact_shadows: false,
            anisotropic_filtering: lore_render::AnisotropicFiltering::X8,
            bloom: 95.0,
            vignette: 95.0,
            film_grain: 95.0,
            chromatic_aberration: 95.0,
            sharpening: 25.0,
        }
    );
    // Each went down one step of 0.05.
    assert!((chosen.mouse_sensitivity - 0.95).abs() < 1e-6);
    assert!((chosen.stick_sensitivity - 0.95).abs() < 1e-6);
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
    let main = "print(game.settings.get(\"volume\"))\n";
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
fn a_script_says_how_sharp_textures_are_at_a_slant() {
    // The first tick reads what a new player has and asks for less, in
    // capitals; the ticks after it read what is then in force. A wrong
    // answer is a mistake in the script, which fails the test.
    let main = r#"
        local ticks = 0
        game.onTick(function()
            ticks += 1
            local now = game.settings.get("anisotropic_filtering")
            if ticks == 1 then
                assert(now == "16x", now)
                game.settings.set("anisotropic_filtering", "4X")
            else
                assert(now == "4x", now)
            end
        end)
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.step();
    played.step();
    assert_eq!(
        played.settings,
        Settings {
            anisotropic_filtering: lore_render::AnisotropicFiltering::X4,
            ..Settings::default()
        }
    );
    assert_eq!(played.changes, 1);
    // The list a menu is built from says what it is and what it can be.
    let told = played.settings.described();
    let setting = told
        .iter()
        .find(|setting| setting.name == "anisotropic_filtering")
        .unwrap();
    assert_eq!(setting.label, "Anisotropic filtering");
    assert_eq!(setting.group, "Graphics");
    assert_eq!(
        setting.kind,
        lore_script::SettingKind::Choice {
            options: ["off", "2x", "4x", "8x", "16x"].map(str::to_owned).to_vec()
        }
    );
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
    let main = main_script().replace("{name}", "test");
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
        &main_script().replace("{name}", "test"),
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
    // Down past the size, the field of view, the brightness, the night
    // brightness, the upscaling and vsync.
    for _ in 0..6 {
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
        game.input.bind("shop", { "F3", "LeftBumper" })
        game.input.bind("help", { "F4", "GamepadSelect" })
        local pauseMenu = game.pauseMenu
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

#[test]
fn the_planetarium_holds_the_observer_still_and_the_walking_keys_zoom() {
    let mut played = Played::example("planetarium", true);
    assert!(
        played.shows("Death Valley, California") && played.shows("36.5° N, 116.9° W"),
        "{:?}",
        played.words()
    );
    // Its first sky is seen at its widest, through the player's own setting.
    assert_eq!(played.settings.field_of_view, 110.0);
    played.input.press(Key::W);
    for _ in 0..30 {
        played.step();
    }
    played.input.release(Key::W);
    played.step();
    assert!(played.settings.field_of_view < 100.0);
    let [x, y, z] = played.world.player().position;
    assert!(
        x.abs() + y.abs() + z.abs() < 0.01,
        "the observer moved to {x} {y} {z}"
    );
    // The pause menu shows the player's own field of view, and closing it
    // zooms in again.
    played.tap(Key::Escape);
    assert!(played.host().paused());
    assert_eq!(
        played.settings.field_of_view,
        Settings::default().field_of_view
    );
    played.tap(Key::Escape);
    assert!(played.settings.field_of_view < 100.0);
}

#[test]
fn the_planetarium_jumps_to_an_eclipse_from_its_menu() {
    let mut played = Played::example("planetarium", true);
    played.tap(Key::E);
    assert_eq!(played.set(), "menu");
    assert!(played.shows("Eclipses") && played.shows("Total solar eclipse, 2024"));
    played.tap(Key::Enter);
    played.step();
    assert_eq!(played.set(), "gameplay");
    assert!(
        played.shows("Total solar eclipse, 100% of the sun covered"),
        "{:?}",
        played.words()
    );
    assert!(played.shows("STOPPED"));
    // Time runs faster with D, as the left stick would push it.
    played.tap(Key::D);
    assert!(played.shows("REAL TIME"), "{:?}", played.words());
}

#[test]
fn the_planetarium_has_no_ground_until_g_brings_it_back() {
    let mut played = Played::example("planetarium", true);
    let ground = |played: &mut Played| {
        // The Sky tab, the sixth, from the Eclipses the menu opens at.
        played.tap(Key::E);
        for _ in 0..5 {
            played.tap(Key::E);
        }
        let shown = played.value_of("Ground");
        played.tap(Key::Escape);
        shown
    };
    assert_eq!(ground(&mut played), "off");
    played.tap(Key::G);
    assert_eq!(ground(&mut played), "on");
    played.tap(Key::G);
    assert_eq!(ground(&mut played), "off");
}

#[test]
fn every_planetarium_event_lands_on_what_it_promises() {
    const TOTAL_SOLAR: &str = "Total solar eclipse, 100% of the sun covered";
    const TOTAL_LUNAR: &str = "Total lunar eclipse";
    let eclipses = [
        TOTAL_SOLAR,
        TOTAL_SOLAR,
        TOTAL_SOLAR,
        "Annular solar eclipse",
        TOTAL_SOLAR,
        TOTAL_SOLAR,
        TOTAL_SOLAR,
        TOTAL_SOLAR,
        TOTAL_LUNAR,
        TOTAL_LUNAR,
        TOTAL_LUNAR,
    ];
    let night = [
        "The Perseids are falling",
        "The Geminids are falling",
        "The Leonids are falling",
        "",
        "",
        "",
        "The northern lights are out",
        "",
        "",
    ];
    let mut played = Played::example("planetarium", true);
    for (tab, list) in [eclipses.as_slice(), night.as_slice()]
        .into_iter()
        .enumerate()
    {
        for (row, expected) in list.iter().enumerate() {
            played.tap(Key::E);
            for _ in 0..tab {
                played.tap(Key::E);
            }
            // A menu keeps its chosen row, so the next event is one down.
            if row > 0 {
                played.tap(Key::ArrowDown);
            }
            played.tap(Key::Enter);
            played.step();
            assert_eq!(played.set(), "gameplay");
            if expected.is_empty() {
                continue;
            }
            assert!(
                played.words().iter().any(|words| words.contains(expected)),
                "event {} of tab {tab}: {:?}",
                row + 1,
                played.words()
            );
        }
    }
}

#[test]
fn the_planetarium_time_tab_runs_and_turns_the_clock() {
    let mut played = Played::example("planetarium", true);
    played.tap(Key::E);
    played.tap(Key::E);
    played.tap(Key::E);
    played.tap(Key::E);
    assert!(played.shows("Time lapse"), "{:?}", played.words());
    assert_eq!(played.value_of("Date"), "Sun 4 Aug 2024");
    assert_eq!(played.value_of("Time lapse"), "<  Real time  >");
    played.tap(Key::ArrowRight);
    assert_eq!(played.value_of("Time lapse"), "<  10x  >");
    played.tap(Key::ArrowDown);
    played.tap(Key::ArrowRight);
    assert_eq!(played.value_of("Hour"), "22:45");
    // Down past Day, Month and Year to "Next sunrise".
    for _ in 0..4 {
        played.tap(Key::ArrowDown);
    }
    played.tap(Key::Enter);
    assert_eq!(played.value_of("Date"), "Mon 5 Aug 2024");
    let hour = played.value_of("Hour");
    assert!(
        hour.as_str() > "04:30" && hour.as_str() < "05:30",
        "sunrise at {hour}"
    );
}

#[test]
fn the_planetarium_month_slider_stops_at_every_month() {
    let mut played = Played::example("planetarium", true);
    played.tap(Key::E);
    played.tap(Key::E);
    played.tap(Key::E);
    played.tap(Key::E);
    // Down from "Time lapse" past Hour and Day to Month, and on to August's
    // last day first, so a month change has a day to keep inside it.
    played.tap(Key::ArrowDown);
    played.tap(Key::ArrowDown);
    for _ in 0..27 {
        played.tap(Key::ArrowRight);
    }
    assert_eq!(played.value_of("Date"), "Sat 31 Aug 2024");
    played.tap(Key::ArrowDown);
    played.tap(Key::ArrowRight);
    // 31 August, a month on, is the last day of September, not 1 October.
    assert_eq!(played.value_of("Date"), "Mon 30 Sep 2024");
    for month in ["October", "November", "December", "January"] {
        played.tap(Key::ArrowRight);
        assert_eq!(played.value_of("Month"), month);
    }
}

#[test]
fn the_planetarium_planets_tab_says_where_each_is_and_turns_to_it() {
    let mut played = Played::example("planetarium", true);
    played.tap(Key::E);
    played.tap(Key::E);
    played.tap(Key::E);
    assert!(
        played.shows("Planets") && played.shows("Neptune"),
        "{:?}",
        played.words()
    );
    // Death Valley on a moonless August night: Saturn is up in the south-east.
    let saturn = played.value_of("Saturn");
    assert_eq!(saturn, "22° up, SE, magnitude 0.7");
    // Down past the sun, the moon, Mercury, Venus, Mars and Jupiter.
    for _ in 0..6 {
        played.tap(Key::ArrowDown);
    }
    played.tap(Key::Enter);
    played.step();
    assert_eq!(played.set(), "gameplay");
    // The name under the crosshair is Saturn's.
    assert!(
        played.shows("Saturn") && played.shows("ALT 22°   ·   SE"),
        "{:?}",
        played.words()
    );
}

#[test]
fn a_game_that_is_only_main_luau_has_the_pause_menu_from_the_engine() {
    let mut played = Played::with_main("game.onTick(function() end)", Settings::default());
    assert!(played.words().is_empty());
    played.tap(Key::Escape);
    assert!(played.host().paused() && played.shows("Resume"));
    assert!(played.shows("Paused"), "{:?}", played.words());
    played.tap(Key::Escape);
    assert!(!played.host().paused() && !played.shows("Resume"));
}

#[test]
fn the_pause_menu_is_drawn_over_the_pieces_the_game_makes_after_it() {
    // A game's own HUD, made after the engine's menu was set up and on a
    // layer of its own, is still under the pause menu.
    let main = r#"
        local over = game.hud.box({ width = 8000, height = 4000, layer = 50 })
        over.color = "red"
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::Escape);
    assert!(played.shows("Resume"));
    let mut layers = Vec::new();
    played
        .host()
        .each_hud_piece(|_, piece| layers.push((piece.layer, piece.visible)));
    let shown: Vec<i32> = layers
        .iter()
        .filter(|(_, visible)| *visible)
        .map(|(layer, _)| *layer)
        .collect();
    assert!(shown.windows(2).all(|pair| pair[0] <= pair[1]), "{shown:?}");
    assert_eq!(shown.first(), Some(&50));
    assert_eq!(shown.last(), Some(&100));
}

#[test]
fn a_game_with_its_own_pause_menu_gets_only_that_one() {
    // The game's own pause_menu.luau, as an older game carries it: it makes
    // a line of its own, and the engine's pause menu steps aside.
    let scripts = [
        (
            "main.luau".to_owned(),
            "require('./pause_menu')\nassert(game.pauseMenu.enabled == false)".to_owned(),
        ),
        (
            "pause_menu.luau".to_owned(),
            "game.hud.text('Our own pause menu', { at = 'center' })".to_owned(),
        ),
    ];
    let game = ScriptGame::from_sources_with_settings(
        scripts,
        &ActionMap::standard(),
        Settings::default(),
    );
    let mut played = Played::of(game, Settings::default(), true);
    assert!(played.shows("Our own pause menu"), "{:?}", played.words());
    played.tap(Key::Escape);
    assert!(!played.host().paused());
    assert!(!played.shows("Resume") && !played.shows("Paused"));
}

#[test]
fn pause_menu_enabled_false_turns_it_off_and_closes_it() {
    let main = r#"
        game.input.bind("off", { "F5", "GamepadSelect" })
        game.onTick(function()
            if game.input.justPressed("off") then
                game.pauseMenu.enabled = false
            end
        end)
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::Escape);
    assert!(played.shows("Resume"));
    played.tap(Key::F5);
    assert!(!played.shows("Resume") && !played.host().paused());
    played.tap(Key::Escape);
    assert!(!played.shows("Resume") && !played.host().paused());
}

/// A game that makes a menu of its own with `game.menu`: five rows that
/// each say on screen what the player did to them. F6 or Select opens it.
const SCRIPT_MENU: &str = r#"
    game.input.bind("sound", { "F6", "GamepadSelect" })
    local told = game.hud.text("none", { at = "bottom" })
    local presses, changes = 0, 0
    local quiet = game.menu.button("Quiet", { onPress = function()
        presses += 1
        -- A rule: it is an error from a draw, so this shows it ran on a tick.
        game.settings.set("volume", 50)
        told.text = "pressed " .. presses
    end })
    local volume = game.menu.slider("Volume", {
        step = 0.1,
        format = game.menu.percent,
        onChange = function(v) changes += 1 told.text = string.format("volume %g", v) end,
    })
    local music = game.menu.toggle("Music", {
        onChange = function(on) changes += 1 told.text = "music " .. tostring(on) end,
    })
    local mode = game.menu.choice("Mode", { options = { "a", "b", "c" } })
    mode:onChange(function(word) changes += 1 told.text = "mode " .. word end)
    local tidy = game.menu.button("Tidy", { onPress = function()
        volume.value = 0.5
        volume.label = "Loudness"
        told.text = "changes " .. changes
    end })
    local sound = game.menu.new("Sound", { quiet, volume, music, mode, tidy })
    game.onTick(function()
        if game.input.justPressed("sound") then sound:toggle() end
    end)
"#;

#[test]
fn a_script_menu_opens_and_a_press_runs_on_a_tick() {
    let mut played = Played::with_main(SCRIPT_MENU, Settings::default());
    assert!(!played.shows("Quiet"));
    played.tap(Key::F6);
    assert_eq!(played.set(), "menu");
    for words in ["Sound", "Quiet", "Volume", "Music", "Mode", "Tidy"] {
        assert!(played.shows(words), "{:?}", played.words());
    }
    assert!(
        !played.host().paused(),
        "a menu of the game's own pauses nothing"
    );
    played.tap(Key::Enter);
    assert!(played.shows("pressed 1"), "{:?}", played.words());
    assert!(
        played.changes > 0,
        "onPress ran in a tick and changed a setting"
    );
    // The key that opened it closes it again, and so does Escape.
    played.tap(Key::F6);
    assert_eq!(played.set(), "gameplay");
    assert!(!played.shows("Quiet"));
    played.tap(Key::F6);
    played.tap(Key::Escape);
    assert_eq!(played.set(), "gameplay");
}

#[test]
fn a_change_tells_onchange_the_new_value_and_a_script_setting_the_value_does_not() {
    let mut played = Played::with_main(SCRIPT_MENU, Settings::default());
    played.tap(Key::F6);
    played.tap(Key::ArrowDown);
    played.tap(Key::ArrowRight);
    assert!(played.shows("volume 0.1"), "{:?}", played.words());
    assert_eq!(played.value_of("Volume"), "10%");
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    assert!(played.shows("music true"), "{:?}", played.words());
    assert_eq!(played.value_of("Music"), "on");
    played.tap(Key::ArrowDown);
    played.tap(Key::ArrowRight);
    assert!(played.shows("mode b"), "{:?}", played.words());
    assert_eq!(played.value_of("Mode"), "<  b  >");
    // The button sets the slider's value and label from the script: the
    // screen follows, and onChange is not called (still three changes).
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    assert!(played.shows("changes 3"), "{:?}", played.words());
    assert!(played.shows("Loudness") && !played.shows("Volume"));
    assert_eq!(played.value_of("Loudness"), "50%");
}

#[test]
fn a_script_menu_works_with_keys_a_gamepad_and_the_mouse() {
    let mut played = Played::with_main(SCRIPT_MENU, Settings::default());
    // A gamepad: Select opens it, the d-pad moves and changes, A presses
    // and B goes back.
    played.tap(GamepadButton::GamepadSelect);
    assert_eq!(played.set(), "menu");
    played.tap(GamepadButton::DpadDown);
    played.tap(GamepadButton::DpadRight);
    assert!(played.shows("volume 0.1"), "{:?}", played.words());
    played.tap(GamepadButton::DpadUp);
    played.tap(GamepadButton::GamepadSouth);
    assert!(played.shows("pressed 1"), "{:?}", played.words());
    played.tap(GamepadButton::GamepadEast);
    assert_eq!(played.set(), "gameplay");
    // The mouse: a click presses a button and turns a toggle over.
    played.tap(Key::F6);
    played.click("Quiet");
    assert!(played.shows("pressed 2"), "{:?}", played.words());
    played.click("Music");
    assert!(played.shows("music true"), "{:?}", played.words());
    played.click("Music");
    assert!(played.shows("music false"), "{:?}", played.words());
}

#[test]
fn a_script_menu_opened_over_the_pause_menu_stacks_and_only_the_top_hears() {
    let mut played = Played::with_main(SCRIPT_MENU, Settings::default());
    played.tap(Key::Escape);
    assert!(played.host().paused() && played.shows("Resume"));
    // The game's key opens its menu over the pause menu, which waits under.
    played.tap(Key::F6);
    assert!(
        played.shows("Quiet") && !played.shows("Resume"),
        "{:?}",
        played.words()
    );
    assert_eq!(played.set(), "menu");
    // Enter goes to the menu on top: it presses Quiet, not Resume.
    played.tap(Key::Enter);
    assert!(played.shows("pressed 1") && played.host().paused());
    // Escape closes only the top menu and the pause menu is back as it was.
    played.tap(Key::Escape);
    assert!(
        played.shows("Resume") && !played.shows("Quiet"),
        "{:?}",
        played.words()
    );
    assert!(played.host().paused());
    assert_eq!(played.set(), "menu");
    // Escape again closes the pause menu, and the game is as it was.
    played.tap(Key::Escape);
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");
    assert!(!played.shows("Resume") && !played.shows("Quiet"));
}

#[test]
fn start_closes_every_open_screen_at_once() {
    let mut played = Played::with_main(SCRIPT_MENU, Settings::default());
    played.tap(Key::Escape);
    played.tap(Key::F6);
    played.tap(GamepadButton::GamepadStart);
    assert!(
        !played.shows("Quiet") && !played.shows("Resume"),
        "{:?}",
        played.words()
    );
    assert!(!played.host().paused());
    assert_eq!(played.set(), "gameplay");
}

#[test]
fn the_pause_menu_takes_a_title_and_a_row_of_the_games_own() {
    let main = r#"
        game.pauseMenu.title = "Game paused"
        game.pauseMenu.addRow(game.menu.button("Photo mode", { onPress = function()
            game.pauseMenu.close()
        end }))
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::Escape);
    for words in ["Game paused", "Resume", "Settings", "Photo mode", "Quit"] {
        assert!(played.shows(words), "{:?}", played.words());
    }
    // The row is above Quit: two down from Resume.
    played.tap(Key::ArrowDown);
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    assert!(!played.host().paused() && !played.shows("Resume"));
}

#[test]
fn a_menu_asked_for_wrongly_says_what_to_write() {
    let main = r#"
        local ok, message = pcall(game.menu.choice, "Size", {})
        assert(not ok and string.find(message, "options"), message)
        local ok2, message2 = pcall(game.menu.button, "Go", { press = function() end })
        assert(not ok2 and string.find(message2, "onPress"), message2)
        local ok3, message3 = pcall(game.menu.new, "Empty", {})
        assert(not ok3 and string.find(message3, "rows"), message3)
        local row = game.menu.toggle("On")
        local ok4, message4 = pcall(function() row.value = 3 end)
        assert(not ok4 and string.find(message4, "true or false"), message4)
        local ok5, message5 = pcall(row.onPress, row, function() end)
        assert(not ok5 and string.find(message5, "onChange"), message5)
    "#;
    Played::with_main(main, Settings::default());
}

#[test]
fn the_pause_menu_is_read_with_is_open_and_opened_with_either_call() {
    let main = r#"
        local pause = game.pauseMenu
        assert(pause.isOpen == false, "isOpen is a boolean")
        game.input.bind("go", { "F6", "GamepadSelect" })
        game.input.bind("stop", { "F5", "GamepadSelect" })
        game.onTick(function()
            if game.input.justPressed("go") then pause:open() end
            if game.input.justPressed("stop") then pause.close() end
        end)
        assert(pause.enabled == true and pause.title == "Paused")
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::F6);
    assert!(played.shows("Resume") && played.host().paused());
    played.tap(Key::F5);
    assert!(!played.shows("Resume") && !played.host().paused());
    played.tap(Key::F6);
    assert!(played.shows("Resume"));
}

#[test]
fn the_pause_menu_refuses_what_it_cannot_do_and_says_what_to_write() {
    let main = r#"
        local pause = game.pauseMenu
        local function says(set, ...)
            local ok, message = pcall(set)
            assert(not ok, "no error")
            for _, part in { ... } do
                assert(string.find(message, part, 1, true), message)
            end
        end
        says(function() pause.enable = false end, "enabled", "title", "game.pauseMenu.enabled = false")
        says(function() pause.colour = "red" end, "enabled, title and controls")
        says(function() pause.isOpen = true end, "read only", "open()", "close()")
        says(function() pause.enabled = "no" end, "true or false")
        says(function() pause.title = 3 end, "words in quotes")
        says(function() pause.open = function() end end, "enabled, title and controls")
        pause.enabled = false
        says(
            function() pause.open() end,
            "the pause menu is off (game.pauseMenu.enabled = false); set it to true to open it"
        )
        pause.enabled = true
        pause:open()
        assert(pause.isOpen)
        pause:close()
        assert(not pause.isOpen)
    "#;
    Played::with_main(main, Settings::default());
}

#[test]
fn a_game_that_says_the_pause_menu_is_off_stays_off_to_the_player() {
    let main = "game.pauseMenu.enabled = false";
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::Escape);
    assert!(!played.shows("Resume") && !played.host().paused());
}

#[test]
fn a_pause_menu_of_the_games_own_in_a_folder_turns_the_engines_off() {
    let scripts = [
        (
            "main.luau".to_owned(),
            "require('./ui/pause_menu')\nassert(game.pauseMenu.enabled == false)".to_owned(),
        ),
        (
            "ui/pause_menu.luau".to_owned(),
            "game.hud.text('Our own pause menu', { at = 'center' })".to_owned(),
        ),
    ];
    let game = ScriptGame::from_sources_with_settings(
        scripts,
        &ActionMap::standard(),
        Settings::default(),
    );
    let mut played = Played::of(game, Settings::default(), true);
    assert!(played.shows("Our own pause menu"), "{:?}", played.words());
    played.tap(Key::Escape);
    assert!(!played.host().paused() && !played.shows("Resume"));
}

#[test]
fn a_menu_and_tabs_say_that_open_and_current_are_read_only() {
    let main = r#"
        local menu = game.menu.new("One", { game.menu.info("Hi", { text = "x" }) })
        local tabs = game.menu.tabs({ menu, game.menu.new("Two", { game.menu.info("Yo") }) })
        local function says(set, ...)
            local ok, message = pcall(set)
            assert(not ok, "no error")
            for _, part in { ... } do
                assert(string.find(message, part, 1, true), message)
            end
        end
        says(function() menu.isOpen = true end, "read only", "menu:open()", "menu:close()")
        says(function() tabs.isOpen = true end, "read only", "tabs:open()", "tabs:close()")
        says(function() tabs.current = menu end, "read only", "tabs:open(menu)")
    "#;
    Played::with_main(main, Settings::default());
}

#[test]
fn a_confirm_and_a_click_in_one_tick_cannot_press_a_row_of_a_closed_screen() {
    let main = r#"
        game.input.bind("open", { "F6", "GamepadSelect" })
        local told = game.hud.text("none", { at = "bottom" })
        local count = 0
        local screen
        screen = game.menu.new("Screen", {
            game.menu.button("Close", { onPress = function() screen:close() end }),
            game.menu.button("Count", { onPress = function()
                count += 1
                told.text = "counted " .. count
            end }),
        })
        game.onTick(function()
            if game.input.justPressed("open") then screen:open() end
        end)
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::F6);
    // The pointer rests over "Count" while the first row, "Close", is chosen.
    played.point_at("Count");
    assert!(played.shows("Close"), "{:?}", played.words());
    played.input.press(Key::Enter);
    played.input.press(MouseButton::MouseLeft);
    played.step();
    assert!(!played.shows("Close"), "the screen closed");
    played.input.release(Key::Enter);
    played.input.release(MouseButton::MouseLeft);
    played.step();
    assert!(!played.shows("counted 1"), "{:?}", played.words());
    assert_eq!(played.set(), "gameplay");
}

#[test]
fn closing_a_script_menu_with_escape_or_b_does_not_open_the_pause_menu() {
    let mut played = Played::with_main(SCRIPT_MENU, Settings::default());
    played.tap(Key::F6);
    played.input.press(Key::Escape);
    played.step();
    assert!(!played.shows("Quiet") && !played.shows("Resume"));
    assert!(!played.host().paused());
    // Held for another tick, and let go: still no pause menu.
    played.step();
    played.input.release(Key::Escape);
    played.step();
    assert!(!played.shows("Resume") && !played.host().paused());

    played.tap(Key::F6);
    played.input.press(GamepadButton::GamepadEast);
    played.step();
    assert!(!played.shows("Quiet") && !played.shows("Resume"));
    assert!(!played.host().paused());
    played.step();
    played.input.release(GamepadButton::GamepadEast);
    played.step();
    assert!(!played.shows("Resume") && !played.host().paused());
    assert_eq!(played.set(), "gameplay");
}

#[test]
fn changing_tabs_over_the_pause_menu_hides_the_last_tab_and_shows_the_next() {
    let main = r#"
        game.input.bind("tabs", { "F6", "GamepadSelect" })
        local sound = game.menu.new("Sound", { game.menu.info("Music row", { text = "on" }) })
        local video = game.menu.new("Video", { game.menu.info("Shadows row", { text = "high" }) })
        local options = game.menu.tabs({ sound, video })
        game.onTick(function()
            if game.input.justPressed("tabs") then options:toggle() end
        end)
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::Escape);
    assert!(played.shows("Resume"));
    played.tap(Key::F6);
    assert!(played.shows("Music row") && !played.shows("Shadows row"));
    assert!(!played.shows("Resume"), "{:?}", played.words());
    played.tap(Key::E);
    assert!(played.shows("Shadows row") && !played.shows("Music row"));
    assert!(!played.shows("Resume"), "{:?}", played.words());
    played.tap(Key::Q);
    assert!(played.shows("Music row") && !played.shows("Shadows row"));
    // Closing the tabs gives the pause menu back.
    played.tap(Key::Escape);
    assert!(played.shows("Resume") && !played.shows("Music row"));
}

#[test]
fn a_change_that_closes_the_screen_is_not_followed_by_a_press_on_it() {
    let main = r#"
        game.input.bind("open", { "F6", "GamepadSelect" })
        local told = game.hud.text("none", { at = "bottom" })
        local heard = 0
        local screen
        local toggle = game.menu.toggle("Music", { onChange = function(on)
            heard += 1
            told.text = "heard " .. heard
            screen:close()
        end })
        screen = game.menu.new("Screen", { toggle })
        game.onTick(function()
            if game.input.justPressed("open") then screen:open() end
        end)
    "#;
    let mut played = Played::with_main(main, Settings::default());
    played.tap(Key::F6);
    // Right changes the toggle, whose handler closes the screen; Enter in
    // the same tick would press the toggle again on a screen that is gone.
    played.input.press(Key::ArrowRight);
    played.input.press(Key::Enter);
    played.step();
    played.input.release(Key::ArrowRight);
    played.input.release(Key::Enter);
    played.step();
    assert!(played.shows("heard 1"), "{:?}", played.words());
    assert_eq!(played.set(), "gameplay");
}

const TEXT_BOX: &str = r#"
    game.input.bind("form", { "F6", "GamepadSelect" })
    local log = "none"
    local changes = 0
    local menu
    local name = game.menu.textBox("Name", {
        placeholder = "Your name",
        maxLength = 5,
        onChange = function(text)
            changes += 1
            if text == "!" then menu:close() end
        end,
        onFinish = function(text) log = "done " .. text end,
    })
    local fill = game.menu.button("Fill", { onPress = function() name.value = "Bob" end })
    local tell = game.menu.info("Log", { text = function()
        return log .. " c" .. changes .. (if name.typing then " typing" else "")
    end })
    menu = game.menu.new("Form", { name, fill, tell })
    game.onTick(function()
        if game.input.justPressed("form") then menu:toggle() end
    end)
"#;

impl Played {
    /// The player types `text` (with marks for Backspace, Enter, Escape),
    /// and a tick hears it.
    fn type_text(&mut self, text: &str) {
        self.input.type_text(text);
        self.step();
    }

    /// A text box game with its menu open and its box pressed to type.
    fn typing_in_a_box() -> Self {
        let mut played = Played::with_main(TEXT_BOX, Settings::default());
        played.tap(Key::F6);
        played.tap(Key::Enter);
        assert!(played.input.is_typing(), "{:?}", played.words());
        played
    }
}

#[test]
fn typing_into_a_text_box_then_enter_finishes_it() {
    let mut played = Played::with_main(TEXT_BOX, Settings::default());
    played.tap(Key::F6);
    assert_eq!(played.value_of("Name"), "Your name", "dimmed when empty");
    assert!(!played.input.is_typing());
    played.tap(Key::Enter);
    assert!(played.input.is_typing());
    assert!(played.shows("none c0 typing"), "{:?}", played.words());
    played.type_text("Ada");
    assert!(played.value_of("Name").starts_with("Ada"));
    assert!(played.shows("none c3 typing"), "onChange for each letter");
    played.type_text("\n");
    assert!(!played.input.is_typing());
    assert_eq!(played.value_of("Name"), "Ada");
    assert!(played.shows("done Ada c3"), "{:?}", played.words());
}

#[test]
fn backspace_takes_off_a_letter_and_escape_puts_the_old_text_back() {
    let mut played = Played::typing_in_a_box();
    for part in ["Adx", "\u{8}", "a", "\u{8}", "é"] {
        played.type_text(part);
    }
    assert!(played.value_of("Name").starts_with("Adé"));
    played.type_text("\u{8}\u{8}\u{8}\u{8}");
    assert!(played.value_of("Name").starts_with('|') || played.value_of("Name").is_empty());
    played.type_text("Ada\n");
    assert!(played.shows("done Ada c13"), "{:?}", played.words());
    // Again: Escape goes back to "Ada", and finishes nothing.
    played.tap(Key::Enter);
    assert!(played.input.is_typing());
    played.type_text("zz");
    assert!(played.value_of("Name").starts_with("Adazz"));
    played.type_text("\u{1b}");
    assert!(!played.input.is_typing());
    assert_eq!(played.value_of("Name"), "Ada");
    assert!(played.shows("done Ada c16"), "{:?}", played.words());
}

#[test]
fn a_text_box_takes_no_more_letters_than_its_max_length() {
    let mut played = Played::typing_in_a_box();
    played.type_text("abcdefgh");
    played.type_text("\n");
    assert_eq!(played.value_of("Name"), "abcde");
    assert!(played.shows("done abcde c5"), "{:?}", played.words());
}

#[test]
fn keys_do_not_drive_the_menu_while_typing_but_typed_letters_go_in() {
    let mut played = Played::typing_in_a_box();
    played.tap(Key::ArrowDown);
    played.tap(Key::S);
    played.tap(Key::Escape);
    played.tap(Key::Q);
    assert!(
        played.input.is_typing(),
        "Escape the key is not Escape typed"
    );
    assert_eq!(played.set(), "menu");
    played.type_text("w");
    played.type_text("\n");
    assert_eq!(played.value_of("Name"), "w");
    // The row did not move: Enter types into the same box again.
    played.tap(Key::Enter);
    assert!(played.input.is_typing());
    played.type_text("\u{1b}");
    // And with typing over the keys work again.
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    assert!(!played.input.is_typing(), "Fill is a button");
    played.tap(Key::Escape);
    assert_eq!(played.set(), "gameplay");
}

#[test]
fn a_click_on_a_text_box_starts_typing() {
    let mut played = Played::with_main(TEXT_BOX, Settings::default());
    played.tap(Key::F6);
    played.click("Your name");
    assert!(played.input.is_typing(), "{:?}", played.words());
    played.type_text("Bo");
    played.type_text("\n");
    assert!(played.shows("done Bo c2"), "{:?}", played.words());
}

#[test]
fn a_script_setting_a_text_box_value_does_not_call_on_change() {
    let mut played = Played::with_main(TEXT_BOX, Settings::default());
    played.tap(Key::F6);
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    assert_eq!(played.value_of("Name"), "Bob");
    assert!(played.shows("none c0"), "{:?}", played.words());
}

#[test]
fn leaving_the_menu_while_typing_stops_typing_and_puts_the_old_text_back() {
    let mut played = Played::typing_in_a_box();
    // The first letter "!" makes the game's onChange close the menu.
    played.type_text("!");
    assert_eq!(played.set(), "gameplay", "the handler closed the menu");
    assert!(
        !played.input.is_typing(),
        "the keyboard is the game's again"
    );
    played.tap(Key::F6);
    assert_eq!(played.set(), "menu", "keys drive actions again");
    assert_eq!(played.value_of("Name"), "Your name", "the old text is back");
}

impl Played {
    /// A text box game with its menu open and its box pressed with a gamepad:
    /// the on-screen keyboard is up, with "a" chosen.
    fn on_screen_keyboard() -> Self {
        let mut played = Played::with_main(TEXT_BOX, Settings::default());
        played.tap(Key::F6);
        played.tap(GamepadButton::GamepadSouth);
        assert!(played.shows("Space"), "{:?}", played.words());
        assert!(
            !played.input.is_typing(),
            "a gamepad has no keyboard to type"
        );
        played
    }

    /// The words of the box now, without the caret.
    fn box_text(&self) -> String {
        self.value_of("Name").trim_end_matches('|').to_owned()
    }
}

#[test]
fn a_gamepad_pressing_a_text_box_opens_the_on_screen_keyboard() {
    let played = Played::on_screen_keyboard();
    for words in ["Shift", "Back", "Done", "q", "1", "m"] {
        assert!(played.shows(words), "{words}: {:?}", played.words());
    }
    assert!(played.shows("none c0 typing"), "{:?}", played.words());
    assert!(
        played.words().iter().any(|w| w.starts_with("A ")
            && w.contains("type")
            && w.contains("erase")
            && w.contains("done")),
        "{:?}",
        played.words()
    );
}

#[test]
fn the_d_pad_walks_to_letters_and_a_types_them() {
    let mut played = Played::on_screen_keyboard();
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::DpadRight);
    played.tap(GamepadButton::DpadRight);
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::DpadLeft);
    played.tap(GamepadButton::DpadLeft);
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.box_text(), "ada");
    assert!(played.shows("none c3 typing"), "{:?}", played.words());
    // Up and down stop at the edges and land on the nearest key.
    played.tap(GamepadButton::DpadUp);
    played.tap(GamepadButton::DpadUp);
    played.tap(GamepadButton::DpadUp);
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.box_text(), "ada1", "q then 1, and no further");
}

#[test]
fn x_erases_and_y_types_a_space() {
    let mut played = Played::on_screen_keyboard();
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::GamepadWest);
    assert_eq!(played.box_text(), "a");
    played.tap(GamepadButton::GamepadNorth);
    assert_eq!(played.box_text(), "a ");
    played.tap(GamepadButton::GamepadWest);
    played.tap(GamepadButton::GamepadWest);
    played.tap(GamepadButton::GamepadWest);
    assert_eq!(
        played.box_text(),
        "",
        "nothing left to erase is not a mistake"
    );
}

#[test]
fn a_bumper_or_the_shift_key_turns_capitals_on_and_off() {
    let mut played = Played::on_screen_keyboard();
    played.tap(GamepadButton::RightBumper);
    assert!(played.shows("A"), "{:?}", played.words());
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::LeftBumper);
    assert!(played.shows("a"));
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.box_text(), "Aa");
    // The Shift key is the bottom row's first: two down from "a".
    played.tap(GamepadButton::DpadDown);
    played.tap(GamepadButton::DpadDown);
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::DpadUp);
    played.tap(GamepadButton::DpadUp);
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.box_text(), "AaA");
}

#[test]
fn start_finishes_and_b_puts_the_old_text_back() {
    let mut played = Played::on_screen_keyboard();
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::GamepadStart);
    assert!(!played.shows("Space"), "closed");
    assert_eq!(played.value_of("Name"), "aa");
    assert!(played.shows("done aa c2"), "{:?}", played.words());
    assert_eq!(played.set(), "menu", "Start was Done, not the menu's close");

    // Again: B cancels, and finishes nothing.
    played.tap(GamepadButton::GamepadSouth);
    assert!(played.shows("Space"));
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::GamepadEast);
    assert!(!played.shows("Space"));
    assert_eq!(played.value_of("Name"), "aa");
    assert!(played.shows("done aa c4"), "{:?}", played.words());
    assert_eq!(played.set(), "menu", "B closed the keyboard, not the menu");
}

#[test]
fn left_and_right_wrap_along_a_row_and_max_length_holds() {
    let mut played = Played::on_screen_keyboard();
    played.tap(GamepadButton::DpadLeft);
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.box_text(), "l", "the home row wraps from a to l");
    played.tap(GamepadButton::DpadRight);
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.box_text(), "la");
    for _ in 0..6 {
        played.tap(GamepadButton::GamepadSouth);
    }
    assert_eq!(played.box_text(), "laaaa", "the box takes five");
    played.tap(GamepadButton::GamepadNorth);
    assert_eq!(played.box_text(), "laaaa");
}

#[test]
fn the_pointer_clicks_keys_and_the_menu_below_hears_nothing() {
    let mut played = Played::on_screen_keyboard();
    played.click("q");
    assert_eq!(played.box_text(), "q");
    played.click("Back");
    assert_eq!(played.box_text(), "");
    // Up would move the menu's chosen row; here it walks the keys.
    played.tap(GamepadButton::DpadUp);
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.box_text().chars().count(), 1);
    played.click("Done");
    assert!(!played.shows("Space"));
}

#[test]
fn enter_on_a_text_box_with_a_keyboard_still_types_on_the_keyboard() {
    let mut played = Played::on_screen_keyboard();
    played.tap(GamepadButton::GamepadEast);
    assert!(!played.shows("Space"));
    played.tap(Key::Enter);
    assert!(played.input.is_typing(), "{:?}", played.words());
    assert!(
        !played.shows("Space"),
        "no on-screen keyboard for a keyboard"
    );
    played.type_text("hi\n");
    assert_eq!(played.value_of("Name"), "hi");
}

/// A text box game with the new events and a short limit, for what a script
/// hears of typing.
const TEXT_BOX_EVENTS: &str = r#"
    local log = ""
    local menu
    local name = game.menu.textBox("Name", {
        maxLength = 4,
        value = "toolong",
        onTyping = function(on) log ..= if on then "T" else "t" end,
        onCancel = function() log ..= "C" end,
        onFinish = function() log ..= "F" end,
        onChange = function(text) if string.sub(text, -1) == "!" then error("boom") end end,
    })
    local tell = game.menu.info("Log", { text = function() return "log " .. log end })
    menu = game.menu.new("Form", { name, tell })
    game.input.bind("form", { "F6", "GamepadSelect" })
    game.onTick(function()
        if game.input.justPressed("form") then menu:toggle() end
    end)
"#;

impl Played {
    fn with_events() -> Self {
        let mut played = Played::with_main(TEXT_BOX_EVENTS, Settings::default());
        played.tap(Key::F6);
        played
    }
}

#[test]
fn a_value_longer_than_max_length_is_cut_and_a_smaller_limit_cuts_it() {
    let played = Played::with_events();
    assert_eq!(played.value_of("Name"), "tool");
    let mut played = Played::with_main(
        r#"local row = game.menu.textBox("Name", { value = "héllo" })
        row.maxLength = 3
        local menu = game.menu.new("Form", { row, game.menu.info("Now", { text = function() return row.value end }) })
        row.value = "ñññññ"
        menu:open()"#,
        Settings::default(),
    );
    played.step();
    assert_eq!(played.value_of("Now"), "ñññ");
}

#[test]
fn on_typing_hears_start_and_stop_and_on_cancel_hears_escape_only() {
    let mut played = Played::with_events();
    played.tap(Key::Enter);
    assert!(played.shows("log T"), "{:?}", played.words());
    played.type_text("\u{1b}");
    assert!(played.shows("log TtC"), "{:?}", played.words());
    played.tap(Key::Enter);
    played.type_text("\n");
    assert!(played.shows("log TtCTtF"), "{:?}", played.words());
    // Start from a gamepad finishes a box typed into from the keyboard.
    played.tap(Key::Enter);
    played.tap(GamepadButton::GamepadStart);
    assert!(played.shows("log TtCTtFTtF"), "{:?}", played.words());
    assert!(!played.input.is_typing());
}

#[test]
fn a_script_error_while_typing_leaves_the_keys_working() {
    let mut played = Played::with_events();
    played.tap(Key::Enter);
    assert!(played.input.is_typing());
    // The first letter fails in onChange: the engine takes typing back.
    played.type_text("\u{8}");
    played.strict = false;
    played.type_text("!");
    played.step();
    assert!(!played.input.is_typing(), "{:?}", played.words());
    played.tap(Key::Enter);
    assert!(
        played.input.is_typing(),
        "the box can be typed into again: {:?}",
        played.words()
    );
}

#[test]
fn a_game_with_its_own_keyboard_gets_typing_without_the_engines() {
    let mut played = Played::with_main(
        &TEXT_BOX_EVENTS.replace(
            "local log",
            "game.menu.onScreenKeyboard = false\n    local log",
        ),
        Settings::default(),
    );
    played.tap(Key::F6);
    played.tap(GamepadButton::GamepadSouth);
    assert!(played.shows("log T"), "{:?}", played.words());
    assert!(!played.shows("Space"), "no engine keyboard");
    assert!(!played.input.is_typing(), "a gamepad types nothing");
    played.tap(GamepadButton::GamepadEast);
    assert!(played.shows("log TtC"), "B cancels: {:?}", played.words());
}

#[test]
fn an_on_screen_keyboard_works_in_a_game_without_erase_and_space_actions() {
    // `justPressed` on an action the game does not have is an error, so the
    // kit asks once and leaves those keys out.
    let mut played = Played::on_screen_keyboard();
    played.tap(GamepadButton::GamepadWest);
    played.tap(GamepadButton::GamepadNorth);
    assert!(played.shows("Space"), "{:?}", played.words());
}

#[test]
fn start_typing_from_a_script_opens_the_keyboard_for_a_gamepad_as_a_press_does() {
    let mut played = Played::with_main(
        r#"local name = game.menu.textBox("Name")
        local go = game.menu.button("Go", { onPress = function() name:startTyping() end })
        local menu = game.menu.new("Form", { go, name })
        menu:open()"#,
        Settings::default(),
    );
    played.tap(GamepadButton::GamepadSouth);
    assert!(played.shows("Space"), "{:?}", played.words());
    assert!(!played.input.is_typing());
}

/// A game with actions of its own, and a line for each that says what it is
/// bound to as the script reads it, for the tests of the rebind rows on the Controls tab.
const BINDINGS_GAME: &str = r#"
    game.input.bind("open_map", { "F1", "GamepadSelect" })
    game.input.bind("ride", { "I/K/J/L", "LeftStick" })
    game.input.bind("lean", { "Q/E", "LeftStickX" })
    local names = { "move", "look", "open_map", "ride", "lean", "jump" }
    local lines = {}
    for number, name in names do
        lines[number] = game.hud.text("", { right = 1200, down = 60 + number * 30 })
    end
    game.onDraw(function()
        for number, name in names do
            lines[number].text = "has " .. name .. "=" .. table.concat(game.input.bindings(name), ",")
        end
    end)
"#;

impl Played {
    /// Opens the pause menu, its settings, and the Controls tab.
    fn on_bindings_page(main: &str) -> Self {
        let mut played = Self::with_main(main, Settings::default());
        played.tap(Key::Escape);
        played.tap(Key::ArrowDown);
        played.tap(Key::Enter);
        for _ in 0..12 {
            if played.shows("Mouse sensitivity") {
                return played;
            }
            played.tap(Key::E);
        }
        panic!("no Controls tab among {:?}", played.words());
    }

    /// Scrolls the tab, with the down arrow, to the row `row`: more rows
    /// than a screen takes scroll, going round at the end.
    fn go_to(&mut self, row: &str) {
        for _ in 0..80 {
            if self.shows(row) {
                return;
            }
            self.tap(Key::ArrowDown);
        }
        panic!("{row} is not on the tab among {:?}", self.words());
    }

    /// Chooses the row so that a gamepad's A presses it: a click on it
    /// chooses it (and starts the wait for a key, which Escape ends).
    fn pad_choose(&mut self, row: &str) {
        self.go_to(row);
        self.click(row);
        self.tap(Key::Escape);
    }

    fn press_row(&mut self, row: &str) {
        self.go_to(row);
        self.click(row);
    }

    fn page_value(&mut self, row: &str) -> String {
        self.go_to(row);
        self.value_of(row)
    }

    /// What the script reads `action` as bound to.
    fn bound(&self, action: &str) -> String {
        let start = format!("has {action}=");
        (self.words().iter())
            .find_map(|text| text.strip_prefix(start.as_str()))
            .unwrap_or_else(|| panic!("no line for {action} among {:?}", self.words()))
            .to_owned()
    }
}

fn bindings_game() -> String {
    BINDINGS_GAME.to_owned()
}

#[test]
fn the_controls_tab_lists_what_a_player_can_rebind_and_leaves_the_menu_keys_out() {
    let mut played = Played::on_bindings_page(&bindings_game());
    for row in [
        "Jump", "Move", "Look", "Use item", "Open map", "Ride", "Lean",
    ] {
        played.go_to(row);
    }
    // The settings come first, then the buttons.
    played.go_to("Invert look");
    played.go_to("Buttons");
    for row in ["Menu confirm", "Menu move", "Menu back"] {
        for _ in 0..40 {
            assert!(!played.shows(row), "{row} in {:?}", played.words());
            played.tap(Key::ArrowDown);
        }
    }
    // The keys of the device in use, as a player reads them.
    assert_eq!(played.page_value("Jump"), "Space");
    assert_eq!(played.page_value("Move"), "W A S D");
    assert_eq!(played.page_value("Look"), "Mouse");
    assert_eq!(played.page_value("Open map"), "F1");
    assert_eq!(played.page_value("Lean"), "Q / E");
}

#[test]
fn a_game_that_turns_on_switch_view_and_lock_on_sees_them_share_select_and_the_bumper() {
    let game = format!(
        "game.onStart(function()\n  game.player:allowViewSwitch(true)\n  game.player:setAbility('lockOn', true)\nend)\n{}",
        bindings_game()
    );
    let mut played = Played::on_bindings_page(&game);
    played.go_to("Open map");
    played.tap(GamepadButton::DpadDown);
    assert_eq!(played.value_of("Open map"), "View  (also Switch view)");
    played.pad_choose("Open map");
    played.tap(GamepadButton::GamepadSouth);
    played.tap(GamepadButton::LeftBumper);
    assert_eq!(played.page_value("Open map"), "LB  (also Lock on)");
}

#[test]
fn the_controls_tab_follows_the_device_the_player_uses() {
    let mut played = Played::on_bindings_page(&bindings_game());
    played.go_to("Open map");
    assert_eq!(played.value_of("Open map"), "F1");
    played.tap(GamepadButton::DpadDown);
    // Select is the standard "switch_view" button too, but this game has
    // not let the player switch views, so the page says nothing of it.
    assert_eq!(played.value_of("Open map"), "View");
    played.tap(Key::ArrowDown);
    assert_eq!(played.value_of("Open map"), "F1");
}

#[test]
fn a_game_hides_the_buttons_or_one_action_from_the_controls_tab() {
    let hidden = format!(
        "game.pauseMenu.hideControl('jump')\ngame.pauseMenu.hideControl('no_such_thing')\n{}",
        bindings_game()
    );
    let mut played = Played::on_bindings_page(&hidden);
    played.go_to("Move");
    played.go_to("Open map");
    for _ in 0..40 {
        assert!(!played.shows("Jump"), "{:?}", played.words());
        played.tap(Key::ArrowDown);
    }

    let none = format!("game.pauseMenu.controls = false\n{}", bindings_game());
    let mut played = Played::with_main(&none, Settings::default());
    played.tap(Key::Escape);
    played.tap(Key::ArrowDown);
    played.tap(Key::Enter);
    // The Controls tab keeps its settings and loses the buttons.
    for _ in 0..12 {
        if played.shows("Mouse sensitivity") {
            break;
        }
        played.tap(Key::E);
    }
    assert!(played.shows("Mouse sensitivity"), "{:?}", played.words());
    for _ in 0..40 {
        for row in ["Buttons", "Jump", "Open map"] {
            assert!(!played.shows(row), "{row} in {:?}", played.words());
        }
        played.tap(Key::ArrowDown);
    }
    assert!(played.shows("Reset all to defaults"));
}

#[test]
fn pressing_a_row_then_a_key_rebinds_and_escape_changes_nothing() {
    let mut played = Played::on_bindings_page(&bindings_game());
    played.press_row("Open map");
    assert_eq!(
        played.page_value("Open map"),
        "Press a key or mouse button (Escape to cancel)"
    );
    // Escape ends the wait, and the menu is still there.
    played.tap(Key::Escape);
    assert_eq!(played.page_value("Open map"), "F1");
    assert_eq!(played.bound("open_map"), "F1,GamepadSelect");
    assert!(played.shows("Mouse sensitivity") || played.shows("Open map"));

    played.press_row("Open map");
    played.tap(Key::F2);
    assert_eq!(played.page_value("Open map"), "F2");
    assert_eq!(played.bound("open_map"), "GamepadSelect,F2");
    assert!(played.host().last_error().is_none());

    // A click is a button too.
    played.press_row("Jump");
    played.tap(MouseButton::MouseMiddle);
    assert_eq!(played.page_value("Jump"), "Middle click");
}

#[test]
fn a_gamepad_hears_its_own_buttons_and_leaves_the_keys_be() {
    let mut played = Played::on_bindings_page(&bindings_game());
    played.pad_choose("Open map");
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(
        played.page_value("Open map"),
        "Press a button (Start to cancel)"
    );
    // A key is not what it listens for.
    played.tap(Key::F5);
    assert!(played.shows("Press a button (Start to cancel)"));
    played.tap(GamepadButton::LeftBumper);
    // Lock on has it too, but this game has not turned it on.
    assert_eq!(played.page_value("Open map"), "LB");
    assert_eq!(played.bound("open_map"), "F1,LeftBumper");
}

#[test]
fn a_direction_takes_a_key_for_each_way_in_turn_on_a_keyboard() {
    let mut played = Played::on_bindings_page(&bindings_game());
    played.press_row("Move");
    for (way, key) in [("up", Key::I), ("down", Key::K), ("left", Key::J)] {
        assert_eq!(
            played.page_value("Move"),
            format!("Press the key for {way} (Escape to cancel)")
        );
        played.tap(key);
    }
    assert_eq!(
        played.page_value("Move"),
        "Press the key for right (Escape to cancel)"
    );
    played.tap(Key::L);
    assert_eq!(played.bound("move"), "LeftStick,I/K/J/L");
    assert_eq!(played.page_value("Move"), "I J K L  (also Ride)");

    // One direction takes two: the way that lessens, then the way that adds.
    played.press_row("Lean");
    assert_eq!(
        played.page_value("Lean"),
        "Press the key for left (Escape to cancel)"
    );
    played.tap(Key::Z);
    played.tap(Key::C);
    assert_eq!(played.bound("lean"), "LeftStickX,Z/C");

    // Look follows the mouse: that is said, not changed.
    played.press_row("Look");
    played.go_to(
        "Look follows the mouse, so there is no key to change. Press it on a gamepad to pick a stick",
    );
    assert!(played.shows("Look follows the mouse, so there is no key to change. Press it on a gamepad to pick a stick"));
    assert_eq!(played.bound("look"), "MouseMotion,RightStick");
}

#[test]
fn pressing_a_direction_on_a_gamepad_goes_through_the_sticks() {
    let mut played = Played::on_bindings_page(&bindings_game());
    played.pad_choose("Move");
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.bound("move"), "WASD,RightStick");
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.bound("move"), "WASD,Dpad");
    assert_eq!(played.page_value("Move"), "D-pad");
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.bound("move"), "WASD,LeftStick");
    // One direction goes from one stick's side to the other's.
    played.pad_choose("Lean");
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.bound("lean"), "Q/E,RightStickX");
    played.tap(GamepadButton::GamepadSouth);
    assert_eq!(played.bound("lean"), "Q/E,LeftStickX");
}

#[test]
fn the_page_says_when_a_button_is_used_twice_and_marks_both_rows() {
    let mut played = Played::on_bindings_page(&bindings_game());
    played.press_row("Open map");
    // M is the standard "map" key, and both are heard in the game.
    played.tap(Key::M);
    played.step();
    played.go_to("M is also used by Map");
    assert!(
        played.shows("M is also used by Map"),
        "{:?}",
        played.words()
    );
    assert_eq!(played.page_value("Open map"), "M  (also Map)");
    assert_eq!(played.page_value("Map"), "M  (also Open map)");
    // A button nothing else has is not told off.
    played.press_row("Open map");
    played.tap(Key::F9);
    played.step();
    assert!(!played.words().iter().any(|text| text.contains("also used")));
    assert_eq!(played.page_value("Open map"), "F9");
}

#[test]
fn every_clash_that_stands_is_marked_but_not_the_pairs_the_engine_shares() {
    let main = format!(
        "{}\ngame.input.bind(\"wave\", {{ \"M\", \"LeftBumper\" }})",
        bindings_game()
    );
    let mut played = Played::on_bindings_page(&main);
    // Nothing was rebound on the page: the clash is there from the start.
    assert_eq!(played.page_value("Wave"), "M  (also Map)");
    assert_eq!(played.page_value("Map"), "M  (also Wave)");
    // Aim and Block share right click on purpose.
    assert_eq!(played.page_value("Aim"), "Right click");
    assert_eq!(played.page_value("Block"), "Right click");
    // The keyboard's clash is not on the gamepad's view, and lock on is not on.
    played.go_to("Wave");
    played.tap(GamepadButton::DpadDown);
    assert_eq!(played.value_of("Wave"), "LB");
}

#[test]
fn putting_back_the_defaults_resets_every_action() {
    let mut played = Played::on_bindings_page(&bindings_game());
    played.press_row("Open map");
    played.tap(Key::F2);
    assert_eq!(played.bound("open_map"), "GamepadSelect,F2");
    played.press_row("Reset all to defaults");
    assert!(played.shows("Every control is back to the defaults"));
    assert_eq!(played.bound("open_map"), "F1,GamepadSelect");
    assert_eq!(played.page_value("Open map"), "F1");
}

/// A menu of `count` buttons called "Row 1", "Row 2" and so on, open.
fn rows_game(count: u32) -> String {
    format!(
        r#"local rows = {{}}
        for number = 1, {count} do
            rows[number] = game.menu.button("Row " .. number)
        end
        game.menu.new("Long", rows):open()"#
    )
}

#[test]
fn a_long_menu_scrolls_to_keep_the_chosen_row_in_view() {
    let mut played = Played::with_main(&rows_game(30), Settings::default());
    assert!(played.shows("Row 1"), "{:?}", played.words());
    assert!(!played.shows("Row 30"));
    assert!(played.shows("… more below") && !played.shows("… more above"));

    // Down past the bottom of the window: the chosen row is always drawn.
    for number in 2..=30 {
        played.tap(Key::ArrowDown);
        assert!(
            played.shows(&format!("Row {number}")),
            "{:?}",
            played.words()
        );
    }
    assert!(!played.shows("Row 1"));
    assert!(played.shows("… more above") && !played.shows("… more below"));
    // Down once more goes round to the first row, and back up to the last.
    played.tap(Key::ArrowDown);
    assert!(played.shows("Row 1") && !played.shows("Row 30"));
    played.tap(Key::ArrowUp);
    assert!(played.shows("Row 30"), "{:?}", played.words());
    for number in (1..=29).rev() {
        played.tap(Key::ArrowUp);
        assert!(
            played.shows(&format!("Row {number}")),
            "{:?}",
            played.words()
        );
    }
    assert!(played.shows("Row 1") && !played.shows("… more above"));
}

#[test]
fn a_menu_held_down_on_a_pad_scrolls_too() {
    let mut played = Played::with_main(&rows_game(40), Settings::default());
    for _ in 0..39 {
        played.tap(GamepadButton::DpadDown);
    }
    assert!(played.shows("Row 40"), "{:?}", played.words());
    assert!(!played.shows("Row 1"));
}

#[test]
fn the_wheel_scrolls_a_long_menu() {
    let mut played = Played::with_main(&rows_game(30), Settings::default());
    let top = played.find("Row 1").1;
    played.input.roll_wheel(-1.0);
    played.step();
    assert!(!played.shows("Row 1"), "{:?}", played.words());
    assert!(played.shows("… more above     … more below"));
    // The first row on show is where the first was, three rows on, and the
    // chosen row came into view with it.
    assert_eq!(played.find("Row 4").1, top);
    played.input.roll_wheel(1.0);
    played.step();
    assert_eq!(played.find("Row 1").1, top);
    // The wheel stops at the ends.
    played.input.roll_wheel(5.0);
    played.step();
    assert_eq!(played.find("Row 1").1, top);
    played.input.roll_wheel(-50.0);
    played.step();
    assert!(played.shows("Row 30") && !played.shows("… more below"));
}

#[test]
fn the_wheel_scrolls_with_the_pointer_resting_on_the_chosen_button() {
    let mut played = Played::with_main(&rows_game(30), Settings::default());
    played.point_at("Row 1");
    played.input.roll_wheel(-1.0);
    played.step();
    assert!(!played.shows("Row 1"), "{:?}", played.words());
}

#[test]
fn a_menu_of_two_rows_far_down_the_screen_still_opens() {
    let main = r#"game.menu.new("Low", { game.menu.button("Row 1"), game.menu.button("Row 2") }, { down = 1000 }):open()"#;
    let played = Played::with_main(main, Settings::default());
    assert!(
        played.shows("Row 1") && played.shows("Row 2"),
        "{:?}",
        played.words()
    );
}

#[test]
fn a_scrolling_menus_message_stays_on_the_fixed_line() {
    let main = r#"local rows = {}
        for number = 1, 30 do rows[number] = game.menu.button("Row " .. number) end
        local m = game.menu.new("Long", rows)
        m.message = function() return "Saved for good" end
        m:open()"#;
    let mut played = Played::with_main(main, Settings::default());
    assert!(played.shows("Saved for good"), "{:?}", played.words());
    played.input.roll_wheel(-50.0);
    played.step();
    assert!(played.shows("Saved for good") && !played.shows("Row 1"));
}

#[test]
fn a_short_menu_is_laid_out_as_it_always_was() {
    let mut played = Played::with_main(&rows_game(3), Settings::default());
    let [x, y] = played.find("Row 1").1;
    // The rows are a row apart and start under the tabs; a menu of more
    // rows starts the same.
    for (number, row) in ["Row 1", "Row 2", "Row 3"].into_iter().enumerate() {
        assert_eq!(played.find(row).1, [x, y + 34.0 * number as f32], "{row}");
    }
    assert!(!played.shows("… more below") && !played.shows("… more above"));
    played.input.roll_wheel(-1.0);
    played.step();
    assert_eq!(played.find("Row 1").1, [x, y]);
    let long = Played::with_main(&rows_game(30), Settings::default());
    assert_eq!(long.find("Row 1").1, [x, y]);
}

#[test]
fn the_controls_tab_holds_the_settings_and_the_buttons_to_rebind() {
    let mut played = Played::on_bindings_page(&bindings_game());
    assert!(played.shows("Mouse sensitivity"));
    played.go_to("Jump");
    played.go_to("Buttons");
    played.go_to("Mouse sensitivity");
    for tab in ["Sound", "Controls", "Graphics"] {
        assert!(played.shows(tab));
    }
    assert!(!played.shows("Bindings"));
}
