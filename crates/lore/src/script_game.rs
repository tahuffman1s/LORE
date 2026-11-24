//! A game whose rules are a Luau script.

use std::path::Path;

use lore_input::{ActionMap, InputFrame};
use lore_script::{AudioCommand, OpenError, ScriptHost};

use crate::error_overlay::ErrorOverlay;
use crate::game::{Frame, Game};
use crate::map::{Map, MapError, MapFile};
use crate::script_hud::ScriptHud;
use crate::settings::Settings;
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
    /// The pictures the script can show on its HUD.
    pictures: crate::hud_images::HudPictures,
    /// What is placed in the level.
    map: Map,
    /// The map's file, looked at for saves. Only a game opened from a
    /// folder has one.
    map_file: Option<MapFile>,
    /// Why the map file as it now is could not be read, if it could not.
    map_error: Option<MapError>,
    /// Which game this is to the list of shaders it hands the renderer.
    shaders: crate::game_shaders::Source,
    /// The player's settings as the script was last told them. They are
    /// not kept here to be put into effect: whoever runs the game has the
    /// settings in force, and is handed only what the script changes.
    told: Settings,
    /// The game's folder, where its model files are found
    /// (`game.player:setModel("models/hero.glb")`). A game with no files
    /// behind it has none.
    folder: Option<std::path::PathBuf>,
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
    fn without_files(mut host: ScriptHost, settings: Settings) -> Self {
        host.set_text_measure(crate::error_overlay::measure());
        let mut game = Self {
            host,
            errors: None,
            hud: ScriptHud::default(),
            sounds: Sounds::starter(),
            pictures: crate::hud_images::HudPictures::default(),
            map: Map::default(),
            map_file: None,
            map_error: None,
            shaders: crate::game_shaders::Source::new(),
            told: settings,
            folder: None,
        };
        game.name_sounds();
        game
    }

    /// Reads a model file of the game's, named as a script names it
    /// (`"models/hero.glb"`): a path inside the game's folder.
    ///
    /// # Errors
    ///
    /// When the game has no folder (a packaged game, for now), the path
    /// leads outside it, or the file cannot be read; the message says what
    /// to do.
    pub(crate) fn read_model(&self, file: &str) -> Result<lore_assets::Scene, String> {
        let Some(folder) = &self.folder else {
            return Err(format!(
                "a packaged game cannot load \"{file}\" yet; use \"mannequin\" or \"mannequin_female\""
            ));
        };
        let relative = Path::new(file);
        let inside = relative.components().all(|part| matches!(part, std::path::Component::Normal(_)));
        if !inside {
            return Err(format!(
                "\"{file}\" is not a file inside the game's folder. Name it from the folder, \
                 like \"models/hero.glb\""
            ));
        }
        lore_assets::Scene::load(folder.join(relative)).map_err(|error| error.to_string())
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

    /// Logs each of the game's HUD images that cannot be shown, saying
    /// what is wrong with it. For `lore check`.
    pub fn check_images(&self) {
        for problem in self.pictures.problems() {
            log::warn!("{problem}");
        }
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
        Self::open_with_settings(folder, actions, Settings::default())
    }

    /// Opens the game in `folder` as [`open`](Self::open) does, for a
    /// player who has chosen `settings`: what the game's script reads with
    /// `game.settings` from its first line on, so a menu it builds as it
    /// loads shows them. Run the game with the same settings
    /// ([`Config::settings`](crate::Config)). From the first frame on the
    /// script reads the settings the game is run with, whatever it was
    /// opened with, and what it changes is put into effect onto those.
    ///
    /// ```no_run
    /// use lore::{Config, ScriptGame};
    ///
    /// fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let file = lore::settings::file();
    ///     let config = Config {
    ///         settings: file.as_deref().map_or_else(Default::default, |file| {
    ///             lore::settings::load(file).0
    ///         }),
    ///         settings_file: file,
    ///         ..Config::default()
    ///     };
    ///     let game = ScriptGame::open_with_settings("my-game", &config.actions, config.settings)?;
    ///     lore::run(config, game)?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Fails if `folder` has no `main.luau` or the file cannot be read.
    pub fn open_with_settings(
        folder: impl AsRef<Path>,
        actions: &ActionMap,
        settings: Settings,
    ) -> Result<Self, OpenError> {
        let folder = folder.as_ref();
        let mut host =
            ScriptHost::open_with_settings(folder, actions.clone(), settings.described())?;
        host.set_text_measure(crate::error_overlay::measure());
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
            pictures: crate::hud_images::HudPictures::in_folder(folder),
            map,
            map_file: Some(map_file),
            map_error,
            shaders: crate::game_shaders::Source::new(),
            told: settings,
            folder: Some(folder.to_owned()),
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
        Self::from_sources(
            [(lore_script::ENTRY_SCRIPT.to_owned(), source.into())],
            actions,
        )
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
        Self::from_sources_with_settings(scripts, actions, Settings::default())
    }

    /// Runs a game whose scripts are given as text, as
    /// [`from_sources`](Self::from_sources) does, for a player who has
    /// chosen `settings`: see [`open_with_settings`](Self::open_with_settings).
    #[must_use]
    pub fn from_sources_with_settings(
        scripts: impl IntoIterator<Item = (String, String)>,
        actions: &ActionMap,
        settings: Settings,
    ) -> Self {
        let host =
            ScriptHost::from_sources_with_settings(scripts, actions.clone(), settings.described());
        Self::without_files(host, settings)
    }

    /// Runs a game whose scripts are given as text, as
    /// [`from_sources_with_settings`](Self::from_sources_with_settings)
    /// does, with HUD images already read, by name: a packaged game's.
    #[must_use]
    pub fn from_sources_with_images(
        scripts: impl IntoIterator<Item = (String, String)>,
        images: Vec<(String, lore_render::HudImage)>,
        actions: &ActionMap,
        settings: Settings,
    ) -> Self {
        let mut infos: Vec<lore_script::HudImageInfo> = images
            .iter()
            .map(|(name, image)| lore_script::HudImageInfo {
                name: name.clone(),
                size: image.size,
            })
            .collect();
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        let host = ScriptHost::from_sources_with_images(
            scripts,
            infos,
            actions.clone(),
            settings.described(),
        );
        let mut game = Self::without_files(host, settings);
        game.pictures = crate::hud_images::HudPictures::given(images);
        game
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

    /// Puts what the script's settings menu changed into effect, and keeps
    /// the script told the settings in force.
    ///
    /// Only the settings the script changed are changed, onto the ones in
    /// force: the rest may have changed since the script last heard (the
    /// player's file was read after the game was opened, or a game played
    /// from the editor changed them before this one started), and the ones
    /// in force are the newer. Whoever runs the game saves them.
    fn follow_settings(&mut self, frame: &mut Frame) {
        let mut settings = frame.settings().copied().unwrap_or(self.told);
        if (self.host).take_changed_settings(|name, value| settings.change(name, value)) {
            frame.change_settings(|now| *now = settings);
        }
        if let Some(&now) = frame.settings()
            && now != self.told
        {
            self.host.set_settings(now.described());
            self.told = now;
        }
    }

    /// The script host, for example to read its latest error.
    #[must_use]
    pub fn host(&self) -> &ScriptHost {
        &self.host
    }

    /// The script host, to tell it what happens in the world: where the player is,
    /// what was hit and that the player died.
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
        // What the script reads while it draws: how long pictures take, who
        // is playing and with which controls, so a label and a graph are
        // right in this very frame.
        if frame.delta_seconds() > 0.0 {
            self.host.frame_drawn(frame.delta_seconds());
        }
        self.host.set_device(frame.device(), frame.gamepad_style());
        if let Some(controls) = frame.new_controls() {
            self.host.set_controls(controls);
        }
        frame.set_time_speed(self.host.time_speed());
        self.follow_settings(frame);
        // After those, so what the script asks for while drawing is in this frame.
        self.host.draw(frame.alpha());
        // After the draw too, so a `shader:set` in `game.onDraw` is in this frame.
        frame.shader_list().follow(self.shaders, &self.host);
        if let Some(rumble) = self.host.take_rumble() {
            frame.rumble(rumble);
        }
        if let Some(size) = self.host.take_moon_size() {
            frame.set_moon_size(size);
        }
        self.pictures.follow(&self.host, frame);
        if let Some(shown) = self.host.take_ground() {
            frame.set_ground(shown);
        }
        if let Some(tint) = self.host.take_moon_tint() {
            frame.set_moon_tint(tint);
        }
        if let Some((times, tint)) = self.host.take_milky_way() {
            frame.set_milky_way(times, tint);
        }
        if let Some(class) = self.host.take_light_pollution() {
            frame.set_light_pollution(class);
        }
        if let Some(kp) = self.host.take_aurora() {
            frame.set_aurora(kp);
        }
        if let Some(times) = self.host.take_meteors() {
            frame.set_meteors(times);
        }
        if let Some(place) = self.host.take_sky_place() {
            frame.set_sky_place(place);
        }
        if let Some(hour) = self.host.take_hour() {
            frame.set_hour(hour);
        }
        if let Some((stops, eye, occlusion)) = self.host.take_lighting() {
            frame.set_lighting(
                stops,
                lore_render::EyeAdaptation {
                    opens: eye.opens,
                    closes: eye.closes,
                    seconds: eye.seconds,
                    night: eye.night,
                },
            );
            frame.set_ambient_occlusion(occlusion.strength, occlusion.radius);
        }
        if let Some((look, seconds)) = self.host.take_look() {
            frame.set_look(look_of(&look), seconds);
        }
        // Once the game knows of any lamp, it says which are lit.
        if self.host.has_lamps() {
            let lamps = frame.say_lamps();
            self.host
                .lamps(|light| lamps.push(crate::lamps::drawn(light)));
        }
        // The weather as the script has said it. A world the script
        // plays in says more after this: what has followed from it.
        frame.set_weather(crate::world::said_weather(&self.host.weather()));
        for (at, number) in self.host.take_strikes() {
            frame.strike(lore_render::Lightning::down_to(
                at,
                number.wrapping_mul(0x9E37_79B1),
            ));
        }
        if let Some(set) = self.host.take_action_set() {
            frame.use_action_set(set);
        }
        if self.host.take_quit() {
            frame.close();
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
                AudioCommand::Move(id, place) => frame.sound(SoundAsk::Move(id, place)),
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
mod menus;

/// The look a script said, as the renderer takes it.
fn look_of(said: &lore_script::LookSaid) -> lore_render::Look {
    lore_render::Look {
        tone_mapping: match said.tone_mapping {
            lore_script::ToneMappingSaid::AgX => lore_render::ToneMapping::AgX,
            lore_script::ToneMappingSaid::Neutral => lore_render::ToneMapping::Neutral,
        },
        bloom: said.bloom,
        warmth: said.warmth,
        tint: said.tint,
        contrast: said.contrast,
        saturation: said.saturation,
        shadows: said.shadows,
        midtones: said.midtones,
        highlights: said.highlights,
        vignette: said.vignette,
        grain: said.grain,
        chromatic_aberration: said.chromatic_aberration,
        night: said.night,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_script_that_says_no_look_is_drawn_with_the_engines_own() {
        assert_eq!(
            super::look_of(&lore_script::LookSaid::default()),
            lore_render::Look::default()
        );
    }

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
                Draw::Rect { .. } | Draw::Turned { .. } => None,
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
                Draw::Text { .. } | Draw::Turned { .. } => None,
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

    #[test]
    fn the_action_set_a_script_chooses_is_asked_of_the_window_once() {
        let script = "game.onStart(function() game.input.useSet(\"menu\") end)";
        let actions = ActionMap::standard();
        let mut game = ScriptGame::from_source(script, &actions);
        game.tick(&InputFrame::default(), 1.0 / 60.0);
        let mut frame = Frame::new(0.0);
        game.frame(&mut frame);
        assert_eq!(frame.action_set(), actions.set("menu").ok());

        game.tick(&InputFrame::default(), 1.0 / 60.0);
        let mut frame = Frame::new(0.0);
        game.frame(&mut frame);
        assert_eq!(frame.action_set(), None, "the window already has it");
    }

    /// The rects `game` draws over one frame that lasted `seconds`, as
    /// `(left, top, colour)`.
    fn rects(game: &mut ScriptGame, seconds: f32) -> Vec<([f32; 2], lore_ui::Color)> {
        let mut frame = Frame::new(0.0).with_pointer(seconds, None, false);
        game.frame(&mut frame);
        frame
            .take_ui()
            .draw()
            .iter()
            .filter_map(|draw| match draw {
                Draw::Rect {
                    position, color, ..
                } => Some((position, color)),
                Draw::Text { .. } | Draw::Turned { .. } => None,
            })
            .collect()
    }

    #[test]
    fn a_frame_graph_draws_its_lines_and_a_mark_for_each_frame_drawn() {
        let script =
            r#"game.hud.frameGraph({ at = "top left", width = 100, height = 50, frames = 10 })"#;
        let mut game = ScriptGame::from_source(script, &ActionMap::default());
        // Nothing drawn yet: only the lines at 60 and 30 frames a second.
        assert_eq!(rects(&mut game, 0.0).len(), 2);
        let first = rects(&mut game, 0.01);
        assert_eq!(first.len(), 3, "{first:?}");
        let more = rects(&mut game, 0.02);
        let marks = rects(&mut game, 0.06);
        assert_eq!((more.len(), marks.len()), (4, 5));
        // The three marks, oldest to newest: they go from left to right,
        // and a slower frame is marked higher up, in a poorer colour.
        let marks = &marks[2..];
        assert!(marks[0].0[0] < marks[1].0[0] && marks[1].0[0] < marks[2].0[0]);
        assert!(marks[0].0[1] > marks[1].0[1] && marks[1].0[1] > marks[2].0[1]);
        assert!(marks[0].1 != marks[1].1 && marks[1].1 != marks[2].1);
        // Frames fill the graph and then push the oldest out.
        for _ in 0..20 {
            rects(&mut game, 0.016);
        }
        assert_eq!(rects(&mut game, 0.016).len(), 12);
    }

    #[test]
    fn a_frame_graph_that_is_hidden_draws_nothing() {
        let script = r#"
            local graph = game.hud.frameGraph({ frames = 10 })
            graph.visible = false
        "#;
        let mut game = ScriptGame::from_source(script, &ActionMap::default());
        game.tick(&InputFrame::default(), 1.0 / 60.0);
        assert_eq!(rects(&mut game, 0.016), []);
    }

    #[test]
    fn the_frame_tells_the_script_the_controls_and_the_device_and_hears_the_speed_of_time() {
        let script = r#"
            local hint = game.hud.text("")
            game.onDraw(function() hint.text = game.input.label("jump") end)
            game.onTick(function() game.setTimeSpeed(0.5) end)
        "#;
        let actions = ActionMap::standard();
        let mut game = ScriptGame::from_source(script, &actions);
        game.tick(&InputFrame::default(), 1.0 / 60.0);
        let mut frame = Frame::new(0.0)
            .with_device(
                lore_input::Device::Gamepad,
                lore_input::GamepadStyle::PlayStation,
            )
            .with_controls(Some(actions));
        game.frame(&mut frame);
        assert_eq!(frame.time_speed(), Some(0.5));
        let said = frame
            .take_ui()
            .draw()
            .iter()
            .find_map(|draw| match draw {
                Draw::Text { text, .. } => Some(text.to_string()),
                Draw::Rect { .. } | Draw::Turned { .. } => None,
            })
            .unwrap_or_default();
        assert_eq!(said, "Cross");
    }

    /// The script error is drawn for real, with no window, and the picture
    /// is read back: a dark panel with a red line along its top and light
    /// letters in it, in the top-left corner, and the game untouched
    /// everywhere else.
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

    /// A game's own screen effect, from its folder to the picture: it paints,
    /// a value set while drawing repaints it with no rebuild, a mistake
    /// saved over it leaves the picture and shows on screen, and a fix
    /// clears the error.
    #[test]
    fn a_screen_effect_from_the_folder_is_drawn_changed_and_fixed_live() {
        use crate::game_shaders::ShaderList;
        use lore_render::{Camera, Headless, RenderError, Upscaling};

        const SIZE: u32 = 64;
        let mut headless = match Headless::new(SIZE, SIZE * 3 / 4, Upscaling::Off) {
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
            std::env::temp_dir().join(format!("lore-script-game-shaders-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("shaders")).unwrap();
        // Green until the game has drawn twice, then blue.
        fs::write(
            folder.join("main.luau"),
            "local paint = game.shaders.screen(\"paint\")\n\
             local drawn = 0\n\
             game.onDraw(function()\n\
                 drawn = drawn + 1\n\
                 if drawn == 3 then paint:set(\"color\", \"#0000ff\") end\n\
             end)\n",
        )
        .unwrap();
        let shader = folder.join("shaders/paint.wgsl");
        let text = "//! param color: color = #00ff00\n\
                    fn screen(in: ScreenIn) -> vec3f { return params.color; }\n";
        fs::write(&shader, text).unwrap();
        let mut game = ScriptGame::open(&folder, &ActionMap::default()).unwrap();
        // The script draws only once the game has ticked.
        game.tick(&InputFrame::default(), 1.0 / 60.0);

        // One frame as the app makes it: the list is lent, followed, handed
        // to the renderer, and what the renderer says comes back.
        #[derive(Default)]
        struct Rig {
            list: ShaderList,
            reports: usize,
            camera: Camera,
        }
        impl Rig {
            fn step(&mut self, game: &mut ScriptGame, headless: &mut Headless) -> [u8; 3] {
                let mut frame = Frame::new(0.0).with_shader_list(std::mem::take(&mut self.list));
                game.frame(&mut frame);
                self.list = frame.take_shader_list();
                self.list
                    .hand_over(|shaders| headless.set_game_shaders(shaders));
                let picture = headless.screenshot(&self.camera).unwrap();
                let heard = headless.take_shader_reports();
                self.reports += heard.len();
                self.list.hear(heard);
                let at = ((SIZE * 3 / 8 * SIZE + SIZE / 2) * 4) as usize;
                [picture[at], picture[at + 1], picture[at + 2]]
            }
        }
        let mut rig = Rig::default();

        let green = rig.step(&mut game, &mut headless);
        assert!(
            green[1] > 200 && green[0] < 60 && green[2] < 60,
            "{green:?}"
        );
        assert_eq!(rig.reports, 1);
        assert_eq!(rig.list.problems().count(), 0);
        let revision = rig.list.hand_over(|shaders| shaders[0].revision);

        // Drawing again changes nothing; the third draw sets the value.
        assert_eq!(rig.step(&mut game, &mut headless), green);
        let blue = rig.step(&mut game, &mut headless);
        assert!(blue[2] > 200 && blue[0] < 60 && blue[1] < 60, "{blue:?}");
        // No new revision went over and the renderer built nothing.
        assert_eq!(rig.list.hand_over(|shaders| shaders[0].revision), revision);
        assert_eq!(rig.reports, 1);

        // A mistake saved over the file: the picture stays as it was.
        let wait_for = |game: &mut ScriptGame, done: &dyn Fn(&ScriptGame) -> bool| {
            let give_up = Instant::now() + Duration::from_secs(20);
            while !done(game) && Instant::now() < give_up {
                std::thread::sleep(Duration::from_millis(20));
                game.tick(&InputFrame::default(), 1.0 / 60.0);
            }
        };
        let revision_now = game.host().shader_revision();
        fs::write(&shader, text.replace("params.color", "params.colour")).unwrap();
        wait_for(&mut game, &|game| {
            game.host().shader_revision() != revision_now
        });
        let kept = rig.step(&mut game, &mut headless);
        assert_eq!(kept, blue);
        assert_eq!(rig.reports, 2);
        let problem = rig.list.problems().next().map(ToString::to_string);
        let problem = problem.unwrap_or_default();
        assert!(problem.contains("shaders/paint.wgsl, line 2"), "{problem}");
        let mut frame = Frame::new(0.0).with_shader_list(std::mem::take(&mut rig.list));
        game.frame(&mut frame);
        let shown: Vec<String> = frame
            .take_ui()
            .draw()
            .iter()
            .filter_map(|draw| match draw {
                Draw::Text { text, .. } => Some(text.to_string()),
                Draw::Rect { .. } | Draw::Turned { .. } => None,
            })
            .collect();
        rig.list = frame.take_shader_list();
        assert_eq!(
            shown[0], "Shader error in shaders/paint.wgsl, line 2",
            "{shown:?}"
        );

        // Saving it right again clears the error and draws the new text.
        fs::write(
            &shader,
            text.replace("return params.color;", "return params.color * 0.5;"),
        )
        .unwrap();
        let revision_now = game.host().shader_revision();
        wait_for(&mut game, &|game| {
            game.host().shader_revision() != revision_now
        });
        let fixed = rig.step(&mut game, &mut headless);
        assert_eq!(rig.list.problems().count(), 0);
        assert!(
            fixed[2] > 20 && fixed[2] < blue[2],
            "{fixed:?} against {blue:?}"
        );
        // The mistake was one report, the fix another.
        assert_eq!(rig.reports, 3);
        assert_eq!(lore_render::validation_errors(), 0);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_game_with_no_file_to_fix_shows_no_error() {
        let actions = ActionMap::default();
        let mut game = ScriptGame::from_source("game.onTik(function() end)", &actions);
        assert!(game.host().last_error().is_some());
        assert_eq!(words(&mut game), Vec::<String>::new());
    }
}
