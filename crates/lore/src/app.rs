//! The window and the main loop.

use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use lore_input::{ActionMap, ActionSetId, Input, InputFrame, MouseMode, MouseWheel};
use lore_render::{Camera, RenderConfig, RenderError, Renderer};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, Ime, KeyEvent, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::controls;
use crate::controls_notice::{self, ControlsNotice};
use crate::game::{Frame, Game, View};
use crate::gamepad::Gamepads;
use crate::pace::{FramePacer, WATCH};
use crate::settings::{self, Settings};
use crate::sound_output::{LazySpeakers, SoundOutput};
use crate::tick::{self, Hitches, TickClock};
use crate::translate;

/// Where the eye is before a game says where to look from: as high as a
/// standing player's, in metres.
const STANDING_EYE: f32 = 1.6;

/// How tall the screen is in the HUD's own units, whatever its size in pixels.
const HUD_HEIGHT: f32 = 1080.0;

/// The smallest and largest size a `Ui` lays the HUD out at.
const UI_SCALES: (f32, f32) = (0.75, 1.5);

/// A view that moves further than this in one frame, in metres, has jumped
/// rather than moved. Falling at terminal speed at 20 frames a second is
/// under 3 m a frame.
const CUT_METRES: f32 = 5.0;

/// How many pixels of smooth scrolling count as one wheel notch.
const PIXELS_PER_NOTCH: f32 = 40.0;

/// The most wheel notches one scroll event may add up to.
const MOST_NOTCHES_PER_EVENT: f32 = 16.0;

/// What the log says after a level that could not be loaded again.
const KEEPS_LEVEL: &str = "The level that was showing is kept. Export it again to try again.";

/// The camera that draws `view`: at the view's own field of view if it has
/// one (the editor's camera), and otherwise at the player's.
fn camera(view: View, settings: &Settings) -> Camera {
    let own = view.fov_degrees.unwrap_or(settings.field_of_view);
    Camera {
        position: view.position,
        yaw: view.yaw,
        pitch: view.pitch,
        // A script's shot may see through a lens of its own.
        fov_degrees: view
            .lens
            .map_or(own, |(degrees, weight)| own + (degrees - own) * weight),
        ..Camera::default()
    }
}

/// Hands the renderer the bodies `frame` asks to draw, uploading each
/// model the first time it is seen. A model that cannot be uploaded is said
/// once in the log and not drawn.
fn show_figures(
    renderer: &mut Renderer,
    frame: &Frame,
    models: &mut Vec<(
        std::sync::Arc<lore_assets::Scene>,
        Option<lore_render::ModelId>,
    )>,
    drawn: &mut Vec<(Option<lore_render::ModelId>, usize)>,
) {
    let (figures, joints) = frame.figures();
    drawn.clear();
    for (index, figure) in figures.iter().enumerate() {
        let known = models
            .iter()
            .find(|(model, _)| std::sync::Arc::ptr_eq(model, &figure.model))
            .map(|(_, id)| *id);
        let id = match known {
            Some(id) => id,
            None => {
                let id = match renderer.add_model(&figure.model) {
                    Ok(id) => Some(id),
                    Err(error) => {
                        log::error!("A body's model could not be drawn: {error}");
                        None
                    }
                };
                models.push((std::sync::Arc::clone(&figure.model), id));
                id
            }
        };
        drawn.push((id, index));
    }
    let list: Vec<lore_render::Figure<'_>> = drawn
        .iter()
        .filter_map(|&(id, index)| {
            let figure = &figures[index];
            Some(lore_render::Figure {
                model: id?,
                transform: figure.transform,
                joints: joints.get(figure.joints.clone())?,
                seen: figure.seen,
                shadows: figure.shadows,
            })
        })
        .collect();
    renderer.set_figures(&list);
}

/// How long to leave the window alone after a frame could not be drawn.
///
/// A hidden or minimised window cannot be drawn into, and on some systems
/// finding that out takes a while. Trying only now and then keeps the
/// simulation on time while the window is out of sight.
const PAUSE_AFTER_SKIPPED_FRAME: Duration = Duration::from_millis(250);

/// How the engine should start.
#[derive(Debug, Clone)]
pub struct Config {
    /// Text in the window's title bar.
    pub title: String,
    /// Window width in pixels, before display scaling.
    pub width: u32,
    /// Window height in pixels, before display scaling.
    pub height: u32,
    /// Simulation ticks per second: how often [`Game::tick`] is called,
    /// however often a picture is drawn. 0 is treated as 1.
    ///
    /// The game's rules step this many times a second on any computer; what
    /// is drawn in between is blended from the last two ticks (see
    /// [`blend`](crate::blend)), and turned by what the mouse and the stick
    /// have done since (see [`Frame::input`]). After a frame that took very long, at most
    /// five ticks are run to catch up and the rest are skipped, which is
    /// logged once.
    ///
    /// ```
    /// let config = lore::Config {
    ///     tick_rate: 30,
    ///     ..lore::Config::default()
    /// };
    /// ```
    pub tick_rate: u32,
    /// The actions the game reads and what they are bound to.
    pub actions: ActionMap,
    /// The level the game is played in. [`level::load`](crate::level::load)
    /// reads the `level.glb` in a game's folder. With `None`, the engine's
    /// stand-in is shown: a chequered floor with a few blocks on it.
    ///
    /// ```no_run
    /// let config = lore::Config {
    ///     level: lore::level::load("my-game".as_ref())?,
    ///     ..lore::Config::default()
    /// };
    /// # Ok::<(), lore::level::LevelError>(())
    /// ```
    pub level: Option<lore_assets::Scene>,
    /// The game's folder, if its `level.glb` should be watched: exporting
    /// the level again then shows up in the open window, as saving a script
    /// does, and [`Game::level_saved`] is told. A level that cannot be read
    /// (one the 3D tool has not finished writing, say) leaves the one that
    /// was showing, with the reason on screen and in the log. `None`, the
    /// default, loads the level once: a packaged game's never changes.
    pub level_folder: Option<std::path::PathBuf>,
    /// What the player has chosen: how loud, how fast the view turns and
    /// which way up, how big the HUD and menus are, and how the picture is
    /// drawn (upscaling, vsync, the frame rate limit, the field of view and
    /// the brightness). A game's script changes them while the game runs
    /// with `game.settings`, as its settings menu does.
    ///
    /// They hold from the first tick. The turning speed and the swap of up
    /// and down are applied to the action named `look`, before the game
    /// reads it. [`settings::load`](crate::settings::load) reads the
    /// player's own file:
    ///
    /// ```no_run
    /// let file = lore::settings::file();
    /// let config = lore::Config {
    ///     settings: file.as_deref().map_or_else(Default::default, |file| {
    ///         lore::settings::load(file).0
    ///     }),
    ///     settings_file: file,
    ///     ..lore::Config::default()
    /// };
    /// ```
    ///
    /// A game that is not the player's to set up says how it is drawn here:
    ///
    /// ```
    /// let mut config = lore::Config::default();
    /// config.settings.vsync = false;
    /// config.settings.frame_rate_limit = 240.0;
    /// config.settings.upscaling = lore::Upscaling::Performance;
    /// ```
    pub settings: Settings,
    /// Where the engine saves the settings a game changes while it runs.
    /// With `None` they last until the game is closed.
    pub settings_file: Option<std::path::PathBuf>,
    /// The player's own bindings file, if the game reads one:
    /// [`controls::file`](crate::controls::file) says where it is kept.
    /// It is read over `actions` when the game starts, and again whenever
    /// it is saved while the game runs, so a player trying a change does
    /// not have to start again. With `None` the controls are `actions`.
    ///
    /// ```
    /// let config = lore::Config {
    ///     controls_file: lore::controls::file(),
    ///     ..lore::Config::default()
    /// };
    /// ```
    pub controls_file: Option<std::path::PathBuf>,
    /// The game's own bindings file, read after the player's and winning
    /// over it, and watched the same way
    /// ([`controls::game_file`](crate::controls::game_file) says where it
    /// is kept). Only used with `controls_file`. `None` reads no such file.
    pub game_controls_file: Option<std::path::PathBuf>,
}

