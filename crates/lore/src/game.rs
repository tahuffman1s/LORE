//! The interface a game implements to be driven by the engine.

use lore_input::{ActionSetId, InputFrame, Rumble};
use lore_ui::{DrawBuffer, Ui};

use crate::settings::Settings;
use crate::sound_output::SoundAsk;

/// A game the engine can run.
pub trait Game {
    /// Advances the simulation by one fixed step of `step` seconds.
    ///
    /// Called at the configured tick rate no matter how fast frames are drawn.
    /// Everything that affects gameplay belongs here.
    fn tick(&mut self, input: &InputFrame, step: f32);

    /// Called once per displayed frame, after that frame's ticks.
    ///
    /// Use it for presentation only. The default does nothing.
    fn frame(&mut self, frame: &mut Frame) {
        let _ = frame;
    }
}

/// A borrowed game is a game, so one can be run and looked at afterwards.
impl<G: Game + ?Sized> Game for &mut G {
    fn tick(&mut self, input: &InputFrame, step: f32) {
        (**self).tick(input, step);
    }

    fn frame(&mut self, frame: &mut Frame) {
        (**self).frame(frame);
    }
}

/// The size a frame says the window is when there is no window: on a
/// server and in tests. It is the size HUD and menu sizes are written for.
const NO_WINDOW: [u32; 2] = [1920, 1080];

/// What a game can read and change while presenting one frame.
#[derive(Debug)]
pub struct Frame {
    alpha: f32,
    /// Seconds since the last tick.
    since_tick: f32,
    /// What the player has done that no tick has seen yet. Without a
    /// window it holds nothing, and every action reads as idle.
    input: InputFrame,
    /// How long the frame before this one took, in seconds.
    seconds: f32,
    screen: [u32; 2],
    /// Where the mouse pointer is, when it is free and over the window.
    pointer: Option<[f32; 2]>,
    /// Whether the main mouse button is held.
    pointer_down: bool,
    /// The player's own size for the HUD and menus.
    ui_scale: f32,
    /// The player's settings to put into effect, when they have changed.
    settings: Option<Settings>,
    ui: DrawBuffer,
    title: Option<String>,
    close: bool,
    view: Option<View>,
    action_set: Option<ActionSetId>,
    rumble: Option<Rumble>,
    /// The sounds to start and stop. Empty, and so without memory of its
    /// own, in a frame that asks for none.
    sounds: Vec<SoundAsk>,
    /// The editor's panels: what they are made with, and what was made.
    #[cfg(feature = "editor")]
    panels: Option<PanelFrame>,
    /// Whether the editor wants its ground grid drawn from now on, if it
    /// said.
    #[cfg(feature = "editor")]
    grid: Option<bool>,
}

/// One frame of the editor's panels.
#[cfg(feature = "editor")]
struct PanelFrame {
    context: egui::Context,
    /// What the mouse and keyboard did; taken when the panels are made.
    input: Option<egui::RawInput>,
    /// What to draw, once the panels are made.
    output: Option<egui::FullOutput>,
}

#[cfg(feature = "editor")]
impl Drop for PanelFrame {
    fn drop(&mut self) {
        // Panels nobody took are not drawn; `egui` asks to be told their
        // texture changes are thrown away on purpose.
        if let Some(output) = &mut self.output {
            output.textures_delta.clear();
        }
    }
}

#[cfg(feature = "editor")]
impl std::fmt::Debug for PanelFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelFrame")
            .field("made", &self.output.is_some())
            .finish_non_exhaustive()
    }
}

/// Where the world is seen from: the eye's position, then which way it
/// faces, as [`Frame::look_from`] takes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct View {
    pub(crate) position: [f32; 3],
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
}

impl Default for Frame {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl Frame {
    /// A frame that is `alpha` of the way from the last tick to the next.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn new(alpha: f32) -> Self {
        Self {
            alpha,
            since_tick: 0.0,
            input: InputFrame::default(),
            seconds: 0.0,
            screen: NO_WINDOW,
            pointer: None,
            pointer_down: false,
            ui_scale: 1.0,
            settings: None,
            ui: DrawBuffer::default(),
            title: None,
            close: false,
            view: None,
            action_set: None,
            rumble: None,
            sounds: Vec::new(),
            #[cfg(feature = "editor")]
            panels: None,
            #[cfg(feature = "editor")]
            grid: None,
        }
    }

