//! The HUD a script made with `game.hud`, drawn over the game.
//!
//! The script keeps a list of pieces; this keeps a [`Ui`] that holds the
//! same pieces. The `Ui` is built again only when a piece is made or
//! removed or the script is saved. On every other frame only the pieces
//! the script changed are touched, so a HUD nobody is changing costs a
//! frame no memory and next to no time.

use lore_script::{FRAME_HISTORY, HudPiece, HudShape, ScriptHost};
use lore_ui::{Anchor, Ui, UiInput, WidgetId};

use lore_ui::Color;

use crate::game::Frame;

/// The frame time at a graph's top edge, in seconds: a frame this long or
/// longer is a spike to the top.
const TOP: f32 = 1.0 / 25.0;
/// The lines at 60 and 30 frames a second.
const LINE: Color = Color::rgba(1.0, 1.0, 1.0, 0.22);
/// Under a sixtieth of a second, under a thirtieth, and slower.
const GOOD: Color = Color::rgba(0.35, 0.9, 0.45, 1.0);
const FAIR: Color = Color::rgba(0.98, 0.78, 0.25, 1.0);
const POOR: Color = Color::rgba(0.98, 0.35, 0.3, 1.0);

/// The colour of a mark for a frame of `seconds`: good while it kept up
/// with 60 frames a second, fair while it kept up with 30, poor beyond.
fn colour_of(seconds: f32) -> Color {
    if seconds <= 1.0 / 58.0 {
        GOOD
    } else if seconds <= 1.0 / 29.0 {
        FAIR
    } else {
        POOR
    }
}

/// How tall one frame's mark in a graph is, in HUD units.
const MARK_HIGH: f32 = 3.0;
/// How thick the lines at 60 and 30 frames a second are.
const LINE_HIGH: f32 = 1.0;

/// Draws a script's HUD.
#[derive(Default)]
pub(crate) struct ScriptHud {
    ui: Ui,
    /// What each piece is drawn with, in the order they are drawn.
    widgets: Vec<Placed>,
    /// The id the script's HUD knows each of those pieces by, in the same
    /// order: what a `clipTo` names.
    ids: Vec<u64>,
    /// What [`ScriptHost::hud_layout`] was when `ui` was built.
    layout: Option<u64>,
    /// The font text is measured with, if the game's own is shared.
    font: Option<lore_render::Font>,
}

/// What one piece of the script's HUD is drawn with.
enum Placed {
    /// A single widget.
    One(WidgetId),
    /// A graph of frame times, made of widgets that are moved every frame.
    Graph(Box<Graph>),
}

/// A graph of the last frame times: two lines and one mark per frame.
/// The marks are small boxes that move up and down, because a box cannot
/// change its height.
struct Graph {
    anchor: Anchor,
    offset: [f32; 2],
    size: [f32; 2],
    visible: bool,
    /// The lines at 60 and 30 frames a second.
    lines: [WidgetId; 2],
    /// One mark for each frame shown, oldest on the left.
    marks: Vec<WidgetId>,
}

impl Graph {
    fn new(ui: &mut Ui, piece: &HudPiece, size: [f32; 2], frames: u16) -> Self {
        let frames = usize::from(frames).clamp(1, FRAME_HISTORY);
        let [width, _] = size;
        let lines = [(); 2].map(|()| ui.hud_rect(piece.anchor, [0.0; 2], [width, LINE_HIGH], LINE));
        #[allow(clippy::cast_precision_loss)]
        let mark = [width / frames as f32, Self::mark_high(size)];
        let marks = (0..frames)
            .map(|_| ui.hud_rect(piece.anchor, [0.0; 2], mark, colour_of(0.0)))
            .collect();
        let graph = Self {
            anchor: piece.anchor,
            offset: piece.offset,
            size,
            visible: piece.visible,
            lines,
            marks,
        };
        graph.place_lines(ui);
        graph.show(ui, graph.visible);
        graph
    }

    /// How tall a mark is: its usual height, or the graph's if that is less.
    fn mark_high(size: [f32; 2]) -> f32 {
        MARK_HIGH.min(size[1].max(0.0))
    }

    /// How far across and down the screen the graph's anchor is.
    fn fractions(&self) -> [f32; 2] {
        match self.anchor {
            Anchor::TopLeft => [0.0, 0.0],
            Anchor::Top => [0.5, 0.0],
            Anchor::TopRight => [1.0, 0.0],
            Anchor::Left => [0.0, 0.5],
            Anchor::Center => [0.5, 0.5],
            Anchor::Right => [1.0, 0.5],
            Anchor::BottomLeft => [0.0, 1.0],
            Anchor::Bottom => [0.5, 1.0],
            Anchor::BottomRight => [1.0, 1.0],
        }
    }

