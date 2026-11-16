//! A game whose rules are a Luau script.

use std::path::Path;

use lore_input::{ActionMap, InputFrame};
use lore_script::{AudioCommand, OpenError, ScriptHost};

use crate::error_overlay::ErrorOverlay;
use crate::game::{Frame, Game};
use crate::map::{Map, MapError, MapFile};
use crate::script_hud::ScriptHud;
use crate::sound_output::SoundAsk;
use crate::sounds::Sounds;

/// A [`Game`] that runs the `main.luau` in a game folder, with the scripts
/// it requires, and loads them again whenever one of them is saved.
///
/// The HUD the script makes with `game.hud` is drawn over the game on
/// every frame.
///
/// A mistake in a script is logged, and a game opened from a folder also
/// shows it on screen, over the game, until a save loads the script again.
/// A game run from text has no file to fix, so it shows nothing: its
/// players did not write it.
///
/// A game opened from a folder also has what its `map.toml` places in the
/// level ([`map`](Self::map)), read again whenever that file is saved.
///
/// ```no_run
/// use lore::{Config, ScriptGame};
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = Config::default();
///     let game = ScriptGame::open("my-game", &config.actions)?;
///     lore::run(config, game)?;
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub struct ScriptGame {
    host: ScriptHost,
    /// Shows the latest script error over the game. Only a game whose
    /// scripts are files that can be fixed and saved has one.
    errors: Option<ErrorOverlay>,
    /// The HUD the script made with `game.hud`.
    hud: ScriptHud,
    /// The sounds the script can play by name.
    sounds: Sounds,
    /// What is placed in the level.
    map: Map,
    /// The map's file, looked at for saves. Only a game opened from a
    /// folder has one.
    map_file: Option<MapFile>,
    /// Why the map file as it now is could not be read, if it could not.
    map_error: Option<MapError>,
}

/// Said after a mistake in a map file, by what the game does about it.
const NOTHING_PLACED: &str = "The game runs with nothing placed in its level. Fix this and \
                              save the file: the game picks it up by itself.";
const KEEPS_MAP: &str = "The game keeps the map it had. Fix this and save the file: the \
                         game picks it up by itself.";

/// Logs what `map`, just read from its file, says that LORE does not act on.
fn said_of(map: &Map) {
    for note in map.notes() {
        log::warn!("{note}");
    }
}

impl ScriptGame {
    /// A game running `host` with no files behind it: it has the starter
    /// sounds, and nothing placed in its level.
    fn without_files(host: ScriptHost) -> Self {
        let mut game = Self {
            host,
            errors: None,
            hud: ScriptHud::default(),
            sounds: Sounds::starter(),
            map: Map::default(),
            map_file: None,
            map_error: None,
        };
        game.name_sounds();
        game
    }

    /// Tells the script which sounds there are.
    fn name_sounds(&mut self) {
        self.host.set_sounds(self.sounds.names());
    }

    /// Gives the game the sounds its script can play, in place of the ones
    /// it has: a game from a folder starts with the sounds in the folder,
    /// and a game from text with the starter sounds alone.
    #[must_use]
    pub fn with_sounds(mut self, sounds: Sounds) -> Self {
        self.sounds = sounds;
        self.name_sounds();
        self
    }

    /// The sounds the game's script can play.
    #[must_use]
    pub fn sounds(&self) -> &Sounds {
        &self.sounds
    }

    /// The sounds, to take one to play.
    pub(crate) fn sounds_mut(&mut self) -> &mut Sounds {
        &mut self.sounds
    }

    /// Reads the game's sound files if they have not been read, as the
    /// first frame of a game with a window does, and logs the ones that
    /// cannot be played. A game run with no window never needs to.
    pub fn load_sounds(&mut self) {
        if self.sounds.load() {
            for problem in self.sounds.problems() {
                log::warn!("{problem}");
            }
            // A sound that did not load is not one the script can play.
            self.name_sounds();
        }
    }

    /// [`load_sounds`](Self::load_sounds), and then decodes from beginning
    /// to end the sounds that a game decodes as they play (music, above
    /// all), logging the ones that are damaged part way through. For
    /// `lore check`: a game that is being played never waits for this.
    pub fn check_sounds(&mut self) {
        self.load_sounds();
        let known = self.sounds.problems().len();
        self.sounds.check_streamed();
        for problem in &self.sounds.problems()[known..] {
            log::warn!("{problem}");
        }
    }

