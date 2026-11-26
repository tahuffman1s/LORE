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

const MAIN: &str = include_str!("../../template/main.luau");

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
                Draw::Rect { .. } | Draw::Turned { .. } => None,
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
        game.input.bind("shop", { "F3" })
        game.input.bind("help", { "F4" })
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
        game.input.bind("off", { "F5" })
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