impl Default for Config {
    /// A 1280x720 window, 60 ticks per second, the standard controls, no
    /// level, and a new player's settings (vsync on, at most 400 frames per
    /// second, FSR upscaling at Quality), saved nowhere.
    fn default() -> Self {
        Self {
            title: "LORE".to_owned(),
            width: 1280,
            height: 720,
            tick_rate: 60,
            actions: ActionMap::standard(),
            level: None,
            settings: Settings::default(),
            settings_file: None,
            controls_file: None,
            game_controls_file: None,
            level_folder: None,
        }
    }
}

/// Why the engine could not start or keep running.
#[derive(Debug)]
pub enum AppError {
    /// The operating system's event loop failed.
    EventLoop(winit::error::EventLoopError),
    /// The window could not be created.
    Window(winit::error::OsError),
    /// The renderer could not start or stopped working.
    Render(RenderError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLoop(error) => write!(
                f,
                "Could not start the window system: {error}. \
                 LORE needs a desktop session; on Linux, check that Wayland or X11 is running \
                 and that WAYLAND_DISPLAY or DISPLAY is set."
            ),
            Self::Window(error) => write!(
                f,
                "Could not open the game window: {error}. \
                 Check that your graphics drivers are installed and try a smaller window size."
            ),
            Self::Render(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EventLoop(error) => Some(error),
            Self::Window(error) => Some(error),
            Self::Render(error) => error.source(),
        }
    }
}

/// Opens a window and runs `game` until the window is closed.
///
/// Keyboard, mouse and gamepad all feed `config.actions`, and
/// [`Game::tick`] is called `config.tick_rate` times per second.
///
/// The live action set decides what the mouse is for. While it is one the
/// player looks around in (as the first set, `gameplay`, is), clicking the
/// window captures the mouse; Escape or switching to another window always
/// lets it go. When the game makes a menu's set live with
/// [`Frame::use_action_set`], the cursor is shown and freed, and it is
/// captured again when the game goes back.
///
/// The engine draws no menu of its own: pausing, the settings and the way
/// out are the game's to show. A game written in Luau has them from
/// `lore new` (`pause_menu.luau`), built on `game.pause`, `game.settings`
/// and `game.quit`.
///
/// Call this from the main thread.
///
/// # Errors
///
/// Fails if the window system cannot be reached, the window cannot be created,
/// or no graphics card here can draw with Vulkan 1.3. The error's message says
/// what to do about it.
pub fn run(mut config: Config, game: impl Game) -> Result<(), AppError> {
    let controls = watch_controls(&mut config);
    start(config, game, controls, false)
}

/// Reads the player's bindings file and the game's onto `config.actions`
/// and starts watching both, as the config says.
fn watch_controls(config: &mut Config) -> Option<controls::Live> {
    let game_file = config.game_controls_file.take();
    config.controls_file.take().map(|file| {
        let (live, said) = controls::Live::start(file, game_file, &mut config.actions);
        for problem in said {
            log::warn!("{problem}");
        }
        live
    })
}

/// Runs `game` as [`run`] does, as the editor: a ground grid is drawn over
/// the level, and holding the right mouse button captures the mouse to look
/// around with, whatever the live action set does with it.
#[cfg(feature = "editor")]
pub(crate) fn run_editor(mut config: Config, game: impl Game) -> Result<(), AppError> {
    // `config.actions` are the editor's, with the game's: the files are
    // read onto both, and the editor takes the result from its frame.
    let controls = watch_controls(&mut config);
    start(config, game, controls, true)
}

/// Opens the window and runs `game` in it, as the editor if `editing`.
fn start(
    mut config: Config,
    game: impl Game,
    mut controls: Option<controls::Live>,
    editing: bool,
) -> Result<(), AppError> {
    for gap in config.actions.parity_gaps() {
        log::warn!("{gap}");
    }
    let event_loop = EventLoop::new().map_err(AppError::EventLoop)?;
    // The player's settings hold from the first tick.
    let look = config.actions.action("look").ok();
    let mut input = Input::new(config.actions.clone());
    if let Some(look) = look {
        let (mouse, stick) = config.settings.look();
        input.set_sensitivities(look, mouse, stick);
    }
    let mut sound = SoundOutput::new(LazySpeakers::default());
    sound.set_volumes(&config.settings);
    let level_file = config
        .level_folder
        .take()
        .map(|folder| crate::level::LevelFile::watch(&folder));
    let mut controls_notice = ControlsNotice::default();
    let mut controls_change = None;
    let at_start = controls
        .as_mut()
        .map(|live| (live.take_start_problems(), live.file()));
    if let Some((problems, file)) = at_start.filter(|(problems, _)| !problems.is_empty()) {
        let change = controls_notice::change_told(Vec::new(), &problems, true, &file);
        controls_notice.read(None, change.problem.clone(), Instant::now());
        controls_change = Some(change);
    }
    let mut app = App {
        controls_notice,
        controls_change,
        level_file,
        level_problem: None,
        clock: TickClock::new(config.tick_rate),
        hitches: Hitches::default(),
        pacer: FramePacer::new(config.settings.frames_per_second(), Instant::now()),
        input,
        look,
        rumble_share: config.settings.rumble_share(),
        controls,
        // The game is told the controls once, so the first label it draws
        // is right: with no bindings file they are the config's own.
        new_controls: Some(config.actions.clone()),
        ui_scale: Some(config.settings.ui_scale())
            .filter(|scale| scale.is_finite())
            .map_or(1.0, |scale| scale.clamp(UI_SCALES.0, UI_SCALES.1)),
        settings_file: SettingsFile::at(config.settings_file.take()),
        pointer: None,
        pointer_down: false,
        pointer_tapped: false,
        gamepads: Gamepads::new(),
        config,
        game,
        editing,
        renderer: None,
        hud_images: None,
        lighting: None,
        occlusion: None,
        game_look: None,
        look_given: false,
        lighting_given: false,
        lamps: Vec::new(),
        figures: (Vec::new(), Vec::new()),
        models: Vec::new(),
        drawn: Vec::new(),
        shaders: crate::game_shaders::ShaderList::default(),
        window: None,
        occluded: false,
        draw_after: Instant::now(),
        focused: false,
        captured: false,
        scroll: 0.0,
        wheel: 0.0,
        last_frame: Instant::now(),
        last_view: None,
        ui: lore_ui::DrawBuffer::default(),
        fluid_drops: Vec::new(),
        pools: Vec::new(),
        unseen: InputFrame::default(),
        sound,
        #[cfg(feature = "editor")]
        panels: None,
        error: None,
    };
    let ran = event_loop.run_app(&mut app).map_err(AppError::EventLoop);
    // A game closed a moment after a setting was changed keeps the change.
    app.settings_file.save(&app.config.settings);
    ran?;
    app.error.map_or(Ok(()), Err)
}

