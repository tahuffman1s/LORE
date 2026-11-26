//! What the player is told on the screen when their bindings file is read
//! again: a short notice of what changed, and, while the file has something
//! wrong in it, a bar that stays until it is put right.
//!
//! The engine draws it for every game, with or without a script, in a
//! packaged game and in the editor's play. A script hears of the same news
//! with `game.input.onControlsChanged` and can draw it its own way after
//! `game.input.showControlsNotice(false)`.

use std::fmt;
use std::time::{Duration, Instant};

use lore_input::{ActionMap, Device, GamepadStyle};
use lore_script::{BindingChange, ControlsChange};
use lore_ui::{Anchor, Color, SMALLEST_TEXT, TextMeasure, Ui, UiInput};

use crate::controls::{Problem, short_name};
use crate::error_overlay::{clean, measure, panel_width, wrap};
use crate::game::Frame;

/// How long the notice of a good reading stays up.
const NOTICE_FOR: Duration = Duration::from_secs(4);
/// How many changes the notice names before "and N more".
const MOST_NAMED: usize = 3;
/// Space between a panel's edge and its text, in pixels on a screen 1080 high.
const PADDING: f32 = 14.0;
/// Space between the top of the screen and the first panel, and between panels.
const GAP: f32 = 10.0;
/// The widest the problem bar's text may wrap to.
const MOST_LINES: usize = 3;
/// A line's height is this many times its text's.
const LINE_SPACING: f32 = 1.3;

const NOTICE_BACKGROUND: Color = Color::rgba(0.09, 0.09, 0.1, 0.92);
const NOTICE_STRIPE: Color = Color::rgb(0.45, 0.8, 0.55);
const PROBLEM_BACKGROUND: Color = Color::rgba(0.42, 0.07, 0.07, 0.96);
const TEXT: Color = Color::rgb(0.96, 0.96, 0.97);

/// The actions whose bindings differ between `before` and `after`, as the
/// player reads them: the buttons of the device they use now. When the
/// change is only on the other device, that is the one named.
pub(crate) fn changes(
    before: &ActionMap,
    after: &ActionMap,
    device: Device,
    style: GamepadStyle,
) -> Vec<BindingChange> {
    let other = if device == Device::Gamepad {
        Device::KeyboardAndMouse
    } else {
        Device::Gamepad
    };
    let mut found = Vec::new();
    for action in after.actions() {
        let name = after.name(action);
        let Ok(was) = before.action(name) else {
            continue;
        };
        if before.bindings(was) == after.bindings(action) {
            continue;
        }
        let labels = [device, other].map(|device| {
            (
                before.label(was, device, style),
                after.label(action, device, style),
            )
        });
        let Some((from, to)) = labels.into_iter().find(|(from, to)| from != to) else {
            continue;
        };
        found.push(BindingChange {
            action: name.to_owned(),
            before: from,
            after: to,
        });
    }
    found
}

/// What the notice says of a good reading: up to three changes, then how
/// many more, or that nothing changed.
pub(crate) fn good_message(changes: &[BindingChange]) -> String {
    if changes.is_empty() {
        return "Controls saved, nothing changed".to_owned();
    }
    let named: Vec<String> = changes
        .iter()
        .take(MOST_NAMED)
        .map(|one| {
            format!(
                "{} {} to {}",
                one.action.replace('_', " "),
                one.before,
                one.after
            )
        })
        .collect();
    let mut message = format!("Controls updated: {}", named.join(", "));
    let more = changes.len().saturating_sub(MOST_NAMED);
    if more > 0 {
        message.push_str(&format!(" and {more} more"));
    }
    message
}

/// What the bar says of the problems in the files: the first one, where it
/// is and what to write, what is done about it, and how many more there are.
/// `at_start` says the game just began, so there are no "last good controls".
pub(crate) fn problem_message(problems: &[Problem], at_start: bool) -> Option<String> {
    let first = problems.first()?;
    let place = match first.line {
        Some(line) => format!("{} line {line}", first.file_name()),
        None => first.file_name(),
    };
    let mut message = format!("{place}: {}", first.text.trim());
    if first.whole_file {
        message.push_str(if at_start {
            " (The game's own controls are used.)"
        } else {
            " (Your last good controls are kept.)"
        });
    }
    let more = problems.len() - 1;
    if more > 0 {
        if more == 1 {
            message.push_str(" (1 more problem is in the log.)");
        } else {
            message.push_str(&format!(" ({more} more problems are in the log.)"));
        }
    }
    Some(message)
}

