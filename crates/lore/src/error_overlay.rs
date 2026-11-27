//! The latest script error and the first shader error, shown over the game
//! until the script loads again or the shader is put right.
//!
//! The same errors are in the terminal, but the person who made them is
//! looking at the game. The panels sit in the top-left corner, the script's
//! first and the shader's under it (the two are fixed in different files,
//! so neither hides the other), and the game carries on behind them.

use std::fmt;

use lore_script::ScriptError;
use lore_shader::ShaderError;
use lore_ui::{Anchor, Color, TextMeasure, Ui, UiInput};

use crate::game::Frame;

/// The widest the panel gets, in pixels on a screen 1080 high.
const WIDEST: f32 = 1100.0;
/// Space between the panel's edge and its text.
const PADDING: f32 = 20.0;
/// Space between the script's panel and the shader's under it.
const GAP: f32 = 12.0;
/// The coloured line along the top of the panel.
const STRIPE: f32 = 6.0;
/// Height of the first line, which says where the error is.
const TITLE_SIZE: f32 = 30.0;
/// Height of every other line: the smallest text the UI draws.
const TEXT_SIZE: f32 = lore_ui::SMALLEST_TEXT;
/// A line's height is this many times its text's.
const LINE_SPACING: f32 = 1.3;
/// Lines are wrapped a little short of the panel, because the widths
/// measured here are for text a hair larger or smaller than what is drawn.
const SLACK: f32 = 0.97;
/// The most lines shown. A longer error ends by pointing at the terminal.
const MOST_LINES: usize = 14;
/// The most characters of one part of an error (its message, its hint)
/// that are looked at. Scripts write their own messages, so one can be
/// any length.
const MOST_CHARACTERS: usize = 600;
/// The last line of an error too long to show whole.
const CUT_SHORT: &str = "The rest is in the terminal.";

const BACKGROUND: Color = Color::rgba(0.09, 0.09, 0.1, 0.94);
const RED: Color = Color::rgb(1.0, 0.33, 0.33);
const TITLE: Color = Color::rgb(1.0, 0.56, 0.52);
const SOURCE: Color = Color::rgb(0.98, 0.86, 0.5);
const MESSAGE: Color = Color::rgb(0.95, 0.95, 0.96);
const QUIET: Color = Color::rgb(0.72, 0.74, 0.78);

/// One line of the panel.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    text: String,
    size: f32,
    color: Color,
}

/// What text is measured with: the font it is drawn in.
pub(crate) fn measure() -> Box<dyn TextMeasure> {
    Box::new(lore_render::Font::bundled())
}

/// Shows a script error and shader errors over the game.
pub(crate) struct ErrorOverlay {
    /// The errors on show and the window size they were laid out for.
    shown: Option<Shown>,
    ui: Ui,
    measure: Box<dyn TextMeasure>,
}

/// What is on show.
#[derive(Debug)]
struct Shown {
    script: Option<ScriptError>,
    shaders: Vec<ShaderError>,
    level: Option<std::rc::Rc<str>>,
    screen: [u32; 2],
}

impl fmt::Debug for ErrorOverlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErrorOverlay")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl Default for ErrorOverlay {
    fn default() -> Self {
        Self {
            shown: None,
            ui: Ui::default(),
            measure: measure(),
        }
    }
}