/// Everything the main loop owns.
struct App<G> {
    config: Config,
    game: G,
    /// The game's level file, watched for the next export.
    level_file: Option<crate::level::LevelFile>,
    /// What is wrong with the level as it was last saved, shown until a
    /// save that can be read.
    level_problem: Option<Rc<str>>,
    /// Whether this is the editor: see [`run_editor`].
    editing: bool,
    input: Input,
    /// The action the player's turning speed is applied to, if the game
    /// has one by that name.
    look: Option<lore_input::ActionId>,
    /// What the strength of every rumble is multiplied by: the player's
    /// setting, which no game gets round.
    rumble_share: f32,
    /// The player's bindings file and the game's, watched for saves.
    controls: Option<controls::Live>,
    /// Controls read from that file since the last frame, for the game to
    /// hear of.
    new_controls: Option<ActionMap>,
    /// What the player is told when the files are read again.
    controls_notice: ControlsNotice,
    /// What the last reading came to, for the game to hear of in the next
    /// frame.
    controls_change: Option<lore_script::ControlsChange>,
    /// The size the HUD and menus are drawn at, which is also what turns
    /// the pointer's pixels into HUD units. It is the player's setting,
    /// but not while the mouse is held down: see [`App::follow_ui_size`].
    ui_scale: f32,
    /// Where the settings are saved when the game changes them.
    settings_file: SettingsFile,
    /// Where the mouse pointer is over the window, in pixels from its
    /// top-left corner.
    pointer: Option<[f32; 2]>,
    /// Whether the main mouse button is held.
    pointer_down: bool,
    /// Whether it was pressed since the last frame, so that a click that
    /// came and went during one slow frame is still seen.
    pointer_tapped: bool,
    clock: TickClock,
    /// Says so when ticks are skipped after a frame that took too long.
    hitches: Hitches,
    /// Holds frames back to the frame rate limit.
    pacer: FramePacer,
    gamepads: Gamepads,
    /// Draws into the window. Declared before it so it is dropped first.
    renderer: Option<Renderer>,
    /// The game's HUD images, until a renderer takes them.
    hud_images: Option<Vec<lore_render::HudImage>>,
    /// The game's own exposure and how the eye adapts, and how dark the
    /// shade in corners is and how far round it is searched for, as the
    /// game last said them, and whether the renderer has them: kept for a
    /// renderer made later, or made again.
    lighting: Option<(f32, lore_render::EyeAdaptation)>,
    occlusion: Option<(f32, f32)>,
    /// The look the game last said, and over how many seconds to fade to
    /// it the first time the renderer hears it.
    game_look: Option<(lore_render::Look, f32)>,
    look_given: bool,
    lighting_given: bool,
    /// The memory the last frame's lamps were said in, lent to the next.
    lamps: Vec<lore_render::Lamp>,
    /// The bodies of the last frame, kept for their memory.
    figures: (Vec<crate::game::Figure>, Vec<lore_assets::Transform>),
    /// The models uploaded to the renderer, by the model they were made
    /// from (each kept alive so its address is never reused).
    models: Vec<(
        std::sync::Arc<lore_assets::Scene>,
        Option<lore_render::ModelId>,
    )>,
    /// The figures handed to the renderer, made again each frame.
    drawn: Vec<(Option<lore_render::ModelId>, usize)>,
    /// The game's own shaders, between frames.
    shaders: crate::game_shaders::ShaderList,
    /// Created once the window system is ready for it.
    window: Option<Rc<Window>>,
    /// Whether the window is completely hidden, so drawing would be wasted.
    occluded: bool,
    /// Drawing is not tried before this time. Pushed back when a frame is skipped.
    draw_after: Instant,
    focused: bool,
    /// Whether the mouse is captured for looking around.
    captured: bool,
    /// Smooth-scroll pixels that have not yet added up to a whole notch.
    scroll: f32,
    /// How far the wheel was rolled over the free pointer since the last
    /// frame, in notches, for a menu to scroll by.
    wheel: f32,
    last_frame: Instant,
    /// Where the game was last seen from, to notice the view jumping.
    last_view: Option<[f32; 3]>,
    /// The HUD and menus the game asked for this frame. Kept from frame to
    /// frame so that gathering them stops asking for memory.
    ui: lore_ui::DrawBuffer,
    /// The fluid's drops the game asked for this frame. Kept from frame to
    /// frame for the same reason.
    fluid_drops: Vec<lore_render::FluidDrop>,
    /// The buffer how the pools stand is gathered in, lent to each frame.
    pools: Vec<lore_render::PoolDrawn>,
    /// What the player did that no tick has seen yet, read again for every
    /// frame. Kept from frame to frame for the same reason.
    unseen: InputFrame,
    /// Plays the sounds the game asks for through the speakers.
    sound: SoundOutput<LazySpeakers>,
    /// What the editor's panels are made with; `None` in a game.
    #[cfg(feature = "editor")]
    panels: Option<PanelInput>,
    /// A failure from inside the event loop, reported once it exits.
    error: Option<AppError>,
}

/// What the editor's panels are made with, and what tells them about the
/// mouse and keyboard.
#[cfg(feature = "editor")]
struct PanelInput {
    context: egui::Context,
    state: egui_winit::State,
}

/// Whether `event` is one the panels can keep from the camera: a press or
/// the wheel. Letting go always reaches the camera, so nothing stays held.
#[cfg(feature = "editor")]
fn starts_something(event: &WindowEvent) -> bool {
    match event {
        WindowEvent::MouseWheel { .. } => true,
        WindowEvent::MouseInput { state, .. } => *state == ElementState::Pressed,
        WindowEvent::KeyboardInput { event, .. } => event.state == ElementState::Pressed,
        _ => false,
    }
}

/// How long after the last change to a setting the file is written. A
/// slider being dragged changes its setting on every tick, and the file is
/// written once, when the hand has stopped.
const SAVE_AFTER: Duration = Duration::from_millis(500);

