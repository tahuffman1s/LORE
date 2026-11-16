//! Turning placed widgets into boxes and text.

use crate::draw::Painter;
use crate::layout::{slider_track, tab_width, text_size};
use crate::node::{Kind, Node, Rect};
use crate::theme::{TextMeasure, Theme};
use crate::ui::Names;

/// How a widget is shown this frame.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    /// The player is on it, or the pointer is over it.
    pub selected: bool,
    /// It gets the gamepad and keyboard outline.
    pub outlined: bool,
}

pub(crate) fn hud(node: &Node, theme: &Theme, painter: &mut Painter<'_>) {
    let rect = node.rect;
    match &node.kind {
        Kind::HudText { size, color, .. } => {
            painter.text([rect.x, rect.y], text_size(*size), *color, &node.text);
        }
        Kind::HudBar {
            fraction, color, ..
        } => {
            painter.rect(rect, theme.track, 0.0);
            painter.rect(
                Rect {
                    w: rect.w * fraction,
                    ..rect
                },
                *color,
                0.0,
            );
        }
        Kind::HudRect { color, .. } => painter.rect(rect, *color, 0.0),
        _ => {}
    }
}

/// Draws one widget. A widget that is `cut` by the edge of a screen that
/// scrolls is only its box, with no words or handle in it: half a
/// slider says nothing, and the box alone shows that there is more.
pub(crate) fn widget(
    node: &Node,
    look: Look,
    cut: bool,
    theme: &Theme,
    measure: &dyn TextMeasure,
    painter: &mut Painter<'_>,
) {
    let rect = node.rect;
    let size = text_size(theme.text_size);
    let color = if look.selected {
        theme.text_selected
    } else {
        theme.text
    };
    let text_y = rect.y + (rect.h - size) / 2.0;
    let right = rect.right() - theme.padding;
    let background = |painter: &mut Painter<'_>| {
        if look.outlined {
            painter.rect(
                rect.grown(theme.outline),
                theme.accent,
                theme.corner + theme.outline,
            );
        }
        let fill = if look.selected {
            theme.widget_selected
        } else {
            theme.widget
        };
        painter.rect(rect, fill, theme.corner);
    };
    let label = |painter: &mut Painter<'_>| {
        painter.text([rect.x + theme.padding, text_y], size, color, &node.text);
    };

    match &node.kind {
        Kind::Label { title } => {
            let size = if *title {
                text_size(theme.title_size)
            } else {
                size
            };
            painter.text(
                [rect.x, rect.y + (rect.h - size) / 2.0],
                size,
                theme.text,
                &node.text,
            );
        }
        Kind::Button | Kind::Toggle { .. } | Kind::Slider { .. } | Kind::Choice { .. } if cut => {
            background(painter);
        }
        Kind::Button => {
            background(painter);
            let width = measure.width(&node.text, size);
            painter.text(
                [rect.x + (rect.w - width) / 2.0, text_y],
                size,
                color,
                &node.text,
            );
        }
        Kind::Toggle { on } => {
            background(painter);
            label(painter);
            let word = if *on { "On" } else { "Off" };
            painter.text(
                [right - measure.width(word, size), text_y],
                size,
                color,
                word,
            );
        }
        Kind::Slider {
            min,
            max,
            step,
            value,
        } => {
            background(painter);
            label(painter);
            let track = slider_track(rect, theme);
            let filled = track.w * ((value - min) / (max - min)).clamp(0.0, 1.0);
            painter.rect(track, theme.track, track.h / 2.0);
            painter.rect(Rect { w: filled, ..track }, theme.accent, track.h / 2.0);
            let handle = Rect {
                x: track.x + filled - 6.0,
                y: rect.y + rect.h / 2.0 - 12.0,
                w: 12.0,
                h: 24.0,
            };
            painter.rect(handle, color, 4.0);
            let decimals = if *step >= 1.0 {
                0
            } else if *step >= 0.1 {
                1
            } else {
                2
            };
            painter.text_with(
                [track.right() + 16.0, text_y],
                size,
                color,
                format_args!("{value:.decimals$}"),
            );
        }
        Kind::Choice { options, selected } => {
            background(painter);
            label(painter);
            let left = right - 280.0;
            let arrow = measure.width(">", size);
            painter.text([left, text_y], size, color, "<");
            painter.text([right - arrow, text_y], size, color, ">");
            let option = options.get(*selected).map_or("", String::as_str);
            let middle = (left + right) / 2.0;
            painter.text(
                [middle - measure.width(option, size) / 2.0, text_y],
                size,
                color,
                option,
            );
        }
        Kind::Tabs { names, open } => {
            let mut x = rect.x;
            for (index, name) in names.iter().enumerate() {
                let tab = Rect {
                    x,
                    y: rect.y,
                    w: tab_width(name, theme, measure),
                    h: theme.row_height,
                };
                let is_open = index == *open;
                let fill = if is_open {
                    theme.widget_selected
                } else {
                    theme.widget
                };
                painter.rect(tab, fill, theme.corner);
                if is_open {
                    let line = Rect {
                        y: tab.bottom() - theme.outline,
                        h: theme.outline,
                        ..tab
                    };
                    painter.rect(line, theme.accent, 0.0);
                }
                let color = if is_open {
                    theme.text_selected
                } else {
                    theme.text
                };
                let y = tab.y + (tab.h - size) / 2.0;
                painter.text([x + theme.padding, y], size, color, name);
                x += tab.w + theme.gap;
            }
        }
        _ => {}
    }
}