/// What a script is told when the files were read: `changes` for a reading
/// that was used, and the problems, if any.
pub(crate) fn change_told(
    changes: Vec<BindingChange>,
    problems: &[Problem],
    at_start: bool,
    file: &std::path::Path,
) -> ControlsChange {
    let problem = problem_message(problems, at_start);
    let first = problems.first();
    ControlsChange {
        message: problem.clone().unwrap_or_else(|| good_message(&changes)),
        changes,
        file: first.map_or_else(|| short_name(file), Problem::file_name),
        line: first.and_then(|problem| problem.line),
        problem,
    }
}

#[cfg(test)]
fn one(action: &str, before: &str, after: &str) -> BindingChange {
    BindingChange {
        action: action.to_owned(),
        before: before.to_owned(),
        after: after.to_owned(),
    }
}

/// The notice and the bar, and what is needed to draw them.
pub(crate) struct ControlsNotice {
    /// What a good reading said, and when it goes.
    notice: Option<(String, Instant)>,
    /// What is wrong with the files, until a reading finds them good.
    problem: Option<String>,
    /// What is laid out in `ui`, and for which screen.
    laid: Option<(Option<String>, Option<String>, [u32; 2])>,
    ui: Ui,
    measure: Box<dyn TextMeasure>,
}

impl fmt::Debug for ControlsNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControlsNotice")
            .field("notice", &self.notice)
            .field("problem", &self.problem)
            .finish_non_exhaustive()
    }
}

impl Default for ControlsNotice {
    fn default() -> Self {
        Self {
            notice: None,
            problem: None,
            laid: None,
            ui: Ui::default(),
            measure: measure(),
        }
    }
}

impl ControlsNotice {
    /// Takes in what a reading came to. A reading that was used puts up
    /// its notice (`good`); the files' problems, if any, stay up until one
    /// that has none.
    pub(crate) fn read(&mut self, good: Option<String>, problem: Option<String>, now: Instant) {
        if let Some(good) = good {
            self.notice = Some((good, now + NOTICE_FOR));
        }
        self.problem = problem;
    }

    /// Puts up the notice of a change the game made to the controls itself
    /// (a controls page): what is wrong with the files, if anything, stays
    /// as it was.
    pub(crate) fn changed(&mut self, good: String, now: Instant) {
        self.notice = Some((good, now + NOTICE_FOR));
    }

    /// The notice that is up, if any.
    #[cfg(test)]
    fn notice(&self) -> Option<&str> {
        self.notice.as_ref().map(|(text, _)| text.as_str())
    }

    /// Draws what is up over this frame, unless the game turned it off.
    /// Lays it out again only when the words or the screen changed.
    pub(crate) fn show(&mut self, frame: &mut Frame, now: Instant) {
        if self.notice.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.notice = None;
        }
        if (self.notice.is_none() && self.problem.is_none()) || frame.controls_notice_hidden() {
            return;
        }
        let screen = frame.screen_size();
        let notice = self.notice.as_ref().map(|(text, _)| text.as_str());
        let current = self.laid.as_ref().is_some_and(|(shown, problem, was)| {
            *was == screen && shown.as_deref() == notice && *problem == self.problem
        });
        if !current {
            self.ui = layout(
                notice,
                self.problem.as_deref(),
                screen,
                self.measure.as_ref(),
            );
            self.laid = Some((notice.map(str::to_owned), self.problem.clone(), screen));
        }
        self.ui.update(&UiInput::default(), 0.0, screen);
        frame.draw_ui(&self.ui);
    }
}