/// The player's settings file, written a moment after a game last changed
/// a setting.
#[derive(Debug)]
struct SettingsFile {
    /// Where it is; with `None` the settings last until the game is closed.
    path: Option<std::path::PathBuf>,
    /// When a setting was last changed, if the file has not been written
    /// since.
    changed: Option<Instant>,
}

impl SettingsFile {
    fn at(path: Option<std::path::PathBuf>) -> Self {
        Self {
            path,
            changed: None,
        }
    }

    /// Notes that a setting changed at `now`.
    fn changed(&mut self, now: Instant) {
        self.changed = Some(now);
    }

    /// Writes `settings` if a change has waited long enough by `now`.
    fn save_when_settled(&mut self, settings: &Settings, now: Instant) {
        if (self.changed).is_some_and(|changed| now.duration_since(changed) >= SAVE_AFTER) {
            self.save(settings);
        }
    }

    /// Writes `settings` now, if a change is waiting to be written.
    fn save(&mut self, settings: &Settings) {
        // Tried once for each change, whether or not it works: a file that
        // cannot be written is not tried again every frame.
        if self.changed.take().is_none() {
            return;
        }
        let Some(file) = &self.path else {
            return;
        };
        if let Err(error) = settings::save(file, settings) {
            log::warn!(
                "Your settings could not be saved to {}: {error}. They last until the game \
                 is closed. Check that you may write there, or set {} to the path of \
                 another file.",
                file.display(),
                settings::FILE_VARIABLE
            );
        }
    }
}

impl<G: Game> App<G> {
    /// Loads the game's level again if it was exported since it was last
    /// read: the new one is drawn and the game is told. One that cannot be
    /// read leaves the level that was showing, and says why.
    fn follow_level(&mut self) {
        let Some(change) = self
            .level_file
            .as_mut()
            .and_then(crate::level::LevelFile::changed)
        else {
            return;
        };
        let shown = change.map_err(|error| error.to_string()).and_then(|level| {
            if let Some(renderer) = &mut self.renderer {
                renderer
                    .set_scene(&level)
                    .map_err(|error| error.to_string())?;
            }
            Ok(level)
        });
        match shown {
            Ok(level) => {
                for warning in &level.warnings {
                    log::warn!("{warning}");
                }
                self.game.level_saved(&level);
                self.stop_typing();
                self.level_problem = None;
                log::info!(
                    "Loaded {} again: {}.",
                    crate::scaffold::LEVEL_FILE,
                    crate::level::summary(&level)
                );
            }
            Err(problem) => {
                log::error!("{problem}\n{KEEPS_LEVEL}");
                self.level_problem = Some(problem.into());
            }
        }
    }

    /// Puts `settings` into effect, all of them: the sound, the gamepad,
    /// how the view turns, how fast pictures are drawn and how. They are
    /// saved a moment later.
    ///
    /// A new setting is put into effect here.
    fn apply_settings(&mut self, settings: Settings) {
        self.sound.set_volumes(&settings);
        self.rumble_share = settings.rumble_share();
        if let Some(look) = self.look {
            let (mouse, stick) = settings.look();
            self.input.set_sensitivities(look, mouse, stick);
        }
        self.pacer
            .set_limit(settings.frames_per_second(), Instant::now());
        if let Some(renderer) = &mut self.renderer {
            renderer.set_upscaling(settings.upscaling);
            renderer.set_vsync(settings.vsync);
            renderer.set_exposure(settings.exposure_stops());
            renderer.set_night_brightness(settings.night_lift());
            renderer.set_reflections(settings.reflections);
            renderer.set_light_shafts(settings.light_shafts);
            renderer.set_lamp_shadows(settings.lamp_shadows);
            renderer.set_bounce(settings.bounce);
            renderer.set_ambient_occlusion(settings.ambient_occlusion);
            renderer.set_contact_shadows(settings.contact_shadows);
            renderer.set_anisotropic_filtering(settings.anisotropic_filtering);
            renderer.set_look_shares(settings.look_shares());
            renderer.set_sharpness(settings.sharpness());
        }
        // The field of view is read when the camera is next placed, and
        // the size of the HUD by `follow_ui_size`.
        self.config.settings = settings;
        self.settings_file.changed(Instant::now());
    }

    /// Lets the HUD and menus take the size the player chose, unless the
    /// mouse is held down: the slider that sets the size would grow under
    /// the pointer dragging it, and run away from it.
    fn follow_ui_size(&mut self) {
        let chosen = self.config.settings.ui_scale();
        if !self.pointer_down && chosen.is_finite() {
            self.ui_scale = chosen.clamp(UI_SCALES.0, UI_SCALES.1);
        }
    }

    /// Hides the cursor and holds it in the window so the mouse can look around.
    fn capture_mouse(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        // Locking keeps the cursor still and is what Wayland and macOS offer;
        // confining it to the window is what X11 and Windows offer.
        let held = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
        if let Err(error) = held {
            log::warn!(
                "Could not capture the mouse ({error}). Looking around still works, \
                 but the cursor may leave the window."
            );
        }
        window.set_cursor_visible(false);
        self.captured = true;
    }

    /// What the mouse is for in the live action set.
    fn mouse_mode(&self) -> MouseMode {
        self.input
            .map()
            .mouse_mode(self.input.active_set())
            .unwrap_or_default()
    }

    /// Makes `set` live and gives the mouse to it: freed for a set that
    /// points, captured for one that looks around.
    fn use_action_set(&mut self, set: ActionSetId) {
        let before = self.input.active_set();
        self.input.set_active_set(set);
        // Asking again for the live set, or for one this map does not have,
        // must not take back a mouse the player freed with Escape.
        if self.input.active_set() == before {
            return;
        }
        match self.mouse_mode() {
            MouseMode::Pointer => self.release_mouse(),
            // The player is already in the game, so no click is asked for.
            // After Escape or a switch of windows, a click still is.
            MouseMode::Look if self.focused && !self.captured => self.capture_mouse(),
            MouseMode::Look => {}
        }
    }

    /// Hands the text of a key press to a script that is taking text:
    /// letters as they are, and marks for Backspace, Enter and Escape.
    /// Repeats count, so a held key goes on typing.
    fn type_key(&mut self, event: &KeyEvent) {
        match &event.logical_key {
            Key::Named(NamedKey::Backspace) => self.input.type_text("\u{8}"),
            // Only a new press of these two: a held Enter that started the
            // typing must not finish it again.
            Key::Named(NamedKey::Enter) if !event.repeat => self.input.type_text("\n"),
            Key::Named(NamedKey::Escape) if !event.repeat => self.input.type_text("\u{1b}"),
            Key::Named(NamedKey::Enter | NamedKey::Escape) => {}
            _ => {
                if let Some(text) = &event.text
                    && text.chars().all(|c| !c.is_control())
                {
                    self.input.type_text(text);
                }
            }
        }
    }

    /// Takes the keyboard back from a game that is typing, and tells the
    /// game: the window lost focus or the level changed under a text box.
    fn stop_typing(&mut self) {
        if self.input.is_typing() {
            self.input.set_typing(false);
            if let Some(window) = &self.window {
                window.set_ime_allowed(false);
            }
        }
        self.game.typing_ended();
    }

