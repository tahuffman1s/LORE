//! The prompt for a thing the player can use: the key or button they have
//! bound to it, and the word or two the script gave ("Open", "Talk").
//!
//! It sits under the crosshair while a usable thing is under it and in
//! reach, with no script code; a thing that is used by holding the button
//! shows a bar filling up. A game that draws its own says
//! `game.showUsePrompt(false)`.

use lore_script::ScriptHost;
use lore_ui::{Anchor, Color, TextMeasure, Ui, UiInput};

use crate::game::Frame;

/// How far under the middle of the screen the prompt is, in pixels on a
/// screen 1080 high: clear of a crosshair, near where the eyes are.
const DOWN: f32 = 110.0;
/// How high its words are.
const TEXT: f32 = 30.0;
/// How high the box round the key's name is, and the least it is wide.
const KEY: f32 = 44.0;
/// Space between the key's name and the edge of its box.
const KEY_PADDING: f32 = 12.0;
/// Space between the key and the words.
const GAP: f32 = 14.0;
/// The bar of a held use: how far under the words, and how big.
const BAR_DOWN: f32 = 38.0;
const BAR: [f32; 2] = [180.0, 6.0];
/// How many steps a held use's bar is drawn in: the prompt is laid out
/// again only when it moves a step.
const STEPS: f32 = 60.0;

const KEY_BOX: Color = Color::rgba(0.96, 0.96, 0.97, 0.95);
const KEY_WORDS: Color = Color::rgb(0.08, 0.09, 0.11);
const BACK: Color = Color::rgba(0.05, 0.06, 0.08, 0.55);
const BAR_BACK: Color = Color::rgba(1.0, 1.0, 1.0, 0.25);

/// What the prompt last showed, so it is only laid out again on a change.
#[derive(Debug, PartialEq)]
struct Shown {
    key: String,
    words: String,
    step: u8,
    screen: [u32; 2],
}

/// Draws the prompt over the game.
pub(crate) struct UsePrompt {
    shown: Option<Shown>,
    ui: Ui,
    measure: Box<dyn TextMeasure>,
}

impl std::fmt::Debug for UsePrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsePrompt")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl Default for UsePrompt {
    fn default() -> Self {
        Self {
            shown: None,
            ui: Ui::default(),
            measure: crate::error_overlay::measure(),
        }
    }
}

impl UsePrompt {
    /// Draws the prompt for what `host` says the player can use, or nothing.
    pub(crate) fn show(&mut self, host: &ScriptHost, frame: &mut Frame) {
        let Some((words, progress)) = host.use_prompt() else {
            self.shown = None;
            return;
        };
        let screen = frame.screen_size();
        // A share from 0 to 1, in sixty steps.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let step = (progress.clamp(0.0, 1.0) * STEPS).round() as u8;
        let now = Shown {
            key: host.use_label(),
            words,
            step,
            screen,
        };
        if self.shown.as_ref() != Some(&now) {
            self.ui = laid_out(&now, self.measure.as_ref());
            self.shown = Some(now);
        }
        self.ui.update(&UiInput::default(), 0.0, screen);
        frame.draw_ui(&self.ui);
    }
}

/// The prompt as a UI: the key in a light box, the words beside it on a
/// dark one, and under them the bar of a held use.
fn laid_out(shown: &Shown, measure: &dyn TextMeasure) -> Ui {
    let mut ui = Ui::default();
    // Measured with the font it is drawn in, so that what is pinned by its
    // middle is where these sums put it.
    ui.set_text_measure(crate::error_overlay::measure());
    let words_wide = lore_ui::hud_text_size(&shown.words, TEXT, measure)[0];
    let key_wide = if shown.key.is_empty() {
        0.0
    } else {
        (lore_ui::hud_text_size(&shown.key, TEXT, measure)[0] + 2.0 * KEY_PADDING).max(KEY)
    };
    let gap = if key_wide > 0.0 { GAP } else { 0.0 };
    let whole = key_wide + gap + words_wide;
    let left = -whole / 2.0;
    // Pieces pinned to the middle are moved by where their own middle goes.
    ui.hud_rect(
        Anchor::Center,
        [0.0, DOWN],
        [whole + 2.0 * KEY_PADDING, KEY + KEY_PADDING],
        BACK,
    );
    if key_wide > 0.0 {
        let middle = left + key_wide / 2.0;
        ui.hud_rect(Anchor::Center, [middle, DOWN], [key_wide, KEY], KEY_BOX);
        let key = ui.hud_text(Anchor::Center, [middle, DOWN], TEXT, &shown.key);
        // The widget is this UI's own HUD text, so colouring it cannot fail.
        let _ = ui.set_color(key, KEY_WORDS);
    }
    ui.hud_text(
        Anchor::Center,
        [left + key_wide + gap + words_wide / 2.0, DOWN],
        TEXT,
        &shown.words,
    );
    if shown.step > 0 {
        let share = f32::from(shown.step) / STEPS;
        ui.hud_rect(Anchor::Center, [0.0, DOWN + BAR_DOWN], BAR, BAR_BACK);
        ui.hud_rect(
            Anchor::Center,
            [-(BAR[0] * (1.0 - share)) / 2.0, DOWN + BAR_DOWN],
            [BAR[0] * share, BAR[1]],
            Color::WHITE,
        );
    }
    ui
}
