//! The marks over the NPCs noticing the player: one switch,
//! `game.showNoticeMarks(true)`, and the player sees they are being
//! noticed before they are seen.
//!
//! Each mark is an upright pill over the NPC's head that fills from the
//! bottom as the NPC notices the player more, with a "?" over it once the
//! NPC is searching and a "!" once alarmed: the fill and the sign say it,
//! the colour (white, amber, red) only helps. One whose NPC is off the
//! screen sits at the edge of the screen in its direction. At most
//! [`MOST_MARKS`] are shown, the fullest first.
//!
//! The marks are made once and only moved, filled and shown or hidden
//! after that, so drawing them sets no memory aside.

use lore_npc::Aware;
use lore_script::{CharacterId, ScriptHost, ViewTold};
use lore_ui::{Anchor, Color, Ui, UiInput, WidgetId};

use crate::game::Frame;

/// The most marks shown at once.
pub(crate) const MOST_MARKS: usize = 4;

/// How high over the feet of an NPC its mark is, in metres: over the head.
const OVER_HEAD: f32 = 2.2;
/// How big the pill is, in pixels on a screen 1080 high.
const PILL: [f32; 2] = [14.0, 40.0];
/// The border round the fill.
const BORDER: f32 = 3.0;
/// How high the sign over the pill is.
const SIGN: f32 = 32.0;
/// How far from the edge of the screen a mark of an NPC off it sits.
const EDGE: f32 = 60.0;
/// Below how full a gauge no mark is shown.
const SHOWN_FROM: f32 = 0.02;

const BACK: Color = Color::rgba(0.05, 0.06, 0.08, 0.6);
const CURIOUS: Color = Color::rgba(0.95, 0.95, 0.95, 0.95);
const SEARCHING: Color = Color::rgba(1.0, 0.75, 0.2, 0.95);
const ALARMED: Color = Color::rgba(1.0, 0.25, 0.2, 0.95);

/// The pieces of one mark.
#[derive(Debug, Clone, Copy)]
struct Mark {
    back: WidgetId,
    fill: WidgetId,
    asking: WidgetId,
    alarmed: WidgetId,
}

/// One NPC noticing the player: where its mark goes and how full it is.
#[derive(Debug, Clone, Copy)]
struct Noticer {
    feet: [f32; 3],
    gauge: f32,
    level: Aware,
}

/// Draws the marks over the game.
pub(crate) struct NoticeMarks {
    ui: Ui,
    marks: [Mark; MOST_MARKS],
}

impl std::fmt::Debug for NoticeMarks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoticeMarks").finish_non_exhaustive()
    }
}

impl Default for NoticeMarks {
    fn default() -> Self {
        let mut ui = Ui::default();
        ui.set_text_measure(crate::error_overlay::measure());
        let marks = std::array::from_fn(|_| {
            let back = ui.hud_rect(Anchor::Center, [0.0; 2], PILL, BACK);
            let fill = ui.hud_rect(Anchor::Center, [0.0; 2], PILL, CURIOUS);
            let asking = ui.hud_text(Anchor::Center, [0.0; 2], SIGN, "?");
            let alarmed = ui.hud_text(Anchor::Center, [0.0; 2], SIGN, "!");
            // The widgets are this UI's own, so none of this can fail.
            let _ = ui.set_corner(back, PILL[0] / 2.0);
            let _ = ui.set_corner(fill, (PILL[0] - 2.0 * BORDER) / 2.0);
            for widget in [back, fill, asking, alarmed] {
                let _ = ui.set_shown(widget, false);
            }
            Mark {
                back,
                fill,
                asking,
                alarmed,
            }
        });
        Self { ui, marks }
    }
}

impl NoticeMarks {
    /// Draws a mark over each NPC noticing the player, the fullest
    /// [`MOST_MARKS`], if the script switched them on.
    pub(crate) fn show(&mut self, host: &ScriptHost, frame: &mut Frame) {
        if !host.shows_notice_marks() {
            return;
        }
        let Some(view) = host.view() else { return };
        let mut fullest: [Option<Noticer>; MOST_MARKS] = [None; MOST_MARKS];
        host.each_npc_awareness(|npc, aware| {
            let Some(notice) = aware.of(CharacterId::PLAYER) else {
                return;
            };
            if notice.gauge < SHOWN_FROM {
                return;
            }
            let Some(feet) = host.character_position(npc) else {
                return;
            };
            let one = Noticer {
                feet,
                gauge: notice.gauge,
                level: notice.level,
            };
            keep_fullest(&mut fullest, one);
        });
        let [across, down] = frame.screen_size();
        #[allow(
            clippy::cast_precision_loss,
            reason = "a window is far smaller than 2^24"
        )]
        let aspect = if down == 0 {
            16.0 / 9.0
        } else {
            across as f32 / down as f32
        };
        let half = [540.0 * aspect, 540.0];
        let ui = &mut self.ui;
        for (mark, noticer) in self.marks.iter().zip(fullest) {
            let Some(noticer) = noticer else {
                for widget in [mark.back, mark.fill, mark.asking, mark.alarmed] {
                    let _ = ui.set_shown(widget, false);
                }
                continue;
            };
            let [x, y, z] = noticer.feet;
            let [right, below] = on_screen_or_edge(&view, [x, y + OVER_HEAD, z], half);
            let inner = [PILL[0] - 2.0 * BORDER, PILL[1] - 2.0 * BORDER];
            let tall = inner[1] * noticer.gauge.clamp(0.0, 1.0);
            let colour = match noticer.level {
                Aware::Alarmed => ALARMED,
                Aware::Searching => SEARCHING,
                Aware::Curious | Aware::Unaware => CURIOUS,
            };
            // The widgets are this UI's own, so none of this can fail.
            let _ = ui.set_offset(mark.back, [right, below]);
            let _ = ui.set_offset(mark.fill, [right, below + (inner[1] - tall) / 2.0]);
            let _ = ui.set_size(mark.fill, [inner[0], tall]);
            let _ = ui.set_color(mark.fill, colour);
            let sign = [right, below - PILL[1] / 2.0 - SIGN * 0.6];
            let _ = ui.set_offset(mark.asking, sign);
            let _ = ui.set_offset(mark.alarmed, sign);
            let _ = ui.set_color(mark.asking, SEARCHING);
            let _ = ui.set_color(mark.alarmed, ALARMED);
            let _ = ui.set_shown(mark.back, true);
            let _ = ui.set_shown(mark.fill, tall > 0.5);
            let _ = ui.set_shown(mark.asking, noticer.level == Aware::Searching);
            let _ = ui.set_shown(mark.alarmed, noticer.level == Aware::Alarmed);
        }
        ui.update(&UiInput::default(), 0.0, frame.screen_size());
        frame.draw_ui(ui);
    }
}

