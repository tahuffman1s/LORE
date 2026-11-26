//! The HUD and menus of one game window.

use crate::build::{ScreenBuilder, index, snap};
use crate::draw::{Command, DrawList, Painter};
use crate::error::UiError;
use crate::fill::Stop;
use crate::focus;
use crate::input::{Direction, InputMode, Repeater, UiInput, held_direction};
use crate::layout::{measure_screen, place_hud, place_screen, safe_area, slider_track, tab_width};
use crate::node::{Anchor, Kind, NONE, Node, Rect, Screen, ScreenId, WidgetId};
use crate::paint::{self, Look};
use crate::theme::{Color, EstimatedText, TextMeasure, Theme, UI_SCALE_RANGE};

/// Sizes are given for a screen this many pixels high.
const REFERENCE_HEIGHT: f32 = 1080.0;
/// The most events one frame can produce, with room to spare.
const MOST_EVENTS: usize = 16;
/// How far the pointer is pulled with the button held before a press
/// becomes a drag of the screen, so a click with an unsteady hand is
/// still a click.
const DRAG_AFTER: f32 = 8.0;
/// How many rows of a long screen one notch of the mouse wheel moves.
const ROWS_PER_NOTCH: f32 = 1.5;

/// What is printed on the buttons that work a menu, for the hints along
/// the bottom of a screen: "A Select", "B Back".
///
/// The window layer knows which device the player last used and what
/// the menu's actions are bound to, and hands the names over with
/// [`Ui::show_button_hints`]. A name left empty has no hint.
///
/// ```
/// use lore_ui::ButtonNames;
///
/// let xbox = ButtonNames {
///     confirm: "A",
///     back: "B",
///     previous_tab: "LB",
///     next_tab: "RB",
///     round: true,
/// };
/// let keyboard = ButtonNames {
///     confirm: "Enter",
///     back: "Esc",
///     previous_tab: "Q",
///     next_tab: "E",
///     round: false,
/// };
/// # let _ = (xbox, keyboard);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ButtonNames<'a> {
    /// The button that chooses the widget the player is on.
    pub confirm: &'a str,
    /// The button that goes back one screen.
    pub back: &'a str,
    /// The button that opens the tab to the left.
    pub previous_tab: &'a str,
    /// The button that opens the tab to the right.
    pub next_tab: &'a str,
    /// Whether the names are drawn in round shapes, as a gamepad's
    /// buttons are, and not in the squarer shape of a key.
    pub round: bool,
}

/// The button names being shown, kept so drawing them asks for no memory.
#[derive(Debug, Default)]
pub(crate) struct Names {
    pub shown: bool,
    pub confirm: String,
    pub back: String,
    pub previous_tab: String,
    pub next_tab: String,
    pub round: bool,
}

/// A press of the mouse button on a screen that scrolls.
#[derive(Debug, Clone, Copy)]
struct Drag {
    /// Where the pointer was when the button went down, top to bottom.
    from: f32,
    /// How far the screen had been scrolled then.
    scroll: f32,
    /// Whether the pointer has been pulled far enough to move the screen.
    moving: bool,
}

/// Something the player did in a menu this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiEvent {
    /// A button was chosen.
    Pressed(WidgetId),
    /// A toggle, slider, choice or tab bar now has this value.
    Changed(WidgetId, Value),
    /// The player pressed back on this screen. The screen has already
    /// closed, unless it was built with
    /// [`stays_open`](ScreenBuilder::stays_open).
    Back(ScreenId),
}

/// What a setting holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// A toggle: on or off.
    Toggle(bool),
    /// A slider's number.
    Slider(f32),
    /// Which option of a choice is picked, counting from 0.
    Choice(usize),
    /// Which tab is open, counting from 0.
    Tab(usize),
}

/// The HUD and menus of one game window.
///
/// Build the HUD pieces and menu screens once, then call
/// [`update`](Self::update) every frame and hand [`draw`](Self::draw) to
/// the renderer. See the [crate docs](crate) for a full example.
pub struct Ui {
    theme: Theme,
    ui_scale: f32,
    measure: Box<dyn TextMeasure>,
    nodes: Vec<Node>,
    screens: Vec<Screen>,
    hud: Vec<u32>,
    /// The open screens, the one on show last.
    stack: Vec<u32>,
    mode: InputMode,
    /// The widget under the pointer.
    hovered: u32,
    /// The widget the mouse button went down on.
    pressed: u32,
    /// The mouse button went down on a slider's track and is still held.
    dragging: bool,
    /// The mouse button went down on a screen that scrolls and is still held.
    drag: Option<Drag>,
    /// The part of the window the open screen's widgets may use.
    room: Rect,
    /// How much of the open screen does not fit in `room`: how far it
    /// can be scrolled. Zero for a screen that fits.
    overflow: f32,
    /// Whether to scroll the widget the player is on into view.
    reveal: bool,
    names: Names,
    repeater: Repeater,
    previous: UiInput,
    events: Vec<UiEvent>,
    commands: Vec<Command>,
    draw_text: String,
    draw_stops: Vec<Stop>,
    /// Real pixels per reference pixel.
    scale: f32,
    /// The screen in reference pixels.
    size: [f32; 2],
}

impl Default for Ui {
    fn default() -> Self {
        Self::new(Theme::default())
    }
}

