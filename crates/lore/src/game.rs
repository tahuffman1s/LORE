//! The interface a game implements to be driven by the engine.

use lore_input::{ActionMap, ActionSetId, Device, GamepadStyle, InputFrame, Rumble};
use lore_render::{FluidDrop, FluidLook, Lightning, PoolDrawn, Splash, Stir, Tread};
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

    /// Called when the game's level was saved again while the game runs
    /// (see [`Config::level_folder`](crate::Config::level_folder)), with
    /// the new level. What is drawn has already changed; a game that
    /// walks its level makes the new shapes solid here. The default does
    /// nothing.
    fn level_saved(&mut self, level: &lore_assets::Scene) {
        let _ = level;
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

    fn level_saved(&mut self, level: &lore_assets::Scene) {
        (**self).level_saved(level);
    }
}

/// The size a frame says the window is when there is no window: in
/// `lore check` and in tests. It is the size HUD and menu sizes are written for.
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
    /// How far the wheel was rolled over the free pointer, in notches.
    wheel: f32,
    /// What the player last used, and whose gamepad it is.
    device: Device,
    gamepad_style: GamepadStyle,
    /// The controls, in the one frame after they changed.
    controls: Option<ActionMap>,
    /// What reading the controls again came to, in the one frame after.
    controls_change: Option<lore_script::ControlsChange>,
    /// Whether the game turned the engine's notice of that off.
    controls_notice_hidden: bool,
    /// The player's own size for the HUD and menus.
    ui_scale: f32,
    /// How fast game time runs, if the game said this frame.
    time_speed: Option<f32>,
    /// The player's settings in force, if whoever runs the game said them,
    /// with what the game changed in this frame.
    settings: Option<Settings>,
    /// Whether the game changed them in this frame.
    settings_changed: bool,
    ui: DrawBuffer,
    title: Option<String>,
    close: bool,
    view: Option<View>,
    action_set: Option<ActionSetId>,
    /// The game's actions, when a change to the game made new ones.
    new_actions: Option<ActionMap>,
    rumble: Option<Rumble>,
    /// The sounds to start and stop. Empty, and so without memory of its
    /// own, in a frame that asks for none.
    sounds: Vec<SoundAsk>,
    /// What went into the water, what is in it, and the bubbles let go
    /// under it, for the picture. Empty, and so without memory of their
    /// own, in a frame with none.
    splashes: Vec<Splash>,
    stirs: Vec<Stir>,
    bubbles: Vec<([f32; 3], u32)>,
    /// The lightning let go this frame, and how stormy the sky was said
    /// to be, if it was said.
    strikes: Vec<Lightning>,
    /// Where the snow was said to be pressed down this frame.
    treads: Vec<Tread>,
    storminess: Option<f32>,
    /// The hour of the day, if the game said it this frame.
    hour: Option<f64>,
    /// Where the world lies on the Earth and when, if the game said it
    /// this frame.
    sky_place: Option<lore_render::SkyPlace>,
    /// How large the moon is drawn, if the game said it this frame.
    moon_size: Option<f32>,
    /// The game's HUD images, if they changed this frame.
    hud_images: Option<Vec<lore_render::HudImage>>,
    /// Whether dark ground is drawn below the horizon, if the game said it
    /// this frame.
    ground: Option<bool>,
    /// What colour the moon is tinted, if the game said it this frame.
    moon_tint: Option<[f32; 3]>,
    /// How bright and what colour the Milky Way is, if the game said it
    /// this frame.
    milky_way: Option<(f32, [f32; 3])>,
    /// How much towns light the night sky, if the game said it this frame.
    light_pollution: Option<f32>,
    /// How stormy the aurora is, if the game said it this frame.
    aurora: Option<f32>,
    /// How many times the real rate meteors cross the sky, if the game said
    /// it this frame.
    meteors: Option<f32>,
    /// Every lamp lit this frame, if the game said them.
    lamps: Vec<lore_render::Lamp>,
    lamps_said: bool,
    /// The bodies to draw this frame, and the skinning matrices they use.
    figures: Vec<Figure>,
    figure_joints: Vec<lore_assets::Transform>,
    /// The game's own shaders, kept between frames.
    shaders: crate::game_shaders::ShaderList,
    /// What is wrong with the level file as it was last saved, if it could
    /// not be read: the level that was showing is kept.
    level_problem: Option<std::rc::Rc<str>>,
    /// The game's own exposure in stops and how the eye adapts, if the
    /// game said them this frame.
    lighting: Option<(f32, lore_render::EyeAdaptation)>,
    /// How dark the shade in corners is drawn and how far round it is
    /// searched for, in metres, if the game said them this frame.
    occlusion: Option<(f32, f32)>,
    /// How the picture is graded and how many seconds to fade to it over,
    /// if the game said it this frame.
    look: Option<(lore_render::Look, f32)>,
    /// How far rain has raised the water and what the sky's clock shows,
    /// if the game said them this frame.
    water_rise: Option<f32>,
    sky_clock: Option<f64>,
    /// How wet the ground is and how full its puddles, if the game said
    /// this frame.
    ground_water: Option<lore_render::GroundWater>,
    /// Whether storms send their own bolts, if the game said this frame.
    storm_bolts: Option<bool>,
    /// The whole of the weather, if the game said it this frame.
    weather: Option<lore_render::Weather>,
    /// Whether the ears are under water, if the game said.
    under_water: Option<bool>,
    /// The fluid to draw, if the game said it this frame, the looks of its
    /// kinds and of the level's water, when they changed.
    fluid: FluidFrame,
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