    /// Draws the editor's ground grid from now on, or stops drawing it: it
    /// is in the way while the level is played.
    #[cfg(feature = "editor")]
    pub(crate) fn show_grid(&mut self, shown: bool) {
        self.grid = Some(shown);
    }

    /// Whether the editor asked for its grid this frame, if it said.
    #[cfg(feature = "editor")]
    pub(crate) fn grid(&self) -> Option<bool> {
        self.grid
    }

    /// Gives the frame what the editor's panels are made with: `context`,
    /// and what the mouse and keyboard did since the last frame.
    #[cfg(feature = "editor")]
    pub(crate) fn with_panels(mut self, context: egui::Context, input: egui::RawInput) -> Self {
        self.panels = Some(PanelFrame {
            context,
            input: Some(input),
            output: None,
        });
        self
    }

    /// Makes the editor's panels for this frame: `build` is given an area
    /// that covers the window. Without a window there is nothing to make
    /// them with and `build` is not called; called twice in a frame, only
    /// the first call counts.
    #[cfg(feature = "editor")]
    pub(crate) fn panels(&mut self, build: impl FnMut(&mut egui::Ui)) {
        if let Some(panels) = &mut self.panels
            && let Some(input) = panels.input.take()
        {
            panels.output = Some(panels.context.run_ui(input, build));
        }
    }

    /// The panels made this frame, if any were.
    #[cfg(feature = "editor")]
    pub(crate) fn take_panels(&mut self) -> Option<egui::FullOutput> {
        self.panels.as_mut()?.output.take()
    }

    /// How far the frame is between the last tick and the next, from 0 to 1.
    ///
    /// Blend the previous and current simulation states by this much for
    /// smooth motion at any frame rate.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// What the player has done since the last tick, which no tick has seen
    /// yet: what the next tick would be given if it ran now.
    ///
    /// This is what lets the view follow the mouse and the stick on every
    /// drawn frame, however few ticks there are to a second. Turn the view
    /// each tick as usual, and draw each frame from those angles plus the
    /// turn that is still on its way:
    ///
    /// ```
    /// use lore::{ActionId, Frame, Game, InputFrame};
    ///
    /// /// Radians per count of mouse movement, and per second on the stick.
    /// const MOUSE: f32 = 0.002;
    /// const STICK: f32 = 3.5;
    ///
    /// struct Looking {
    ///     look: ActionId,
    ///     /// Which way the player faces as of the last tick.
    ///     yaw: f32,
    /// }
    ///
    /// impl Looking {
    ///     /// `yaw` after what `input` holds, with the stick held `seconds`.
    ///     fn turned(&self, input: &InputFrame, seconds: f32) -> f32 {
    ///         let (mouse, stick) = (input.delta(self.look), input.axis2d(self.look));
    ///         // Mouse counts are a distance: never multiply them by a time.
    ///         self.yaw - mouse[0] * MOUSE - stick[0] * STICK * seconds
    ///     }
    /// }
    ///
    /// impl Game for Looking {
    ///     fn tick(&mut self, input: &InputFrame, step: f32) {
    ///         self.yaw = self.turned(input, step);
    ///     }
    ///
    ///     fn frame(&mut self, frame: &mut Frame) {
    ///         let yaw = self.turned(frame.input(), frame.since_tick());
    ///         frame.look_from([0.0, 1.7, 0.0], yaw, 0.0);
    ///     }
    /// }
    /// ```
    ///
    /// The next tick is given the same mouse movement, so nothing is turned
    /// twice and the picture does not jump when the tick runs.
    ///
    /// It is for the picture only: the game's rules read the input their
    /// tick is given and nothing else, or they would depend on the frame
    /// rate. A game on a server has no player at the keyboard, and every
    /// action here reads as idle.
    #[must_use]
    pub fn input(&self) -> &InputFrame {
        &self.input
    }