/// The bar, then the notice under it, along the top middle of a `screen`.
fn layout(
    notice: Option<&str>,
    problem: Option<&str>,
    screen: [u32; 2],
    measure: &dyn TextMeasure,
) -> Ui {
    let mut ui = Ui::default();
    let (wide, text_width) = panel_width(screen);
    #[allow(
        clippy::cast_precision_loss,
        reason = "screens are far smaller than 2^24 pixels"
    )]
    let across = screen[0] as f32 * 1080.0 / screen[1].max(1) as f32;
    let mut top = GAP;
    let mut place = |text: &str, background: Color, stripe: Option<Color>, most: usize| {
        let mut lines = Vec::new();
        wrap(
            &clean(text),
            (text_width - 2.0 * PADDING).min(wide),
            SMALLEST_TEXT,
            measure,
            &mut lines,
        );
        if lines.len() > most {
            lines.truncate(most);
            if let Some(last) = lines.last_mut() {
                last.push_str("...");
            }
        }
        let widest = lines
            .iter()
            .map(|line| measure.width(line, SMALLEST_TEXT))
            .fold(0.0, f32::max);
        let width = (widest + 2.0 * PADDING).min(wide);
        let left = ((across - width) / 2.0).max(0.0);
        let height = 2.0 * PADDING * 0.6 + lines.len() as f32 * SMALLEST_TEXT * LINE_SPACING;
        ui.hud_rect(Anchor::TopLeft, [left, top], [width, height], background);
        if let Some(stripe) = stripe {
            ui.hud_rect(Anchor::TopLeft, [left, top], [width, 4.0], stripe);
        }
        let mut down = top + PADDING * 0.6;
        for line in &lines {
            let gap = SMALLEST_TEXT * (LINE_SPACING - 1.0) / 2.0;
            let id = ui.hud_text(
                Anchor::TopLeft,
                [left + PADDING, down + gap],
                SMALLEST_TEXT,
                line,
            );
            // The widget is this UI's own HUD text, so colouring it cannot fail.
            let _ = ui.set_color(id, TEXT);
            down += SMALLEST_TEXT * LINE_SPACING;
        }
        top += height + GAP;
    };
    if let Some(problem) = problem {
        place(problem, PROBLEM_BACKGROUND, None, MOST_LINES);
    }
    if let Some(notice) = notice {
        place(notice, NOTICE_BACKGROUND, Some(NOTICE_STRIPE), 1);
    }
    ui
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use lore_input::Binding;

    use super::*;

    fn bound(map: &mut ActionMap, action: &str, key: &str) {
        let id = map.action(action).expect("a standard action");
        let binding: Binding = key.parse().expect("a key name");
        map.set_bindings(id, [binding]).expect("a fitting binding");
    }

    #[test]
    fn changed_actions_are_listed_with_the_buttons_they_had_and_have() {
        let before = ActionMap::standard();
        let mut after = before.clone();
        bound(&mut after, "jump", "F");
        let told = changes(
            &before,
            &after,
            Device::KeyboardAndMouse,
            GamepadStyle::Xbox,
        );
        assert_eq!(told, vec![one("jump", "Space", "F")]);
        assert_eq!(good_message(&told), "Controls updated: jump Space to F");
        let same = changes(
            &before,
            &before,
            Device::KeyboardAndMouse,
            GamepadStyle::Xbox,
        );
        assert!(same.is_empty());
        assert_eq!(good_message(&same), "Controls saved, nothing changed");
    }

    #[test]
    fn a_change_to_only_the_gamepad_is_named_by_its_gamepad_buttons() {
        let before = ActionMap::standard();
        let mut after = before.clone();
        bound(&mut after, "jump", "GamepadWest");
        let told = changes(
            &before,
            &after,
            Device::KeyboardAndMouse,
            GamepadStyle::Xbox,
        );
        assert_eq!(told.len(), 1, "{told:?}");
        assert_eq!(told[0].action, "jump");
        assert_ne!(told[0].before, told[0].after);
    }

    #[test]
    fn more_than_three_changes_end_with_how_many_more() {
        let list: Vec<BindingChange> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|name| one(name, "X", "Y"))
            .collect();
        assert_eq!(
            good_message(&list),
            "Controls updated: a X to Y, b X to Y, c X to Y and 2 more"
        );
        assert!(good_message(&list[..3]).ends_with("c X to Y"));
        assert!(good_message(&[one("use_item", "G", "H")]).contains("use item G"));
    }

    fn problem(file: &str, line: Option<usize>, whole: bool) -> Problem {
        Problem {
            file: PathBuf::from(file),
            line,
            text: "\"Spcae\" is not a known key or button. Did you mean \"Space\"?".to_owned(),
            whole_file: whole,
        }
    }

    #[test]
    fn a_problem_names_the_file_the_line_and_what_is_kept() {
        let said = problem_message(
            &[problem("/home/p/lore/bindings.toml", Some(4), false)],
            false,
        )
        .expect("a problem");
        assert_eq!(
            said,
            "bindings.toml line 4: \"Spcae\" is not a known key or button. Did you mean \
             \"Space\"?"
        );
        let games = problem("/home/p/lore/games/hello/bindings.toml", None, true);
        let said = problem_message(std::slice::from_ref(&games), false).expect("a problem");
        assert!(said.starts_with("games/hello/bindings.toml: "), "{said}");
        assert!(
            said.ends_with("(Your last good controls are kept.)"),
            "{said}"
        );
        let said = problem_message(&[games.clone(), games.clone()], true).expect("a problem");
        assert!(
            said.contains("(The game's own controls are used.)"),
            "{said}"
        );
        assert!(said.ends_with("(1 more problem is in the log.)"), "{said}");
        assert_eq!(problem_message(&[], false), None);
    }

    #[test]
    fn what_a_script_is_told_is_what_the_notice_says() {
        let file = PathBuf::from("/home/p/lore/bindings.toml");
        let good = change_told(vec![one("jump", "Space", "F")], &[], false, &file);
        assert_eq!(good.message, "Controls updated: jump Space to F");
        assert_eq!((good.problem.as_deref(), good.line), (None, None));
        assert_eq!(good.file, "bindings.toml");
        let bad = change_told(
            Vec::new(),
            &[problem("/home/p/lore/bindings.toml", Some(4), true)],
            false,
            &file,
        );
        assert_eq!(Some(bad.message.as_str()), bad.problem.as_deref());
        assert_eq!(bad.line, Some(4));
    }

    #[test]
    fn a_good_reading_clears_the_bar_and_the_notice_goes_after_four_seconds() {
        let now = Instant::now();
        let mut notice = ControlsNotice::default();
        notice.read(None, Some("bad".to_owned()), now);
        assert!(notice.problem.is_some() && notice.notice().is_none());
        notice.read(Some("good".to_owned()), None, now);
        assert!(notice.problem.is_none());
        assert_eq!(notice.notice(), Some("good"));
    }

    #[test]
    fn the_notice_is_drawn_once_and_laid_out_again_only_when_it_changes() {
        let now = Instant::now();
        let mut notice = ControlsNotice::default();
        notice.read(
            Some("Controls saved, nothing changed".to_owned()),
            None,
            now,
        );
        let mut frame = Frame::new(0.0);
        notice.show(&mut frame, now);
        assert!(notice.laid.is_some());
        assert!(!frame.take_ui().draw().is_empty());
        let mut frame = Frame::new(0.0);
        frame.hide_controls_notice(true);
        notice.show(&mut frame, now);
        assert!(frame.take_ui().draw().is_empty(), "hidden by the script");
        let mut frame = Frame::new(0.0);
        notice.show(&mut frame, now + NOTICE_FOR);
        assert!(notice.notice().is_none());
        assert!(frame.take_ui().draw().is_empty(), "gone after four seconds");
    }

    /// Draws the notice and the bar over a plain frame, with no window, and
    /// reads the picture back. With `LORE_SAVE_PICTURES` set to a folder, the
    /// pictures are saved there as PNG files.
    #[test]
    fn the_notice_and_the_bar_are_drawn_over_the_frame() {
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
        let camera = Camera::default();
        let bare = headless.screenshot(&camera).unwrap();
        let now = Instant::now();
        let good = good_message(&[one("jump", "Space", "F"), one("crouch", "C", "Left Ctrl")]);
        let bad = problem_message(
            &[problem(
                "/home/p/lore/games/hello/bindings.toml",
                Some(4),
                true,
            )],
            false,
        );
        for (name, good, bad) in [
            ("controls-notice-good", Some(good), None),
            ("controls-notice-error", None, bad),
        ] {
            let mut notice = ControlsNotice::default();
            notice.read(good, bad, now);
            let mut frame =
                Frame::new(0.0).for_window(Some(SCREEN), lore_ui::DrawBuffer::default());
            notice.show(&mut frame, now);
            headless.set_ui(frame.take_ui().draw());
            let shown = headless.screenshot(&camera).unwrap();
            assert_eq!(lore_render::validation_errors(), 0);

            let changed = |rows: std::ops::Range<u32>| {
                rows.flat_map(|y| (0..SCREEN[0]).map(move |x| ((y * SCREEN[0] + x) * 4) as usize))
                    .filter(|&at| shown[at..at + 3] != bare[at..at + 3])
                    .count()
            };
            assert!(
                changed(0..SCREEN[1] / 4) > 2000,
                "{name}: the notice is on the top of the screen"
            );
            assert_eq!(
                changed(SCREEN[1] / 2..SCREEN[1]),
                0,
                "{name}: nothing below"
            );

            if let Some(folder) = std::env::var_os("LORE_SAVE_PICTURES") {
                let path = std::path::Path::new(&folder).join(format!("{name}.png"));
                let file = std::fs::File::create(&path).unwrap();
                let mut encoder = png::Encoder::new(file, SCREEN[0], SCREEN[1]);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&shown)
                    .unwrap();
                eprintln!("Saved {}", path.display());
            }
        }
    }
}