/// A model drawn this frame: which, where, bent by which joints of
/// [`Frame::figures`], and whether it is seen or only casts its shadow.
#[derive(Debug, Clone)]
pub(crate) struct Figure {
    pub(crate) model: std::sync::Arc<lore_assets::Scene>,
    pub(crate) transform: lore_assets::Transform,
    pub(crate) joints: std::ops::Range<usize>,
    pub(crate) seen: bool,
    pub(crate) shadows: bool,
}

/// Where the world is seen from: the eye's position, then which way it
/// faces, as [`Frame::look_from`] takes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct View {
    pub(crate) position: [f32; 3],
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    /// How much is seen from the bottom of the picture to the top, in
    /// degrees, when the view has its own: the editor's camera does. With
    /// `None` it is the player's setting.
    pub(crate) fov_degrees: Option<f32>,
    /// A script's lens: a field of view in degrees, and how far from the
    /// view's own toward it the picture is, from 0 to 1.
    pub(crate) lens: Option<(f32, f32)>,
}

impl Default for Frame {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl Frame {
    /// A frame that is `alpha` of the way from the last tick to the next.
    pub(crate) fn new(alpha: f32) -> Self {
        Self {
            alpha,
            since_tick: 0.0,
            input: InputFrame::default(),
            seconds: 0.0,
            screen: NO_WINDOW,
            pointer: None,
            pointer_down: false,
            wheel: 0.0,
            device: Device::KeyboardAndMouse,
            gamepad_style: GamepadStyle::Xbox,
            controls: None,
            controls_change: None,
            controls_notice_hidden: false,
            ui_scale: 1.0,
            time_speed: None,
            settings: None,
            settings_changed: false,
            ui: DrawBuffer::default(),
            title: None,
            close: false,
            view: None,
            action_set: None,
            new_actions: None,
            rumble: None,
            sounds: Vec::new(),
            splashes: Vec::new(),
            stirs: Vec::new(),
            bubbles: Vec::new(),
            strikes: Vec::new(),
            treads: Vec::new(),
            storminess: None,
            hour: None,
            sky_place: None,
            moon_size: None,
            hud_images: None,
            ground: None,
            moon_tint: None,
            milky_way: None,
            light_pollution: None,
            aurora: None,
            meteors: None,
            lamps: Vec::new(),
            lamps_said: false,
            figures: Vec::new(),
            figure_joints: Vec::new(),
            shaders: crate::game_shaders::ShaderList::default(),
            level_problem: None,
            lighting: None,
            occlusion: None,
            look: None,
            water_rise: None,
            sky_clock: None,
            ground_water: None,
            storm_bolts: None,
            weather: None,
            under_water: None,
            fluid: FluidFrame::default(),
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
    /// rate. A game run with no window has no player at the keyboard, and
    /// every action here reads as idle.
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
    /// A game run with no window draws no frames; there this is 0.
    #[must_use]
    pub fn delta_seconds(&self) -> f32 {
        self.seconds
    }

    /// Where the mouse pointer is, in pixels from the top-left corner of
    /// the window: what a menu's [`UiInput`](lore_ui::UiInput) takes as
    /// its `pointer`.
    ///
    /// `None` while the mouse is captured to look around with, while the
    /// pointer is outside the window, and in a game run with no window.
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

    /// How far the mouse wheel was rolled since the last frame, in
    /// notches: what a menu's [`UiInput`](lore_ui::UiInput) takes as its
    /// `wheel`, to scroll a screen longer than the window. Above zero is
    /// away from the player. A touchpad gives parts of a notch.
    ///
    /// Like a click, this is not an action: only a mouse has a wheel,
    /// and a gamepad scrolls a menu by moving through it. Zero while the
    /// mouse is captured, where the wheel is a button an action can be
    /// bound to (`MouseWheelUp`).
    #[must_use]
    pub fn wheel(&self) -> f32 {
        self.wheel
    }

    /// What the player last used on purpose: the keyboard and mouse, or
    /// a gamepad. Show the names of that device's buttons in hints and
    /// tutorials, and they change the moment the player picks up the
    /// other one.
    ///
    /// A stick that drifts or a mouse on a bumped desk does not count;
    /// see [`Input::last_device`](lore_input::Input::last_device). With
    /// no window it is always the keyboard and mouse.
    #[must_use]
    pub fn device(&self) -> Device {
        self.device
    }

    /// Whose letters are on the gamepad the player is using, so a hint
    /// can say "A", "Cross" or "B" for the same button:
    /// [`GamepadButton::label`](lore_input::GamepadButton::label) takes
    /// it. A gamepad that is not known to be a PlayStation or Nintendo
    /// one is taken to be lettered like an Xbox one, as most are.
    #[must_use]
    pub fn gamepad_style(&self) -> GamepadStyle {
        self.gamepad_style
    }

    /// Says what the player last used and whose gamepad it is.
    pub(crate) fn with_device(mut self, device: Device, style: GamepadStyle) -> Self {
        self.device = device;
        self.gamepad_style = style;
        self
    }

    /// The player's controls as they now are, in the one frame after their
    /// bindings file was saved and read again while the game runs
    /// ([`Config::controls_file`](crate::Config)). `None` in every other
    /// frame.
    ///
    /// The engine has already put them into effect, and every
    /// [`ActionId`](lore_input::ActionId) means what it did. This is for a
    /// game that shows which key or button does something, to read the
    /// names again:
    ///
    /// ```
    /// # fn frame(frame: &mut lore::Frame, prompt: &mut String) {
    /// if let Some(controls) = frame.new_controls() {
    ///     if let Ok(jump) = controls.action("jump") {
    ///         *prompt = format!("{:?}", controls.bindings(jump));
    ///     }
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn new_controls(&self) -> Option<&ActionMap> {
        self.controls.as_ref()
    }

    pub(crate) fn with_controls(mut self, controls: Option<ActionMap>) -> Self {
        self.controls = controls;
        self
    }

    /// What reading the player's bindings file again came to, in the one
    /// frame after: the actions that changed, or what is wrong with the file.
    /// `None` in every other frame.
    #[must_use]
    pub fn controls_change(&self) -> Option<&lore_script::ControlsChange> {
        self.controls_change.as_ref()
    }

    pub(crate) fn with_controls_change(
        mut self,
        change: Option<lore_script::ControlsChange>,
    ) -> Self {
        self.controls_change = change;
        self
    }

    /// Turns the engine's own notice of a reread of the controls off (or
    /// back on), for a game that shows it its own way.
    pub fn hide_controls_notice(&mut self, hidden: bool) {
        self.controls_notice_hidden = hidden;
    }

    /// Whether the game turned the engine's notice of the controls off.
    pub(crate) fn controls_notice_hidden(&self) -> bool {
        self.controls_notice_hidden
    }

    /// Says how far the wheel was rolled over the free pointer.
    pub(crate) fn with_wheel(mut self, notches: f32) -> Self {
        self.wheel = notches;
        self
    }

    /// The size the player chose for the HUD and menus in their settings:
    /// 1 is the designed size. Give it to
    /// [`Ui::set_ui_scale`] so a game's own HUD grows and shrinks with
    /// a script's.
    #[must_use]
    pub fn ui_scale(&self) -> f32 {
        self.ui_scale
    }

    /// Says how long the frame before this one took, and where the free
    /// mouse pointer is and whether its main button is held.
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
    pub(crate) fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
    }

    /// Says how fast game time runs from now on: the window's clock runs
    /// that many ticks for each one real time asks for.
    pub(crate) fn set_time_speed(&mut self, speed: f32) {
        self.time_speed = Some(speed);
    }

    /// How fast the game said time runs, if it said this frame.
    pub(crate) fn time_speed(&self) -> Option<f32> {
        self.time_speed
    }

    /// Says the player's settings in force, for the game to read and change.
    pub(crate) fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = Some(settings);
        self
    }