    /// Seconds since the last tick: how long a stick read through
    /// [`input`](Self::input) has been held without a tick seeing it. It
    /// is [`alpha`](Self::alpha) of a tick's length.
    #[must_use]
    pub fn since_tick(&self) -> f32 {
        self.since_tick
    }

    /// Gives the frame what no tick has seen yet (`input`, from
    /// [`Input::peek`](lore_input::Input::peek)) and the length of a tick
    /// in seconds.
    #[cfg_attr(not(any(feature = "window", test)), allow(dead_code))]
    pub(crate) fn with_input(mut self, input: InputFrame, step: f32) -> Self {
        self.input = input;
        self.since_tick = if self.alpha.is_finite() && step.is_finite() {
            self.alpha.clamp(0.0, 1.0) * step.max(0.0)
        } else {
            0.0
        };
        self
    }

    /// The input the frame was given, to be filled again for the next
    /// frame without asking for memory.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn take_input(&mut self) -> InputFrame {
        std::mem::take(&mut self.input)
    }

    /// A frame drawn `alpha` of the way from the last tick to the next one
    /// of `step` seconds, as the window makes it: with what `input` holds
    /// that no tick has seen.
    #[cfg(test)]
    pub(crate) fn between_ticks(input: &lore_input::Input, alpha: f32, step: f32) -> Self {
        let mut unseen = InputFrame::default();
        input.peek(&mut unseen);
        Self::new(alpha).with_input(unseen, step)
    }

    /// How long the frame before this one took, in seconds: what a
    /// [`Ui`] is updated with, and what anything shown that moves by
    /// itself is moved by. Gameplay belongs in [`Game::tick`], which has
    /// its own fixed step.
    ///
    /// A game on a server draws no frames; there this is 0.
    #[must_use]
    pub fn delta_seconds(&self) -> f32 {
        self.seconds
    }

    /// Where the mouse pointer is, in pixels from the top-left corner of
    /// the window: what a menu's [`UiInput`](lore_ui::UiInput) takes as
    /// its `pointer`.
    ///
    /// `None` while the mouse is captured to look around with, while the
    /// pointer is outside the window, and on a server.
    #[must_use]
    pub fn pointer(&self) -> Option<[f32; 2]> {
        self.pointer
    }

    /// Whether the main mouse button is held: what a menu's
    /// [`UiInput`](lore_ui::UiInput) takes as its `pointer_down`.
    ///
    /// A click on a menu is not an action, because a pointer is something
    /// only a mouse has; everything a click can do, the menu actions do
    /// from a gamepad or keyboard. False while the mouse is captured.
    #[must_use]
    pub fn pointer_down(&self) -> bool {
        self.pointer_down
    }

    /// The size the player chose for the HUD and menus in the settings
    /// menu: 1 is the designed size. Give it to
    /// [`Ui::set_ui_scale`] so a game's own HUD grows and shrinks with
    /// the engine's menus.
    #[must_use]
    pub fn ui_scale(&self) -> f32 {
        self.ui_scale
    }

    /// Says how long the frame before this one took, and where the free
    /// mouse pointer is and whether its main button is held.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn with_pointer(
        mut self,
        seconds: f32,
        pointer: Option<[f32; 2]>,
        pointer_down: bool,
    ) -> Self {
        self.seconds = seconds;
        self.pointer = pointer;
        self.pointer_down = pointer_down;
        self
    }

    /// Sets the player's own size for the HUD and menus, for the game to read.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
    }

    /// Puts the player's settings into effect: how loud the speakers are
    /// and how the view turns.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn apply_settings(&mut self, settings: Settings) {
        self.settings = Some(settings);
    }

    /// The settings to put into effect, if they changed this frame.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn take_settings(&mut self) -> Option<Settings> {
        self.settings.take()
    }

    /// The size of the window in pixels, `[width, height]`: what a
    /// [`Ui`] is updated with so that it fills the window.
    ///
    /// A game on a server has no window; there this is 1920 by 1080.
    #[must_use]
    pub fn screen_size(&self) -> [u32; 2] {
        self.screen
    }

    /// Draws `ui` over the game this frame: its HUD, and its menu if one
    /// is open.
    ///
    /// Call it every frame the UI should be seen, after
    /// [`Ui::update`]; a frame without the call shows the game alone. Called
    /// with several UIs in one frame, each is drawn on top of the ones
    /// before it. The UI is drawn at the window's full size after
    /// upscaling, so its text stays sharp.
    ///
    /// ```
    /// use lore::ui::{Anchor, Ui, UiInput};
    /// use lore::{Frame, Game, InputFrame};
    ///
    /// struct Hud {
    ///     ui: Ui,
    /// }
    ///
    /// impl Hud {
    ///     fn new() -> Self {
    ///         let mut ui = Ui::default();
    ///         ui.hud_text(Anchor::BottomRight, [0.0, 0.0], 32.0, "30 / 90");
    ///         Self { ui }
    ///     }
    /// }
    ///
    /// impl Game for Hud {
    ///     fn tick(&mut self, _input: &InputFrame, _step: f32) {}
    ///
    ///     fn frame(&mut self, frame: &mut Frame) {
    ///         self.ui
    ///             .update(&UiInput::default(), 1.0 / 60.0, frame.screen_size());
    ///         frame.draw_ui(&self.ui);
    ///     }
    /// }
    /// ```
    ///
    /// A new `Ui` guesses how wide text is. In a game with a window, give
    /// it the font text is drawn in, so that centred text is centred
    /// exactly: `ui.set_text_measure(Box::new(lore::render::Font::bundled()))`.
    ///
    /// A game on a server has no window, so nothing is drawn there.
    pub fn draw_ui(&mut self, ui: &Ui) {
        self.ui.add(ui.draw());
    }

    /// Starts the frame for a window of `screen` pixels, if there is one yet, with `buffer` to
    /// gather the UI in: the last frame's, so its memory is used again.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn for_window(mut self, screen: Option<[u32; 2]>, mut buffer: DrawBuffer) -> Self {
        buffer.clear();
        // A minimised window has no size; a UI still needs one to lay out in.
        self.screen = screen.map_or(NO_WINDOW, |[width, height]| [width.max(1), height.max(1)]);
        self.ui = buffer;
        self
    }

    /// Everything the game asked to be drawn over it this frame.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn take_ui(&mut self) -> DrawBuffer {
        std::mem::take(&mut self.ui)
    }

    /// Changes the window title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    /// Draws the world as seen from `position`, in metres, from this frame
    /// until it is called again.
    ///
    /// `yaw` is which way the eye faces, in radians: 0 looks along -Z and it
    /// turns left as the number grows. `pitch` is how far it looks up
    /// (positive) or down. These are the angles the player physics uses. A
    /// game on a server has no window, so nothing happens there.
    pub fn look_from(&mut self, position: [f32; 3], yaw: f32, pitch: f32) {
        self.view = Some(View {
            position,
            yaw,
            pitch,
        });
    }

    /// Makes `set` the live action set from the next tick on: call it with
    /// the menu's set when a menu opens, and with
    /// [`ActionMap::GAMEPLAY`](lore_input::ActionMap::GAMEPLAY) when it
    /// closes.
    ///
    /// Only the live set's actions hear the player. The set also says what
    /// the mouse is for: one made with
    /// [`MouseMode::Pointer`](lore_input::MouseMode::Pointer) shows the
    /// cursor and lets it go, and one made with `MouseMode::Look` hides it
    /// and holds it in the window again. The key that was held to open the
    /// menu does nothing in the menu until it is pressed again; see
    /// [`Input::set_active_set`](lore_input::Input::set_active_set).
    ///
    /// Asking for the set that is already live does nothing, so a game can
    /// call this every frame. A game on a server has no player at the
    /// keyboard, so nothing happens there.
    pub fn use_action_set(&mut self, set: ActionSetId) {
        self.action_set = Some(set);
    }

    /// Shakes the gamepad the player is holding: `Rumble::new(strong, weak,
    /// seconds)`.
    ///
    /// It replaces the rumble that is playing, so
    /// [`Rumble::STOP`](lore_input::Rumble::STOP) ends one early, and it
    /// stops by itself when its time is up, when the window is no longer in
    /// front and when the gamepad is unplugged. Called twice in one frame,
    /// the later call wins.
    ///
    /// Nothing happens when the player is on keyboard and mouse, when the
    /// gamepad has no rumble motors, or on macOS, where rumble is not
    /// supported yet. Never make rumble the only sign of something: show or
    /// play it as well. A game on a server has no gamepad, so nothing happens
    /// there.
    pub fn rumble(&mut self, rumble: Rumble) {
        self.rumble = Some(rumble);
    }

    /// Closes the window and ends the game after this frame, as if the
    /// player had closed it. A game on a server has no window, so nothing
    /// happens there.
    pub fn close(&mut self) {
        self.close = true;
    }

    /// Whether the game asked to end this frame.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn closing(&self) -> bool {
        self.close
    }

    /// Where the game asked the world to be seen from this frame, if it did.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn view(&self) -> Option<View> {
        self.view
    }

    /// The action set requested this frame, if any.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn action_set(&self) -> Option<ActionSetId> {
        self.action_set
    }

    /// The rumble requested this frame, if any.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn rumble_requested(&self) -> Option<Rumble> {
        self.rumble
    }

    /// Starts or stops a sound for the player at this computer. A game on
    /// a server has no speakers, so nothing happens there.
    pub(crate) fn sound(&mut self, ask: SoundAsk) {
        self.sounds.push(ask);
    }

    /// The sounds asked for this frame, in order.
    #[cfg_attr(not(any(feature = "window", test)), allow(dead_code))]
    pub(crate) fn take_sounds(&mut self) -> Vec<SoundAsk> {
        std::mem::take(&mut self.sounds)
    }

    /// The title requested this frame, if any.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn take_title(&mut self) -> Option<String> {
        self.title.take()
    }
}