impl Ui {
    /// An empty UI that will look like `theme`.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            ui_scale: 1.0,
            measure: Box::new(EstimatedText),
            nodes: Vec::new(),
            screens: Vec::new(),
            hud: Vec::new(),
            stack: Vec::new(),
            mode: InputMode::default(),
            hovered: NONE,
            pressed: NONE,
            dragging: false,
            drag: None,
            room: Rect::default(),
            overflow: 0.0,
            reveal: false,
            names: Names::default(),
            repeater: Repeater::default(),
            previous: UiInput::default(),
            events: Vec::with_capacity(MOST_EVENTS),
            commands: Vec::new(),
            draw_text: String::new(),
            draw_stops: Vec::new(),
            scale: 1.0,
            size: [1920.0, REFERENCE_HEIGHT],
        }
    }

    /// The player's own size setting: 1 is normal, and it is kept
    /// between 0.75 and 1.5.
    pub fn set_ui_scale(&mut self, scale: f32) {
        let (smallest, largest) = UI_SCALE_RANGE;
        self.ui_scale = if scale.is_finite() {
            scale.clamp(smallest, largest)
        } else {
            1.0
        };
    }

    /// Says along the bottom of every menu screen what the buttons do:
    /// "A Select", "B Back", and "LB RB Switch tab" on a screen with
    /// tabs. `names` is what is printed on each button of the device the
    /// player is using; call this again when they pick up another.
    ///
    /// The hints follow the widget the player is on: a button is
    /// selected, a toggle changed, and a slider or choice changed with
    /// left and right. Names no longer than the ones before are copied
    /// in place.
    ///
    /// ```
    /// use lore_ui::{ButtonNames, Ui};
    ///
    /// let mut ui = Ui::default();
    /// ui.show_button_hints(&ButtonNames {
    ///     confirm: "A",
    ///     back: "B",
    ///     previous_tab: "LB",
    ///     next_tab: "RB",
    ///     round: true,
    /// });
    /// ```
    pub fn show_button_hints(&mut self, names: &ButtonNames<'_>) {
        let kept = &mut self.names;
        kept.shown = true;
        kept.round = names.round;
        let mut grew = false;
        for (kept, new) in [
            (&mut kept.confirm, names.confirm),
            (&mut kept.back, names.back),
            (&mut kept.previous_tab, names.previous_tab),
            (&mut kept.next_tab, names.next_tab),
        ] {
            if kept != new {
                grew |= new.len() > kept.capacity();
                kept.clear();
                kept.push_str(new);
            }
        }
        if grew {
            self.reserve();
        }
    }

    /// Stops showing what the buttons do.
    pub fn hide_button_hints(&mut self) {
        self.names.shown = false;
    }

    /// Uses real text widths, once the renderer has a font to measure with.
    pub fn set_text_measure(&mut self, measure: Box<dyn TextMeasure>) {
        self.measure = measure;
    }

    // ---- Building -------------------------------------------------------

    /// Adds a menu screen with `title` at the top and the widgets that
    /// `build` adds under it. The screen stays hidden until
    /// [`open`](Self::open) shows it.
    ///
    /// # Errors
    ///
    /// The first mistake made inside `build`, such as a slider whose
    /// lowest value is above its highest. The screen is not added.
    pub fn add_screen(
        &mut self,
        title: &str,
        build: impl FnOnce(&mut ScreenBuilder<'_>),
    ) -> Result<ScreenId, UiError> {
        let first = self.nodes.len();
        let gap = self.theme.gap;
        let mut builder = ScreenBuilder {
            nodes: &mut self.nodes,
            gap,
            root: index(first),
            parent: NONE,
            error: None,
            wrap: true,
            stays_open: false,
            start: NONE,
        };
        builder.parent = builder.add(Kind::Column { gap }, "");
        if !title.is_empty() {
            builder.add(Kind::Label { title: true }, title);
        }
        build(&mut builder);
        let ScreenBuilder {
            root,
            error,
            wrap,
            stays_open,
            start,
            ..
        } = builder;
        if let Some(error) = error {
            // The half-built widgets stay in the list, unreachable, so the
            // ids handed out inside `build` can never name a later widget.
            return Err(error);
        }
        let end = index(self.nodes.len());
        let start = if (root..end).contains(&start) {
            start
        } else {
            NONE
        };
        self.screens.push(Screen {
            root,
            end,
            focus: NONE,
            start,
            wrap,
            stays_open,
            scroll: 0.0,
        });
        self.stack.reserve(self.screens.len());
        self.reserve();
        Ok(ScreenId(index(self.screens.len() - 1)))
    }

    /// Adds text to the HUD, such as an ammo count.
    ///
    /// The text is pinned to `anchor` and then moved by `offset`:
    /// `[right, down]` in pixels on a screen 1080 high. `size` is the
    /// text's height in the same units, and is raised to
    /// [`SMALLEST_TEXT`](crate::SMALLEST_TEXT) if it is below it.
    pub fn hud_text(
        &mut self,
        anchor: Anchor,
        offset: [f32; 2],
        size: f32,
        text: &str,
    ) -> WidgetId {
        let color = self.theme.text;
        self.add_hud(
            Kind::HudText {
                anchor,
                offset,
                size,
                color,
            },
            [0.0; 2],
            text,
        )
    }

    /// Adds a bar to the HUD, such as health. `fraction` is how full it
    /// is, from 0 to 1; `size` is its `[width, height]` when full.
    pub fn hud_bar(
        &mut self,
        anchor: Anchor,
        offset: [f32; 2],
        size: [f32; 2],
        color: Color,
        fraction: f32,
    ) -> WidgetId {
        let fraction = clean_fraction(fraction);
        self.add_hud(
            Kind::HudBar {
                anchor,
                offset,
                fraction,
                color,
            },
            size,
            "",
        )
    }

    /// Adds a plain box to the HUD, such as one arm of a crosshair.
    pub fn hud_rect(
        &mut self,
        anchor: Anchor,
        offset: [f32; 2],
        size: [f32; 2],
        color: Color,
    ) -> WidgetId {
        self.add_hud(
            Kind::HudRect {
                anchor,
                offset,
                color,
                corner: 0.0,
                angle: 0.0,
            },
            size,
            "",
        )
    }

    /// Adds one of the game's pictures to the HUD, `size` big and tinted
    /// by `color` (white leaves it as it is). `image` is which of the
    /// pictures the renderer was given, counting from 0.
    pub fn hud_image(
        &mut self,
        anchor: Anchor,
        offset: [f32; 2],
        size: [f32; 2],
        image: u32,
        color: Color,
    ) -> WidgetId {
        self.add_hud(
            Kind::HudImage {
                anchor,
                offset,
                color,
                image,
                angle: 0.0,
            },
            size,
            "",
        )
    }

    fn add_hud(&mut self, kind: Kind, size: [f32; 2], text: &str) -> WidgetId {
        let new = index(self.nodes.len());
        let mut node = Node::new(kind, text);
        node.size = size;
        self.nodes.push(node);
        self.hud.push(new);
        self.reserve();
        WidgetId(new)
    }

    /// Makes sure a frame's drawing never has to ask for more memory.
    fn reserve(&mut self) {
        // The backdrop and panel, the scroll bar, and the hints with the
        // names on them.
        let names = &self.names;
        let named = names.confirm.len()
            + names.back.len()
            + names.previous_tab.len()
            + names.next_tab.len();
        let (mut commands, mut text) = (4 + paint::MOST_HINT_DRAWS, named + 64);
        for node in &self.nodes {
            let (boxes, letters) = match &node.kind {
                Kind::Tabs { names, .. } => (3 * names.len(), names.iter().map(String::len).sum()),
                Kind::Choice { options, .. } => {
                    (8, options.iter().map(String::len).max().unwrap_or(0))
                }
                _ => (8, 0),
            };
            commands += boxes;
            text += letters + node.text.capacity() + 64;
        }
        self.commands.reserve(commands);
        self.draw_text.reserve(text);
    }

    // ---- The menu stack -------------------------------------------------

    /// Shows a screen on top of whatever is open. If it is already open
    /// underneath, the screens above it close instead.
    ///
    /// # Errors
    ///
    /// [`UiError::UnknownScreen`] if `screen` came from another `Ui`.
    pub fn open(&mut self, screen: ScreenId) -> Result<(), UiError> {
        if screen.0 as usize >= self.screens.len() {
            return Err(UiError::UnknownScreen);
        }
        match self.stack.iter().position(|&open| open == screen.0) {
            Some(place) => self.stack.truncate(place + 1),
            None => self.stack.push(screen.0),
        }
        self.reveal = true;
        self.pointer_left();
        Ok(())
    }

    /// Closes the screen on show, revealing the one under it. Returns
    /// false if no menu was open.
    pub fn close(&mut self) -> bool {
        self.pointer_left();
        self.stack.pop().is_some()
    }

    /// Closes every screen, leaving only the HUD.
    pub fn close_all(&mut self) {
        self.pointer_left();
        self.stack.clear();
    }

    /// The screen on show, if a menu is open.
    #[must_use]
    pub fn open_screen(&self) -> Option<ScreenId> {
        self.stack.last().map(|&screen| ScreenId(screen))
    }

    /// Whether a menu is open. While one is, the game should stop
    /// sending the player's input to the world.
    #[must_use]
    pub fn is_menu_open(&self) -> bool {
        !self.stack.is_empty()
    }

    /// The widget the player is on, if a menu is open and has one.
    #[must_use]
    pub fn focused(&self) -> Option<WidgetId> {
        let focus = self.screens[*self.stack.last()? as usize].focus;
        (focus != NONE).then_some(WidgetId(focus))
    }

    /// Whether the player last used the mouse, or a gamepad or keyboard.
    #[must_use]
    pub fn mode(&self) -> InputMode {
        self.mode
    }

    // ---- Changing widgets -----------------------------------------------

    fn node(&mut self, widget: WidgetId) -> Result<&mut Node, UiError> {
        self.nodes
            .get_mut(widget.0 as usize)
            .ok_or(UiError::UnknownWidget)
    }

    /// Changes the words on a label, button, setting or HUD text.
    ///
    /// Text no longer than the widget has held before is copied in
    /// place, so updating an ammo count every frame costs nothing.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or has no words.
    pub fn set_text(&mut self, widget: WidgetId, text: &str) -> Result<(), UiError> {
        let node = self.node(widget)?;
        match node.kind {
            Kind::Label { .. }
            | Kind::Button
            | Kind::Toggle { .. }
            | Kind::Slider { .. }
            | Kind::Choice { .. }
            | Kind::HudText { .. } => {}
            _ => {
                return Err(UiError::WrongKind {
                    expected: "widget with text",
                    found: node.kind.name(),
                });
            }
        }
        let grows = text.len() > node.text.capacity();
        node.text.clear();
        node.text.push_str(text);
        if grows {
            self.reserve();
        }
        Ok(())
    }

    /// Changes the colour of a HUD text, bar or rectangle.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a HUD piece.
    pub fn set_color(&mut self, widget: WidgetId, new: Color) -> Result<(), UiError> {
        let node = self.node(widget)?;
        match &mut node.kind {
            Kind::HudText { color, .. }
            | Kind::HudBar { color, .. }
            | Kind::HudRect { color, .. }
            | Kind::HudImage { color, .. } => {
                *color = new;
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "HUD piece",
                found: other.name(),
            }),
        }
    }

    /// Moves a HUD text, bar or rectangle: `offset` is how far it sits
    /// from where it is pinned, `[right, down]` in pixels on a screen
    /// 1080 high.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a HUD piece.
    pub fn set_offset(&mut self, widget: WidgetId, new: [f32; 2]) -> Result<(), UiError> {
        match &mut self.node(widget)?.kind {
            Kind::HudText { offset, .. }
            | Kind::HudBar { offset, .. }
            | Kind::HudRect { offset, .. }
            | Kind::HudImage { offset, .. } => {
                *offset = new;
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "HUD piece",
                found: other.name(),
            }),
        }
    }

    /// Shows a HUD text, bar or rectangle, or hides it: a hidden piece
    /// keeps its place and its values and is not drawn.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a HUD piece.
    pub fn set_shown(&mut self, widget: WidgetId, shown: bool) -> Result<(), UiError> {
        let node = self.node(widget)?;
        match &node.kind {
            Kind::HudText { .. }
            | Kind::HudBar { .. }
            | Kind::HudRect { .. }
            | Kind::HudImage { .. } => {
                node.visible = shown;
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "HUD piece",
                found: other.name(),
            }),
        }
    }

    /// Changes the `[width, height]` of a HUD bar (when full), rectangle
    /// or image, in pixels on a screen 1080 high. Below 0 is taken as 0.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a HUD bar,
    /// rectangle or image.
    pub fn set_size(&mut self, widget: WidgetId, new: [f32; 2]) -> Result<(), UiError> {
        let node = self.node(widget)?;
        match &node.kind {
            Kind::HudBar { .. } | Kind::HudRect { .. } | Kind::HudImage { .. } => {
                node.size = new.map(|part| if part.is_finite() { part.max(0.0) } else { 0.0 });
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "HUD bar, rectangle or image",
                found: other.name(),
            }),
        }
    }

    /// Turns a HUD rectangle or image about its middle: `angle` radians,
    /// clockwise on the screen. Where it is pinned is where it would be
    /// unturned.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a HUD rectangle or
    /// image.
    pub fn set_angle(&mut self, widget: WidgetId, new: f32) -> Result<(), UiError> {
        let new = if new.is_finite() { new } else { 0.0 };
        match &mut self.node(widget)?.kind {
            Kind::HudRect { angle, .. } | Kind::HudImage { angle, .. } => {
                *angle = new;
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "HUD rectangle or image",
                found: other.name(),
            }),
        }
    }

    /// Rounds the corners of a HUD rectangle: `radius` in pixels on a
    /// screen 1080 high, held to half its shorter side when drawn.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a HUD rectangle.
    pub fn set_corner(&mut self, widget: WidgetId, radius: f32) -> Result<(), UiError> {
        let radius = if radius.is_finite() {
            radius.max(0.0)
        } else {
            0.0
        };
        match &mut self.node(widget)?.kind {
            Kind::HudRect { corner, .. } => {
                *corner = radius;
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "HUD rectangle",
                found: other.name(),
            }),
        }
    }

    /// Changes how full a HUD bar is, from 0 to 1.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a HUD bar.
    pub fn set_bar(&mut self, widget: WidgetId, new: f32) -> Result<(), UiError> {
        match &mut self.node(widget)?.kind {
            Kind::HudBar { fraction, .. } => {
                *fraction = clean_fraction(new);
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "HUD bar",
                found: other.name(),
            }),
        }
    }

    /// Turns a toggle on or off. No event is sent.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a toggle.
    pub fn set_toggle(&mut self, widget: WidgetId, new: bool) -> Result<(), UiError> {
        match &mut self.node(widget)?.kind {
            Kind::Toggle { on } => {
                *on = new;
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "toggle",
                found: other.name(),
            }),
        }
    }

    /// Sets a slider, moving `new` to the nearest value the slider can
    /// hold. No event is sent.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a slider.
    pub fn set_slider(&mut self, widget: WidgetId, new: f32) -> Result<(), UiError> {
        match &mut self.node(widget)?.kind {
            Kind::Slider {
                min,
                max,
                step,
                value,
            } => {
                *value = snap(*min, *max, *step, new);
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "slider",
                found: other.name(),
            }),
        }
    }

    /// Picks an option of a choice, counting from 0. A number past the
    /// end picks the last option. No event is sent.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a choice.
    pub fn set_choice(&mut self, widget: WidgetId, new: usize) -> Result<(), UiError> {
        match &mut self.node(widget)?.kind {
            Kind::Choice { options, selected } => {
                *selected = new.min(options.len().saturating_sub(1));
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "choice",
                found: other.name(),
            }),
        }
    }

    /// Opens a tab of a tab bar, counting from 0. A number past the end
    /// opens the last tab. No event is sent.
    ///
    /// # Errors
    ///
    /// If the widget is not from this `Ui`, or is not a tab bar.
    pub fn set_tab(&mut self, widget: WidgetId, new: usize) -> Result<(), UiError> {
        match &mut self.node(widget)?.kind {
            Kind::Tabs { names, open } => {
                *open = new.min(names.len().saturating_sub(1));
                Ok(())
            }
            other => Err(UiError::WrongKind {
                expected: "tab bar",
                found: other.name(),
            }),
        }
    }

    /// What a toggle, slider, choice or tab bar holds right now, or
    /// `None` for any other widget.
    #[must_use]
    pub fn value(&self, widget: WidgetId) -> Option<Value> {
        match &self.nodes.get(widget.0 as usize)?.kind {
            Kind::Toggle { on } => Some(Value::Toggle(*on)),
            Kind::Slider { value, .. } => Some(Value::Slider(*value)),
            Kind::Choice { selected, .. } => Some(Value::Choice(*selected)),
            Kind::Tabs { open, .. } => Some(Value::Tab(*open)),
            _ => None,
        }
    }

    // ---- Every frame ----------------------------------------------------

    /// Moves the UI on by one frame and says what the player did.
    ///
    /// Call this every frame, menu open or not, with what the player is
    /// holding, the seconds since the last frame, and the window's
    /// `[width, height]` in pixels. Then [`draw`](Self::draw) has this
    /// frame's picture.
    pub fn update(&mut self, input: &UiInput, dt: f32, screen: [u32; 2]) -> &[UiEvent] {
        self.events.clear();
        #[allow(
            clippy::cast_precision_loss,
            reason = "screens are far smaller than 2^24 pixels"
        )]
        let [width, height] = [screen[0] as f32, screen[1].max(1) as f32];
        self.scale = height / REFERENCE_HEIGHT * self.ui_scale;
        self.size = [width / self.scale, height / self.scale];

        self.layout();
        self.handle(input, dt);
        self.layout();
        self.paint();
        &self.events
    }

    /// What to draw this frame, back to front, in real pixels.
    #[must_use]
    pub fn draw(&self) -> DrawList<'_> {
        DrawList {
            commands: &self.commands,
            text: &self.draw_text,
            stops: &self.draw_stops,
        }
    }

    fn layout(&mut self) {
        let safe = safe_area(self.size, self.theme.safe_margin);
        for &piece in &self.hud {
            place_hud(&mut self.nodes[piece as usize], safe, self.measure.as_ref());
        }

        let Some(&top) = self.stack.last() else {
            self.overflow = 0.0;
            return;
        };
        // The panel around the widgets is inside the safe area too, and
        // the hints have a line of their own under it.
        let pad = self.theme.padding * 2.0;
        let hints = if self.names.shown {
            paint::hint_height(&self.theme) + self.theme.gap
        } else {
            0.0
        };
        let room = Rect {
            x: safe.x + pad,
            y: safe.y + pad,
            w: safe.w - 2.0 * pad,
            h: (safe.h - 2.0 * pad - hints).max(self.theme.row_height),
        };
        if std::mem::replace(&mut self.room, room) != room && self.mode == InputMode::Focus {
            // The window or the player's size setting changed: what was
            // in view may not be any more.
            self.reveal = true;
        }
        let Screen { root, end, .. } = self.screens[top as usize];
        measure_screen(
            &mut self.nodes,
            root as usize,
            end as usize,
            &self.theme,
            self.measure.as_ref(),
        );
        self.overflow = (self.nodes[root as usize].size[1] - self.room.h).max(0.0);
        self.place(top);

        // The player is always on something they can see.
        let screen = &mut self.screens[top as usize];
        let can_land = |widget: u32| widget != NONE && self.nodes[widget as usize].focusable();
        if !can_land(screen.focus) {
            screen.focus = if can_land(screen.start) {
                screen.start
            } else {
                (screen.root..screen.end)
                    .find(|&widget| can_land(widget))
                    .unwrap_or(NONE)
            };
            self.reveal = true;
        }
        if std::mem::take(&mut self.reveal) && self.overflow > 0.0 {
            let before = self.screens[top as usize].scroll;
            let wanted = self.scroll_to_show(top);
            if wanted.to_bits() != before.to_bits() {
                self.screens[top as usize].scroll = wanted;
                self.place(top);
            }
        }
    }

    /// The part of the window a screen too long for it is shown through.
    fn clip(&self) -> Option<Rect> {
        (self.overflow > 0.0).then_some(self.room)
    }

    /// Puts the widgets of the open screen where its scroll says. A
    /// screen that fits sits in the middle of the window.
    fn place(&mut self, top: u32) {
        let screen = &mut self.screens[top as usize];
        screen.scroll = if screen.scroll.is_finite() {
            screen.scroll.clamp(0.0, self.overflow)
        } else {
            0.0
        };
        let (first, end) = (screen.root as usize, screen.end as usize);
        let [width, height] = self.size;
        let root = &mut self.nodes[first];
        let [w, h] = root.size;
        let y = if self.overflow > 0.0 {
            self.room.y - screen.scroll
        } else {
            // Moved up only when the hints need the room underneath.
            ((height - h) / 2.0).min(self.room.bottom() - h)
        };
        root.rect = Rect {
            x: (width - w) / 2.0,
            y,
            w,
            h,
        };
        root.visible = true;
        place_screen(&mut self.nodes, first, end, &self.theme);
    }

    /// How far the open screen should be scrolled for the widget the
    /// player is on to be seen: as little further as will do, with a row
    /// to spare so the next widget shows too. On the first widget the
    /// screen goes back to its top, title and all, and on the last to
    /// its end.
    fn scroll_to_show(&self, top: u32) -> f32 {
        let screen = &self.screens[top as usize];
        let Some(on) = self.nodes.get(screen.focus as usize) else {
            return screen.scroll;
        };
        let content = self.nodes[screen.root as usize].rect;
        let (start, finish) = (on.rect.y - content.y, on.rect.bottom() - content.y);
        let others = || {
            (screen.root..screen.end)
                .map(|widget| &self.nodes[widget as usize])
                .filter(|node| node.focusable())
        };
        let spare = (self.theme.row_height + self.theme.gap)
            .min((self.room.h - on.rect.h) / 2.0)
            .max(0.0);
        let highest = if others().any(|node| node.rect.y < on.rect.y - 0.5) {
            start - spare
        } else {
            0.0
        };
        let lowest = if others().any(|node| node.rect.bottom() > on.rect.bottom() + 0.5) {
            finish + spare
        } else {
            content.h
        };
        // When both cannot be had, seeing the widget's top matters more.
        screen
            .scroll
            .max(lowest - self.room.h)
            .min(highest.max(finish - self.room.h).min(start))
            .clamp(0.0, self.overflow)
    }

    fn handle(&mut self, input: &UiInput, dt: f32) {
        let before = std::mem::replace(&mut self.previous, *input);
        let held = held_direction(input.direction);
        let Some(&top) = self.stack.last() else {
            self.repeater.wait_for_release(held);
            self.pointer_left();
            return;
        };
        let step = self.repeater.step(held, dt);
        let Screen { root, end, .. } = self.screens[top as usize];

        // The mouse.
        let clicked = input.pointer_down && !before.pointer_down;
        let let_go = !input.pointer_down && before.pointer_down;
        let moved = match (input.pointer, before.pointer) {
            (Some(now), Some(then)) => {
                (now[0] - then[0]).abs() > 0.5 || (now[1] - then[1]).abs() > 0.5
            }
            _ => false,
        };
        let pointer = input.pointer.map(|[x, y]| [x / self.scale, y / self.scale]);
        let rolled = input.wheel.is_finite() && input.wheel != 0.0 && self.overflow > 0.0;
        if (moved || clicked || rolled) && pointer.is_some() {
            self.mode = InputMode::Pointer;
        }
        if self.mode == InputMode::Pointer
            && let Some(pointer) = pointer
        {
            self.hover(top, pointer);
            if clicked {
                self.pressed = self.hovered;
                self.dragging = self.on_slider_track(self.hovered, pointer);
                if self.hovered == NONE && self.in_view(pointer) {
                    self.click_tab(root, end, pointer);
                }
                if self.overflow > 0.0 && !self.dragging {
                    self.drag = Some(Drag {
                        from: pointer[1],
                        scroll: self.screens[top as usize].scroll,
                        moving: false,
                    });
                }
            }
            if input.pointer_down && self.dragging {
                self.drag_slider(self.pressed, pointer[0]);
            }
            if self.scroll_by_mouse(top, input, pointer) {
                // The widgets have moved under the pointer.
                self.layout();
                self.hover(top, pointer);
            }
        } else {
            // The pointer has left the window, or is not in use.
            self.hovered = NONE;
        }
        if !input.pointer_down {
            self.drag = None;
        }
        if let_go {
            let pressed = std::mem::replace(&mut self.pressed, NONE);
            if pressed != NONE && pressed == self.hovered && self.mode == InputMode::Pointer {
                self.activate(pressed);
            }
        }

        // The gamepad and keyboard.
        let confirm = input.confirm && !before.confirm;
        if self.mode == InputMode::Pointer && (step.is_some() || confirm) {
            // The first press only brings the outline back, so the player
            // sees where they are before anything moves or is chosen.
            self.mode = InputMode::Focus;
            self.reveal = true;
            self.pointer_left();
        } else {
            if let Some(direction) = step {
                self.go(top, direction);
                self.reveal = true;
            }
            if confirm {
                self.activate(self.screens[top as usize].focus);
            }
        }
        let tab_step = i32::from(input.next_tab && !before.next_tab)
            - i32::from(input.previous_tab && !before.previous_tab);
        if tab_step != 0
            && let Some(tabs) = (root..end).find(|&widget| {
                let node = &self.nodes[widget as usize];
                node.visible && matches!(node.kind, Kind::Tabs { .. })
            })
        {
            self.nudge(tabs, tab_step);
            self.reveal = true;
            // Whatever the mouse was holding may now be on a hidden page.
            self.pointer_left();
        }
        if input.back && !before.back {
            self.push_event(UiEvent::Back(ScreenId(top)));
            if !self.screens[top as usize].stays_open {
                self.stack.pop();
                self.pointer_left();
            }
        }
    }

    /// Forgets what the pointer was over or holding.
    fn pointer_left(&mut self) {
        self.hovered = NONE;
        self.pressed = NONE;
        self.dragging = false;
        self.drag = None;
    }

    /// Whether `pointer` is where the open screen's widgets are shown. On
    /// a screen that scrolls, a widget that has gone past the edge of
    /// the panel is not there to be clicked.
    fn in_view(&self, pointer: [f32; 2]) -> bool {
        self.clip().is_none_or(|clip| clip.contains(pointer))
    }

    /// Finds the widget under the pointer, and puts the player on it.
    fn hover(&mut self, top: u32, pointer: [f32; 2]) {
        let Screen { root, end, .. } = self.screens[top as usize];
        self.hovered = if self.in_view(pointer) {
            (root..end)
                .rev()
                .find(|&widget| {
                    let node = &self.nodes[widget as usize];
                    node.focusable() && node.rect.contains(pointer)
                })
                .unwrap_or(NONE)
        } else {
            NONE
        };
        if self.hovered != NONE {
            self.screens[top as usize].focus = self.hovered;
        }
    }

    /// Scrolls a screen longer than the window by the wheel, or by a
    /// drag with the button held. Returns whether it moved.
    fn scroll_by_mouse(&mut self, top: u32, input: &UiInput, pointer: [f32; 2]) -> bool {
        if self.overflow <= 0.0 {
            return false;
        }
        let before = self.screens[top as usize].scroll;
        let mut scroll = before;
        if let Some(drag) = &mut self.drag
            && input.pointer_down
        {
            let pulled = pointer[1] - drag.from;
            if !drag.moving && pulled.abs() > DRAG_AFTER {
                drag.moving = true;
                // Whatever the button went down on is not being clicked.
                self.pressed = NONE;
            }
            if drag.moving {
                // The screen follows the pointer, as under a finger.
                scroll = drag.scroll - pulled;
            }
        }
        if input.wheel.is_finite() && input.wheel != 0.0 {
            let rolled = input.wheel * ROWS_PER_NOTCH * (self.theme.row_height + self.theme.gap);
            scroll -= rolled;
            // A drag that is under way carries on from where the wheel put it.
            if let Some(drag) = &mut self.drag {
                drag.scroll -= rolled;
            }
        }
        let scroll = scroll.clamp(0.0, self.overflow);
        self.screens[top as usize].scroll = scroll;
        scroll.to_bits() != before.to_bits()
    }

    /// Whether `pointer` is on the part of a slider that can be dragged:
    /// its track, as tall as the whole row and a little wider.
    fn on_slider_track(&self, widget: u32, pointer: [f32; 2]) -> bool {
        let Some(node) = self.nodes.get(widget as usize) else {
            return false;
        };
        let track = slider_track(node.rect, &self.theme);
        let reach = Rect {
            x: track.x - 12.0,
            y: node.rect.y,
            w: track.w + 24.0,
            h: node.rect.h,
        };
        matches!(node.kind, Kind::Slider { .. }) && reach.contains(pointer)
    }

    fn push_event(&mut self, event: UiEvent) {
        // Never grows: a frame cannot produce more than a handful.
        if self.events.len() < self.events.capacity() {
            self.events.push(event);
        }
    }

    /// Handles a direction press on the open screen.
    fn go(&mut self, top: u32, direction: Direction) {
        let screen = &self.screens[top as usize];
        let focus = screen.focus;
        if focus == NONE {
            return;
        }
        let node = &self.nodes[focus as usize];
        let sideways = match direction {
            Direction::Left => -1,
            Direction::Right => 1,
            Direction::Up | Direction::Down => 0,
        };
        if sideways != 0 && matches!(node.kind, Kind::Slider { .. } | Kind::Choice { .. }) {
            self.nudge(focus, sideways);
            return;
        }
        let chosen = node.neighbors[direction.index()];
        let next = if chosen != NONE && self.nodes[chosen as usize].focusable() {
            Some(chosen)
        } else {
            let (first, end) = (screen.root as usize, screen.end as usize);
            focus::next(
                &self.nodes,
                first,
                end,
                focus as usize,
                direction,
                screen.wrap,
            )
            .map(index)
        };
        if let Some(next) = next {
            self.screens[top as usize].focus = next;
        }
    }

    /// Chooses a widget: presses a button, flips a toggle, or moves a
    /// choice on by one.
    fn activate(&mut self, widget: u32) {
        let Some(node) = self.nodes.get_mut(widget as usize) else {
            return;
        };
        match &mut node.kind {
            Kind::Button => self.push_event(UiEvent::Pressed(WidgetId(widget))),
            Kind::Toggle { on } => {
                *on = !*on;
                let event = UiEvent::Changed(WidgetId(widget), Value::Toggle(*on));
                self.push_event(event);
            }
            Kind::Choice { .. } => self.nudge(widget, 1),
            _ => {}
        }
    }

    /// Moves a slider by a step, or a choice or tab bar to the next or
    /// previous entry, going round at the ends.
    fn nudge(&mut self, widget: u32, by: i32) {
        let around = |now: usize, count: usize| {
            if count == 0 {
                0
            } else if by > 0 {
                (now + 1) % count
            } else {
                (now + count - 1) % count
            }
        };
        let id = WidgetId(widget);
        let event = match &mut self.nodes[widget as usize].kind {
            Kind::Slider {
                min,
                max,
                step,
                value,
            } => {
                // Counted in whole steps from the lowest value, so a tiny
                // step on a huge number still moves.
                let steps = ((*value - *min) / *step).round() + if by > 0 { 1.0 } else { -1.0 };
                let new = (*min + steps * *step).clamp(*min, *max);
                let changed = new.to_bits() != value.to_bits();
                *value = new;
                changed.then_some(UiEvent::Changed(id, Value::Slider(new)))
            }
            Kind::Choice { options, selected } => {
                let new = around(*selected, options.len());
                let changed = new != *selected;
                *selected = new;
                changed.then_some(UiEvent::Changed(id, Value::Choice(new)))
            }
            Kind::Tabs { names, open } => {
                let new = around(*open, names.len());
                let changed = new != *open;
                *open = new;
                changed.then_some(UiEvent::Changed(id, Value::Tab(new)))
            }
            _ => None,
        };
        if let Some(event) = event {
            self.push_event(event);
        }
    }

    /// Sets the slider being dragged from where the pointer is.
    fn drag_slider(&mut self, widget: u32, pointer_x: f32) {
        let node = &mut self.nodes[widget as usize];
        let track = slider_track(node.rect, &self.theme);
        let Kind::Slider {
            min,
            max,
            step,
            value,
        } = &mut node.kind
        else {
            return;
        };
        let along = ((pointer_x - track.x) / track.w).clamp(0.0, 1.0);
        let new = snap(*min, *max, *step, *min + along * (*max - *min));
        if new.to_bits() != value.to_bits() {
            *value = new;
            self.push_event(UiEvent::Changed(WidgetId(widget), Value::Slider(new)));
        }
    }

    /// Opens the tab under the pointer, if there is one.
    fn click_tab(&mut self, root: u32, end: u32, pointer: [f32; 2]) {
        for widget in root..end {
            let node = &mut self.nodes[widget as usize];
            if !node.visible {
                continue;
            }
            let rect = node.rect;
            let Kind::Tabs { names, open } = &mut node.kind else {
                continue;
            };
            let mut x = rect.x;
            for (tab, name) in names.iter().enumerate() {
                let w = tab_width(name, &self.theme, self.measure.as_ref());
                let hit = Rect {
                    x,
                    y: rect.y,
                    w,
                    h: self.theme.row_height,
                }
                .contains(pointer);
                if hit && tab != *open {
                    *open = tab;
                    self.push_event(UiEvent::Changed(WidgetId(widget), Value::Tab(tab)));
                    return;
                }
                x += w + self.theme.gap;
            }
        }
    }

    fn paint(&mut self) {
        self.commands.clear();
        self.draw_text.clear();
        self.draw_stops.clear();
        let clip = self.clip();
        let mut painter = Painter {
            commands: &mut self.commands,
            text: &mut self.draw_text,
            stops: &mut self.draw_stops,
            scale: self.scale,
            clip: None,
        };
        for &piece in &self.hud {
            let node = &self.nodes[piece as usize];
            if node.visible {
                paint::hud(node, &self.theme, &mut painter);
            }
        }

        let Some(screen) = self
            .stack
            .last()
            .map(|&screen| &self.screens[screen as usize])
        else {
            return;
        };
        let [width, height] = self.size;
        painter.rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
            },
            self.theme.backdrop,
            0.0,
        );
        // A screen that scrolls is seen through a panel as tall as its room.
        let content = self.nodes[screen.root as usize].rect;
        let shown = clip.map_or(content, |clip| Rect {
            y: clip.y,
            h: clip.h,
            ..content
        });
        let panel = shown.grown(self.theme.padding * 2.0);
        painter.rect(panel, self.theme.panel, self.theme.corner * 2.0);
        // The outline reaches a little past the widget it is around.
        painter.clip = clip.map(|clip| Rect {
            x: 0.0,
            w: width,
            ..clip.grown(self.theme.outline)
        });
        for widget in screen.root..screen.end {
            let node = &self.nodes[widget as usize];
            if !node.visible {
                continue;
            }
            let on_it = widget == screen.focus;
            let look = match self.mode {
                InputMode::Focus => Look {
                    selected: on_it,
                    outlined: on_it,
                },
                InputMode::Pointer => Look {
                    selected: widget == self.hovered,
                    outlined: false,
                },
            };
            let cut = clip.is_some_and(|clip| {
                node.rect.y < clip.y - 0.5 || node.rect.bottom() > clip.bottom() + 0.5
            });
            paint::widget(
                node,
                look,
                cut,
                &self.theme,
                self.measure.as_ref(),
                &mut painter,
            );
        }
        painter.clip = None;
        if let Some(clip) = clip {
            paint::scroll_bar(
                clip,
                content.right() + self.theme.padding,
                screen.scroll,
                content.h,
                &self.theme,
                &mut painter,
            );
        }
        if self.names.shown {
            let on = match self.mode {
                InputMode::Focus => self.nodes.get(screen.focus as usize),
                // The mouse chooses by clicking, which needs no hint.
                InputMode::Pointer => None,
            };
            let tabs = (screen.root..screen.end).any(|widget| {
                let node = &self.nodes[widget as usize];
                node.visible && matches!(node.kind, Kind::Tabs { .. })
            });
            let hints = paint::Hints {
                names: &self.names,
                on: on.map(|node| &node.kind),
                tabs,
            };
            let middle = width / 2.0;
            let top = self.room.bottom() + self.theme.padding * 2.0 + self.theme.gap;
            hints.paint(
                [middle, top],
                &self.theme,
                self.measure.as_ref(),
                &mut painter,
            );
        }
    }
}

/// A bar's fullness, kept between empty and full.
fn clean_fraction(fraction: f32) -> f32 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