    /// The player's settings in force: as the player last chose them, with
    /// what the game changed earlier in this frame. A settings menu shows
    /// these. `None` in a frame drawn with no window, unless the game
    /// changed them in it.
    #[must_use]
    pub fn settings(&self) -> Option<&Settings> {
        self.settings.as_ref()
    }

    /// Changes the player's settings, as a game's settings menu does:
    /// `change` is handed the settings in force and changes what the player
    /// asked for. The engine puts them into effect (how loud the speakers
    /// are, how the view turns and how the picture is drawn) and saves them
    /// to the player's file. What `change` leaves alone stays as it is in
    /// force. In a frame drawn with no window, `change` is handed a new
    /// player's settings and nothing is put into effect.
    ///
    /// ```
    /// use lore::{Frame, Game, InputFrame};
    ///
    /// /// A game whose own menu has a row that turns the music off.
    /// struct MyGame {
    ///     /// The player pressed that row in the last tick.
    ///     music_off: bool,
    /// }
    ///
    /// impl Game for MyGame {
    ///     fn tick(&mut self, _: &InputFrame, _: f32) {}
    ///
    ///     fn frame(&mut self, frame: &mut Frame) {
    ///         if std::mem::take(&mut self.music_off) {
    ///             frame.change_settings(|settings| settings.music_volume = 0.0);
    ///         }
    ///     }
    /// }
    /// ```
    pub fn change_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        change(self.settings.get_or_insert_with(Settings::default));
        self.settings_changed = true;
    }

    /// The settings in force after the game changed them this frame, if it
    /// did. The engine takes them from the game it runs; this is here for a
    /// tool that draws a game with no window.
    pub fn take_settings(&mut self) -> Option<Settings> {
        std::mem::take(&mut self.settings_changed)
            .then_some(self.settings)
            .flatten()
    }

    /// The size of the window in pixels, `[width, height]`: what a
    /// [`Ui`] is updated with so that it fills the window.
    ///
    /// For a game run with no window this is 1920 by 1080.
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
    /// In a game run with no window nothing is drawn.
    pub fn draw_ui(&mut self, ui: &Ui) {
        self.ui.add(ui.draw());
    }

    /// Starts the frame for a window of `screen` pixels, if there is one yet, with `buffer` to
    /// gather the UI in: the last frame's, so its memory is used again. The
    /// engine does this for the game it runs; it is here for a tool that
    /// draws a game's HUD with no window.
    #[must_use]
    pub fn for_window(mut self, screen: Option<[u32; 2]>, mut buffer: DrawBuffer) -> Self {
        buffer.clear();
        // A minimised window has no size; a UI still needs one to lay out in.
        self.screen = screen.map_or(NO_WINDOW, |[width, height]| [width.max(1), height.max(1)]);
        self.ui = buffer;
        self
    }