    /// The offset that puts a box `box_size` high with its left edge
    /// `across` into the graph, and as far up as a frame of `seconds`
    /// reaches. A box is pinned by its own size, so the graph's place is
    /// worked out from the box's.
    fn offset_of(&self, across: f32, box_size: [f32; 2], seconds: f32) -> [f32; 2] {
        let [fraction_across, fraction_down] = self.fractions();
        let share = (seconds / TOP).clamp(0.0, 1.0);
        let spare = self.size[1] - box_size[1];
        [
            self.offset[0] + across + fraction_across * (box_size[0] - self.size[0]),
            self.offset[1] - fraction_down * spare + spare.max(0.0) * (1.0 - share),
        ]
    }

    fn place_lines(&self, ui: &mut Ui) {
        for (line, seconds) in self.lines.iter().zip([1.0 / 60.0, 1.0 / 30.0]) {
            let at = self.offset_of(0.0, [self.size[0], LINE_HIGH], seconds);
            // The lines are this UI's own boxes, so moving them cannot fail.
            let _ = ui.set_offset(*line, at);
        }
    }

    fn show(&self, ui: &mut Ui, shown: bool) {
        for widget in self.lines.iter().chain(&self.marks) {
            let _ = ui.set_shown(*widget, shown);
        }
    }

    /// Takes a change the script made to the piece.
    fn change(&mut self, ui: &mut Ui, piece: &HudPiece) {
        let moved = self.offset != piece.offset;
        self.offset = piece.offset;
        if moved {
            self.place_lines(ui);
        }
        if self.visible != piece.visible {
            self.visible = piece.visible;
            self.show(ui, piece.visible);
        }
    }

    /// Moves every mark to the frame it shows, newest on the right.
    fn follow(&self, ui: &mut Ui, host: &ScriptHost) {
        if !self.visible {
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let wide = self.size[0] / self.marks.len() as f32;
        let high = Self::mark_high(self.size);
        // Frames not drawn yet leave the left of the graph empty.
        let mut known = 0;
        host.each_frame_time(self.marks.len(), |_| known += 1);
        let empty = self.marks.len() - known;
        for mark in &self.marks[..empty] {
            let _ = ui.set_shown(*mark, false);
        }
        let mut place = empty;
        host.each_frame_time(self.marks.len(), |seconds| {
            let Some(&mark) = self.marks.get(place) else {
                return;
            };
            #[allow(clippy::cast_precision_loss)]
            let at = self.offset_of(place as f32 * wide, [wide, high], seconds);
            let _ = ui.set_shown(mark, true);
            let _ = ui.set_offset(mark, at);
            let _ = ui.set_color(mark, colour_of(seconds));
            place += 1;
        });
    }
}

impl std::fmt::Debug for ScriptHud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHud")
            .field("pieces", &self.widgets.len())
            .finish_non_exhaustive()
    }
}

impl ScriptHud {
    /// A HUD that measures text with `font`, which follows the game's fonts.
    pub(crate) fn measuring(font: lore_render::Font) -> Self {
        Self {
            font: Some(font),
            ..Self::default()
        }
    }

    /// Draws the HUD `host`'s script has made over this frame.
    pub(crate) fn show(&mut self, host: &mut ScriptHost, frame: &mut Frame) {
        let layout = host.hud_layout();
        if self.layout == Some(layout) {
            let (ui, widgets, ids) = (&mut self.ui, &mut self.widgets, &self.ids);
            host.take_hud_changes(|place, piece| {
                let clip = clip_of(piece, ids, widgets);
                match widgets.get_mut(place) {
                    Some(Placed::One(widget)) => change(ui, *widget, piece, clip),
                    Some(Placed::Graph(graph)) => graph.change(ui, piece),
                    None => {}
                }
            });
            // A graph moves with every frame, whether or not the script
            // touched it.
            for placed in widgets {
                if let Placed::Graph(graph) = placed {
                    graph.follow(ui, host);
                }
            }
        } else {
            // A piece was made or removed, or the script was saved: build
            // the HUD again from the script's list.
            let mut ui = Ui::default();
            match &self.font {
                Some(font) => ui.set_text_measure(Box::new(font.clone())),
                None => ui.set_text_measure(crate::error_overlay::measure()),
            }
            self.widgets.clear();
            self.ids.clear();
            host.each_hud_piece(|_, piece| {
                self.ids.push(piece.id);
                self.widgets.push(add(&mut ui, piece));
            });
            // Pieces are cut to other pieces, so styles are set once all
            // of them are made.
            host.each_hud_piece(|place, piece| {
                if let Some(Placed::One(widget)) = self.widgets.get(place) {
                    style(
                        &mut ui,
                        *widget,
                        piece,
                        clip_of(piece, &self.ids, &self.widgets),
                    );
                }
            });
            for placed in &self.widgets {
                if let Placed::Graph(graph) = placed {
                    graph.follow(&mut ui, host);
                }
            }
            // Every piece was just read as it is now.
            host.take_hud_changes(|_, _| {});
            self.ui = ui;
            self.layout = Some(layout);
        }
        if self.widgets.is_empty() {
            return;
        }
        self.ui.set_ui_scale(frame.ui_scale());
        self.ui.update(
            &UiInput::default(),
            frame.delta_seconds(),
            frame.screen_size(),
        );
        frame.draw_ui(&self.ui);
    }
}

