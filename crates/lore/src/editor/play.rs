//! Playing the level from the editor: one key, or one gamepad button,
//! starts the game where the camera is, and the same one comes back.
//!
//! What is played is the game as `lore run` plays it (its script, the
//! level's shapes solid), on a copy of the map the editor holds, saved or not. Nothing done while
//! playing reaches the editor: its camera, what is chosen and the map are
//! as they were when playing stops.
//!
//! The player's eyes start where the camera was, looking the same way, and
//! the player drops to the floor from there. A camera with nothing under
//! it to land on (off the edge of the level, say) starts the player at a
//! spawn point, as the game itself would, and that is said.

use std::path::Path;

use lore_input::ActionMap;

use crate::game::{Frame, Game};
use crate::map::Map;
use crate::script_game::ScriptGame;
use crate::settings::Settings;
use crate::world::{Ground, Playing};

/// Said on the screen for as long as the level is played.
const PLAYING: &str = "Playing    F5 or Select: back to editing";
/// Said under it when the player could not start where the camera was.
const NOT_FROM_HERE: &str = "There is nothing under the camera to stand on, so you started \
                             at a spawn point.";

/// The level being played from the editor.
#[derive(Debug)]
pub(super) struct Play {
    game: Playing,
    /// Whether the player started where the camera was.
    from_here: bool,
    /// The game asked to end (`game.quit`), which here means going back to
    /// editing.
    quit: bool,
}

impl Play {
    /// Starts the game in `folder` (or, with no folder, a game with no
    /// script) on `ground`, with `map` placed in it, for a player with the
    /// `settings` in force, and puts the player's eyes at `eye` looking
    /// the way `facing` says (yaw, then pitch).
    ///
    /// # Errors
    ///
    /// Says, for the person at the editor, why the game could not start.
    pub(super) fn start(
        folder: Option<&Path>,
        map: &Map,
        ground: &Ground,
        actions: &ActionMap,
        settings: Settings,
        eye: [f32; 3],
        facing: [f32; 2],
    ) -> Result<Self, String> {
        let game = match folder {
            Some(folder) => ScriptGame::open_with_settings(folder, actions, settings)
                .map_err(|error| format!("Could not play: {error}"))?,
            None => ScriptGame::from_sources_with_settings(
                [(lore_script::ENTRY_SCRIPT.to_owned(), String::new())],
                actions,
                settings,
            ),
        };
        // The editor's controls are fixed, so a game's own actions are not
        // heard here; `lore run` plays with them.
        if game.host().actions().actions().count() > actions.actions().count() {
            log::info!(
                "The game's own actions (made with game.input.bind) cannot be pressed while \
                 playing from the editor. Play the game with `lore run` to try them."
            );
        }
        // The map as the editor has it, whatever is in the file by now.
        let game = game.with_map(map.clone());
        let (game, from_here) = Playing::start_from(game, ground, actions, eye, facing);
        Ok(Self {
            game,
            from_here,
            quit: false,
        })
    }

    /// Says what lamps the level places, by name, for the script to find
    /// and change. Call it before the first tick.
    pub(super) fn set_level_lamps(&mut self, lamps: &[(String, lore_script::LampLight)]) {
        self.game
            .world_mut()
            .game_mut()
            .host_mut()
            .set_level_lamps(lamps);
    }

    /// The action set the game's script has chosen: `gameplay` unless it
    /// said otherwise.
    pub(super) fn action_set(&self) -> lore_input::ActionSetId {
        self.game.action_set()
    }

    /// Whether the game has asked to end.
    pub(super) fn quit(&self) -> bool {
        self.quit
    }

    /// Plays one tick.
    pub(super) fn tick(&mut self, input: &lore_input::InputFrame, step: f32) {
        self.game.tick(input, step);
    }

    /// Presents the game's frame, with a line over it that says how to get
    /// back to editing.
    pub(super) fn frame(&mut self, frame: &mut Frame) {
        self.game.frame(frame);
        // The game asking to end is its player leaving it: back to editing,
        // not out of the editor.
        self.quit |= frame.take_close();
        let from_here = self.from_here;
        frame.panels(|ui| {
            // A name being typed when playing began must not go on taking
            // the keys that now walk.
            ui.ctx().memory_mut(|memory| {
                if let Some(typing) = memory.focused() {
                    memory.surrender_focus(typing);
                }
            });
            banner(ui, from_here);
        });
    }

    /// Where the player's eyes are.
    #[cfg(test)]
    pub(super) fn eye(&self) -> [f32; 3] {
        self.game.eye_position()
    }

    /// Which way the player looks: yaw, then pitch.
    #[cfg(test)]
    pub(super) fn facing(&self) -> [f32; 2] {
        [self.game.yaw(), self.game.pitch()]
    }

    /// Whether the player started where the camera was.
    #[cfg(test)]
    pub(super) fn started_here(&self) -> bool {
        self.from_here
    }

    /// The map the game is played with.
    #[cfg(test)]
    pub(super) fn map(&self) -> &Map {
        self.game.world().game().map()
    }

    /// What runs the game's script.
    #[cfg(test)]
    pub(super) fn host(&self) -> &lore_script::ScriptHost {
        self.game.world().game().host()
    }
}

/// Says along the top of the picture that the level is being played and
/// how to get back. Only painted: it takes no clicks from the game.
fn banner(ui: &egui::Ui, from_here: bool) {
    let window = ui.ctx().content_rect();
    let painter = ui.painter();
    let dark = egui::Color32::from_black_alpha(190);
    let mut top = window.top() + 10.0;
    let lines = [
        (PLAYING, 16.0, egui::Color32::WHITE),
        (NOT_FROM_HERE, 14.0, egui::Color32::from_rgb(255, 214, 110)),
    ];
    for (words, size, ink) in lines.into_iter().take(if from_here { 1 } else { 2 }) {
        let galley =
            painter.layout_no_wrap(words.to_owned(), egui::FontId::proportional(size), ink);
        let at = egui::pos2(window.center().x - galley.size().x / 2.0, top + 5.0);
        let back = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(12.0, 5.0));
        painter.rect_filled(back, 6.0, dark);
        painter.galley(at, galley, ink);
        top = back.bottom() + 4.0;
    }
}