    /// Everything the game asked to be drawn over it this frame.
    pub fn take_ui(&mut self) -> DrawBuffer {
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
    /// (positive) or down. These are the angles the player physics uses. In
    /// a game run with no window nothing happens.
    pub fn look_from(&mut self, position: [f32; 3], yaw: f32, pitch: f32) {
        self.view = Some(View {
            position,
            yaw,
            pitch,
            fov_degrees: None,
            lens: None,
        });
    }

    /// Draws the world as [`look_from`](Self::look_from) does, through a
    /// lens: `weight` of the way (0 to 1) from the player's field of view
    /// to `degrees`, as a script's shot asks while the view travels to it.
    pub(crate) fn look_from_through(
        &mut self,
        position: [f32; 3],
        yaw: f32,
        pitch: f32,
        (degrees, weight): (f32, f32),
    ) {
        self.view = Some(View {
            position,
            yaw,
            pitch,
            fov_degrees: None,
            lens: (weight > 0.0).then_some((degrees, weight.min(1.0))),
        });
    }

    /// Draws the world as [`look_from`](Self::look_from) does, seeing
    /// `fov_degrees` from the bottom of the picture to the top whatever the
    /// player's field of view: for the editor's camera, which is a tool,
    /// not the player's eyes.
    #[cfg(feature = "editor")]
    pub(crate) fn look_from_with_fov(
        &mut self,
        position: [f32; 3],
        yaw: f32,
        pitch: f32,
        fov_degrees: f32,
    ) {
        self.view = Some(View {
            position,
            yaw,
            pitch,
            fov_degrees: Some(fov_degrees),
            lens: None,
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
    /// call this every frame. A game run with no window has no player at
    /// the keyboard, so nothing happens there.
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
    /// play it as well. A game run with no window has no gamepad, so nothing
    /// happens there. The player's rumble setting scales what is asked for
    /// here, down to nothing: it is theirs, and no game gets round it.
    pub fn rumble(&mut self, rumble: Rumble) {
        self.rumble = Some(rumble);
    }

    /// Closes the window and ends the game after this frame, as if the
    /// player had closed it. In a game run with no window nothing happens.
    pub fn close(&mut self) {
        self.close = true;
    }

    /// Whether the game asked to end this frame.
    pub(crate) fn closing(&self) -> bool {
        self.close
    }

    /// Takes back a [`close`](Self::close): whether the game asked to end
    /// this frame, which it then has not. The editor does this for the
    /// game it plays, which ends by going back to editing.
    #[cfg(feature = "editor")]
    pub(crate) fn take_close(&mut self) -> bool {
        std::mem::take(&mut self.close)
    }

    /// Where the game asked the world to be seen from this frame, if it did.
    pub(crate) fn view(&self) -> Option<View> {
        self.view
    }

    /// Replaces the game's own actions and action sets while it runs, as a
    /// saved script does when it adds, changes or removes one.
    ///
    /// `actions` is the game's map: the standard controls and the game's own
    /// on top, as they were first given in [`Config::actions`](crate::Config).
    /// The window reads the player's bindings file onto it again and puts the
    /// result into use, so what the player rebound stays rebound. The frame
    /// after, [`new_controls`](Self::new_controls) has the map. Ids of the
    /// standard actions stay as they were as long as the game only adds
    /// actions after them. A game run with no window ignores this.
    pub fn replace_actions(&mut self, actions: ActionMap) {
        self.new_actions = Some(actions);
    }

    /// The actions a game replaced this frame, if it did.
    pub(crate) fn take_new_actions(&mut self) -> Option<ActionMap> {
        self.new_actions.take()
    }

    /// The action set requested this frame, if any.
    pub(crate) fn action_set(&self) -> Option<ActionSetId> {
        self.action_set
    }

    /// The rumble requested this frame, if any.
    pub(crate) fn rumble_requested(&self) -> Option<Rumble> {
        self.rumble
    }

    /// Starts or stops a sound for the player at this computer. A game run
    /// with no window has no speakers, so nothing happens there.
    pub(crate) fn sound(&mut self, ask: SoundAsk) {
        self.sounds.push(ask);
    }

    /// Says that something went into the water, or pushed through its
    /// surface, since the last frame: drops fly up from where it did,
    /// spray hangs for a moment, bubbles are dragged under, and rings run
    /// out across the water. The player's own splashes are shown without
    /// being asked for; this is for whatever else a game drops in.
    ///
    /// ```
    /// use lore::render::Splash;
    ///
    /// fn stone_lands(frame: &mut lore::Frame) {
    ///     frame.splash(Splash {
    ///         place: [4.0, 0.0, -7.5],
    ///         moving: [1.0, -6.0, 0.0],
    ///         wide: 0.08,
    ///     });
    /// }
    /// ```
    pub fn splash(&mut self, splash: Splash) {
        self.splashes.push(splash);
    }

    /// Says that something is in the water during this frame: rings run
    /// out from it, and it trails a wake as it moves. Say it every frame
    /// for as long as the thing is there. The player is there already.
    pub fn stir(&mut self, stir: Stir) {
        self.stirs.push(stir);
    }

    /// Presses the snow down at a place: a foot coming down in it, or a
    /// body wading through. The track stays until new snow has filled it
    /// in; nothing is seen of it where no snow lies. The player's own
    /// tracks are made already.
    pub fn tread(&mut self, tread: Tread) {
        self.treads.push(tread);
    }

    /// Lets `count` bubbles go at `place`, which is under water: they rise
    /// and are gone at the surface.
    pub fn bubbles(&mut self, place: [f32; 3], count: u32) {
        self.bubbles.push((place, count));
    }

    /// Says whether the ears are under water this frame: with them under,
    /// everything that is heard is muffled.
    pub(crate) fn hear_under_water(&mut self, under: bool) {
        self.under_water = Some(under);
    }

    /// Where the snow was said to be pressed down this frame.
    pub(crate) fn take_treads(&mut self) -> Vec<Tread> {
        std::mem::take(&mut self.treads)
    }

    /// The splashes said this frame, in order.
    pub(crate) fn take_splashes(&mut self) -> Vec<Splash> {
        std::mem::take(&mut self.splashes)
    }

    /// What was said to be in the water this frame.
    pub(crate) fn take_stirs(&mut self) -> Vec<Stir> {
        std::mem::take(&mut self.stirs)
    }

    /// Lets a flash of lightning go: a forked bolt made from its seed that
    /// comes down, flickers and is gone in under a second, lighting the
    /// clouds and the land. `Lightning::down_to` makes one that strikes a
    /// place on the ground.
    pub fn strike(&mut self, lightning: Lightning) {
        self.strikes.push(lightning);
    }

    /// Says how stormy the sky is, from 0, fair weather with the odd storm
    /// passing, to 1, storms over most of it. Under a storm it rains, the
    /// wind blows harder and lightning strikes by itself.
    pub fn set_storminess(&mut self, storminess: f32) {
        self.storminess = Some(storminess);
    }

    /// Says what hour of the day it is: 12 is noon, 18 the sun setting, 24
    /// midnight, and hours past 24 run on into the next day. The sun and
    /// the moon stand where that hour has them, night falls and the stars
    /// come out. Until a game says the hour it is always day.
    pub fn set_hour(&mut self, hour: f64) {
        self.hour = Some(hour);
    }

    /// The hour of the day, if the game said it this frame.
    pub(crate) fn take_hour(&mut self) -> Option<f64> {
        self.hour.take()
    }

    /// Says where on the Earth the world lies, which way north is in the
    /// level and on what day hour 0 was midnight: the sun, the moon and the
    /// stars then stand where they really would there, the hour being the
    /// sun's own time. Until a game says it the sky is a made-up one.
    pub fn set_sky_place(&mut self, place: lore_render::SkyPlace) {
        self.sky_place = Some(place);
    }

    /// Says how large the moon is drawn against its real size, from 1 to
    /// 10. Its light on the land does not change.
    pub fn set_moon_size(&mut self, size: f32) {
        self.moon_size = Some(size);
    }

    /// How large the moon is drawn, if the game said it this frame.
    pub(crate) fn take_moon_size(&mut self) -> Option<f32> {
        self.moon_size.take()
    }

    /// Hands over the game's HUD images: what a HUD image piece numbered
    /// `n` shows is the `n`th. Said when they change, not every frame.
    pub fn set_hud_images(&mut self, images: Vec<lore_render::HudImage>) {
        self.hud_images = Some(images);
    }

    /// The game's HUD images, if they changed this frame.
    pub(crate) fn take_hud_images(&mut self) -> Option<Vec<lore_render::HudImage>> {
        self.hud_images.take()
    }

    /// Says whether dark ground is drawn below the horizon (the usual), or
    /// the sky goes all the way round, its stars below the player's feet.
    pub fn set_ground(&mut self, shown: bool) {
        self.ground = Some(shown);
    }

    /// Whether dark ground is drawn below the horizon, if the game said it
    /// this frame.
    pub(crate) fn take_ground(&mut self) -> Option<bool> {
        self.ground.take()
    }

    /// Says how the Milky Way is drawn: `times` its real brightness, from
    /// 0 (not at all) to 10, tinted `tint` in linear red, green and blue
    /// (`[1.0; 3]` is its own colour).
    pub fn set_milky_way(&mut self, times: f32, tint: [f32; 3]) {
        self.milky_way = Some((times, tint));
    }

    /// How the Milky Way is drawn, if the game said it this frame.
    pub(crate) fn take_milky_way(&mut self) -> Option<(f32, [f32; 3])> {
        self.milky_way.take()
    }

    /// Says how much towns light the night sky, on Bortle's scale from 1
    /// (none, the usual) to 9 (an inner city).
    pub fn set_light_pollution(&mut self, class: f32) {
        self.light_pollution = Some(class);
    }

    /// How much towns light the night sky, if the game said it this frame.
    pub(crate) fn take_light_pollution(&mut self) -> Option<f32> {
        self.light_pollution.take()
    }

    /// Says how stormy the aurora is: the Kp index, from 0 (none) to 9.
    pub fn set_aurora(&mut self, kp: f32) {
        self.aurora = Some(kp);
    }

    /// How stormy the aurora is, if the game said it this frame.
    pub(crate) fn take_aurora(&mut self) -> Option<f32> {
        self.aurora.take()
    }

    /// Says how many times the real rate meteors cross the sky: 0 for
    /// none, 1 for the real one, up to 1000.
    pub fn set_meteors(&mut self, times: f32) {
        self.meteors = Some(times);
    }

    /// How many times the real rate meteors cross the sky, if the game said
    /// it this frame.
    pub(crate) fn take_meteors(&mut self) -> Option<f32> {
        self.meteors.take()
    }

    /// Says what colour the moon is tinted, in linear red, green and blue:
    /// its face, the glow round it and its light on the land.
    pub fn set_moon_tint(&mut self, tint: [f32; 3]) {
        self.moon_tint = Some(tint);
    }

    /// What colour the moon is tinted, if the game said it this frame.
    pub(crate) fn take_moon_tint(&mut self) -> Option<[f32; 3]> {
        self.moon_tint.take()
    }

    /// Where the world lies, if the game said it this frame.
    pub(crate) fn take_sky_place(&mut self) -> Option<lore_render::SkyPlace> {
        self.sky_place.take()
    }

    /// Says which lamps light the world from this frame on: all of them,
    /// the level's as the game has left them and its own. Until a game
    /// says, the level's lamps are lit as its file places them. Each is
    /// as bright as a real lamp of its [`candela`](lore_render::Lamp::candela)
    /// against the sun, the sky and the moon of the hour.
    pub fn set_lamps(&mut self, lamps: &[lore_render::Lamp]) {
        self.lamps.clear();
        self.lamps.extend_from_slice(lamps);
        self.lamps_said = true;
    }

    /// Says how the picture is exposed for the light from this frame on:
    /// `stops` brighter (or, below 0, darker) than the eye would see it, on
    /// top of the player's brightness, and how the eye adapts (see
    /// [`lore_render::EyeAdaptation`]). Until a game says, it is 0 and
    /// [`EyeAdaptation::default`](lore_render::EyeAdaptation::default).
    pub fn set_lighting(&mut self, stops: f32, eye: lore_render::EyeAdaptation) {
        self.lighting = Some((stops, eye));
    }

    /// How the picture is exposed, if the game said it this frame.
    pub fn take_lighting(&mut self) -> Option<(f32, lore_render::EyeAdaptation)> {
        self.lighting.take()
    }

    /// Says how the soft shade in corners and under things (ambient
    /// occlusion) is drawn from this frame on: `strength` is how dark, from
    /// 0 (none) to 4, where 1 is as much as really arrives, and `radius`
    /// how far round each point is searched for what shades it, in metres,
    /// from 0.05 to 4. Until a game says, they are 1 and 0.6. The player's
    /// own graphics setting says whether it is drawn at all.
    pub fn set_ambient_occlusion(&mut self, strength: f32, radius: f32) {
        self.occlusion = Some((strength, radius));
    }

    /// How the shade in corners is drawn, if the game said it this frame:
    /// its strength, then its radius in metres.
    pub fn take_ambient_occlusion(&mut self) -> Option<(f32, f32)> {
        self.occlusion.take()
    }

    /// Says how the picture is graded from this frame on (see
    /// [`lore_render::Look`]), fading to it from the look before over
    /// `seconds`. Until a game says, it is [`Look::default`](lore_render::Look::default).
    pub fn set_look(&mut self, look: lore_render::Look, seconds: f32) {
        self.look = Some((look, seconds));
    }

    /// How the picture is graded, if the game said it this frame.
    pub fn take_look(&mut self) -> Option<(lore_render::Look, f32)> {
        self.look.take()
    }

    /// Draws `model` this frame as `drawn` places and bends it: the
    /// player's body, as seen and as it throws its shadow.
    pub(crate) fn show_figure(
        &mut self,
        model: &std::sync::Arc<lore_assets::Scene>,
        drawn: &crate::character::Drawn<'_>,
    ) {
        let seen_start = self.figure_joints.len();
        self.figure_joints.extend_from_slice(drawn.seen_joints);
        let shadow_start = self.figure_joints.len();
        self.figure_joints.extend_from_slice(drawn.shadow_joints);
        let end = self.figure_joints.len();
        if drawn.seen {
            self.figures.push(Figure {
                model: std::sync::Arc::clone(model),
                transform: drawn.transform,
                joints: seen_start..shadow_start,
                seen: true,
                shadows: false,
            });
        }
        self.figures.push(Figure {
            model: std::sync::Arc::clone(model),
            transform: drawn.transform,
            joints: shadow_start..end,
            seen: false,
            shadows: true,
        });
    }

    /// Draws `model` this frame at `transform` with nothing bending it:
    /// what a character holds. It casts a shadow whether or not it is
    /// `seen` in the picture.
    pub(crate) fn show_thing(
        &mut self,
        model: &std::sync::Arc<lore_assets::Scene>,
        transform: lore_assets::Transform,
        seen: bool,
    ) {
        let none = self.figure_joints.len();
        self.figures.push(Figure {
            model: std::sync::Arc::clone(model),
            transform,
            joints: none..none,
            seen,
            shadows: true,
        });
    }

    /// The bodies drawn this frame and the joints they index into.
    pub(crate) fn figures(&self) -> (&[Figure], &[lore_assets::Transform]) {
        (&self.figures, &self.figure_joints)
    }

    /// Lends the frame the memory of an earlier frame's bodies.
    pub(crate) fn with_figure_buffers(
        mut self,
        (figures, joints): (Vec<Figure>, Vec<lore_assets::Transform>),
    ) -> Self {
        self.figures = figures;
        self.figures.clear();
        self.figure_joints = joints;
        self.figure_joints.clear();
        self
    }

    /// The memory the bodies were said in, to lend the next frame.
    pub(crate) fn figure_buffers(&mut self) -> (Vec<Figure>, Vec<lore_assets::Transform>) {
        (
            std::mem::take(&mut self.figures),
            std::mem::take(&mut self.figure_joints),
        )
    }

    /// Lends the frame the memory of an earlier frame's lamps, so that
    /// saying them needs no new memory.
    #[must_use]
    pub(crate) fn with_lamp_buffer(mut self, mut buffer: Vec<lore_render::Lamp>) -> Self {
        buffer.clear();
        self.lamps = buffer;
        self
    }

    /// Says which lamps light the world from this frame on, as
    /// [`set_lamps`](Self::set_lamps) does, by filling the list this gives.
    pub(crate) fn say_lamps(&mut self) -> &mut Vec<lore_render::Lamp> {
        self.lamps.clear();
        self.lamps_said = true;
        &mut self.lamps
    }

    /// The memory the lamps were said in, to lend the next frame.
    pub(crate) fn lamp_buffer(&mut self) -> Vec<lore_render::Lamp> {
        std::mem::take(&mut self.lamps)
    }

    /// Lends the frame the game's shaders as they were left, so that
    /// following the script's takes no new memory.
    #[must_use]
    pub(crate) fn with_shader_list(mut self, list: crate::game_shaders::ShaderList) -> Self {
        self.shaders = list;
        self
    }

    /// The game's shaders, for the game to follow its script's.
    /// Says what is wrong with the level file as last saved, for this frame.
    #[must_use]
    pub(crate) fn with_level_problem(mut self, problem: Option<std::rc::Rc<str>>) -> Self {
        self.level_problem = problem;
        self
    }

    /// What is wrong with the level file as it was last saved, if it could
    /// not be read.
    pub(crate) fn level_problem(&self) -> Option<&std::rc::Rc<str>> {
        self.level_problem.as_ref()
    }

    pub(crate) fn shader_list(&mut self) -> &mut crate::game_shaders::ShaderList {
        &mut self.shaders
    }

    /// The game's shaders, to lend the next frame and to hand the renderer.
    pub(crate) fn take_shader_list(&mut self) -> crate::game_shaders::ShaderList {
        std::mem::take(&mut self.shaders)
    }

    /// The lamps, if the game said them this frame.
    pub fn take_lamps(&mut self) -> Option<&[lore_render::Lamp]> {
        std::mem::take(&mut self.lamps_said).then_some(self.lamps.as_slice())
    }

    /// Says how far rain has raised the level's still water, in metres, so
    /// that it is drawn as high as the game's rules have it.
    pub fn set_water_rise(&mut self, rise: f32) {
        self.water_rise = Some(rise);
    }

    /// Says how wet the ground is and how full its puddles, as the game's
    /// rules count them from the rain that has fallen: from then on they
    /// are drawn as the rules have them, and not counted by the picture.
    pub fn set_ground_water(&mut self, ground: lore_render::GroundWater) {
        self.ground_water = Some(ground);
    }

    /// How wet the ground is and how full its puddles, if the game said
    /// this frame.
    pub(crate) fn take_ground_water(&mut self) -> Option<lore_render::GroundWater> {
        self.ground_water.take()
    }

    /// Says what time the sky's clock shows, in seconds of the game's own
    /// time: clouds, storms and rain are then where the game's rules have
    /// them.
    pub fn set_sky_clock(&mut self, seconds: f64) {
        self.sky_clock = Some(seconds);
    }

    /// Says whether storms send their own bolts of lightning to the ground
    /// (as they do until told otherwise) or the game sends each itself with
    /// [`strike`](Self::strike), knowing where it falls.
    pub fn set_storm_bolts(&mut self, own: bool) {
        self.storm_bolts = Some(own);
    }

    /// Whether storms send their own bolts, if the game said this frame.
    pub(crate) fn take_storm_bolts(&mut self) -> Option<bool> {
        self.storm_bolts.take()
    }

    /// Says what the weather is, all of it: how much cloud, how stormy,
    /// how hard it rains or snows, how warm the air is, the wind, the fog,
    /// the snow that lies and the ice on the water, and the tornado if
    /// there is one. See [`lore_render::Weather`]. What is said last in a
    /// frame is what is drawn; it takes the place of
    /// [`set_storminess`](Self::set_storminess).
    pub fn set_weather(&mut self, weather: lore_render::Weather) {
        self.weather = Some(weather);
    }

    /// The weather, if the game said it this frame.
    pub(crate) fn take_weather(&mut self) -> Option<lore_render::Weather> {
        self.weather.take()
    }

    /// How far rain has raised the water and what the sky's clock shows,
    /// if the game said them this frame.
    pub(crate) fn take_water(&mut self) -> (Option<f32>, Option<f64>) {
        (self.water_rise.take(), self.sky_clock.take())
    }

    /// The lightning let go this frame.
    pub(crate) fn take_strikes(&mut self) -> Vec<Lightning> {
        std::mem::take(&mut self.strikes)
    }

    /// How stormy the sky was said to be this frame, if it was said.
    pub(crate) fn take_storminess(&mut self) -> Option<f32> {
        self.storminess.take()
    }

    /// The bubbles let go this frame: where, and how many.
    pub(crate) fn take_bubbles(&mut self) -> Vec<([f32; 3], u32)> {
        std::mem::take(&mut self.bubbles)
    }

    /// Whether the ears are under water, if the game said this frame.
    pub(crate) fn under_water(&self) -> Option<bool> {
        self.under_water
    }

    /// The sounds asked for this frame, in order.
    pub(crate) fn take_sounds(&mut self) -> Vec<SoundAsk> {
        std::mem::take(&mut self.sounds)
    }

    /// The title requested this frame, if any.
    pub(crate) fn take_title(&mut self) -> Option<String> {
        self.title.take()
    }

    /// Lends the frame `buffer` to gather the fluid's drops in: the last
    /// frame's, from [`take_fluid`](Self::take_fluid), so its memory is used
    /// again and drawing fluid allocates nothing once it has grown.
    #[must_use]
    pub fn with_fluid_buffer(mut self, mut buffer: Vec<FluidDrop>) -> Self {
        buffer.clear();
        self.fluid.drops = buffer;
        self
    }

    /// Says which drops of fluid are drawn, until it is said again: every
    /// drop in the world, flying and lying. An empty list draws none.
    pub fn set_fluid(&mut self, drops: impl IntoIterator<Item = FluidDrop>) {
        self.fluid.drops.clear();
        self.fluid.drops.extend(drops);
        self.fluid.said = true;
    }

    /// Says how each kind of fluid looks: the place in the list is the kind.
    /// Say it when it changes; the picture keeps it until then.
    /// Kinds past the 32 the picture draws are left out.
    pub fn set_fluid_looks(&mut self, looks: &[FluidLook]) {
        let mut all = [FluidLook::default(); lore_render::MOST_FLUID_KINDS];
        for (slot, look) in all.iter_mut().zip(looks) {
            *slot = *look;
        }
        // Kept in the frame itself: a script that changes a colour on
        // every tick costs no memory.
        self.fluid.looks = Some(all);
    }

    /// Lends the frame `buffer` to gather how the pools stand in: the last
    /// frame's, from [`take_fluid`](Self::take_fluid), so that saying it
    /// every frame allocates nothing.
    #[must_use]
    pub fn with_pool_buffer(mut self, mut buffer: Vec<PoolDrawn>) -> Self {
        buffer.clear();
        self.fluid.pools = buffer;
        self
    }

    /// Says how each body of the level's still fluid stands and looks, in
    /// the order of the level's waters: how far from where its maker put
    /// it the surface is, and what it is drawn as. Say it every frame.
    pub fn set_pools(&mut self, pools: &[PoolDrawn]) {
        self.fluid.pools.clear();
        self.fluid.pools.extend_from_slice(pools);
        self.fluid.pools_said = true;
    }

    /// Says which hollows nobody marked have been found with fluid lying
    /// in them, and are pools since: they are drawn after the level's own
    /// waters, in this order, as [`set_pools`](Self::set_pools) says each
    /// stands. Say it when the list changes.
    pub fn set_found_waters(&mut self, found: &[std::sync::Arc<lore_assets::Water>]) {
        self.fluid.found = Some(found.to_vec());
    }

    /// Says how the level's own lakes and creeks are drawn: as `look`, or
    /// with `None` as the level says. Say it when it changes.
    pub fn set_water_look(&mut self, look: Option<FluidLook>) {
        self.fluid.water_look = Some(look);
    }

    /// What the game said of the fluid this frame, with the buffer it was
    /// lent back. Hand the buffer to the next frame with
    /// [`with_fluid_buffer`](Self::with_fluid_buffer).
    pub fn take_fluid(&mut self) -> FluidFrame {
        std::mem::take(&mut self.fluid)
    }
}

/// What a game said of the fluid in one frame, for whoever draws it.
///
/// Hand it to the picture in this order: the looks, the water's look, then
/// the drops, so that new drops are never drawn with an old look.
#[derive(Debug, Default)]
pub struct FluidFrame {
    /// The drops to draw, when [`said`](Self::said); otherwise the empty
    /// buffer, to lend to the next frame all the same.
    pub drops: Vec<FluidDrop>,
    /// Whether the drops were said this frame. When not, draw what was
    /// drawn before.
    pub said: bool,
    /// How each kind looks, by kind, if that changed: all 32, water's
    /// look for kinds the game has not made.
    pub looks: Option<[FluidLook; lore_render::MOST_FLUID_KINDS]>,
    /// How the level's water is drawn, if that changed: `Some(None)` is as
    /// the level says.
    pub water_look: Option<Option<FluidLook>>,
    /// How each pool stands and looks, when
    /// [`pools_said`](Self::pools_said); otherwise the empty buffer, to
    /// lend to the next frame all the same.
    pub pools: Vec<PoolDrawn>,
    /// Whether the pools were said this frame.
    pub pools_said: bool,
    /// The hollows found since the level was made, if that list changed:
    /// hand it to the picture before the pools.
    pub found: Option<Vec<std::sync::Arc<lore_assets::Water>>>,
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
                Draw::Rect { .. } | Draw::Turned { .. } => None,
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