#[cfg(test)]
mod tests {
    use lore_ui::{Anchor, Draw, UiInput};

    use super::*;

    /// A UI with one line of HUD text, laid out for `frame`.
    fn hud(words: &str, frame: &Frame) -> Ui {
        let mut ui = Ui::default();
        ui.hud_text(Anchor::BottomRight, [0.0, 0.0], 32.0, words);
        ui.update(&UiInput::default(), 1.0 / 60.0, frame.screen_size());
        ui
    }

    fn words(buffer: &DrawBuffer) -> Vec<String> {
        buffer
            .draw()
            .iter()
            .filter_map(|draw| match draw {
                Draw::Text { text, .. } => Some(text.to_owned()),
                Draw::Rect { .. } => None,
            })
            .collect()
    }

    #[test]
    fn a_frame_gathers_each_ui_in_the_order_it_was_drawn() {
        let mut frame = Frame::new(0.5);
        assert_eq!(frame.screen_size(), [1920, 1080]);
        frame.draw_ui(&hud("30 / 90", &frame));
        frame.draw_ui(&hud("Paused", &frame));
        let drawn = frame.take_ui();
        assert_eq!(words(&drawn), ["30 / 90", "Paused"]);

        // The next frame starts with nothing, in the window's size.
        let mut next = Frame::new(0.5).for_window(Some([1280, 720]), drawn);
        assert_eq!(next.screen_size(), [1280, 720]);
        assert!(next.take_ui().draw().is_empty());
    }

    #[test]
    fn a_window_with_no_size_still_gives_a_ui_room() {
        let minimised = Frame::new(0.0).for_window(Some([0, 0]), DrawBuffer::default());
        assert_eq!(minimised.screen_size(), [1, 1]);
        let not_open_yet = Frame::new(0.0).for_window(None, DrawBuffer::default());
        assert_eq!(not_open_yet.screen_size(), [1920, 1080]);
    }
}
