//! The frame meter: how many frames a second the game is drawing and how
//! long each of the last hundred or so took, drawn in the top-right corner
//! for a player who asks for it in the settings.
//!
//! The number says how fast the game runs; the graph says how evenly. A
//! flat line is smooth; a spike is a hitch that was felt.

use lore_ui::{Anchor, Color, Ui, UiInput, WidgetId};

use crate::game::Frame;

/// How many frames the graph shows.
const FRAMES: usize = 120;
/// The graph's width and height, on a screen 1080 high.
const WIDE: f32 = 2.0 * FRAMES as f32;
const HIGH: f32 = 84.0;
/// How far the panel stands in from the corner.
const MARGIN: f32 = 16.0;
const PADDING: f32 = 8.0;
const TEXT: f32 = 20.0;
/// The frame time at the graph's top edge, in seconds: a frame this long
/// or longer is a spike to the top.
const TOP: f32 = 1.0 / 25.0;
/// How often the numbers are written again, in seconds: often enough to
/// follow, seldom enough to read.
const WRITE_EVERY: f32 = 0.25;

const PANEL: Color = Color::rgba(0.05, 0.05, 0.06, 0.72);
const LINE: Color = Color::rgba(1.0, 1.0, 1.0, 0.22);
/// Under a sixtieth of a second, under a thirtieth, and slower.
const GOOD: Color = Color::rgba(0.35, 0.9, 0.45, 1.0);
const FAIR: Color = Color::rgba(0.98, 0.78, 0.25, 1.0);
const POOR: Color = Color::rgba(0.98, 0.35, 0.3, 1.0);

/// Counts frames and draws the meter.
pub(crate) struct FrameMeter {
    ui: Ui,
    /// One mark for each frame of the graph, oldest first.
    marks: [WidgetId; FRAMES],
    words: WidgetId,
    /// How long each of the last frames took, in seconds; `next` is the
    /// one written next, which is the oldest.
    times: [f32; FRAMES],
    next: usize,
    /// How long ago the numbers were last written.
    since_written: f32,
}

impl FrameMeter {
    pub(crate) fn new() -> Self {
        let mut ui = Ui::default();
        ui.set_text_measure(crate::error_overlay::measure());
        let panel = [WIDE + 2.0 * PADDING, HIGH + TEXT + 3.0 * PADDING];
        ui.hud_rect(Anchor::TopRight, [-MARGIN, MARGIN], panel, PANEL);
        // Where a frame of a sixtieth and of a thirtieth of a second falls.
        for seconds in [1.0 / 60.0, 1.0 / 30.0] {
            ui.hud_rect(
                Anchor::TopRight,
                [-MARGIN - PADDING, Self::down(seconds)],
                [WIDE, 1.0],
                LINE,
            );
        }
        let marks = std::array::from_fn(|number| {
            // The number of frames is far below where f32 stops being exact.
            #[allow(clippy::cast_precision_loss)]
            let left = (FRAMES - 1 - number) as f32 * 2.0;
            ui.hud_rect(
                Anchor::TopRight,
                [-MARGIN - PADDING - left, Self::down(0.0)],
                [2.0, 3.0],
                GOOD,
            )
        });
        let words = ui.hud_text(
            Anchor::TopRight,
            [-MARGIN - PADDING, MARGIN + PADDING],
            TEXT,
            "",
        );
        Self {
            ui,
            marks,
            words,
            times: [0.0; FRAMES],
            next: 0,
            since_written: WRITE_EVERY,
        }
    }

    /// How far down the screen a frame of `seconds` is marked.
    fn down(seconds: f32) -> f32 {
        let share = (seconds / TOP).clamp(0.0, 1.0);
        MARGIN + 2.0 * PADDING + TEXT + HIGH * (1.0 - share) - 3.0
    }

    /// Counts the frame being drawn, and draws the meter over it if the
    /// player asked for it. Counting goes on while it is hidden, so that it
    /// is right the moment it is shown.
    pub(crate) fn show(&mut self, frame: &mut Frame, shown: bool) {
        let seconds = frame.delta_seconds();
        if seconds.is_finite() && seconds > 0.0 {
            self.times[self.next] = seconds;
            self.next = (self.next + 1) % FRAMES;
            self.since_written += seconds;
        }
        if !shown {
            return;
        }
        // The marks, oldest on the left. They are this UI's own, so moving
        // and colouring them cannot fail.
        for (number, mark) in self.marks.iter().enumerate() {
            let seconds = self.times[(self.next + number) % FRAMES];
            #[allow(clippy::cast_precision_loss)]
            let left = (FRAMES - 1 - number) as f32 * 2.0;
            let _ = self
                .ui
                .set_offset(*mark, [-MARGIN - PADDING - left, Self::down(seconds)]);
            let colour = if seconds <= 1.0 / 58.0 {
                GOOD
            } else if seconds <= 1.0 / 29.0 {
                FAIR
            } else {
                POOR
            };
            let _ = self.ui.set_color(*mark, colour);
        }
        if self.since_written >= WRITE_EVERY {
            self.since_written = 0.0;
            let _ = self.ui.set_text(self.words, &self.words());
        }
        self.ui.set_ui_scale(frame.ui_scale());
        self.ui
            .update(&UiInput::default(), 0.0, frame.screen_size());
        frame.draw_ui(&self.ui);
    }

    /// The numbers: frames a second, the usual frame and the slowest.
    fn words(&self) -> String {
        let counted = self.times.iter().filter(|seconds| **seconds > 0.0);
        let (mut total, mut slowest, mut frames) = (0.0_f32, 0.0_f32, 0.0_f32);
        for seconds in counted {
            total += seconds;
            slowest = slowest.max(*seconds);
            frames += 1.0;
        }
        if frames == 0.0 || total <= 0.0 {
            return String::new();
        }
        let usual = total / frames;
        format!(
            "{:.0} fps   {:.1} ms   slowest {:.1} ms",
            1.0 / usual,
            usual * 1000.0,
            slowest * 1000.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meter_says_how_fast_and_how_evenly_the_last_frames_were_drawn() {
        let mut meter = FrameMeter::new();
        assert_eq!(meter.words(), "", "nothing drawn yet");
        // A hundred frames at 100 a second, and one hitch of 50 ms.
        for _ in 0..99 {
            meter.times[meter.next] = 0.01;
            meter.next = (meter.next + 1) % FRAMES;
        }
        meter.times[meter.next] = 0.05;
        meter.next = (meter.next + 1) % FRAMES;
        assert_eq!(meter.words(), "96 fps   10.4 ms   slowest 50.0 ms");
        // A slow frame is marked higher up the graph than a fast one, and
        // nothing is marked off its top.
        assert!(FrameMeter::down(0.03) < FrameMeter::down(0.01));
        assert_eq!(FrameMeter::down(1.0), FrameMeter::down(TOP));
    }

    #[test]
    fn it_is_drawn_only_when_asked_for_and_counts_all_the_while() {
        let mut meter = FrameMeter::new();
        let mut frame = Frame::new(0.0).with_pointer(0.02, None, false);
        meter.show(&mut frame, false);
        assert!(frame.take_ui().draw().is_empty(), "hidden");
        let mut frame = Frame::new(0.0).with_pointer(0.02, None, false);
        meter.show(&mut frame, true);
        let drawn = frame.take_ui();
        assert!(!drawn.draw().is_empty(), "shown");
        // Both frames were counted.
        assert_eq!(meter.words(), "50 fps   20.0 ms   slowest 20.0 ms");
    }
}