impl ErrorOverlay {
    /// Draws `error` and the frame's shader problems over this frame, or
    /// nothing when there are none.
    ///
    /// The panels are laid out again only when what they say or the
    /// window's size changes, so showing the same errors frame after frame
    /// asks for no memory.
    pub(crate) fn show(&mut self, error: Option<&ScriptError>, frame: &mut Frame) {
        let screen = frame.screen_size();
        let level = frame.level_problem().cloned();
        let shaders = frame.shader_list();
        if error.is_none() && level.is_none() && shaders.problems().next().is_none() {
            self.shown = None;
            return;
        }
        let current = self.shown.as_ref().is_some_and(|shown| {
            shown.script.as_ref() == error
                && shown.level == level
                && shown.screen == screen
                && shaders.problems().eq(shown.shaders.iter())
        });
        if !current {
            let shaders: Vec<ShaderError> = shaders.problems().cloned().collect();
            self.ui = panels(
                error,
                &shaders,
                level.as_deref(),
                screen,
                self.measure.as_ref(),
            );
            self.shown = Some(Shown {
                script: error.cloned(),
                shaders,
                level,
                screen,
            });
        }
        self.ui.update(&UiInput::default(), 0.0, screen);
        frame.draw_ui(&self.ui);
    }
}

/// How long the notice of a problem stays up after the problem is new.
const NOTICE_SECONDS: f32 = 12.0;
/// The most problems written to one run's log: a game that breaks every
/// tick in a new way must not fill a player's disk.
const MOST_LOGGED: usize = 100;
/// What a player is told. The script's own words stay in the log.
const NOTICE: &str = "This game has hit a problem and may not work as it should.";

/// Tells the player of a packaged game that its script has failed, and
/// writes the error where the game's maker can be sent it.
///
/// A player cannot fix a script, so none of the error is put on the screen:
/// one short notice names the log file, stays up for
/// [`NOTICE_SECONDS`] and comes back only when a different problem happens.
pub(crate) struct ProblemNotice {
    /// Where the log goes: the first of these that can be written.
    places: Vec<std::path::PathBuf>,
    /// The log, once the first problem has been written to it.
    log: Option<std::path::PathBuf>,
    /// The problems written so far this run.
    seen: Vec<ScriptError>,
    /// When the newest problem was first seen.
    since: Option<std::time::Instant>,
    /// The window size the notice was laid out for.
    screen: [u32; 2],
    ui: Ui,
    measure: Box<dyn TextMeasure>,
}

impl fmt::Debug for ProblemNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProblemNotice")
            .field("places", &self.places)
            .field("log", &self.log)
            .field("seen", &self.seen.len())
            .finish_non_exhaustive()
    }
}

impl ProblemNotice {
    /// A notice whose log is the first of `places` that can be written.
    pub(crate) fn new(places: Vec<std::path::PathBuf>) -> Self {
        Self {
            places,
            log: None,
            seen: Vec::new(),
            since: None,
            screen: [0, 0],
            ui: Ui::default(),
            measure: measure(),
        }
    }

    /// Takes note of `error`, the script's latest: one not seen before this
    /// run is written to the log and puts the notice up. Says whether it
    /// was new.
    pub(crate) fn note(&mut self, error: Option<&ScriptError>) -> bool {
        let Some(error) = error else {
            return false;
        };
        if self.seen.contains(error) || self.seen.len() >= MOST_LOGGED {
            return false;
        }
        self.write(error);
        self.seen.push(error.clone());
        self.since = Some(std::time::Instant::now());
        // Laid out again, since the log may only now have a place.
        self.screen = [0, 0];
        true
    }

    /// Adds `error` to the log. The first problem of a run starts the file
    /// again, so the log a player sends is about the run that went wrong.
    fn write(&mut self, error: &ScriptError) {
        use std::io::Write as _;

        let text = format!("{error}\n\n");
        if let Some(log) = &self.log {
            let added = std::fs::OpenOptions::new()
                .append(true)
                .open(log)
                .and_then(|mut file| file.write_all(text.as_bytes()));
            if let Err(problem) = added {
                log::warn!("{} could not be added to: {problem}.", log.display());
            }
            return;
        }
        let heading = format!(
            "What went wrong in this game's scripts the last time it was played. Send \
             this file to the game's maker.\nLORE {}\n\n",
            env!("CARGO_PKG_VERSION")
        );
        for place in &self.places {
            if let Some(folder) = place.parent() {
                // A folder that is already there is no problem; one that
                // cannot be made fails the write below.
                let _ = std::fs::create_dir_all(folder);
            }
            if std::fs::write(place, format!("{heading}{text}")).is_ok() {
                self.log = Some(place.clone());
                return;
            }
        }
        log::warn!("The game's problems could not be written to a log file; they are only here.");
    }