    /// Opens the game in `folder`. The script reads input through `actions`,
    /// which should be the same actions the engine is started with.
    ///
    /// A mistake inside the script does not fail here: it is logged and
    /// shown on screen, and the game waits for the file to be fixed and
    /// saved.
    ///
    /// The same goes for a mistake in the game's `map.toml`: it is logged,
    /// nothing is placed, and the game waits for the file to be fixed and
    /// saved ([`map_error`](Self::map_error) says what is wrong meanwhile).
    ///
    /// # Errors
    ///
    /// Fails if `folder` has no `main.luau` or the file cannot be read.
    pub fn open(folder: impl AsRef<Path>, actions: &ActionMap) -> Result<Self, OpenError> {
        let folder = folder.as_ref();
        let host = ScriptHost::open(folder, actions.clone())?;
        let (map_file, read) = MapFile::open(folder);
        let (map, map_error) = match read {
            Ok(map) => {
                said_of(&map);
                (map, None)
            }
            Err(error) => {
                log::error!("{error}\n{NOTHING_PLACED}");
                (Map::default(), Some(error))
            }
        };
        let mut game = Self {
            host,
            errors: Some(ErrorOverlay::default()),
            hud: ScriptHud::default(),
            sounds: Sounds::in_folder(folder),
            map,
            map_file: Some(map_file),
            map_error,
        };
        game.name_sounds();
        Ok(game)
    }

    /// Runs `source` as the game's `main.luau` with no file behind it and no
    /// other scripts to require. Nothing is watched or loaded again, and a
    /// mistake inside the script is logged and kept in the host's
    /// `last_error`.
    #[must_use]
    pub fn from_source(source: impl Into<String>, actions: &ActionMap) -> Self {
        Self::without_files(ScriptHost::from_source(source, actions.clone()))
    }

    /// Runs a game whose scripts are given as text, as a packaged game does.
    /// `scripts` are pairs of a name (`main.luau`, `weapons/rifle.luau`) and
    /// the script's text; `main.luau` is run and can require the others.
    /// Nothing is watched or loaded again, and a mistake inside a script is
    /// logged and kept in the host's `last_error`.
    #[must_use]
    pub fn from_sources(
        scripts: impl IntoIterator<Item = (String, String)>,
        actions: &ActionMap,
    ) -> Self {
        Self::without_files(ScriptHost::from_sources(scripts, actions.clone()))
    }

    /// Places `map` in the game's level, as a packaged game does with the
    /// map it was packed with. It takes the place of any map the game had.
    #[must_use]
    pub fn with_map(mut self, map: Map) -> Self {
        self.map = map;
        self.map_error = None;
        self
    }

    /// What is placed in the game's level: its spawn points, zones and
    /// objectives. For a game opened from a folder this is its `map.toml`
    /// as last saved without a mistake.
    #[must_use]
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// What is wrong with the game's `map.toml` as it now is on disk, if it
    /// could not be read. The game keeps the map it had.
    #[must_use]
    pub fn map_error(&self) -> Option<&MapError> {
        self.map_error.as_ref()
    }

    /// Reads the map file again if it was saved since it was last read.
    fn poll_map(&mut self) {
        let Some(change) = self.map_file.as_mut().and_then(MapFile::changed) else {
            return;
        };
        match change {
            Ok(map) => {
                log::info!("Loaded {} again: {}.", crate::map::MAP_FILE, map.summary());
                said_of(&map);
                self.map = map;
                self.map_error = None;
            }
            Err(error) => {
                log::error!("{error}\n{KEEPS_MAP}");
                self.map_error = Some(error);
            }
        }
    }

    /// The script host, for example to read its latest error.
    #[must_use]
    pub fn host(&self) -> &ScriptHost {
        &self.host
    }

    /// The script host, to tell it what players do: who joined, who left and
    /// who killed whom.
    pub fn host_mut(&mut self) -> &mut ScriptHost {
        &mut self.host
    }
}