/// Puts `one` among `fullest` if its gauge is fuller than the emptiest
/// there, keeping them fullest first.
fn keep_fullest(fullest: &mut [Option<Noticer>; MOST_MARKS], one: Noticer) {
    let at = fullest
        .iter()
        .position(|kept| kept.is_none_or(|kept| one.gauge > kept.gauge));
    if let Some(at) = at {
        fullest[at..].rotate_right(1);
        fullest[at] = Some(one);
    }
}

/// Where the place `at` of the world is in the picture drawn from `view`,
/// as how far right and down of the middle of a screen `half` across and
/// down from its middle to its edges, in pixels on a screen 1080 high. A
/// place off the screen, or behind the camera, is put at the screen's edge
/// ([`EDGE`] in) in its direction.
fn on_screen_or_edge(view: &ViewTold, at: [f32; 3], half: [f32; 2]) -> [f32; 2] {
    let (sy, cy) = (libm::sinf(view.yaw), libm::cosf(view.yaw));
    let (sp, cp) = (libm::sinf(view.pitch), libm::cosf(view.pitch));
    let forward = [-sy * cp, sp, -cy * cp];
    let right = [cy, 0.0, -sy];
    let up = [
        right[1] * forward[2] - right[2] * forward[1],
        right[2] * forward[0] - right[0] * forward[2],
        right[0] * forward[1] - right[1] * forward[0],
    ];
    let to: [f32; 3] = std::array::from_fn(|i| at[i] - view.position[i]);
    let along = |axis: [f32; 3]| -> f32 { (0..3).map(|i| to[i] * axis[i]).sum() };
    let (ahead, across, high) = (along(forward), along(right), along(up));
    let room = [(half[0] - EDGE).max(1.0), (half[1] - EDGE).max(1.0)];
    let tan = libm::tanf(view.fov_degrees.to_radians() / 2.0);
    let point = if ahead > 0.01 && tan > 0.0 {
        let pixels = half[1] / (ahead * tan);
        let point = [across * pixels, -high * pixels];
        if point[0].abs() <= room[0] && point[1].abs() <= room[1] {
            return point;
        }
        point
    } else if across.abs() + high.abs() > 1e-4 {
        [across, -high]
    } else {
        // Straight behind: at the bottom.
        [0.0, 1.0]
    };
    if !(point[0].is_finite() && point[1].is_finite()) {
        return [0.0, room[1]];
    }
    // Out along the line from the middle until it meets the edge.
    let reach = (point[0].abs() / room[0]).max(point[1].abs() / room[1]);
    if reach <= 0.0 {
        return [0.0, room[1]];
    }
    [point[0] / reach, point[1] / reach]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ahead() -> ViewTold {
        ViewTold {
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_degrees: 90.0,
        }
    }

    const HALF: [f32; 2] = [960.0, 540.0];

    #[test]
    fn a_place_in_view_is_where_it_is_drawn() {
        let [right, down] = on_screen_or_edge(&ahead(), [0.0, 0.0, -10.0], HALF);
        assert!(right.abs() < 1e-3 && down.abs() < 1e-3);
        let [right, down] = on_screen_or_edge(&ahead(), [2.0, 1.0, -10.0], HALF);
        assert!((right - 108.0).abs() < 0.1 && (down + 54.0).abs() < 0.1);
    }

    #[test]
    fn a_place_off_the_screen_or_behind_is_at_the_edge_its_way() {
        let [right, down] = on_screen_or_edge(&ahead(), [50.0, 0.0, -1.0], HALF);
        assert!((right - (HALF[0] - EDGE)).abs() < 0.1, "{right}");
        assert!(down.abs() < 0.1);
        // Behind and to the left: at the left edge.
        let [right, _] = on_screen_or_edge(&ahead(), [-3.0, 0.0, 5.0], HALF);
        assert!((right + (HALF[0] - EDGE)).abs() < 0.1, "{right}");
        // Straight behind: at the bottom.
        let [right, down] = on_screen_or_edge(&ahead(), [0.0, 0.0, 5.0], HALF);
        assert!(right.abs() < 0.1 && (down - (HALF[1] - EDGE)).abs() < 0.1);
    }

    #[test]
    fn the_fullest_are_kept_fullest_first() {
        let mut fullest = [None; MOST_MARKS];
        for gauge in [0.3, 0.9, 0.1, 0.5, 0.7, 0.2] {
            let one = Noticer {
                feet: [0.0; 3],
                gauge,
                level: Aware::Curious,
            };
            keep_fullest(&mut fullest, one);
        }
        let kept: Vec<f32> = fullest.iter().flatten().map(|n| n.gauge).collect();
        assert_eq!(kept, [0.9, 0.7, 0.5, 0.3]);
    }
}