    /// The lines of the notice, none wider than `width`.
    fn lines(&self, width: f32) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut add = |text: &str, size: f32, color: Color| {
            let mut wrapped = Vec::new();
            wrap(text, width, size, self.measure.as_ref(), &mut wrapped);
            lines.extend(wrapped.into_iter().map(|text| Line { text, size, color }));
        };
        add(NOTICE, TEXT_SIZE, MESSAGE);
        match &self.log {
            Some(log) => add(
                &format!("Its maker can find the details in {}", log.display()),
                TEXT_SIZE,
                QUIET,
            ),
            None => add(
                "Its maker can find the details by starting it from a terminal.",
                TEXT_SIZE,
                QUIET,
            ),
        }
        lines
    }

    /// Notes `error` and draws the notice over this frame while it is new.
    pub(crate) fn show(&mut self, error: Option<&ScriptError>, frame: &mut Frame) {
        self.note(error);
        let Some(since) = self.since else {
            return;
        };
        if since.elapsed().as_secs_f32() > NOTICE_SECONDS {
            self.since = None;
            return;
        }
        let screen = frame.screen_size();
        if screen != self.screen {
            let (wide, width) = panel_width(screen);
            let lines = self.lines(width);
            self.ui = Ui::default();
            panel(&mut self.ui, 0.0, wide, &lines);
            self.screen = screen;
        }
        self.ui.update(&UiInput::default(), 0.0, screen);
        frame.draw_ui(&self.ui);
    }
}

/// How wide a panel is on a `screen` of that many pixels, and how wide its
/// text may be.
pub(crate) fn panel_width(screen: [u32; 2]) -> (f32, f32) {
    // The UI lays out on a screen 1080 high and keeps its middle 90%.
    #[allow(
        clippy::cast_precision_loss,
        reason = "screens are far smaller than 2^24 pixels"
    )]
    let across = screen[0] as f32 * 1080.0 / screen[1].max(1) as f32;
    let wide = (across * 0.9).min(WIDEST);
    (wide, (wide - 2.0 * PADDING) * SLACK)
}

/// A UI that shows `error` and the first of `shaders` in the top-left
/// corner of a `screen` of that many pixels, one under the other.
fn panels(
    error: Option<&ScriptError>,
    shaders: &[ShaderError],
    level: Option<&str>,
    screen: [u32; 2],
    measure: &dyn TextMeasure,
) -> Ui {
    let mut ui = Ui::default();
    let (wide, width) = panel_width(screen);

    let mut top = 0.0;
    let script = error.map(|error| lines(error, width, measure));
    let shader = (!shaders.is_empty()).then(|| shader_lines(shaders, width, measure));
    let level = level.map(|problem| level_lines(problem, width, measure));
    for lines in script.into_iter().chain(shader).chain(level) {
        top += panel(&mut ui, top, wide, &lines) + GAP;
    }
    ui
}

/// Adds one panel of `lines` to `ui`, `top` pixels down the screen, and
/// says how high it is.
fn panel(ui: &mut Ui, top: f32, wide: f32, lines: &[Line]) -> f32 {
    let high: f32 = STRIPE
        + 2.0 * PADDING
        + lines
            .iter()
            .map(|line| line.size * LINE_SPACING)
            .sum::<f32>();
    ui.hud_rect(Anchor::TopLeft, [0.0, top], [wide, high], BACKGROUND);
    ui.hud_rect(Anchor::TopLeft, [0.0, top], [wide, STRIPE], RED);
    let mut down = top + STRIPE + PADDING;
    for line in lines {
        let gap = line.size * (LINE_SPACING - 1.0) / 2.0;
        let text = ui.hud_text(
            Anchor::TopLeft,
            [PADDING, down + gap],
            line.size,
            &line.text,
        );
        // The widget is this UI's own HUD text, so colouring it cannot fail.
        let _ = ui.set_color(text, line.color);
        down += line.size * LINE_SPACING;
    }
    // Every piece is pinned by its left edge, so the UI's own guess at how
    // wide text is moves nothing.
    high
}

