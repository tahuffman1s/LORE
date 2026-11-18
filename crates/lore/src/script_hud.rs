//! The HUD a script made with `game.hud`, drawn over the game.
//!
//! The script keeps a list of pieces; this keeps a [`Ui`] that holds the
//! same pieces. The `Ui` is built again only when a piece is made or
//! removed or the script is saved. On every other frame only the pieces
//! the script changed are touched, so a HUD nobody is changing costs a
//! frame no memory and next to no time.

use lore_script::{HudPiece, HudShape, ScriptHost};
use lore_ui::{Color, Ui, UiInput, WidgetId};

use crate::game::Frame;

/// Draws a script's HUD.
#[derive(Default)]
pub(crate) struct ScriptHud {
    ui: Ui,
    /// The widget of each piece, in the order the script made them.
    widgets: Vec<WidgetId>,
    /// What [`ScriptHost::hud_layout`] was when `ui` was built.
    layout: Option<u64>,
}

impl std::fmt::Debug for ScriptHud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHud")
            .field("pieces", &self.widgets.len())
            .finish_non_exhaustive()
    }
}

impl ScriptHud {
    /// Draws the HUD `host`'s script has made over this frame.
    pub(crate) fn show(&mut self, host: &mut ScriptHost, frame: &mut Frame) {
        let layout = host.hud_layout();
        if self.layout == Some(layout) {
            let (ui, widgets) = (&mut self.ui, &self.widgets);
            host.take_hud_changes(|place, piece| {
                if let Some(&widget) = widgets.get(place) {
                    change(ui, widget, piece);
                }
            });
        } else {
            // A piece was made or removed, or the script was saved: build
            // the HUD again from the script's list.
            let mut ui = Ui::default();
            ui.set_text_measure(crate::error_overlay::measure());
            self.widgets.clear();
            host.each_hud_piece(|_, piece| self.widgets.push(add(&mut ui, piece)));
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
fn add(ui: &mut Ui, piece: &HudPiece) -> WidgetId {
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
        // The frame graph is drawn by the engine from the frame times; until
        // that is built, it stands in as a box of the same size that cannot
        // be seen, so the piece keeps its place in the list.
        HudShape::Graph { size, .. } => ui.hud_rect(
            piece.anchor,
            piece.offset,
            *size,
            Color::rgba(0.0, 0.0, 0.0, 0.0),
        ),
    };
    if !piece.visible {
        let _ = ui.set_shown(widget, false);
    }
    widget
}

/// Makes `widget` show what `piece` now holds. Where a piece is pinned
/// and how big it is never change, so only the rest is copied.
fn change(ui: &mut Ui, widget: WidgetId, piece: &HudPiece) {
    // `widget` was made from this piece by `add`, so it is of the kind
    // each of these takes and none of them can fail.
    let _ = match &piece.shape {
        HudShape::Text { text, .. } => ui.set_text(widget, text),
        HudShape::Bar { value, .. } => ui.set_bar(widget, *value),
        HudShape::Box { .. } | HudShape::Graph { .. } => Ok(()),
    };
    // The stand-in for a graph stays unseen.
    if !matches!(piece.shape, HudShape::Graph { .. }) {
        let _ = ui.set_color(widget, piece.color);
    }
    let _ = ui.set_offset(widget, piece.offset);
    let _ = ui.set_shown(widget, piece.visible);
}