    /// Gives the cursor back to the desktop.
    fn release_mouse(&mut self) {
        if let Some(window) = &self.window {
            // Releasing a grab that was never taken is not an error worth reporting.
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
        self.captured = false;
    }

    /// Tells the editor's panels about `event`, and says whether it was
    /// meant for them and not for the camera: a click or the wheel over a
    /// panel, or a key while a text field is being typed in.
    #[cfg(feature = "editor")]
    fn offer_to_panels(&mut self, event: &WindowEvent) -> bool {
        let (Some(panels), Some(window)) = (&mut self.panels, &self.window) else {
            return false;
        };
        if self.captured {
            // The mouse is looking around, and its hidden cursor must not
            // brush over the panels. They still hear the button let go, and
            // Shift let go or the window left, so nothing stays held there.
            if matches!(
                event,
                WindowEvent::MouseInput { .. }
                    | WindowEvent::ModifiersChanged(_)
                    | WindowEvent::Focused(_)
            ) {
                let _ = panels.state.on_window_event(window, event);
            }
            return false;
        }
        let consumed = panels.state.on_window_event(window, event).consumed;
        if matches!(event, WindowEvent::KeyboardInput { .. }) {
            // A button that was clicked may keep the keyboard's attention;
            // only typing takes the flying keys away.
            panels.context.text_edit_focused()
        } else {
            consumed
        }
    }

    /// Hands the panels made this frame to the renderer, and lets them
    /// change the cursor.
    #[cfg(feature = "editor")]
    fn show_panels(&mut self, mut output: egui::FullOutput) {
        let (Some(panels), Some(renderer)) = (&mut self.panels, &mut self.renderer) else {
            // Nothing will draw these; `egui` asks to be told that its
            // texture changes are thrown away on purpose.
            output.textures_delta.clear();
            return;
        };
        // While the mouse looks around, the cursor stays hidden.
        if !self.captured
            && let Some(window) = &self.window
        {
            panels
                .state
                .handle_platform_output(window, output.platform_output);
        }
        renderer.set_panels(lore_render::Panels {
            shapes: panels
                .context
                .tessellate(output.shapes, output.pixels_per_point),
            textures: output.textures_delta,
            pixels_per_point: output.pixels_per_point,
        });
    }

    /// Feeds whole wheel notches into the input state.
    fn scroll(&mut self, delta: MouseScrollDelta) {
        let notches = match delta {
            MouseScrollDelta::LineDelta(_, lines) => lines,
            // Pixel deltas are f64; a wheel notch needs nowhere near that precision.
            #[allow(clippy::cast_possible_truncation)]
            MouseScrollDelta::PixelDelta(position) => position.y as f32 / PIXELS_PER_NOTCH,
        };
        if !self.captured && notches.is_finite() {
            // A menu scrolls by parts of a notch too, so a touchpad is smooth.
            self.wheel =
                (self.wheel + notches).clamp(-MOST_NOTCHES_PER_EVENT, MOST_NOTCHES_PER_EVENT);
            // A script reads the wheel over the free pointer as well.
            self.input.roll_wheel(notches);
        }
        self.scroll += notches;
        // A faulty device can report absurd distances. Without a limit, a
        // value too large for subtracting 1 to change would loop forever below.
        self.scroll = if self.scroll.is_finite() {
            self.scroll
                .clamp(-MOST_NOTCHES_PER_EVENT, MOST_NOTCHES_PER_EVENT)
        } else {
            0.0
        };
        while self.scroll >= 1.0 {
            self.input.press(MouseWheel::MouseWheelUp);
            self.scroll -= 1.0;
        }
        while self.scroll <= -1.0 {
            self.input.press(MouseWheel::MouseWheelDown);
            self.scroll += 1.0;
        }
    }

    /// Draws one frame. A renderer failure ends the main loop.
    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        if !self.can_draw() {
            return;
        }
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        match renderer.render() {
            Ok(true) => {}
            Ok(false) => self.draw_after = Instant::now() + PAUSE_AFTER_SKIPPED_FRAME,
            Err(error) => {
                self.error = Some(AppError::Render(error));
                event_loop.exit();
            }
        }
    }

    /// Whether it is worth trying to draw a frame right now.
    fn can_draw(&self) -> bool {
        !self.occluded && Instant::now() >= self.draw_after
    }

    /// Puts the player's bindings files into effect if one was saved since
    /// they were last read.
    fn follow_controls(&mut self) {
        let Some(reread) = self.controls.as_mut().and_then(controls::Live::changed) else {
            return;
        };
        for problem in &reread.said {
            log::warn!("{problem}");
        }
        let file = self
            .controls
            .as_ref()
            .map(controls::Live::file)
            .unwrap_or_default();
        let told = match reread.controls {
            Some(controls) => {
                let changes = controls_notice::changes(
                    self.input.map(),
                    &controls,
                    self.input.last_device(),
                    self.gamepads.style(),
                );
                self.input
                    .set_bindings(self.keep_settings(controls.clone()));
                self.new_controls = Some(controls);
                log::info!("Your controls were saved, so they were read again and are in use.");
                let told = controls_notice::change_told(changes, &reread.problems, false, &file);
                // The controls were used, so the notice says what changed, and
                // any part of the file that was not used stays on show.
                let good = controls_notice::good_message(&told.changes);
                self.controls_notice
                    .read(Some(good), told.problem.clone(), Instant::now());
                told
            }
            None => {
                let told = controls_notice::change_told(Vec::new(), &reread.problems, false, &file);
                self.controls_notice
                    .read(None, told.problem.clone(), Instant::now());
                told
            }
        };
        self.controls_change = Some(told);
    }

    /// `controls` with the player's own settings kept: how fast the view
    /// turns is theirs, not the file's. Warns of an action a gamepad
    /// cannot press.
    fn keep_settings(&self, mut controls: ActionMap) -> ActionMap {
        for gap in controls.parity_gaps() {
            log::warn!("{gap}");
        }
        if let Some(look) = self.look {
            let now = self.input.map();
            controls.set_sensitivities(look, now.sensitivity(look), now.stick_sensitivity(look));
        }
        controls
    }

    /// Puts into use the actions a game changed while it runs: `own` is its
    /// map, the player's bindings files are read onto it again, and the input
    /// takes the result.
    fn follow_game_actions(&mut self, own: ActionMap) {
        let controls = match self.controls.as_mut() {
            Some(live) => {
                let (controls, said) = live.rebase(own);
                for problem in said {
                    log::warn!("{problem}");
                }
                controls
            }
            None => own,
        };
        let controls = self.keep_settings(controls);
        self.input.set_map(controls.clone());
        self.new_controls = Some(controls);
    }