/// The lines of the panel for `error`, none wider than `width`: where it
/// is, the line of the script, what went wrong, what to try and what the
/// engine did about it.
fn lines(error: &ScriptError, width: f32, measure: &dyn TextMeasure) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut add = |text: &str, size: f32, color: Color| {
        let mut wrapped = Vec::new();
        wrap(&clean(text), width, size, measure, &mut wrapped);
        lines.extend(wrapped.into_iter().map(|text| Line { text, size, color }));
    };

    let place = match error.line {
        Some(line) => format!("Script error in {}, line {line}", error.file),
        None => format!("Script error in {}", error.file),
    };
    add(&place, TITLE_SIZE, TITLE);
    if let (Some(line), Some(source)) = (error.line, &error.source_line) {
        add(&format!("{line} | {}", source.trim()), TEXT_SIZE, SOURCE);
    }
    add(&error.message, TEXT_SIZE, MESSAGE);
    if let Some(hint) = &error.hint {
        add(&format!("Hint: {hint}"), TEXT_SIZE, QUIET);
    }
    if let Some(note) = &error.note {
        add(note, TEXT_SIZE, QUIET);
    }

    cut_short(&mut lines);
    lines
}

/// The lines of the panel for the first of `errors`, none wider than
/// `width`, and when there are more, a quiet line saying so.
fn shader_lines(errors: &[ShaderError], width: f32, measure: &dyn TextMeasure) -> Vec<Line> {
    let mut lines = Vec::new();
    let Some(error) = errors.first() else {
        return lines;
    };
    let mut add = |text: &str, size: f32, color: Color| {
        let mut wrapped = Vec::new();
        wrap(&clean(text), width, size, measure, &mut wrapped);
        lines.extend(wrapped.into_iter().map(|text| Line { text, size, color }));
    };

    let whose = if error.lore_fault { "LORE's own " } else { "" };
    let place = match error.line {
        Some(line) => format!("Shader error in {whose}{}, line {line}", error.file),
        None => format!("Shader error in {whose}{}", error.file),
    };
    add(&place, TITLE_SIZE, TITLE);
    if let (Some(line), Some(source)) = (error.line, &error.source_line)
        && !source.trim().is_empty()
    {
        add(&format!("{line} | {}", source.trim()), TEXT_SIZE, SOURCE);
    }
    add(&error.message, TEXT_SIZE, MESSAGE);
    if let Some(hint) = &error.hint {
        add(&format!("Hint: {hint}"), TEXT_SIZE, QUIET);
    }
    if let Some(note) = &error.note {
        add(note, TEXT_SIZE, QUIET);
    }
    cut_short(&mut lines);

    let more = errors.len() - 1;
    if more > 0 {
        let text = if more == 1 {
            "1 more shader error is in the terminal.".to_owned()
        } else {
            format!("{more} more shader errors are in the terminal.")
        };
        lines.push(Line {
            text,
            size: TEXT_SIZE,
            color: QUIET,
        });
    }
    lines
}

/// The lines of the panel for a level that could not be loaded again,
/// none wider than `width`.
fn level_lines(problem: &str, width: f32, measure: &dyn TextMeasure) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut add = |text: &str, size: f32, color: Color| {
        let mut wrapped = Vec::new();
        wrap(&clean(text), width, size, measure, &mut wrapped);
        lines.extend(wrapped.into_iter().map(|text| Line { text, size, color }));
    };
    add("The saved level could not be loaded", TITLE_SIZE, TITLE);
    add(problem, TEXT_SIZE, MESSAGE);
    add(
        "The level that was showing is kept. Export it again to try again.",
        TEXT_SIZE,
        QUIET,
    );
    cut_short(&mut lines);
    lines
}