impl Game for ScriptGame {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        self.host.poll_reload();
        self.poll_map();
        // A sound file added, saved or removed while the game runs.
        if self.sounds.poll() {
            self.name_sounds();
        }
        self.host.tick(input, step);
    }

    fn frame(&mut self, frame: &mut Frame) {
        // First, so what the script asks for while drawing is in this frame.
        self.host.draw(frame.alpha());
        if let Some(rumble) = self.host.take_rumble() {
            frame.rumble(rumble);
        }
        // A frame means someone is here to listen.
        self.load_sounds();
        while let Some(command) = self.host.take_sound() {
            match command {
                AudioCommand::Play(request) => match self.sounds.play(&request) {
                    Some(play) => frame.sound(SoundAsk::Play(request.id, play)),
                    // Its file went between the script asking and now.
                    None => log::warn!("The sound \"{}\" is no longer there.", request.name),
                },
                AudioCommand::Stop(id) => frame.sound(SoundAsk::Stop(id)),
                AudioCommand::StopAll => frame.sound(SoundAsk::StopAll),
            }
        }
        // After the script has drawn, so what it changed is in this frame.
        self.hud.show(&mut self.host, frame);
        // Last, so it is on top of anything the game draws.
        if let Some(errors) = &mut self.errors {
            errors.show(self.host.last_error(), frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant};

    use lore_ui::Draw;

    use super::*;

    /// The lines of text `game` draws over one frame.
    fn words(game: &mut ScriptGame) -> Vec<String> {
        let mut frame = Frame::new(0.0);
        game.frame(&mut frame);
        frame
            .take_ui()
            .draw()
            .iter()
            .filter_map(|draw| match draw {
                Draw::Text { text, .. } => Some(text.to_owned()),
                Draw::Rect { .. } => None,
            })
            .collect()
    }

    #[test]
    fn a_script_error_is_on_screen_until_a_save_loads_the_script_again() {
        let folder = std::env::temp_dir().join(format!("lore-script-game-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let script = folder.join("main.luau");
        fs::write(&script, "local ticks = 0\ngame.onTik(function() end)\n").unwrap();

        let actions = ActionMap::default();
        let mut game = ScriptGame::open(&folder, &actions).unwrap();
        let shown = words(&mut game);
        assert_eq!(shown[0], "Script error in main.luau, line 2", "{shown:?}");
        assert_eq!(shown[1], "2 | game.onTik(function() end)");
        assert!(shown.iter().any(|line| line.contains("game.onTick")));
        // It stays while the file is as it was.
        game.tick(&InputFrame::default(), 1.0 / 60.0);
        assert_eq!(words(&mut game), shown);

        // The file is fixed and saved, and the game picks it up.
        fs::write(&script, "game.onTick(function() end)\n").unwrap();
        let give_up = Instant::now() + Duration::from_secs(20);
        while game.host().last_error().is_some() && Instant::now() < give_up {
            std::thread::sleep(Duration::from_millis(20));
            game.tick(&InputFrame::default(), 1.0 / 60.0);
        }
        assert_eq!(words(&mut game), Vec::<String>::new());

        // A mistake that only shows while the game runs is shown too.
        fs::write(
            &script,
            "game.onTick(function() local a = nil; a.b = 1 end)\n",
        )
        .unwrap();
        let give_up = Instant::now() + Duration::from_secs(20);
        while game.host().last_error().is_none() && Instant::now() < give_up {
            std::thread::sleep(Duration::from_millis(20));
            game.tick(&InputFrame::default(), 1.0 / 60.0);
        }
        let shown = words(&mut game);
        assert_eq!(shown[0], "Script error in main.luau, line 1", "{shown:?}");
        let _ = fs::remove_dir_all(&folder);
    }

    /// The boxes `game` draws over one frame of a 1920 by 1080 screen:
    /// where each is, how big, and its colour.
    fn boxes(game: &mut ScriptGame) -> Vec<([f32; 2], [f32; 2], lore_ui::Color)> {
        let mut frame = Frame::new(0.0);
        game.frame(&mut frame);
        frame
            .take_ui()
            .draw()
            .iter()
            .filter_map(|draw| match draw {
                Draw::Rect {
                    position,
                    size,
                    color,
                    ..
                } => Some((position, size, color)),
                Draw::Text { .. } => None,
            })
            .collect()
    }

    #[test]
    fn the_scripts_hud_is_drawn_and_follows_what_the_script_changes() {
        let script = r#"
            local ammo = game.hud.text("30 / 90", { at = "bottom right" })
            local health = game.hud.bar(1, { at = "bottom left", width = 400, height = 20, color = "red" })
            local ticks = 0
            game.onTick(function()
                ticks += 1
                ammo.text = (30 - ticks) .. " / 90"
                if ticks == 3 then ammo.visible = false end
                if ticks == 4 then
                    ammo.visible = true
                    game.hud.text("Reloading", { at = "center" })
                end
                if ticks == 5 then ammo:remove() end
            end)
            game.onDraw(function()
                health.value = 0.25
                health.right = 100
            end)
        "#;
        let mut game = ScriptGame::from_source(script, &ActionMap::default());
        // Before the first tick the HUD is as the file made it.
        assert_eq!(words(&mut game), ["30 / 90"]);
        let full = boxes(&mut game);
        // The track, then the full part over all of it, inside the safe area.
        assert_eq!(full.len(), 2);
        assert_eq!(full[0].1, [400.0, 20.0]);
        assert_eq!(full[1].1, [400.0, 20.0]);
        assert_eq!(full[1].0, [96.0, 1080.0 - 54.0 - 20.0]);
        assert!(full[1].2.r > 0.8 && full[1].2.g < 0.4, "{:?}", full[1].2);

        let step = 1.0 / 60.0;
        game.tick(&InputFrame::default(), step);
        // What `game.onDraw` changes is in the frame it ran for.
        let moved = boxes(&mut game)[1];
        assert_eq!(moved.1, [100.0, 20.0]);
        assert_eq!(moved.0, [196.0, 1080.0 - 54.0 - 20.0]);
        assert_eq!(words(&mut game), ["29 / 90"]);
        game.tick(&InputFrame::default(), step);
        assert_eq!(words(&mut game), ["28 / 90"]);
        game.tick(&InputFrame::default(), step);
        assert_eq!(words(&mut game), Vec::<String>::new());
        assert_eq!(boxes(&mut game).len(), 2, "the bar is still there");
        game.tick(&InputFrame::default(), step);
        assert_eq!(words(&mut game), ["26 / 90", "Reloading"]);
        game.tick(&InputFrame::default(), step);
        assert_eq!(words(&mut game), ["Reloading"]);
        assert!(
            game.host().last_error().is_none(),
            "{:?}",
            game.host().last_error()
        );
    }

    #[test]
    fn a_game_with_no_hud_draws_nothing() {
        let mut game =
            ScriptGame::from_source("game.onTick(function() end)", &ActionMap::default());
        game.tick(&InputFrame::default(), 1.0 / 60.0);
        let mut frame = Frame::new(0.0);
        game.frame(&mut frame);
        assert!(frame.take_ui().draw().is_empty());
    }

    #[test]
    fn a_save_puts_the_new_files_hud_on_screen() {
        let folder =
            std::env::temp_dir().join(format!("lore-script-game-hud-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let script = folder.join("main.luau");
        fs::write(&script, "game.hud.text(\"Score: 0\", { at = \"top\" })\n").unwrap();
        let mut game = ScriptGame::open(&folder, &ActionMap::default()).unwrap();
        assert_eq!(words(&mut game), ["Score: 0"]);

        fs::write(
            &script,
            "game.hud.text(\"Points: 0\", { at = \"top\" })\ngame.hud.text(\"Lives: 3\")\n",
        )
        .unwrap();
        let give_up = Instant::now() + Duration::from_secs(20);
        while words(&mut game) == ["Score: 0"] && Instant::now() < give_up {
            std::thread::sleep(Duration::from_millis(20));
            game.tick(&InputFrame::default(), 1.0 / 60.0);
        }
        assert_eq!(words(&mut game), ["Points: 0", "Lives: 3"]);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_frame_calls_the_scripts_draw_functions_between_the_last_two_ticks() {
        let script = "local shown = game.tween(0, 60, 1)\n\
                      game.onDraw(function()\n\
                      if shown.value ~= 1.25 then error(\"drawn at \" .. shown.value) end\n\
                      game.input.rumble(1, 1, 1)\n\
                      end)";
        let mut game = ScriptGame::from_source(script, &ActionMap::default());
        game.tick(&InputFrame::default(), 1.0 / 60.0);
        game.tick(&InputFrame::default(), 1.0 / 60.0);
        let mut frame = Frame::new(0.25);
        game.frame(&mut frame);
        assert!(
            game.host().last_error().is_none(),
            "{:?}",
            game.host().last_error()
        );
        // What the script asked for while drawing is in the same frame.
        assert!(frame.rumble_requested().is_some());
    }

    /// The script error is drawn for real, with no window, and the picture
    /// is read back: a dark panel with a red line along its top and light
    /// letters in it, in the top-left corner, and the game untouched
    /// everywhere else.
    #[cfg(feature = "window")]
    #[test]
    fn the_error_is_drawn_as_a_dark_panel_with_light_text_in_the_corner() {
        use lore_render::{Camera, Headless, RenderError, Upscaling};

        const SCREEN: [u32; 2] = [1280, 720];
        let mut headless = match Headless::new(SCREEN[0], SCREEN[1], Upscaling::Off) {
            Ok(headless) => headless,
            Err(
                error @ (RenderError::NoVulkan(_)
                | RenderError::OldVulkan { .. }
                | RenderError::NoSuitableGpu { .. }),
            ) => {
                let required = std::env::var("LORE_REQUIRE_VULKAN").is_ok_and(|value| value != "0");
                assert!(!required, "LORE_REQUIRE_VULKAN is set, but: {error}");
                eprintln!("Skipping the drawing test: {error}");
                return;
            }
            Err(error) => panic!("{error}"),
        };
        let folder =
            std::env::temp_dir().join(format!("lore-script-game-drawn-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("main.luau"), "game.onTik(function() end)\n").unwrap();
        let mut game = ScriptGame::open(&folder, &ActionMap::default()).unwrap();
        let _ = fs::remove_dir_all(&folder);

        let camera = Camera::default();
        let bare = headless.screenshot(&camera).unwrap();
        let mut frame = Frame::new(0.0).for_window(Some(SCREEN), lore_ui::DrawBuffer::default());
        game.frame(&mut frame);
        let ui = frame.take_ui();
        let Some(Draw::Rect { position, size, .. }) = ui.draw().iter().next() else {
            panic!("the panel is drawn first");
        };
        headless.set_ui(ui.draw());
        let shown = headless.screenshot(&camera).unwrap();
        assert_eq!(lore_render::validation_errors(), 0);

        let pixel = |picture: &[u8], x: u32, y: u32| {
            let start = ((y * SCREEN[0] + x) * 4) as usize;
            [picture[start], picture[start + 1], picture[start + 2]]
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let [left, top, right, bottom] = [
            position[0],
            position[1],
            position[0] + size[0],
            position[1] + size[1],
        ]
        .map(|edge| edge.round() as u32);
        assert!(
            left >= 60 && top >= 30,
            "inside the safe area: {position:?}"
        );
        assert!(right < SCREEN[0] && bottom < SCREEN[1] / 2, "{size:?}");

        // The red line along the top, in the colour it was asked for.
        let [red, green, blue] = pixel(&shown, (left + right) / 2, top + 1);
        assert!(red > 240 && green.abs_diff(84) < 12 && blue.abs_diff(84) < 12);

        // Inside: mostly the dark panel, with a share of light letters.
        let (mut dark, mut light, mut all) = (0_u32, 0_u32, 0_u32);
        for y in top + 6..bottom - 2 {
            for x in left + 2..right - 2 {
                let [red, green, blue] = pixel(&shown, x, y);
                all += 1;
                dark += u32::from(red < 60 && green < 60 && blue < 60);
                light += u32::from(red > 170 && green > 130);
            }
        }
        assert!(dark * 10 > all * 7, "{dark} dark pixels of {all}");
        assert!(light * 200 > all, "{light} light pixels of {all}");
        assert!(light * 4 < all, "{light} light pixels of {all}");

        // Outside the panel the picture is as it was.
        for (x, y) in [
            (left - 3, top + 10),
            (right + 3, top + 10),
            (left + 10, bottom + 3),
            (SCREEN[0] - 1, SCREEN[1] - 1),
            (SCREEN[0] / 2, SCREEN[1] - 40),
        ] {
            assert_eq!(pixel(&shown, x, y), pixel(&bare, x, y), "at {x}, {y}");
        }
    }

    /// The script's HUD is drawn for real, with no window, and the picture
    /// is read back: a box in the middle in the colour asked for, a bar a
    /// quarter full in the bottom-left corner, light letters at the top,
    /// and the game untouched where the HUD is not.
    #[cfg(feature = "window")]
    #[test]
    fn the_scripts_hud_is_drawn_where_and_in_the_colours_it_asked_for() {
        use lore_render::{Camera, Headless, RenderError, Upscaling};

        const SCREEN: [u32; 2] = [1920, 1080];
        let mut headless = match Headless::new(SCREEN[0], SCREEN[1], Upscaling::Off) {
            Ok(headless) => headless,
            Err(
                error @ (RenderError::NoVulkan(_)
                | RenderError::OldVulkan { .. }
                | RenderError::NoSuitableGpu { .. }),
            ) => {
                let required = std::env::var("LORE_REQUIRE_VULKAN").is_ok_and(|value| value != "0");
                assert!(!required, "LORE_REQUIRE_VULKAN is set, but: {error}");
                eprintln!("Skipping the drawing test: {error}");
                return;
            }
            Err(error) => panic!("{error}"),
        };
        let script = r##"
            game.hud.box({ at = "center", width = 200, height = 100, color = "#3366cc" })
            game.hud.bar(0.25, { at = "bottom left", width = 400, height = 40, color = "#00ff00" })
            game.hud.text("SCORE 1250", { at = "top", size = 64 })
            local hidden = game.hud.box({ at = "top right", width = 300, height = 300, color = "red" })
            hidden.visible = false
        "##;
        let mut game = ScriptGame::from_source(script, &ActionMap::default());
        game.tick(&InputFrame::default(), 1.0 / 60.0);

        let camera = Camera::default();
        let bare = headless.screenshot(&camera).unwrap();
        let mut frame = Frame::new(0.0).for_window(Some(SCREEN), lore_ui::DrawBuffer::default());
        game.frame(&mut frame);
        let ui = frame.take_ui();
        headless.set_ui(ui.draw());
        let shown = headless.screenshot(&camera).unwrap();
        assert_eq!(lore_render::validation_errors(), 0);

        let pixel = |picture: &[u8], x: u32, y: u32| {
            let start = ((y * SCREEN[0] + x) * 4) as usize;
            [picture[start], picture[start + 1], picture[start + 2]]
        };
        let near = |[red, green, blue]: [u8; 3], wanted: [u8; 3]| {
            red.abs_diff(wanted[0]) < 12
                && green.abs_diff(wanted[1]) < 12
                && blue.abs_diff(wanted[2]) < 12
        };

        // The box: 200 by 100 around the middle of the screen.
        for (x, y) in [(960, 540), (865, 495), (1055, 585)] {
            assert!(near(pixel(&shown, x, y), [0x33, 0x66, 0xcc]), "at {x}, {y}");
        }
        for (x, y) in [(850, 540), (1070, 540), (960, 480), (960, 600)] {
            assert_eq!(pixel(&shown, x, y), pixel(&bare, x, y), "at {x}, {y}");
        }

        // The bar: its bottom-left corner in the safe area's, a quarter
        // of its 400 pixels green and the rest the grey track.
        let (left, bottom) = (96, 1080 - 54);
        assert!(near(pixel(&shown, left + 50, bottom - 20), [0, 255, 0]));
        let track = pixel(&shown, left + 250, bottom - 20);
        assert!(near(track, [77, 82, 97]), "{track:?}");
        assert_eq!(
            pixel(&shown, left + 410, bottom - 20),
            pixel(&bare, left + 410, bottom - 20)
        );

        // The text: light letters in a band across the top middle.
        let mut light = 0_u32;
        for y in 54..54 + 64 {
            for x in 760..1160 {
                let [red, green, blue] = pixel(&shown, x, y);
                light += u32::from(red > 200 && green > 200 && blue > 200);
            }
        }
        assert!(light > 1500, "{light} light pixels");

        // The hidden box is not there.
        assert_eq!(pixel(&shown, 1700, 200), pixel(&bare, 1700, 200));
    }

    #[test]
    fn a_game_with_no_file_to_fix_shows_no_error() {
        let actions = ActionMap::default();
        let mut game = ScriptGame::from_source("game.onTik(function() end)", &actions);
        assert!(game.host().last_error().is_some());
        assert_eq!(words(&mut game), Vec::<String>::new());
    }
}