    /// Tells the input where the free pointer is, in the HUD's own units,
    /// so what a tick reads of it matches what is drawn: the same scale as
    /// the HUD is laid out with. A captured mouse points at nothing.
    fn point_input(&mut self) {
        let Some(window) = &self.window else {
            self.input.set_pointer(None, [0.0, 0.0]);
            return;
        };
        let size = window.inner_size();
        #[allow(clippy::cast_precision_loss)]
        let (width, height) = (size.width as f32, size.height.max(1) as f32);
        let scale = height / HUD_HEIGHT * self.ui_scale;
        if !(scale.is_finite() && scale > 0.0) {
            self.input.set_pointer(None, [0.0, 0.0]);
            return;
        }
        let at = if self.captured {
            None
        } else {
            self.pointer.map(|[x, y]| [x / scale, y / scale])
        };
        self.input.set_pointer(at, [width / scale, height / scale]);
    }

    /// Runs the ticks that are due and lets the game prepare the next frame.
    /// Returns whether the game asked to end.
    fn update(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame);
        self.last_frame = now;

        self.follow_controls();

        self.gamepads.poll(&mut self.input, self.focused);
        self.follow_ui_size();
        self.point_input();
        let step = self.clock.step().as_secs_f32();
        let due = tick::run_due(
            &mut self.clock,
            elapsed,
            &mut self.input,
            &mut self.game,
            |_| {},
        );
        if let Some(said) = self.hitches.note(due.skipped, self.clock.step(), now) {
            log::warn!("{said}");
        }

        let screen = self.window.as_ref().map(|window| {
            let size = window.inner_size();
            [size.width, size.height]
        });
        // What the ticks above did not take: the frame turns the view by
        // it, so looking around never waits for a tick.
        self.input.peek(&mut self.unseen);
        // A captured mouse turns the view; it points at nothing.
        let tapped = std::mem::take(&mut self.pointer_tapped);
        let wheel = std::mem::take(&mut self.wheel);
        let (pointer, pointer_down, wheel) = if self.captured {
            (None, false, 0.0)
        } else {
            (self.pointer, self.pointer_down || tapped, wheel)
        };
        self.follow_level();
        let mut frame = Frame::new(self.clock.alpha())
            .with_input(std::mem::take(&mut self.unseen), step)
            .for_window(screen, std::mem::take(&mut self.ui))
            .with_fluid_buffer(std::mem::take(&mut self.fluid_drops))
            .with_pool_buffer(std::mem::take(&mut self.pools))
            .with_lamp_buffer(std::mem::take(&mut self.lamps))
            .with_figure_buffers(std::mem::take(&mut self.figures))
            .with_shader_list(std::mem::take(&mut self.shaders))
            .with_level_problem(self.level_problem.clone())
            .with_pointer(elapsed.as_secs_f32(), pointer, pointer_down)
            .with_wheel(wheel)
            .with_device(self.input.last_device(), self.gamepads.style())
            .with_controls(self.new_controls.take())
            .with_controls_change(self.controls_change.take())
            .with_settings(self.config.settings);
        frame.set_ui_scale(self.ui_scale);
        #[cfg(feature = "editor")]
        if let (Some(panels), Some(window)) = (&mut self.panels, &self.window) {
            let input = panels.state.take_egui_input(window);
            frame = frame.with_panels(panels.context.clone(), input);
        }
        self.game.frame(&mut frame);
        self.controls_notice.show(&mut frame, Instant::now());
        #[cfg(feature = "editor")]
        if let Some(output) = frame.take_panels() {
            self.show_panels(output);
        }
        #[cfg(feature = "editor")]
        if let (Some(shown), Some(renderer)) = (frame.grid(), &mut self.renderer) {
            renderer.set_grid(shown);
        }
        self.unseen = frame.take_input();
        self.ui = frame.take_ui();
        if let Some(renderer) = &mut self.renderer {
            renderer.set_ui(self.ui.draw());
        }
        if let (Some(view), Some(renderer)) = (frame.view(), &mut self.renderer) {
            let camera = camera(view, &self.config.settings);
            // Nobody walks or falls this far in one frame: it is a respawn
            // or a teleport, and the frames from before it do not belong
            // in the picture.
            let jumped = self.last_view.is_some_and(|last| {
                let moved: f32 = (0..3)
                    .map(|axis| (view.position[axis] - last[axis]).powi(2))
                    .sum();
                moved > CUT_METRES * CUT_METRES
            });
            if jumped {
                renderer.cut_to(camera);
            } else {
                renderer.set_camera(camera);
            }
            self.last_view = Some(view.position);
        }
        // What was done to the water since the last frame is shown.
        let (splashes, stirs) = (frame.take_splashes(), frame.take_stirs());
        let bubbles = frame.take_bubbles();
        let treads = frame.take_treads();
        let (strikes, storminess) = (frame.take_strikes(), frame.take_storminess());
        let hour = frame.take_hour();
        let sky_place = frame.take_sky_place();
        let moon_size = frame.take_moon_size();
        let ground = frame.take_ground();
        if let Some(images) = frame.take_hud_images() {
            self.hud_images = Some(images);
        }
        let moon_tint = frame.take_moon_tint();
        let milky_way = frame.take_milky_way();
        let light_pollution = frame.take_light_pollution();
        let aurora = frame.take_aurora();
        let meteors = frame.take_meteors();
        if let (Some(renderer), Some(lamps)) = (&mut self.renderer, frame.take_lamps()) {
            renderer.set_lamps(lamps);
        }
        self.lamps = frame.lamp_buffer();
        if let Some(renderer) = &mut self.renderer {
            show_figures(renderer, &frame, &mut self.models, &mut self.drawn);
        }
        self.figures = frame.figure_buffers();
        self.shaders = frame.take_shader_list();
        // Both are cheap when nothing has changed: the renderer builds
        // only a shader whose revision is new.
        if let Some(renderer) = &mut self.renderer {
            self.shaders
                .hand_over(|list| renderer.set_game_shaders(list));
            self.shaders.hear(renderer.take_shader_reports());
        }
        if let Some(said) = frame.take_lighting() {
            self.lighting = Some(said);
            self.lighting_given = false;
        }
        if let Some(said) = frame.take_ambient_occlusion() {
            self.occlusion = Some(said);
            self.lighting_given = false;
        }
        if let Some(said) = frame.take_look() {
            self.game_look = Some(said);
            self.look_given = false;
        }
        if let (Some(renderer), false) = (&mut self.renderer, self.lighting_given) {
            if let Some((stops, eye)) = self.lighting {
                renderer.set_game_exposure(stops);
                renderer.set_eye_adaptation(eye);
            }
            if let Some((strength, radius)) = self.occlusion {
                renderer.set_game_occlusion(strength, radius);
            }
            self.lighting_given = true;
        }
        if let (Some(renderer), false) = (&mut self.renderer, self.look_given) {
            if let Some((look, seconds)) = &mut self.game_look {
                // Faded to once; a renderer made again starts with it.
                renderer.set_look(*look, std::mem::take(seconds));
            }
            self.look_given = true;
        }
        let (water_rise, sky_clock) = frame.take_water();
        let ground_water = frame.take_ground_water();
        let storm_bolts = frame.take_storm_bolts();
        let weather = frame.take_weather();
        let fluid = frame.take_fluid();
        if let Some(renderer) = &mut self.renderer {
            // The looks first, so that new drops are never drawn in an old one.
            if let Some(looks) = &fluid.looks {
                renderer.set_fluid_looks(looks);
            }
            if let Some(look) = fluid.water_look {
                renderer.set_water_look(look);
            }
            if fluid.said {
                renderer.set_fluid(&fluid.drops);
            }
            // A hollow that became a pool is drawn from now on.
            if let Some(found) = &fluid.found
                && let Err(error) = renderer.set_found_waters(found)
            {
                log::warn!("A pool that formed in the level could not be drawn: {error}");
            }
            // Each pool where the game's rules have it, as what it holds.
            if fluid.pools_said {
                renderer.set_pools(&fluid.pools);
            }
            for splash in splashes {
                renderer.splash(splash);
            }
            for stir in stirs {
                renderer.stir(stir);
            }
            for (place, count) in bubbles {
                renderer.bubbles(place, count);
            }
            for tread in treads {
                renderer.tread(tread);
            }
            for strike in strikes {
                renderer.strike(strike);
            }
            if let Some(storminess) = storminess {
                renderer.set_storminess(storminess);
            }
            if let Some(weather) = weather {
                renderer.set_weather(weather);
            }
            // The place before the hour, so the sky is worked out once.
            if sky_place.is_some() {
                renderer.set_sky_place(sky_place);
            }
            if let Some(size) = moon_size {
                renderer.set_moon_size(size);
            }
            if let Some(shown) = ground {
                renderer.set_ground(shown);
            }
            // Kept until there is a renderer to take them.
            if let Some(images) = self.hud_images.take() {
                renderer.set_hud_images(&images);
            }
            if let Some(tint) = moon_tint {
                renderer.set_moon_tint(tint);
            }
            if let Some((times, tint)) = milky_way {
                renderer.set_milky_way(times, tint);
            }
            if let Some(class) = light_pollution {
                renderer.set_light_pollution(class);
            }
            if let Some(kp) = aurora {
                renderer.set_aurora(kp);
            }
            if let Some(times) = meteors {
                renderer.set_meteors(times);
            }
            if hour.is_some() {
                renderer.set_hour(hour);
            }
            if let Some(rise) = water_rise {
                renderer.set_water_rise(rise);
            }
            if let Some(ground) = ground_water {
                renderer.set_ground_water(ground);
            }
            if sky_clock.is_some() {
                renderer.set_sky_clock(sky_clock);
            }
            if let Some(own) = storm_bolts {
                renderer.set_storm_bolts(own);
            }
        }
        self.fluid_drops = fluid.drops;
        self.pools = fluid.pools;
        // The ears are where the eyes are.
        if let Some(view) = frame.view() {
            self.sound.listen_from(view.position, view.yaw);
        }
        if let Some(under) = frame.under_water() {
            self.sound.set_underwater(if under { 1.0 } else { 0.0 });
        }
        if let Some(speed) = frame.time_speed() {
            self.clock.set_speed(speed);
            // The world's sounds stop with it and slow with it.
            self.sound.set_world_speed(speed);
        }
        if let Some(settings) = frame.take_settings()
            && settings != self.config.settings
        {
            self.apply_settings(settings);
        }
        (self.settings_file).save_when_settled(&self.config.settings, now);
        self.sound.handle(frame.take_sounds());
        if let Some(own) = frame.take_new_actions() {
            self.follow_game_actions(own);
        }
        if let Some(set) = frame.action_set() {
            self.use_action_set(set);
        }
        if let Some(on) = frame.typing() {
            self.input.set_typing(on);
            if let Some(window) = &self.window {
                // Typed text from an input method (accents, other scripts)
                // arrives as a commit only while one is allowed.
                window.set_ime_allowed(on);
            }
        }
        // Like every other gamepad event, rumble is for the window in front.
        if let Some(rumble) = frame.rumble_requested()
            && self.focused
        {
            self.gamepads.rumble(rumble.scaled(self.rumble_share));
        }
        if let (Some(title), Some(window)) = (frame.take_title(), &self.window) {
            window.set_title(&title);
        }
        frame.closing()
    }
}