/// Ends a panel of more than [`MOST_LINES`] by pointing at the terminal.
fn cut_short(lines: &mut Vec<Line>) {
    if lines.len() > MOST_LINES {
        lines.truncate(MOST_LINES - 1);
        lines.push(Line {
            text: CUT_SHORT.to_owned(),
            size: TEXT_SIZE,
            color: QUIET,
        });
    }
}

/// `text` as one run of words that can be drawn: line breaks, tabs and
/// other characters with no shape become spaces, and text past
/// [`MOST_CHARACTERS`] is left out with `...` in its place.
pub(crate) fn clean(text: &str) -> String {
    let mut cleaned: String = text
        .chars()
        .take(MOST_CHARACTERS)
        .map(|character| {
            if character.is_control() || character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if text.chars().nth(MOST_CHARACTERS).is_some() {
        cleaned.push_str("...");
    }
    cleaned
}

/// Splits `text` into lines no wider than `width` when drawn `size` high,
/// between words where it can. A word wider than a whole line is split
/// wherever the line ends.
pub(crate) fn wrap(
    text: &str,
    width: f32,
    size: f32,
    measure: &dyn TextMeasure,
    lines: &mut Vec<String>,
) {
    let fits = |line: &str| measure.width(line, size) <= width;
    let mut line = String::new();
    for word in text.split_whitespace() {
        let before = line.len();
        if before > 0 {
            line.push(' ');
        }
        line.push_str(word);
        if fits(&line) {
            continue;
        }
        // The word goes on a line of its own.
        line.truncate(before);
        if before > 0 {
            lines.push(std::mem::take(&mut line));
        }
        for character in word.chars() {
            line.push(character);
            // A line holds at least one character, however narrow it is.
            if !fits(&line) && line.chars().nth(1).is_some() {
                line.pop();
                lines.push(std::mem::take(&mut line));
                line.push(character);
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
}

#[cfg(test)]
mod tests {
    use lore_ui::{Draw, DrawBuffer, EstimatedText};

    use super::*;

    fn error() -> ScriptError {
        ScriptError {
            file: "main.luau".to_owned(),
            line: Some(4),
            message: "game.onTik does not exist. Did you mean game.onTick?".to_owned(),
            source_line: Some("game.onTik(function(step)".to_owned()),
            hint: Some("Check the spelling.".to_owned()),
            note: Some("The game keeps running.".to_owned()),
        }
    }

    fn wrapped(text: &str, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        // Every character is 0.55 * 20 = 11 wide.
        wrap(text, width, 20.0, &EstimatedText, &mut lines);
        lines
    }

    /// A box's top-left corner and its size.
    type Placed = ([f32; 2], [f32; 2]);

    /// The words and boxes of what a frame was asked to draw.
    fn drawn(buffer: &DrawBuffer) -> (Vec<String>, Vec<Placed>) {
        let mut words = Vec::new();
        let mut boxes = Vec::new();
        for draw in buffer.draw() {
            match draw {
                Draw::Text { text, .. } => words.push(text.to_owned()),
                Draw::Rect { position, size, .. }
                | Draw::Turned { position, size, .. }
                | Draw::Box { position, size, .. }
                | Draw::Shadow { position, size, .. } => {
                    boxes.push((position, size));
                }
                Draw::Group { .. } | Draw::EndGroup => {}
            }
        }
        (words, boxes)
    }

    #[test]
    fn a_level_that_could_not_be_loaded_again_has_a_panel_of_its_own() {
        let mut frame =
            Frame::new(0.0).with_level_problem(Some("level.glb is not a glTF file.".into()));
        let mut overlay = ErrorOverlay::default();
        overlay.show(None, &mut frame);
        let (words, boxes) = drawn(&frame.take_ui());
        assert_eq!(words[0], "The saved level could not be loaded");
        assert!(words.iter().any(|line| line.contains("not a glTF file")));
        assert!(words.last().unwrap().contains("Export it again"));
        assert_eq!(boxes.len(), 2, "a panel and its stripe");

        // A level that loads takes the panel away.
        let mut frame = Frame::new(0.0);
        overlay.show(None, &mut frame);
        assert!(drawn(&frame.take_ui()).0.is_empty());
    }

    #[test]
    fn text_is_wrapped_between_words() {
        // Ten characters fit in 110.
        assert_eq!(
            wrapped("one two three four", 110.0),
            ["one two", "three four"]
        );
        assert_eq!(wrapped("  spaced   out  ", 110.0), ["spaced out"]);
        assert!(wrapped("", 110.0).is_empty());
    }

    #[test]
    fn a_word_longer_than_a_line_is_split() {
        assert_eq!(
            wrapped("a abcdefghijklmnopqrstuvwxyz b", 110.0),
            ["a", "abcdefghij", "klmnopqrst", "uvwxyz b"]
        );
        // Even where nothing fits, each line takes a character and it ends.
        assert_eq!(wrapped("abc", 1.0), ["a", "b", "c"]);
        assert_eq!(wrapped("abc", f32::NAN), ["a", "b", "c"]);
    }

    #[test]
    fn text_that_cannot_be_drawn_is_made_plain_and_long_text_is_cut() {
        assert_eq!(clean("one\ttwo\nthree\u{7}"), "one two three ");
        let long = "é".repeat(MOST_CHARACTERS + 50);
        let cleaned = clean(&long);
        assert_eq!(cleaned.chars().count(), MOST_CHARACTERS + 3);
        assert!(cleaned.ends_with("é..."));
        // Text of exactly the limit is left whole.
        let whole = "x".repeat(MOST_CHARACTERS);
        assert_eq!(clean(&whole), whole);
    }

    #[test]
    fn the_lines_say_where_what_and_what_to_do_in_that_order() {
        let said: Vec<String> = lines(&error(), 2000.0, &EstimatedText)
            .into_iter()
            .map(|line| line.text)
            .collect();
        assert_eq!(
            said,
            [
                "Script error in main.luau, line 4",
                "4 | game.onTik(function(step)",
                "game.onTik does not exist. Did you mean game.onTick?",
                "Hint: Check the spelling.",
                "The game keeps running.",
            ]
        );

        let bare = ScriptError {
            line: None,
            source_line: None,
            hint: None,
            note: None,
            ..error()
        };
        let said: Vec<String> = lines(&bare, 2000.0, &EstimatedText)
            .into_iter()
            .map(|line| line.text)
            .collect();
        assert_eq!(
            said,
            [
                "Script error in main.luau",
                "game.onTik does not exist. Did you mean game.onTick?",
            ]
        );
    }

    #[test]
    fn an_error_too_long_to_show_ends_by_pointing_at_the_terminal() {
        let long = ScriptError {
            message: "word ".repeat(5000),
            ..error()
        };
        let lines = lines(&long, 300.0, &EstimatedText);
        assert_eq!(lines.len(), MOST_LINES);
        assert_eq!(lines[MOST_LINES - 1].text, CUT_SHORT);
        assert_eq!(lines[0].text, "Script error in");
    }

    #[test]
    fn the_panel_is_drawn_inside_the_window_whatever_its_shape() {
        let huge = ScriptError {
            message: "x".repeat(5000),
            hint: Some("y ".repeat(5000)),
            ..error()
        };
        for screen in [[1920, 1080], [1280, 720], [640, 1080], [3440, 1440], [1, 1]] {
            for error in [error(), huge.clone()] {
                let mut overlay = ErrorOverlay::default();
                let mut frame = Frame::new(0.0).for_window(Some(screen), DrawBuffer::default());
                overlay.show(Some(&error), &mut frame);
                let (words, boxes) = drawn(&frame.take_ui());
                assert!(words.len() >= 2, "{screen:?}");
                // The panel, then the stripe along its top.
                let (corner, size) = boxes[0];
                #[allow(clippy::cast_precision_loss)]
                let [width, height] = screen.map(|pixels| pixels as f32);
                assert!(corner[0] >= 0.0 && corner[1] >= 0.0, "{screen:?}");
                assert!(corner[0] + size[0] <= width + 1.0, "{screen:?}");
                assert!(corner[1] + size[1] <= height + 1.0, "{screen:?}");
                assert_eq!(boxes[1].0, corner, "{screen:?}");
            }
        }
    }

    #[test]
    fn the_error_is_drawn_until_there_is_none() {
        let mut overlay = ErrorOverlay::default();
        let mut frame = Frame::new(0.0);
        overlay.show(Some(&error()), &mut frame);
        let (words, boxes) = drawn(&frame.take_ui());
        assert_eq!(words[0], "Script error in main.luau, line 4");
        assert_eq!(words.len(), 5);
        assert_eq!(boxes.len(), 2);

        // The same error on the next frame is drawn again, and a new one
        // takes its place.
        let other = ScriptError {
            line: Some(9),
            ..error()
        };
        for (error, title) in [
            (error(), "Script error in main.luau, line 4"),
            (other, "Script error in main.luau, line 9"),
        ] {
            let mut frame = Frame::new(0.0);
            overlay.show(Some(&error), &mut frame);
            assert_eq!(drawn(&frame.take_ui()).0[0], title);
        }

        let mut frame = Frame::new(0.0);
        overlay.show(None, &mut frame);
        assert!(frame.take_ui().draw().is_empty());
    }

    fn shader_error(file: &str) -> ShaderError {
        ShaderError {
            file: file.to_owned(),
            line: Some(3),
            column: Some(5),
            message: "`ScreenIn` has no field `colour`.".to_owned(),
            source_line: Some("    return in.colour;".to_owned()),
            hint: Some("Did you mean `color`?".to_owned()),
            note: Some("The last version that worked is still drawn.".to_owned()),
            lore_fault: false,
        }
    }

    /// A frame of the default size whose shader list holds these problems.
    fn frame_with(problems: &[ShaderError]) -> Frame {
        use lore_input::ActionMap;
        use lore_script::ScriptHost;

        let wgsl = "fn screen(in: ScreenIn) -> vec3f { return in.color; }";
        let mut files = vec![("main.luau".to_owned(), String::new())];
        let mut script = String::new();
        for (at, _) in problems.iter().enumerate() {
            script.push_str(&format!("game.shaders.screen(\"s{at}\", {{}})\n"));
            files.push((format!("shaders/s{at}.wgsl"), wgsl.to_owned()));
        }
        files[0].1 = script;
        let host = ScriptHost::from_sources(files, ActionMap::standard());
        let mut list = crate::game_shaders::ShaderList::default();
        list.follow(crate::game_shaders::Source::new(), &host);
        list.hear(
            problems
                .iter()
                .enumerate()
                .map(|(at, error)| lore_render::ShaderReport {
                    name: format!("s{at}"),
                    file: format!("shaders/s{at}.wgsl"),
                    material: None,
                    revision: 1 + at as u64,
                    outcome: Err(error.clone()),
                })
                .collect(),
        );
        Frame::new(0.0).with_shader_list(list)
    }

    #[test]
    fn a_shader_error_is_shown_under_the_script_error() {
        let mut overlay = ErrorOverlay::default();
        let mut frame = frame_with(&[shader_error("shaders/heat.wgsl")]);
        overlay.show(Some(&error()), &mut frame);
        let (words, boxes) = drawn(&frame.take_ui());
        assert_eq!(words[0], "Script error in main.luau, line 4");
        assert_eq!(words.len(), 5 + 5);
        assert_eq!(
            words[5..],
            [
                "Shader error in shaders/heat.wgsl, line 3",
                "3 | return in.colour;",
                "`ScreenIn` has no field `colour`.",
                "Hint: Did you mean `color`?",
                "The last version that worked is still drawn.",
            ]
        );
        // Script panel and stripe, then the shader's, entirely below.
        assert_eq!(boxes.len(), 4);
        assert!(boxes[2].0[1] >= boxes[0].0[1] + boxes[0].1[1]);
        assert_eq!(boxes[2].0[0], boxes[0].0[0]);
        assert_eq!(boxes[3].0, boxes[2].0);
    }

    #[test]
    fn a_shader_error_alone_is_at_the_top_and_more_are_counted() {
        let mut overlay = ErrorOverlay::default();
        let mut lore = shader_error("shaders/b.wgsl");
        lore.lore_fault = true;
        lore.line = None;
        let mut frame = frame_with(&[shader_error("shaders/a.wgsl")]);
        overlay.show(None, &mut frame);
        let (words, boxes) = drawn(&frame.take_ui());
        assert_eq!(words[0], "Shader error in shaders/a.wgsl, line 3");
        assert_eq!(words.len(), 5);
        // At the top of the safe area, as the script panel is.
        assert!(boxes[0].0[1] < 100.0, "{:?}", boxes[0]);

        let mut frame = frame_with(&[shader_error("shaders/a.wgsl"), lore.clone(), lore.clone()]);
        overlay.show(None, &mut frame);
        let words = drawn(&frame.take_ui()).0;
        assert_eq!(
            words.last().map(String::as_str),
            Some("2 more shader errors are in the terminal.")
        );
        let mut frame = frame_with(&[shader_error("shaders/a.wgsl"), lore.clone()]);
        overlay.show(None, &mut frame);
        let words = drawn(&frame.take_ui()).0;
        assert_eq!(
            words.last().map(String::as_str),
            Some("1 more shader error is in the terminal.")
        );

        let said: Vec<String> = shader_lines(&[lore], 2000.0, &EstimatedText)
            .into_iter()
            .map(|line| line.text)
            .collect();
        assert_eq!(said[0], "Shader error in LORE's own shaders/b.wgsl");
    }

    #[test]
    fn nothing_is_drawn_when_nothing_is_wrong() {
        let mut overlay = ErrorOverlay::default();
        let mut frame = frame_with(&[]);
        overlay.show(None, &mut frame);
        assert!(frame.take_ui().draw().is_empty());

        // A shader error that is put right goes.
        let mut frame = frame_with(&[shader_error("shaders/a.wgsl")]);
        overlay.show(None, &mut frame);
        assert!(!frame.take_ui().draw().is_empty());
        let mut frame = frame_with(&[]);
        overlay.show(None, &mut frame);
        assert!(frame.take_ui().draw().is_empty());
    }

    #[test]
    fn a_resized_window_gets_the_panel_laid_out_again() {
        let mut overlay = ErrorOverlay::default();
        let long = ScriptError {
            message: "several words that need more than one line when narrow ".repeat(3),
            ..error()
        };
        let mut lines_at = |screen| {
            let mut frame = Frame::new(0.0).for_window(Some(screen), DrawBuffer::default());
            overlay.show(Some(&long), &mut frame);
            drawn(&frame.take_ui()).0.len()
        };
        let wide = lines_at([1920, 1080]);
        let narrow = lines_at([700, 1080]);
        assert!(narrow > wide, "{narrow} lines narrow, {wide} wide");
        assert_eq!(lines_at([1920, 1080]), wide);
    }
}