/// Shows how much of a long screen is on show and where: a thin bar
/// down the right of the panel with the part in view marked on it.
pub(crate) fn scroll_bar(
    room: Rect,
    x: f32,
    scroll: f32,
    content: f32,
    theme: &Theme,
    painter: &mut Painter<'_>,
) {
    let width = 6.0;
    let track = Rect {
        x: x - width / 2.0,
        y: room.y,
        w: width,
        h: room.h,
    };
    painter.rect(track, theme.track, width / 2.0);
    if content <= room.h {
        return;
    }
    // Never so short that it cannot be seen.
    let length = (room.h * room.h / content).max(24.0).min(room.h);
    let along = (scroll / (content - room.h)).clamp(0.0, 1.0);
    let thumb = Rect {
        y: room.y + (room.h - length) * along,
        h: length,
        ..track
    };
    painter.rect(thumb, theme.text, width / 2.0);
}

/// Space between the edge of a button's shape and its name.
const CAP_PADDING: f32 = 12.0;
/// Space between a button's shape and what it does.
const CAP_GAP: f32 = 10.0;
/// Space between one hint and the next.
const HINT_GAP: f32 = 36.0;
/// The most boxes and pieces of text the hints are drawn with: three
/// hints of two buttons each.
pub(crate) const MOST_HINT_DRAWS: usize = 15;

/// Height of the line of hints under a menu.
pub(crate) fn hint_height(theme: &Theme) -> f32 {
    text_size(theme.text_size) + CAP_PADDING
}

/// The hints under a menu: what each button does right now.
pub(crate) struct Hints<'a> {
    pub names: &'a Names,
    /// The widget a gamepad or keyboard is on.
    pub on: Option<&'a Kind>,
    /// Whether the screen has tabs to switch between.
    pub tabs: bool,
}

/// One hint: up to two buttons, then what they do.
type Hint<'a> = ([&'a str; 2], &'static str);

impl<'a> Hints<'a> {
    /// The hints to show, left to right. One whose button has no name is
    /// left out, since there would be nothing to press.
    fn list(&self) -> [Option<Hint<'a>>; 3] {
        let names = self.names;
        let named = |caps: [&'a str; 2], does| {
            caps.iter()
                .any(|cap| !cap.is_empty())
                .then_some((caps, does))
        };
        let on = match self.on {
            Some(Kind::Button) => named([names.confirm.as_str(), ""], "Select"),
            Some(Kind::Toggle { .. }) => named([names.confirm.as_str(), ""], "Change"),
            // A direction, not a button: it reads the same on every device.
            Some(Kind::Slider { .. } | Kind::Choice { .. }) => named(["<", ">"], "Change"),
            _ => None,
        };
        let tabs = if self.tabs {
            named(
                [names.previous_tab.as_str(), names.next_tab.as_str()],
                "Switch tab",
            )
        } else {
            None
        };
        [on, named([names.back.as_str(), ""], "Back"), tabs]
    }

    /// Draws the hints in one line, centred on `middle_top[0]` with
    /// their top at `middle_top[1]`.
    pub(crate) fn paint(
        &self,
        middle_top: [f32; 2],
        theme: &Theme,
        measure: &dyn TextMeasure,
        painter: &mut Painter<'_>,
    ) {
        let size = text_size(theme.text_size);
        let height = hint_height(theme);
        let cap_width = |name: &str| (measure.width(name, size) + 2.0 * CAP_PADDING).max(height);
        let hint_width = |(caps, does): &Hint<'_>| {
            let mut width = measure.width(does, size);
            for cap in caps.iter().filter(|cap| !cap.is_empty()) {
                width += cap_width(cap) + CAP_GAP;
            }
            width
        };
        let list = self.list();
        let mut total = -HINT_GAP;
        for hint in list.iter().flatten() {
            total += hint_width(hint) + HINT_GAP;
        }
        let [middle, top] = middle_top;
        let mut x = middle - total / 2.0;
        let text_y = top + (height - size) / 2.0;
        for (caps, does) in list.iter().flatten() {
            for cap in caps.iter().filter(|cap| !cap.is_empty()) {
                let width = cap_width(cap);
                let shape = Rect {
                    x,
                    y: top,
                    w: width,
                    h: height,
                };
                let corner = if self.names.round {
                    height / 2.0
                } else {
                    theme.corner
                };
                painter.rect(shape, theme.widget_selected, corner);
                let name_x = x + (width - measure.width(cap, size)) / 2.0;
                painter.text([name_x, text_y], size, theme.text_selected, cap);
                x += width + CAP_GAP;
            }
            painter.text([x, text_y], size, theme.text, does);
            x += measure.width(does, size) + HINT_GAP;
        }
    }
}