impl<G: Game> ApplicationHandler for App<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height));
        match event_loop.create_window(attributes) {
            Ok(window) => {
                let window = Rc::new(window);
                let size = window.inner_size();
                let settings = self.config.settings;
                let config = RenderConfig {
                    vsync: settings.vsync,
                    upscaling: settings.upscaling,
                };
                let renderer = Renderer::new(Rc::clone(&window), size.width, size.height, config)
                    .and_then(|mut renderer| {
                        // Said before the level is copied, so its samplers
                        // are made once, as the player has them.
                        renderer.set_anisotropic_filtering(settings.anisotropic_filtering);
                        // The graphics card has its own copy from here on.
                        match self.config.level.take() {
                            Some(level) => renderer.set_scene(&level)?,
                            None => renderer.set_scene(&crate::standin::scene())?,
                        }
                        renderer.set_grid(self.editing);
                        renderer.set_exposure(settings.exposure_stops());
                        renderer.set_night_brightness(settings.night_lift());
                        renderer.set_reflections(settings.reflections);
                        renderer.set_light_shafts(settings.light_shafts);
                        renderer.set_lamp_shadows(settings.lamp_shadows);
                        renderer.set_bounce(settings.bounce);
                        renderer.set_ambient_occlusion(settings.ambient_occlusion);
                        renderer.set_contact_shadows(settings.contact_shadows);
                        renderer.set_look_shares(settings.look_shares());
                        renderer.set_sharpness(settings.sharpness());
                        renderer.set_camera(Camera {
                            position: [0.0, STANDING_EYE, 0.0],
                            fov_degrees: settings.field_of_view,
                            ..Camera::default()
                        });
                        Ok(renderer)
                    });
                match renderer {
                    Ok(renderer) => {
                        self.renderer = Some(renderer);
                        // A new renderer has not heard the game's lighting
                        // or its look.
                        self.lighting_given = false;
                        self.look_given = false;
                    }
                    Err(error) => {
                        self.error = Some(AppError::Render(error));
                        event_loop.exit();
                        return;
                    }
                }
                window.request_redraw();
                #[cfg(feature = "editor")]
                if self.editing {
                    let context = crate::editor::panel_context();
                    // Text is drawn as large as the display's scaling asks.
                    #[allow(clippy::cast_possible_truncation)]
                    let scale = window.scale_factor() as f32;
                    let state = egui_winit::State::new(
                        context.clone(),
                        egui::ViewportId::ROOT,
                        &*window,
                        Some(scale),
                        None,
                        None,
                    );
                    self.panels = Some(PanelInput { context, state });
                }
                self.window = Some(window);
                self.last_frame = Instant::now();
            }
            Err(error) => {
                self.error = Some(AppError::Window(error));
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        #[cfg(feature = "editor")]
        if self.offer_to_panels(&event) && starts_something(&event) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                // A window that was minimised may be back.
                self.draw_after = Instant::now();
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                self.draw_after = Instant::now();
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                if focused {
                    self.gamepads.resync(&mut self.input);
                } else {
                    self.stop_typing();
                    self.release_mouse();
                    self.input.release_all();
                    self.pointer_down = false;
                    self.pointer_tapped = false;
                    self.gamepads.stop_rumble();
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                let pressed = event.state == ElementState::Pressed;
                if pressed && !is_synthetic && self.input.is_typing() {
                    self.type_key(&event);
                }
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                // Escape always frees the mouse, whatever the live action
                // set does with it, so a player can never be trapped in the
                // window. It still reaches the action map.
                // Only a new press does: a key that repeats while held
                // would free a mouse the game has just captured again.
                if pressed && !event.repeat && !is_synthetic && code == KeyCode::Escape {
                    self.release_mouse();
                }
                let Some(key) = translate::key(code) else {
                    return;
                };
                if !pressed {
                    self.input.release(key);
                } else if !event.repeat && !is_synthetic {
                    // Synthetic presses describe keys already down when the
                    // window gained focus; the player did not just press them.
                    self.input.press(key);
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if text.chars().all(|c| !c.is_control()) {
                    self.input.type_text(&text);
                }
            }
            // In pixels, as the window's size is, so a menu can tell what
            // the pointer is over.
            #[allow(clippy::cast_possible_truncation)]
            WindowEvent::CursorMoved { position, .. } => {
                let now = [position.x as f32, position.y as f32];
                // A pointer moved on purpose means the mouse is in the
                // hand again, and hints show the keyboard's names.
                if let Some(before) = self.pointer.replace(now)
                    && !self.captured
                {
                    self.input
                        .pointer_moved((now[0] - before[0]).abs() + (now[1] - before[1]).abs());
                }
            }
            WindowEvent::CursorLeft { .. } => self.pointer = None,
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                if button == winit::event::MouseButton::Left {
                    // Letting go always counts, so a menu never sees the
                    // button stuck down.
                    self.pointer_down =
                        pressed && !self.captured && self.mouse_mode() == MouseMode::Pointer;
                    self.pointer_tapped |= self.pointer_down;
                }
                // In the editor the right button is held to look around, as
                // in the viewports of other editors, and the cursor comes
                // back when it is let go. Not while the level is played
                // from the editor: there the button is the game's.
                if self.editing
                    && button == winit::event::MouseButton::Right
                    && self.mouse_mode() == MouseMode::Pointer
                {
                    if pressed {
                        self.capture_mouse();
                    } else {
                        self.release_mouse();
                    }
                }
                if pressed && !self.captured && self.mouse_mode() == MouseMode::Look {
                    // This click only captures the mouse. Passing it on would
                    // fire a shot the player did not mean. A set that points
                    // wants its clicks.
                    self.capture_mouse();
                    return;
                }
                let Some(button) = translate::mouse_button(button) else {
                    return;
                };
                if pressed {
                    self.input.press(button);
                } else {
                    self.input.release(button);
                }
            }
            WindowEvent::MouseWheel { delta, .. }
                if self.captured || self.mouse_mode() == MouseMode::Pointer =>
            {
                self.scroll(delta);
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (x, y) } = event
            && self.captured
        {
            // Mouse counts are small numbers, so f32 holds them exactly. The
            // window system reports down as positive; LORE uses up.
            #[allow(clippy::cast_possible_truncation)]
            self.input.move_mouse(x as f32, -y as f32);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            return;
        }
        // The frame rate limit is waited out here, before input is read and
        // the game is asked for a frame, so what is drawn is as fresh as it
        // can be. A hidden window draws nothing and is paced by the ticks.
        if self.can_draw() {
            let now = Instant::now();
            let wait = self.pacer.wait(now);
            if wait > WATCH {
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + wait - WATCH));
                return;
            }
            let due = now + wait;
            while Instant::now() < due {
                std::hint::spin_loop();
            }
            self.pacer.start(Instant::now());
        }
        if self.update() {
            event_loop.exit();
            return;
        }
        // Drawing happens when the window system delivers the redraw. With
        // vsync a frame takes as long as the display allows, and that paces
        // the loop; without it, the frame rate limit above does.
        if self.can_draw()
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
        // While nothing is drawn (a hidden or minimised window), sleep until
        // the next tick is due instead of spinning a CPU core.
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + self.clock.until_next_tick(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_written_once_the_hand_has_stopped_and_when_the_game_closes() {
        let folder = std::env::temp_dir().join(format!("lore-app-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join("settings.toml");
        let mut file = SettingsFile::at(Some(path.clone()));
        let start = Instant::now();
        let mut settings = Settings::default();

        // Nothing has changed, so nothing is written.
        file.save_when_settled(&settings, start + SAVE_AFTER * 4);
        file.save(&settings);
        assert!(!path.exists());

        // A slider dragged for a while: every tick is a change, and none of
        // them is written while they keep coming.
        for tick in 0..120 {
            let now = start + Duration::from_millis(16) * tick;
            settings.volume = 100.0 - 0.5 * f32::from(u8::try_from(tick).unwrap());
            file.changed(now);
            file.save_when_settled(&settings, now);
        }
        assert!(!path.exists());
        let let_go = start + Duration::from_millis(16) * 119;
        file.save_when_settled(&settings, let_go + SAVE_AFTER);
        assert_eq!(settings::load(&path), (settings, Vec::new()));

        // Written once: taking the file away shows nothing writes it again.
        std::fs::remove_file(&path).unwrap();
        file.save_when_settled(&settings, let_go + SAVE_AFTER * 3);
        assert!(!path.exists());

        // A game closed a moment after a change keeps the change.
        settings.vsync = false;
        file.changed(let_go + SAVE_AFTER * 4);
        file.save(&settings);
        assert_eq!(settings::load(&path), (settings, Vec::new()));
        let _ = std::fs::remove_dir_all(&folder);

        // With nowhere to save, a change is simply kept until the game closes.
        let mut nowhere = SettingsFile::at(None);
        nowhere.changed(start);
        nowhere.save(&settings);
    }

    #[test]
    fn a_view_is_drawn_at_its_own_field_of_view_or_else_at_the_players() {
        let wide = Settings {
            field_of_view: 110.0,
            ..Settings::default()
        };
        let mut frame = Frame::new(0.0);
        frame.look_from([1.0, 2.0, 3.0], 0.5, -0.25);
        let eyes = camera(frame.view().unwrap(), &wide);
        assert_eq!(eyes.fov_degrees, 110.0);
        assert_eq!(
            (eyes.position, eyes.yaw, eyes.pitch),
            ([1.0, 2.0, 3.0], 0.5, -0.25)
        );
        #[cfg(feature = "editor")]
        {
            frame.look_from_with_fov([1.0, 2.0, 3.0], 0.5, -0.25, 60.0);
            assert_eq!(camera(frame.view().unwrap(), &wide).fov_degrees, 60.0);
        }
    }
}