/// Adds `piece` to `ui`.
fn add(ui: &mut Ui, piece: &HudPiece) -> Placed {
    let widget = match &piece.shape {
        HudShape::Text { text, size } => {
            let widget = ui.hud_text(piece.anchor, piece.offset, *size, text);
            // The widget is this UI's own HUD text, so colouring it cannot
            // fail.
            let _ = ui.set_color(widget, piece.color);
            widget
        }
        HudShape::Bar { value, size } => {
            ui.hud_bar(piece.anchor, piece.offset, *size, piece.color, *value)
        }
        HudShape::Box { size } => ui.hud_rect(piece.anchor, piece.offset, *size, piece.color),
        HudShape::Line { to, thickness } => {
            let (offset, size, angle) =
                lore_ui::hud_line(piece.anchor, piece.offset, *to, *thickness);
            let widget = ui.hud_rect(piece.anchor, offset, size, piece.color);
            let _ = ui.set_angle(widget, angle);
            widget
        }
        HudShape::Image { image, size } => {
            ui.hud_image(piece.anchor, piece.offset, *size, *image, piece.color)
        }
        // The frame graph is drawn by the engine from the frame times.
        HudShape::Graph { size, frames } => {
            return Placed::Graph(Box::new(Graph::new(ui, piece, *size, *frames)));
        }
    };
    if !piece.visible {
        let _ = ui.set_shown(widget, false);
    }
    Placed::One(widget)
}

/// Makes `widget` show what `piece` now holds. Where a piece is pinned
/// never changes, so only the rest is copied.
fn change(ui: &mut Ui, widget: WidgetId, piece: &HudPiece, clip: Option<WidgetId>) {
    // `widget` was made from this piece by `add`, so it is of the kind
    // each of these takes and none of them can fail.
    let mut offset = piece.offset;
    match &piece.shape {
        HudShape::Text { text, .. } => {
            let _ = ui.set_text(widget, text);
        }
        HudShape::Bar { value, size } => {
            let _ = ui.set_bar(widget, *value);
            let _ = ui.set_size(widget, *size);
        }
        HudShape::Box { size } => {
            let _ = ui.set_size(widget, *size);
        }
        HudShape::Line { to, thickness } => {
            let (placed, size, angle) =
                lore_ui::hud_line(piece.anchor, piece.offset, *to, *thickness);
            offset = placed;
            let _ = ui.set_size(widget, size);
            let _ = ui.set_angle(widget, angle);
        }
        HudShape::Image { size, .. } => {
            let _ = ui.set_size(widget, *size);
        }
        HudShape::Graph { .. } => {}
    }
    let _ = ui.set_color(widget, piece.color);
    let _ = ui.set_offset(widget, offset);
    let _ = ui.set_shown(widget, piece.visible);
    style(ui, widget, piece, clip);
}

/// The widget of the piece `piece` is cut to, if it is cut to one.
fn clip_of(piece: &HudPiece, ids: &[u64], widgets: &[Placed]) -> Option<WidgetId> {
    let target = piece.style.as_deref()?.clip_to?;
    let place = ids.iter().position(|id| *id == target)?;
    match widgets.get(place)? {
        Placed::One(widget) => Some(*widget),
        Placed::Graph(_) => None,
    }
}

/// Gives `widget` the style of `piece`, cut to `clip`.
fn style(ui: &mut Ui, widget: WidgetId, piece: &HudPiece, clip: Option<WidgetId>) {
    let look = piece
        .style
        .as_deref()
        .map(|held| lore_ui::HudStyle { clip, ..held.look });
    // `widget` is a HUD piece of this UI's own, so this cannot fail.
    let _ = ui.set_style(widget, look.as_ref());
}
