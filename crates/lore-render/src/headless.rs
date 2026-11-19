//! Drawing with no window, to measure how long a frame takes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ash::vk;
use lore_assets::Scene;
use lore_ui::DrawList;

use crate::bolt::Lightning;
use crate::bounce::Bounce;
use crate::buffer::Buffer;
use crate::camera::Camera;
use crate::effects::{Air, Effects, Splash};
use crate::error::RenderError;
use crate::eye::{Eye, EyeAdaptation};
use crate::fluid::{FluidDrop, FluidLook, PoolDrawn};
use crate::font::Font;
use crate::game_shaders::{GameShader, GameShaders, ShaderReport};
use crate::gpu::Gpu;
use crate::lamp_shadow::LampShadows;
use crate::lamps::Lamp;
use crate::look::{Fade, Look, LookShares, Said};
use crate::mesh::{Contents, MeshPipeline, Meshes};
use crate::occlusion::{AmbientOcclusion, Shade};
use crate::overlay::Overlay;
use crate::swapchain::COLOUR;
use crate::texture::AnisotropicFiltering;
use crate::tonemap::{Canvas, Over, Then, TonemapPipeline};
use crate::tracks::Tread;
use crate::ui_pass::UiPass;
use crate::upscale::Upscaling;
use crate::view::{Mirroring, SceneView};
use crate::water::Stir;
use crate::weather::{GroundWater, Weather};

/// The format of the picture nobody sees: what most windows use, without
/// the sRGB curve built in.
const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

/// The frame time the upscaler is told about. Fixed, so that every run of a
/// benchmark gives the graphics card the same work.
const FRAME_SECONDS: f32 = 1.0 / 60.0;

/// How long one frame drawn by [`Headless::render`] took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameTime {
    /// The engine's own work: telling the graphics card what to draw.
    pub recording: Duration,
    /// From handing the frame over until the graphics card had finished it.
    pub drawing: Duration,
}

impl FrameTime {
    /// The whole frame: recording and drawing.
    #[must_use]
    pub fn total(&self) -> Duration {
        self.recording + self.drawing
    }
}

/// Draws a scene exactly as [`Renderer`](crate::Renderer) does, but into a
/// picture that is never shown, and waits for each frame to finish.
///
/// It needs no window or display, so it runs in automated
/// checks, on any computer with Vulkan 1.3. It is what `lore bench` times.
///
/// ```no_run
/// use lore_render::{Camera, Headless, Upscaling};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut headless = Headless::new(1280, 720, Upscaling::Quality)?;
/// headless.set_scene(&lore_assets::Scene::load("level.glb")?)?;
/// let time = headless.render(&Camera::default())?;
/// println!("That frame took {:?}.", time.total());
/// # Ok(())
/// # }
/// ```
pub struct Headless {
    // Dropped top to bottom after `drop` has destroyed the raw handles: the
    // tone mapping before the picture it reads, the graphics card last.
    ui: UiPass,
    /// The HUD and menus drawn over each frame.
    overlay: Overlay,
    /// The editor's panels, drawn over those.
    #[cfg(feature = "panels")]
    panels: crate::panels::PanelHost,
    /// The game's own shaders; before the tone mapping and the picture it reads.
    shaders: GameShaders,
    tonemap: TonemapPipeline,
    mesh: MeshPipeline,
    meshes: Option<Meshes>,
    /// What a splash threw up, and what is stirring the water.
    effects: Effects,
    /// Whether the editor's ground grid is drawn.
    grid: bool,
    /// The lamps lit: the level's own, or what was said last.
    lamps: Vec<Lamp>,
    /// What the eye has got used to, and the last measuring of the picture
    /// it took in. It does not adapt unless told to.
    eye: Eye,
    measuring: Vec<u8>,
    /// Whether wet ground, puddles and polished things mirror the world.
    reflections: bool,
    /// Whether the air is lit, as for a window.
    light_shafts: bool,
    /// Which lamps cast shadows, as for a window.
    lamp_shadows: LampShadows,
    /// How much light bounces, as for a window.
    bounce: Bounce,
    /// How sharp textures seen at a slant are, as for a window.
    filtering: AnisotropicFiltering,
    /// The shade in corners, as for a window but off to begin with.
    shade: Shade,
    /// How the picture is graded: [`Look::plain`] to begin with, unlike a
    /// window, so that tests see the colours a scene was given.
    look: Fade,
    look_shares: LookShares,
    /// Counts finished frames, so the film grain moves.
    frame_count: u32,
    view: SceneView,
    extent: vk::Extent2D,
    image: vk::Image,
    memory: vk::DeviceMemory,
    image_view: vk::ImageView,
    pool: vk::CommandPool,
    commands: vk::CommandBuffer,
    finished: vk::Fence,
    gpu: Arc<Gpu>,
}

/// The raw Vulkan objects of a [`Headless`], destroyed together.
struct Raw {
    image: vk::Image,
    memory: vk::DeviceMemory,
    image_view: vk::ImageView,
    pool: vk::CommandPool,
    commands: vk::CommandBuffer,
    finished: vk::Fence,
}

impl Raw {
    /// # Safety
    ///
    /// The objects must belong to `gpu`'s device and no longer be in use.
    unsafe fn destroy(&self, gpu: &Gpu) {
        let device = gpu.device();
        // SAFETY: the caller's guarantee; destroying a null handle is allowed.
        unsafe {
            device.destroy_fence(self.finished, None);
            device.destroy_command_pool(self.pool, None);
            device.destroy_image_view(self.image_view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

impl Headless {
    /// Gets ready to draw pictures `width` by `height` pixels.
    ///
    /// If `upscaling` cannot be had on this graphics card, the reason is
    /// logged and the scene is drawn at full size.
    ///
    /// # Errors
    ///
    /// Fails if Vulkan 1.3 is not available, no graphics card can run the
    /// renderer, or the graphics driver reports a failure. The error's
    /// message says what to do about it.
    pub fn new(width: u32, height: u32, upscaling: Upscaling) -> Result<Self, RenderError> {
        // SAFETY: there is no window, so nothing has to outlive the `Gpu`.
        let gpu = Arc::new(unsafe { Gpu::new(None) }?);
        let extent = vk::Extent2D {
            width: width.max(1),
            height: height.max(1),
        };
        let mut view = SceneView::new(&gpu, extent, upscaling)?;
        // The same camera gives the same picture, whenever it is drawn.
        view.still = true;
        let mesh = MeshPipeline::new(&gpu, 1)?;
        let mut tonemap = TonemapPipeline::new(&gpu, FORMAT)?;
        let ui = UiPass::new(&gpu, FORMAT, 1)?;
        // SAFETY: the picture is `view`'s, which lives as long as `tonemap`
        // does, and nothing is being drawn yet.
        unsafe { tonemap.read_from(view.picture(), view.output_size()) }?;

        let mut raw = Raw {
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            image_view: vk::ImageView::null(),
            pool: vk::CommandPool::null(),
            commands: vk::CommandBuffer::null(),
            finished: vk::Fence::null(),
        };
        // SAFETY: the device is alive and every object is made from valid settings.
        if let Err(error) = unsafe { raw.create(&gpu, extent) } {
            // SAFETY: whatever was made has not been used.
            unsafe { raw.destroy(&gpu) };
            return Err(error);
        }
        let Raw {
            image,
            memory,
            image_view,
            pool,
            commands,
            finished,
        } = raw;
        Ok(Self {
            ui,
            overlay: Overlay::new(Font::bundled()),
            #[cfg(feature = "panels")]
            panels: crate::panels::PanelHost::default(),
            shaders: GameShaders::new(&gpu, 1),
            tonemap,
            mesh,
            meshes: None,
            effects: Effects::new(),
            grid: false,
            lamps: Vec::new(),
            eye: Eye::new(EyeAdaptation::FIXED),
            measuring: Vec::new(),
            reflections: true,
            light_shafts: true,
            lamp_shadows: LampShadows::default(),
            bounce: Bounce::default(),
            filtering: AnisotropicFiltering::default(),
            shade: Shade::default(),
            look: Fade::new(Look::plain()),
            look_shares: LookShares::default(),
            frame_count: 0,
            view,
            extent,
            image,
            memory,
            image_view,
            pool,
            commands,
            finished,
            gpu,
        })
    }

    /// The graphics card being used.
    #[must_use]
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// The size the scene is drawn at, in pixels: smaller than the finished
    /// picture when it is upscaled.
    #[must_use]
    pub fn drawn_size(&self) -> (u32, u32) {
        let size = self.view.render_size();
        (size.width, size.height)
    }

    /// Draws the meshes of `scene` from now on, as
    /// [`Renderer::set_scene`](crate::Renderer::set_scene) does.
    ///
    /// # Errors
    ///
    /// Fails if the graphics card runs out of memory or its driver reports a
    /// failure.
    pub fn set_scene(&mut self, scene: &Scene) -> Result<(), RenderError> {
        // Every frame is waited for, so the old meshes are not in use.
        self.shaders.idle();
        self.shaders.scene_changed();
        self.meshes = None;
        self.meshes = Some(Meshes::new(&self.gpu, scene, self.filtering)?);
        self.lamps.clear();
        self.lamps
            .extend(scene.lights.iter().filter_map(Lamp::from_light));
        // Nothing that flew in the old level is in the new one.
        self.effects.clear();
        // Nothing of the old level belongs in the new one's first frame.
        self.view.cut();
        Ok(())
    }

    /// Sets the game's own shaders, as
    /// [`Renderer::set_game_shaders`](crate::Renderer::set_game_shaders)
    /// does, but waits until every one has been compiled and built, so that
    /// the next picture is the same every time.
    pub fn set_game_shaders(&mut self, shaders: &[GameShader<'_>]) {
        self.shaders.want(shaders);
        self.shaders.settle(&self.mesh);
    }

    /// What the game's shaders keep, for tests.
    #[cfg(test)]
    pub(crate) fn game_shaders(&self) -> &GameShaders {
        &self.shaders
    }

    /// What became of the game's shaders since the last call, as
    /// [`Renderer::take_shader_reports`](crate::Renderer::take_shader_reports)
    /// gives it.
    pub fn take_shader_reports(&mut self) -> Vec<ShaderReport> {
        self.shaders.take_reports()
    }

    /// Draws these lamps from now on, in place of the level's own (which
    /// [`set_scene`](Self::set_scene) draws until this is called) or those
    /// given before. Give the whole list each time anything about them
    /// changes: a picture of a torch carried calls this before each frame.
    ///
    /// Each lamp's brightness is real ([`Lamp::candela`]), and is drawn
    /// against the sun, the sky and the moon as a real lamp is seen: faint
    /// by day, and by night far brighter than the moon. Past
    /// [`MOST_LAMPS`](crate::MOST_LAMPS) in view, the nearest are drawn.
    pub fn set_lamps(&mut self, lamps: &[Lamp]) {
        self.lamps.clear();
        self.lamps.extend_from_slice(lamps);
    }

    /// Says how the picture follows the light, as
    /// [`Renderer::set_eye_adaptation`](crate::Renderer::set_eye_adaptation)
    /// does. Unlike a window's, a picture drawn here does not adapt until
    /// told to: [`EyeAdaptation::FIXED`] is the usual. While time is held
    /// still the eye gets used to each picture at once, for the next, and
    /// [`screenshot`](Self::screenshot) draws one first for it to see, so
    /// that the same camera gives the same picture.
    pub fn set_eye_adaptation(&mut self, adaptation: EyeAdaptation) {
        self.eye.adaptation = adaptation.sound();
    }

    /// Says how wet the ground is, from 0 for dry to 1 for soaked, with
    /// its puddles as full: rain takes a minute to soak it and fill them,
    /// and a picture of a wet street should not have to wait. The picture
    /// goes on counting from there.
    pub fn set_wet_ground(&mut self, wet: f32) {
        self.effects.soak(wet);
    }

    /// Says how wet the ground is and how full its puddles, as the game's
    /// rules count them, as
    /// [`Renderer::set_ground_water`](crate::Renderer::set_ground_water)
    /// does.
    pub fn set_ground_water(&mut self, ground: GroundWater) {
        self.effects.set_ground_water(ground);
    }

    /// Lets time pass between pictures, or holds it still again. It is held
    /// still to begin with, so that the same camera gives the same
    /// picture; with time passing, each picture is a sixtieth of a second
    /// after the last, and clouds drift and what bends in the wind moves.
    pub fn let_time_pass(&mut self, passing: bool) {
        self.view.still = !passing;
    }

    /// Says that something went into the water, or is pushing through it:
    /// drops fly up from where it did, spray hangs for a moment, bubbles
    /// are dragged under, and rings run out across the water.
    ///
    /// Call it once for each thing, when it happens. See [`Splash`].
    pub fn splash(&mut self, splash: Splash) {
        self.effects.splash(splash);
    }

    /// Lets a stroke of lightning go from one place to another: a crooked,
    /// forked bolt made by rule from its `seed`, which comes down, lights
    /// up, flickers and is gone in two thirds of a second, and lights the
    /// clouds and the land round it while it lasts. Storms in the sky send
    /// their own. See [`Lightning`].
    pub fn strike(&mut self, lightning: Lightning) {
        self.effects.strike(lightning);
    }

    /// Says how stormy the sky is, from 0, fair weather with the odd storm
    /// passing, to 1, storms over most of it. Under a storm it rains, and
    /// hails in its core; the wind blows harder, the ground gets wet and
    /// lightning strikes. Anything that is not a number from 0 to 1 is
    /// taken as the nearest that is.
    pub fn set_storminess(&mut self, storminess: f32) {
        self.effects.set_storminess(storminess);
    }

    /// Says what the weather is: how much cloud, how stormy, how hard it
    /// rains or snows, how warm the air is, the wind, the fog, the snow
    /// that lies and the ice on the water, and the tornado if there is
    /// one. See [`Weather`]. The picture is drawn in it from the next
    /// frame on; numbers out of their range are brought into it.
    pub fn set_weather(&mut self, weather: Weather) {
        self.effects.set_weather(weather);
    }

    /// The weather it was last said to be: fair, until something is said.
    #[must_use]
    pub fn weather(&self) -> Weather {
        *self.effects.said()
    }

    /// Says what hour of the day it is: 12 is noon, 18 the sun setting, 24
    /// midnight, and hours past 24 run on into the next day, which moves
    /// the moon a day further round its month. The sun and the moon stand
    /// where that hour has them, dusk and night fall, the stars come out
    /// and the moon lights the land. With `None`, which is how a level
    /// starts, the level's own light is the light and it is always day.
    pub fn set_hour(&mut self, hour: Option<f64>) {
        self.effects.set_hour(hour);
    }

    /// Says whether storms send their own bolts to the ground, as they do
    /// until told otherwise. A game that sends them itself with
    /// [`strike`](Self::strike), so that it knows where each fell and can
    /// let its thunder be heard, says `false`. Flashes that stay inside
    /// the cloud are the storm's own either way.
    pub fn set_storm_bolts(&mut self, own: bool) {
        self.effects.set_bolts_of_its_own(own);
    }

    /// Says how far rain has raised the level's still water, in metres:
    /// every lake and pond is drawn that much higher, and over whatever
    /// ground beside it is lower than that. The game's rules decide it
    /// (`Level::set_water_rise` in the physics); this shows it.
    pub fn set_water_rise(&mut self, rise: f32) {
        self.effects.set_rise(rise);
    }

    /// Says how each body of still fluid stands and looks, one for each of
    /// the scene's waters in their order: how far from where it was put,
    /// and what it is drawn as. See [`Renderer::set_pools`](crate::Renderer::set_pools).
    pub fn set_pools(&mut self, pools: &[PoolDrawn]) {
        if let Some(meshes) = &mut self.meshes {
            meshes.set_pools(pools);
        }
    }

    /// Says which hollows were found since the level was set: places
    /// nobody marked as water that fluid was found lying in, and that are
    /// pools since (`lore_fluid::Fluids::found`). They are drawn after the
    /// scene's own waters, in this order, as
    /// [`set_pools`](Self::set_pools) says each stands; up to 32. Say it
    /// when the list changes: the same list again costs nothing, and a
    /// new one waits for the frames in progress.
    ///
    /// # Errors
    ///
    /// Fails if the graphics card runs out of memory or its driver reports
    /// a failure.
    pub fn set_found_waters(
        &mut self,
        found: &[std::sync::Arc<lore_assets::Water>],
    ) -> Result<(), RenderError> {
        let Some(meshes) = &mut self.meshes else {
            return Ok(());
        };
        if meshes.has_found(found) {
            return Ok(());
        }
        // Every frame is waited for, so no hollow is in use.
        // SAFETY: nothing is being drawn with the scene.
        unsafe { meshes.set_found(found) }
    }

    /// Says how each kind of fluid looks, by its number: the first look is
    /// kind 0. Up to 32; kinds not given look like clear water.
    pub fn set_fluid_looks(&mut self, looks: &[FluidLook]) {
        self.effects.fluid_mut().set_looks(looks);
    }

    /// Says which drops of fluid are drawn, from now on until said again.
    /// Drops of one kind that touch are drawn as one surface. With none,
    /// drawing fluid costs nothing.
    pub fn set_fluid(&mut self, drops: &[FluidDrop]) {
        self.effects.fluid_mut().set_drops(drops);
    }

    /// Says what every lake and creek of the level is drawn as: with
    /// `Some`, that look in place of the level's own water colour, seen
    /// from above and from under it alike; with `None`, as the level says.
    pub fn set_water_look(&mut self, look: Option<FluidLook>) {
        self.effects.fluid_mut().set_water(look);
    }

    /// Says what time the sky's clock shows, in seconds: the game's own
    /// time, so that clouds, storms and rain are where the game's rules
    /// have them and a saved game finds the same sky. With `None`, which
    /// is how it starts, the clock is how long the picture has been drawn.
    pub fn set_sky_clock(&mut self, clock: Option<f64>) {
        self.view.set_clock(clock);
    }

    /// Says that something is in the water during the next frame drawn: a
    /// leg wading, a body swimming. Rings run out from it, and it trails
    /// a wake as it moves. Call it every frame for as long as it is there.
    pub fn stir(&mut self, stir: Stir) {
        self.effects.stir(stir);
    }

    /// Presses the snow down at a place: a foot coming down in it, or a
    /// body wading through. The track stays, and is carved out of the snow
    /// wherever it is drawn within twenty-five metres of the eye, until
    /// new snow has filled it in. Nothing is seen of it where no snow lies.
    pub fn tread(&mut self, tread: Tread) {
        self.effects.tread(tread);
    }

    /// Lets `count` bubbles go at `place`, under water: breath, or the air
    /// a hand drags down with it. They rise and are gone at the surface.
    pub fn bubbles(&mut self, place: [f32; 3], count: u32) {
        self.effects.bubbles(place, count);
    }

    /// Draws the editor's ground grid or not from now on, as
    /// [`Renderer::set_grid`](crate::Renderer::set_grid) does.
    pub fn set_grid(&mut self, shown: bool) {
        self.grid = shown;
    }

    /// Lets wet ground, puddles and polished things mirror the world, or
    /// only the light of the sky, as
    /// [`Renderer::set_reflections`](crate::Renderer::set_reflections)
    /// does. They do to begin with.
    pub fn set_reflections(&mut self, on: bool) {
        self.reflections = on;
    }

    /// Lights the air, or leaves it as the fog alone has it, as
    /// [`Renderer::set_light_shafts`](crate::Renderer::set_light_shafts)
    /// does. On to begin with.
    pub fn set_light_shafts(&mut self, on: bool) {
        self.light_shafts = on;
    }

    /// How many lamps cast shadows, and how sharp, as
    /// [`Renderer::set_lamp_shadows`](crate::Renderer::set_lamp_shadows)
    /// says. Eight lamps to begin with.
    pub fn set_lamp_shadows(&mut self, shadows: LampShadows) {
        self.lamp_shadows = shadows;
    }

    /// How much light bounces, as
    /// [`Renderer::set_bounce`](crate::Renderer::set_bounce) says.
    /// `Medium` to begin with.
    pub fn set_bounce(&mut self, bounce: Bounce) {
        self.bounce = bounce;
    }

    /// How sharp textures seen at a slant are, as
    /// [`Renderer::set_anisotropic_filtering`](crate::Renderer::set_anisotropic_filtering)
    /// says: from the next picture on, for the scene there is and any set
    /// later.
    pub fn set_anisotropic_filtering(&mut self, filtering: AnisotropicFiltering) {
        self.filtering = filtering;
    }

    /// How finely the shade in corners is worked out, as
    /// [`Renderer::set_ambient_occlusion`](crate::Renderer::set_ambient_occlusion)
    /// says. `Off` to begin with, unlike a window: a picture drawn with no
    /// window has no shade found for it unless it is asked for.
    pub fn set_ambient_occlusion(&mut self, quality: AmbientOcclusion) {
        self.shade.quality = quality;
    }

    /// What the game says of that shade, as
    /// [`Renderer::set_game_occlusion`](crate::Renderer::set_game_occlusion)
    /// takes it.
    pub fn set_game_occlusion(&mut self, strength: f32, radius: f32) {
        self.shade = self.shade.said(strength, radius);
    }

    /// Grades the picture with `look`, fading to it over `seconds` of the
    /// picture's time, as [`Renderer::set_look`](crate::Renderer::set_look)
    /// does. [`Look::plain`] to begin with, unlike a window: pass
    /// [`Look::default`] to see what a player sees. A picture drawn with no
    /// window moves its time on only when it is not still.
    pub fn set_look(&mut self, look: Look, seconds: f32) {
        self.look.start(look, seconds);
    }

    /// How much of the look's glow, vignette, grain and fringes is drawn,
    /// as [`Renderer::set_look_shares`](crate::Renderer::set_look_shares)
    /// takes it.
    pub fn set_look_shares(&mut self, shares: LookShares) {
        self.look_shares = shares;
    }

    /// Draws `list` over every frame from now on, as
    /// [`Renderer::set_ui`](crate::Renderer::set_ui) does.
    pub fn set_ui(&mut self, list: DrawList<'_>) {
        self.overlay.set(list);
    }

    /// Draws the editor's panels over every frame from now on, as
    /// [`Renderer::set_panels`](crate::Renderer::set_panels) does.
    #[cfg(feature = "panels")]
    pub fn set_panels(&mut self, panels: crate::Panels) {
        self.panels.set(panels);
    }

    /// Draws one frame as `camera` sees it, waits until the graphics card
    /// has finished it, and says how long each half took.
    ///
    /// While time is held still and anything in the scene mirrors the
    /// world, the frame is drawn twice: first a picture for it to mirror,
    /// from the same camera, so that the same camera gives the same
    /// picture. The time given is for both.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver or the upscaler reports a failure; this
    /// should not be used afterwards.
    pub fn render(&mut self, camera: &Camera) -> Result<FrameTime, RenderError> {
        let mut time = self.render_once(camera)?;
        // Held still, the picture is drawn again until every probe of
        // bounce light has traced, so that the same camera gives the same
        // picture from the first time it is drawn.
        let mut again = 0;
        while self.view.still && !self.mesh.bounce_settled() && again < 8 {
            let more = self.render_once(camera)?;
            time.recording += more.recording;
            time.drawing += more.drawing;
            again += 1;
        }
        Ok(time)
    }

    /// Draws one frame, or two when a picture held still mirrors itself.
    fn render_once(&mut self, camera: &Camera) -> Result<FrameTime, RenderError> {
        if let Some(meshes) = &mut self.meshes
            && meshes.filtering() != self.filtering
        {
            // SAFETY: every frame is waited for, so nothing is being drawn.
            unsafe { meshes.set_filtering(self.filtering) }?;
        }
        #[cfg(feature = "panels")]
        self.panels.prepare(&self.gpu, FORMAT, 1)?;
        // What flies moves on by a frame, unless time is held still.
        let passed = if self.view.still { 0.0 } else { FRAME_SECONDS };
        self.effects.storms(self.view.seconds(), camera.position);
        self.effects
            .step(passed, self.meshes.as_ref().map_or(&[][..], Meshes::waters));
        let weather = self.effects.air(self.view.seconds());
        let tracks = self.effects.take_tracks();
        let mirrors = self.reflections
            && (weather.rows[0][2].max(weather.rows[6][1]) > 0.0
                || self.meshes.as_ref().is_some_and(Meshes::glossy));
        // Held still, the same camera gives the same picture: what smooth
        // things mirror is a picture drawn just before from the same camera,
        // which mirrors nothing itself, and not the last picture, which may
        // be of anywhere.
        if self.view.still && mirrors {
            self.view.mirroring = Mirroring::Keeping;
            let first = self.draw(camera, weather, tracks, false)?;
            self.view.mirroring = Mirroring::Reading;
            let second = self.draw(camera, weather, None, true)?;
            return Ok(FrameTime {
                recording: first.recording + second.recording,
                drawing: first.drawing + second.drawing,
            });
        }
        self.view.mirroring = if self.reflections {
            Mirroring::Running
        } else {
            Mirroring::Off
        };
        self.draw(camera, weather, tracks, true)
    }

    /// Draws one frame in `weather`, with `tracks` the rows of tracks that
    /// changed, waits for it, and says how long each half took. Unless it
    /// is `finished`, only the scene is drawn: it is a picture for the
    /// next to mirror, and nothing is shown.
    fn draw(
        &mut self,
        camera: &Camera,
        weather: Air,
        tracks: Option<(u32, u32)>,
        finished: bool,
    ) -> Result<FrameTime, RenderError> {
        let device = self.gpu.device();
        let start = Instant::now();
        // SAFETY: every handle belongs to `self.gpu`'s device and is alive;
        // the last frame was waited for, so nothing here is still in use,
        // and this frame is waited for before anything can be destroyed.
        let handed_over = unsafe {
            device
                .reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty())
                .map_err(RenderError::driver("starting a frame"))?;
            device
                .begin_command_buffer(
                    self.commands,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(RenderError::driver("recording a frame"))?;
            let passed = if self.view.still { 0.0 } else { FRAME_SECONDS };
            self.look.step(passed);
            self.shaders.start_frame(
                0,
                passed,
                self.view.output_size(),
                &self.mesh,
                self.meshes.as_ref(),
            );
            self.view.measured = finished && self.eye.adaptation.adapts();
            self.view.shafts = self.light_shafts;
            self.view.lamp_shadows = self.lamp_shadows;
            self.view.bounce = self.bounce;
            self.view.shade = self.shade;
            self.view.record(
                self.commands,
                0,
                &self.mesh,
                Contents {
                    meshes: self.meshes.as_ref(),
                    sky: true,
                    grid: self.grid,
                    stirs: self.effects.stirs(),
                    bits: self.effects.bytes(),
                    flashes: self.effects.flashes(),
                    weather,
                    day: self.effects.day(),
                    tracks: self.effects.tracks(tracks),
                    fluid: Some(self.effects.fluid()),
                    lamps: &self.lamps,
                    surfaces: self.shaders.surface_draws(),
                    fluids: self.shaders.fluid_draws(),
                },
                camera,
                FRAME_SECONDS,
            )?;
            if finished {
                let read = self
                    .shaders
                    .record_screen(self.commands, 0, &self.view, camera);
                self.ui.prepare(self.commands, 0, &mut self.overlay)?;
                self.tonemap.record(
                    self.commands,
                    Canvas {
                        image: self.image,
                        view: self.image_view,
                        extent: self.extent,
                    },
                    &Said {
                        look: self.look.now(),
                        shares: self.look_shares,
                        exposure: self.eye.multiplier(),
                        frame: self.frame_count,
                        per_lux: self
                            .effects
                            .day()
                            .map_or(crate::lamps::DAYLIGHT_PER_LUX, |day| day.per_lux),
                    },
                    Over {
                        ui: Some(&self.ui),
                        #[cfg(feature = "panels")]
                        panels: Some(&mut self.panels),
                    },
                    read,
                    Then::Copy,
                )?;
                if !self.view.still {
                    self.frame_count = self.frame_count.wrapping_add(1);
                }
            }
            device
                .end_command_buffer(self.commands)
                .map_err(RenderError::driver("recording a frame"))?;
            device
                .reset_fences(&[self.finished])
                .map_err(RenderError::driver("drawing a frame"))?;
            let buffers = [vk::CommandBufferSubmitInfo::default().command_buffer(self.commands)];
            device
                .queue_submit2(
                    self.gpu.queue(),
                    &[vk::SubmitInfo2::default().command_buffer_infos(&buffers)],
                    self.finished,
                )
                .map_err(RenderError::driver("drawing a frame"))?;
            self.shaders.submitted();
            let handed_over = Instant::now();
            device
                .wait_for_fences(&[self.finished], true, u64::MAX)
                .map_err(RenderError::driver("waiting for the graphics card"))?;
            // The eye takes in this picture for the next, as a window's does:
            // all at once while time is held still.
            if self.mesh.measured(0, &mut self.measuring)? {
                let per_lux = self
                    .effects
                    .day()
                    .map_or(crate::lamps::DAYLIGHT_PER_LUX, |day| day.per_lux);
                let said = self.effects.said();
                let [x, _, z] = camera.position;
                let weather =
                    crate::eye::weather_darker(said, said.storm_at([x, z], self.view.seconds()));
                self.eye.see(
                    &self.measuring,
                    per_lux,
                    FRAME_SECONDS,
                    self.view.still,
                    (weather, self.effects.flashing()),
                );
            }
            handed_over
        };
        Ok(FrameTime {
            recording: handed_over - start,
            drawing: handed_over.elapsed(),
        })
    }

    /// Draws one frame as `camera` sees it, as [`render`](Self::render)
    /// does, and gives the finished picture: four bytes a pixel (red,
    /// green, blue, opacity), row by row from the top-left corner, as a
    /// screen would show them.
    ///
    /// It waits for the picture to be copied off the graphics card, so it
    /// is for tests and tools, not for every frame.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver or the upscaler reports a failure, or
    /// there is no memory for the picture.
    pub fn screenshot(&mut self, camera: &Camera) -> Result<Vec<u8>, RenderError> {
        let doing = "reading a picture back";
        // While time is held still the eye gets used to a picture at once,
        // but only for the next: one is drawn first for it to see, so that
        // the same camera gives the same picture whatever was drawn before.
        if self.view.still && self.eye.adaptation.adapts() {
            self.render(camera)?;
        }
        self.render(camera)?;
        let length = self.extent.width as usize * self.extent.height as usize * 4;
        let pixels = Buffer::writable(
            &self.gpu,
            length as u64,
            vk::BufferUsageFlags::TRANSFER_DST,
            doing,
        )?;
        let device = self.gpu.device();
        let whole = vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };
        // SAFETY: every handle belongs to `self.gpu`'s device and is alive.
        // The frame above was waited for, so the command buffer is free to
        // record again, and it left the picture laid out to be copied from.
        // The buffer holds exactly the picture's bytes, and the copy is
        // waited for before the buffer is read or dropped.
        unsafe {
            device
                .reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty())
                .map_err(RenderError::driver(doing))?;
            device
                .begin_command_buffer(
                    self.commands,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(RenderError::driver(doing))?;
            device.cmd_copy_image_to_buffer(
                self.commands,
                self.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                pixels.handle,
                &[vk::BufferImageCopy::default()
                    .image_subresource(whole)
                    .image_extent(self.extent.into())],
            );
            device
                .end_command_buffer(self.commands)
                .map_err(RenderError::driver(doing))?;
            device
                .reset_fences(&[self.finished])
                .map_err(RenderError::driver(doing))?;
            let buffers = [vk::CommandBufferSubmitInfo::default().command_buffer(self.commands)];
            device
                .queue_submit2(
                    self.gpu.queue(),
                    &[vk::SubmitInfo2::default().command_buffer_infos(&buffers)],
                    self.finished,
                )
                .map_err(RenderError::driver(doing))?;
            if let Err(code) = device.wait_for_fences(&[self.finished], true, u64::MAX) {
                // The copy may still be running, and the buffer it writes
                // is about to be dropped. If this fails too the driver is
                // gone, and so is its work.
                let _ = device.device_wait_idle();
                return Err(RenderError::driver(doing)(code));
            }
            pixels.read(length)
        }
    }
}

impl Raw {
    /// Makes the picture, its memory and what a frame is recorded with.
    ///
    /// # Safety
    ///
    /// `self` must hold only null handles, and be destroyed on `gpu`'s device.
    unsafe fn create(&mut self, gpu: &Gpu, extent: vk::Extent2D) -> Result<(), RenderError> {
        let doing = "preparing to draw without a window";
        let device = gpu.device();
        // SAFETY: the device is alive, the settings are valid, and each
        // object is stored in `self` straight away so the caller can destroy it.
        unsafe {
            self.image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(FORMAT)
                        .extent(extent.into())
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .usage(
                            vk::ImageUsageFlags::COLOR_ATTACHMENT
                                | vk::ImageUsageFlags::TRANSFER_SRC,
                        ),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            let needs = device.get_image_memory_requirements(self.image);
            let kind = gpu
                .memory_type(needs.memory_type_bits, vk::MemoryPropertyFlags::empty())
                .ok_or(RenderError::OutOfMemory { doing })?;
            self.memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(needs.size)
                        .memory_type_index(kind),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            device
                .bind_image_memory(self.image, self.memory, 0)
                .map_err(RenderError::driver(doing))?;
            self.image_view = device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(self.image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(FORMAT)
                        .subresource_range(COLOUR),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            self.pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default().queue_family_index(gpu.queue_family()),
                    None,
                )
                .map_err(RenderError::driver(doing))?;
            self.commands = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(self.pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(RenderError::driver(doing))?
                .first()
                .copied()
                .unwrap_or_default();
            self.finished = device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(RenderError::driver(doing))?;
        }
        Ok(())
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        let raw = Raw {
            image: self.image,
            memory: self.memory,
            image_view: self.image_view,
            pool: self.pool,
            commands: self.commands,
            finished: self.finished,
        };
        // SAFETY: the device is alive; once it is idle nothing is in use,
        // even if a frame failed half way.
        unsafe {
            // If waiting fails the driver is gone, and so is its work.
            let _ = self.gpu.device().device_wait_idle();
            raw.destroy(&self.gpu);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::validation_errors;
    use crate::look::{Look, LookShares, ToneMapping};

    /// Something to draw `width` by `height` pictures with, or `None` on a
    /// computer that cannot draw, unless `LORE_REQUIRE_VULKAN` says it must.
    fn headless(width: u32, height: u32) -> Option<Headless> {
        // FSR runs on one graphics device at a time in a program, and the
        // offscreen tests beside these use it; `lore bench`'s own test
        // draws through here with upscaling on.
        match Headless::new(width, height, Upscaling::Off) {
            Ok(headless) => Some(headless),
            Err(
                error @ (RenderError::NoVulkan(_)
                | RenderError::OldVulkan { .. }
                | RenderError::NoSuitableGpu { .. }),
            ) => {
                let required = std::env::var("LORE_REQUIRE_VULKAN").is_ok_and(|value| value != "0");
                assert!(!required, "LORE_REQUIRE_VULKAN is set, but: {error}");
                eprintln!("Skipping the drawing test: {error}");
                None
            }
            Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn the_ground_grid_is_drawn_with_its_axes_in_colour() {
        const SIZE: [usize; 2] = [256, 144];
        let Some(mut headless) = headless(256, 144) else {
            return;
        };
        headless.gpu().require_validation();
        // Five metres back from the middle of the world and three up,
        // looking straight at it: the X axis runs across the middle of the
        // picture, and the Z axis from the middle down to the bottom.
        let camera = Camera {
            position: [0.0, 3.0, 5.0],
            pitch: -(3.0_f32 / 5.0).atan(),
            ..Camera::default()
        };
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_grid(true);
        let with_grid = headless.screenshot(&camera).unwrap();
        // Set `LORE_SAVE_PICTURES` to a folder to look at the picture:
        // 256 by 144 pixels of four bytes each.
        if let Some(folder) = std::env::var_os("LORE_SAVE_PICTURES") {
            std::fs::write(std::path::Path::new(&folder).join("grid.rgba"), &with_grid).unwrap();
        }
        let pixel = |picture: &[u8], x: usize, y: usize| {
            let at = (y * SIZE[0] + x) * 4;
            [picture[at], picture[at + 1], picture[at + 2]].map(i32::from)
        };
        // The most coloured of the pixels around the place, since a line
        // is a pixel or two wide and what is behind it is grey.
        let near = |x: usize, y: usize| {
            let coloured =
                |[red, green, blue]: [i32; 3]| red.max(green).max(blue) - red.min(green).min(blue);
            let mut best = [0; 3];
            for (x, y) in (y - 2..=y + 2).flat_map(|y| (x - 2..=x + 2).map(move |x| (x, y))) {
                let found = pixel(&with_grid, x, y);
                if coloured(found) > coloured(best) {
                    best = found;
                }
            }
            best
        };
        let [red, green, blue] = near(SIZE[0] / 4 + 5, SIZE[1] / 2);
        assert!(
            red > green + 40 && red > blue + 40,
            "X axis: {red} {green} {blue}"
        );
        let [red, green, blue] = near(SIZE[0] / 2, SIZE[1] * 3 / 4 + 3);
        assert!(
            blue > red + 40 && blue > green,
            "Z axis: {red} {green} {blue}"
        );
        // Lines a metre apart, lighter than the background between them.
        let background = pixel(&plain, 40, 100);
        let lit = (0..SIZE[0])
            .filter(|&x| pixel(&with_grid, x, 100)[1] > background[1] + 12)
            .count();
        assert!((4..SIZE[0] / 2).contains(&lit), "{lit} lit pixels in a row");
        // Nothing above the horizon, and nothing once it is switched off.
        assert_eq!(pixel(&with_grid, 128, 4), pixel(&plain, 128, 4));
        headless.set_grid(false);
        assert_eq!(headless.screenshot(&camera).unwrap(), plain);
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn the_sky_is_blue_above_and_brightest_at_the_sun_and_dark_below() {
        const SIZE: [usize; 2] = [96, 64];
        let Some(mut headless) = headless(96, 64) else {
            return;
        };
        headless.gpu().require_validation();
        let middle = |headless: &mut Headless, yaw: f32, pitch: f32| {
            let camera = Camera {
                yaw,
                pitch,
                ..Camera::default()
            };
            let picture = headless.screenshot(&camera).unwrap();
            let at = (SIZE[1] / 2 * SIZE[0] + SIZE[0] / 2) * 4;
            [picture[at], picture[at + 1], picture[at + 2]].map(i32::from)
        };
        // Away from the usual sun, which stands to the right and behind:
        // well up, the sky or a cloud; either way no redder than blue.
        let [red, green, blue] = middle(&mut headless, 0.9, 1.0);
        assert!(blue >= red && blue > 90, "above: {red} {green} {blue}");
        // Straight at the sun: as bright as the picture goes, unless a
        // cloud is in the way, and then still bright.
        let towards = [0.399_44_f32, 0.768_92, 0.499_3];
        let yaw = (-towards[0]).atan2(-towards[2]);
        let [red, green, blue] = middle(&mut headless, yaw, towards[1].asin());
        assert!(red + green + blue > 500, "the sun: {red} {green} {blue}");
        // Under the horizon there is ground, not more sky.
        let [red, green, blue] = middle(&mut headless, 0.9, -0.8);
        assert!(red + green + blue < 150, "below: {red} {green} {blue}");
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn far_ground_fades_into_the_colour_of_the_sky() {
        use lore_assets::{Instance, Material, Mesh, MeshPart};

        const SIZE: [usize; 2] = [96, 64];
        let Some(mut headless) = headless(96, 64) else {
            return;
        };
        headless.gpu().require_validation();
        // A white floor four kilometres across.
        let mut scene = Scene::default();
        let corners = [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]];
        scene.meshes.push(Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: corners.map(|[x, z]| [x * 2000.0, 0.0, z * 2000.0]).to_vec(),
                normals: vec![[0.0, 1.0, 0.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                ..MeshPart::default()
            }],
        });
        scene.materials.push(Material::default());
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        scene.instances.push(Instance {
            mesh: 0,
            transform,
            mirrored: false,
            name: None,
            extras: None,
        });
        headless.set_scene(&scene).unwrap();
        // From sixty metres up, looking a little down: the floor at the
        // foot of the picture is under a hundred metres off, and just
        // under the horizon it is more than a kilometre away.
        let camera = Camera {
            position: [0.0, 60.0, 0.0],
            pitch: -0.3,
            ..Camera::default()
        };
        let picture = headless.screenshot(&camera).unwrap();
        let pixel = |x: usize, y: usize| {
            let at = (y * SIZE[0] + x) * 4;
            [picture[at], picture[at + 1], picture[at + 2]].map(i32::from)
        };
        // The horizon is between rows 18 and 19 of the picture.
        let (near, far, sky) = (pixel(48, 62), pixel(48, 21), pixel(48, 16));
        assert!(
            (near[0] - near[2]).abs() < 30,
            "white up close, whatever shades it: {near:?}"
        );
        assert!(
            far[2] - far[0] > near[2] - near[0] + 5,
            "bluer with distance: {far:?} against {near:?}"
        );
        // And it meets the sky without a line.
        assert!(
            (0..3).all(|colour| (far[colour] - sky[colour]).abs() < 45),
            "{far:?} against the sky's {sky:?}"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn water_shows_the_bottom_less_the_deeper_it_is_and_the_sky_when_looked_along() {
        use lore_assets::{Instance, Material, Mesh, MeshPart};

        const SIZE: [usize; 2] = [96, 64];
        let Some(mut headless) = headless(96, 64) else {
            return;
        };
        headless.gpu().require_validation();
        // A white floor, and a sheet of dark water over it: how far over
        // is set below.
        let square = |wide: f32, height: f32, material: u32| Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]
                    .map(|[x, z]| [x * wide, height, z * wide])
                    .to_vec(),
                normals: vec![[0.0, 1.0, 0.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                material,
                ..MeshPart::default()
            }],
        };
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        let placed = |mesh: u32, extras: Option<&str>| Instance {
            mesh,
            transform,
            mirrored: false,
            name: None,
            extras: extras.map(str::to_owned),
        };
        let scene = |deep: f32| {
            let mut scene = Scene::default();
            scene.meshes.push(square(500.0, 0.0, 0));
            scene.meshes.push(square(500.0, deep, 1));
            scene.materials.push(Material::default());
            scene.materials.push(Material {
                base_color: [0.02, 0.07, 0.08, 1.0],
                ..Material::default()
            });
            scene.instances.push(placed(0, None));
            scene.instances.push(placed(1, Some(r#"{"water": true}"#)));
            scene
        };
        let middle = |headless: &mut Headless, height: f32, pitch: f32| {
            let camera = Camera {
                position: [0.0, height, 0.0],
                pitch,
                ..Camera::default()
            };
            let picture = headless.screenshot(&camera).unwrap();
            let at = (SIZE[1] / 2 * SIZE[0] + SIZE[0] / 2) * 4;
            [picture[at], picture[at + 1], picture[at + 2]].map(i32::from)
        };
        let light = |[red, green, blue]: [i32; 3]| red + green + blue;

        // Straight down into water a hand deep: the white floor, nearly
        // as it is.
        headless.set_scene(&scene(0.1)).unwrap();
        let shallow = middle(&mut headless, 3.0, -1.5);
        // Into water ten metres deep: its own dark colour, and the red
        // gone first.
        headless.set_scene(&scene(10.0)).unwrap();
        let deep = middle(&mut headless, 13.0, -1.5);
        assert!(
            light(shallow) > light(deep) + 250,
            "shallow {shallow:?}, deep {deep:?}"
        );
        assert!(deep[0] < deep[2], "deep water is not red: {deep:?}");
        // Two metres down the floor still shows, bluer than it is.
        headless.set_scene(&scene(2.0)).unwrap();
        let between = middle(&mut headless, 5.0, -1.5);
        assert!(
            light(between) > light(deep) + 60 && between[2] > between[0] + 25,
            "two metres down {between:?}, deep {deep:?}"
        );
        // Along deep water: the sky, far brighter than the water itself.
        headless.set_scene(&scene(10.0)).unwrap();
        let along = middle(&mut headless, 13.0, -0.05);
        assert!(
            light(along) > light(deep) + 200 && along[2] >= along[0],
            "along {along:?}, down {deep:?}"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn still_water_is_drawn_in_the_shape_it_filled_and_a_splash_stirs_it() {
        use lore_assets::{Instance, Material, Mesh, MeshPart, Water};

        const SIZE: [usize; 2] = [96, 64];
        let Some(mut headless) = headless(96, 64) else {
            return;
        };
        headless.gpu().require_validation();
        // A white floor a metre down, and a body of dark water that filled
        // a round hollow three metres across over the middle of it (the
        // hollow itself is not drawn: only where the water says it is).
        let mut scene = Scene::default();
        scene.meshes.push(Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]
                    .map(|[x, z]| [x * 500.0, -1.0, z * 500.0])
                    .to_vec(),
                normals: vec![[0.0, 1.0, 0.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                ..MeshPart::default()
            }],
        });
        scene.materials.push(Material::default());
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        scene.instances.push(Instance {
            mesh: 0,
            transform,
            mirrored: false,
            name: None,
            extras: None,
        });
        let points = 41_u32;
        let depths = (0..points * points)
            .map(|at| {
                let (x, z) = (
                    (at % points) as f32 * 0.25 - 5.0,
                    (at / points) as f32 * 0.25 - 5.0,
                );
                if x.hypot(z) < 1.5 { 1.0 } else { Water::DRY }
            })
            .collect();
        scene.waters.push(Water {
            name: None,
            // No sheet stands for it.
            sources: Vec::new(),
            level: 0.0,
            brim: 0.0,
            origin: [-5.0, -5.0],
            cell: 0.25,
            size: [points, points],
            depths,
            colour: [0.0, 0.02, 0.02, 0.3],
            fluid: None,
            amount: 1.0,
            spill: None,
        });
        headless.set_scene(&scene).unwrap();
        let light = |picture: &[u8], [x, y]: [usize; 2]| {
            let at = (y * SIZE[0] + x) * 4;
            i32::from(picture[at]) + i32::from(picture[at + 1]) + i32::from(picture[at + 2])
        };
        // From four metres up, straight down: murky water in the middle,
        // the white floor beside it, and a round shore between.
        let above = Camera {
            position: [0.0, 4.0, 0.0],
            pitch: -1.5,
            ..Camera::default()
        };
        let picture = headless.screenshot(&above).unwrap();
        // Water takes the red out of what is seen through it first.
        let red = |picture: &[u8], [x, y]: [usize; 2]| i32::from(picture[(y * SIZE[0] + x) * 4]);
        let (middle, beside) = (red(&picture, [48, 32]), red(&picture, [6, 32]));
        assert!(
            beside > middle + 40,
            "red in the water {middle}, on the floor {beside}"
        );
        assert!(
            red(&picture, [48, 3]) > middle + 40,
            "the floor past its far shore"
        );
        assert!(light(&picture, [6, 32]) > light(&picture, [48, 32]) + 40);
        // From under it, looking up, the picture is the water's and not
        // the bright sky's.
        let under = Camera {
            position: [0.0, -0.6, 0.0],
            pitch: 0.4,
            ..Camera::default()
        };
        let from_below = headless.screenshot(&under).unwrap();
        let dry = Camera {
            position: [20.0, -0.6, 0.0],
            ..under
        };
        let from_air = headless.screenshot(&dry).unwrap();
        assert!(
            light(&from_air, [48, 8]) > light(&from_below, [48, 8]) + 40,
            "sky {} against water overhead {}",
            light(&from_air, [48, 8]),
            light(&from_below, [48, 8])
        );

        // Time passes; something drops in; drops fly and rings spread,
        // and the picture of the water is not what it was.
        headless.let_time_pass(true);
        let side = Camera {
            position: [0.0, 1.2, 3.0],
            pitch: -0.4,
            ..Camera::default()
        };
        for _ in 0..4 {
            headless.render(&side).unwrap();
        }
        let calm = headless.screenshot(&side).unwrap();
        headless.splash(crate::Splash {
            place: [0.0, 0.0, 0.0],
            moving: [0.0, -7.0, 0.0],
            wide: 0.5,
        });
        headless.stir(crate::Stir {
            place: [0.4, 0.0],
            wide: 0.3,
            moving: [1.0, 0.0],
            body: 0.1,
            churn: 0.5,
            ..crate::Stir::default()
        });
        headless.bubbles([0.0, -0.5, 0.0], 12);
        for _ in 0..12 {
            headless.render(&side).unwrap();
        }
        let stirred = headless.screenshot(&side).unwrap();
        let changed = calm
            .iter()
            .zip(&stirred)
            .filter(|(before, after)| before.abs_diff(**after) > 24)
            .count();
        assert!(changed > 150, "only {changed} bytes of the picture changed");
        // And it all dies away: nothing is left in the air.
        for _ in 0..600 {
            headless.render(&side).unwrap();
        }
        assert_eq!(headless.effects.count(), 0);
        // Something moving through the water, with nothing thrown up,
        // changes the picture too: that is its rings and its wake.
        let at_rest = headless.screenshot(&side).unwrap();
        for frame in 0..40_u8 {
            headless.stir(crate::Stir {
                place: [-0.8 + 0.04 * f32::from(frame), 0.2],
                wide: 0.3,
                moving: [2.4, 0.0],
                body: 0.2,
                ..crate::Stir::default()
            });
            headless.render(&side).unwrap();
        }
        assert_eq!(headless.effects.count(), 0);
        let waked = headless.screenshot(&side).unwrap();
        let changed = at_rest
            .iter()
            .zip(&waked)
            .filter(|(before, after)| before.abs_diff(**after) > 16)
            .count();
        assert!(changed > 60, "a wake changed only {changed} bytes");
        // The level set again is drawn again, water and all, with nothing
        // left pointing at the old one's pictures.
        headless.set_scene(&scene).unwrap();
        headless.render(&side).unwrap();
        headless.render(&above).unwrap();
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn a_creek_beside_a_pool_of_something_else_is_still_drawn_as_water() {
        use lore_assets::{Instance, Material, Mesh, MeshPart, Water};

        const SIZE: [usize; 2] = [96, 64];
        let Some(mut headless) = headless(96, 64) else {
            return;
        };
        headless.gpu().require_validation();
        // A white floor a metre down, a round pool over the middle of it,
        // and sixteen metres east a creek: a sheet marked as water that
        // no still water took the place of.
        let flat = |low: [f32; 2], high: [f32; 2], y: f32| MeshPart {
            positions: vec![
                [low[0], y, high[1]],
                [high[0], y, high[1]],
                [high[0], y, low[1]],
                [low[0], y, low[1]],
            ],
            normals: vec![[0.0, 1.0, 0.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..MeshPart::default()
        };
        let mut scene = Scene::default();
        for part in [
            flat([-500.0, -500.0], [500.0, 500.0], -1.0),
            flat([16.0, -3.0], [19.0, 3.0], -0.5),
        ] {
            scene.meshes.push(Mesh {
                name: None,
                parts: vec![part],
            });
        }
        scene.materials.push(Material::default());
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        for (mesh, extras) in [(0, None), (1, Some(r#"{"water": true}"#))] {
            scene.instances.push(Instance {
                mesh,
                transform,
                mirrored: false,
                name: None,
                extras: extras.map(str::to_owned),
            });
        }
        let points = 41_u32;
        let depths = (0..points * points)
            .map(|at| {
                let (x, z) = (
                    (at % points) as f32 * 0.25 - 5.0,
                    (at / points) as f32 * 0.25 - 5.0,
                );
                if x.hypot(z) < 1.5 { 1.0 } else { Water::DRY }
            })
            .collect();
        scene.waters.push(Water {
            name: None,
            sources: Vec::new(),
            level: 0.0,
            brim: 0.0,
            origin: [-5.0, -5.0],
            cell: 0.25,
            size: [points, points],
            depths,
            colour: [0.0, 0.02, 0.02, 0.3],
            fluid: None,
            amount: 1.0,
            spill: None,
        });
        headless.set_scene(&scene).unwrap();
        // Looking straight down on the creek, the pool out of sight.
        let over_creek = Camera {
            position: [17.5, 3.0, 0.0],
            pitch: -1.5,
            ..Camera::default()
        };
        let red = |picture: &[u8], [x, y]: [usize; 2]| i32::from(picture[(y * SIZE[0] + x) * 4]);
        let as_water = headless.screenshot(&over_creek).unwrap();
        assert!(
            red(&as_water, [3, 32]) > red(&as_water, [48, 32]) + 2,
            "the creek is drawn: {} beside it, {} in it",
            red(&as_water, [3, 32]),
            red(&as_water, [48, 32])
        );
        // The pool becomes lava. The creek does not.
        let lava = FluidLook {
            absorbs: [0.5, 4.0, 6.0],
            glows: [6.0, 1.5, 0.2],
            ..FluidLook::default()
        };
        headless.set_pools(&[PoolDrawn {
            look: Some(lava),
            freezes: false,
            ..PoolDrawn::default()
        }]);
        assert!(
            headless.screenshot(&over_creek).unwrap() == as_water,
            "the creek took the look of the pool"
        );
        // The pool itself did.
        let over_pool = Camera {
            position: [0.0, 4.0, 0.0],
            ..over_creek
        };
        let pool = headless.screenshot(&over_pool).unwrap();
        assert!(red(&pool, [48, 32]) > 200, "{}", red(&pool, [48, 32]));
        // What a game makes of water itself is what its creeks are too.
        headless.set_water_look(Some(lava));
        let as_lava = headless.screenshot(&over_creek).unwrap();
        assert!(
            red(&as_lava, [48, 32]) > red(&as_water, [48, 32]) + 60,
            "{} then {}",
            red(&as_water, [48, 32]),
            red(&as_lava, [48, 32])
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn a_hollow_found_while_playing_is_drawn_once_its_game_says_how_it_stands() {
        use std::sync::Arc;

        use lore_assets::{Instance, Material, Mesh, MeshPart, Water};

        const SIZE: [usize; 2] = [96, 64];
        let Some(mut headless) = headless(96, 64) else {
            return;
        };
        headless.gpu().require_validation();
        // A white floor a metre down and no water in the level at all.
        let mut scene = Scene::default();
        scene.meshes.push(Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]
                    .map(|[x, z]| [x * 500.0, -1.0, z * 500.0])
                    .to_vec(),
                normals: vec![[0.0, 1.0, 0.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                ..MeshPart::default()
            }],
        });
        scene.materials.push(Material::default());
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        scene.instances.push(Instance {
            mesh: 0,
            transform,
            mirrored: false,
            name: None,
            extras: None,
        });
        headless.set_scene(&scene).unwrap();
        // A round hollow three metres across over the middle of the
        // floor, a metre deep, as one is found: `wide` says how round.
        let hollow = |wide: f32| {
            let points = 41_u32;
            let depths = (0..points * points)
                .map(|at| {
                    let (x, z) = (
                        (at % points) as f32 * 0.25 - 5.0,
                        (at / points) as f32 * 0.25 - 5.0,
                    );
                    if x.hypot(z) < wide { 1.0 } else { Water::DRY }
                })
                .collect();
            Arc::new(Water {
                name: None,
                sources: Vec::new(),
                level: 0.0,
                brim: 0.5,
                origin: [-5.0, -5.0],
                cell: 0.25,
                size: [points, points],
                depths,
                colour: [0.0, 0.02, 0.02, 0.3],
                fluid: None,
                amount: 0.0,
                spill: None,
            })
        };
        let above = Camera {
            position: [0.0, 4.0, 0.0],
            pitch: -1.5,
            ..Camera::default()
        };
        let red = |picture: &[u8], [x, y]: [usize; 2]| i32::from(picture[(y * SIZE[0] + x) * 4]);
        let floor = headless.screenshot(&above).unwrap();
        let bare = red(&floor, [48, 32]);
        // Found, and nothing said of how it stands: it is not drawn.
        let found = [hollow(1.5)];
        headless.set_found_waters(&found).unwrap();
        assert_eq!(headless.screenshot(&above).unwrap(), floor);
        // Said to stand where it was found, as murky water: drawn in its
        // shape, the floor beside it as it was.
        let murky = FluidLook {
            absorbs: [3.0, 0.4, 0.4],
            ..FluidLook::default()
        };
        let stands = [PoolDrawn {
            look: Some(murky),
            ..PoolDrawn::default()
        }];
        headless.set_pools(&stands);
        let pooled = headless.screenshot(&above).unwrap();
        let (middle, beside) = (red(&pooled, [48, 32]), red(&pooled, [6, 32]));
        assert!(
            bare > middle + 40 && beside > middle + 40,
            "red in the pool {middle}, on the floor {beside}, {bare} before"
        );
        // Said again with the same hollows, nothing is made again; found
        // to have another shape, it is drawn in that one, once its game
        // says how it stands again.
        headless.set_found_waters(&found).unwrap();
        assert_eq!(headless.screenshot(&above).unwrap(), pooled);
        headless
            .set_found_waters(&[hollow(0.6), hollow(1.5)])
            .unwrap();
        assert_eq!(headless.screenshot(&above).unwrap(), floor);
        headless.set_pools(&[
            stands[0],
            PoolDrawn {
                shift: -2.0,
                ..stands[0]
            },
        ]);
        let smaller = headless.screenshot(&above).unwrap();
        assert!(
            bare > red(&smaller, [48, 32]) + 40,
            "the small one is drawn"
        );
        assert!(
            red(&smaller, [56, 32]) > red(&pooled, [56, 32]) + 40,
            "where the wide one stood, the empty one shows the floor"
        );
        // With none found the level is as it was, and nothing is left
        // pointing at their pictures.
        headless.set_found_waters(&[]).unwrap();
        assert_eq!(headless.screenshot(&above).unwrap(), floor);
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn what_a_level_says_bends_in_the_wind_moves_and_nothing_else_does() {
        use lore_assets::{Instance, Material, Mesh, MeshPart};

        const SIZE: [usize; 2] = [96, 64];
        let Some(mut headless) = headless(96, 64) else {
            return;
        };
        headless.gpu().require_validation();
        // A red leaf a metre across, eight metres over where it is rooted,
        // above a white floor.
        let square = |wide: f32, height: f32, material: u32| Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]
                    .map(|[x, z]| [x * wide, height, z * wide])
                    .to_vec(),
                normals: vec![[0.0, 1.0, 0.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                material,
                ..MeshPart::default()
            }],
        };
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        let scene = |properties: Option<&str>| {
            let mut scene = Scene::default();
            scene.meshes.push(square(200.0, 0.0, 0));
            scene.meshes.push(square(0.5, 8.0, 1));
            scene.materials.push(Material::default());
            scene.materials.push(Material {
                base_color: [1.0, 0.0, 0.0, 1.0],
                ..Material::default()
            });
            for (mesh, extras) in [(0, None), (1, properties)] {
                scene.instances.push(Instance {
                    mesh,
                    transform,
                    mirrored: false,
                    name: None,
                    extras: extras.map(str::to_owned),
                });
            }
            scene
        };
        let camera = Camera {
            position: [0.0, 14.0, 0.0],
            pitch: -std::f32::consts::FRAC_PI_2,
            ..Camera::default()
        };
        // Where the leaf is in the picture: the middle of its red pixels.
        let leaf = |headless: &mut Headless| {
            let picture = headless.screenshot(&camera).unwrap();
            let (mut across, mut down, mut count) = (0.0_f32, 0.0_f32, 0.0_f32);
            for y in 0..SIZE[1] {
                for x in 0..SIZE[0] {
                    let at = (y * SIZE[0] + x) * 4;
                    if picture[at] > 120 && picture[at + 1] < 60 {
                        #[allow(clippy::cast_precision_loss)]
                        {
                            across += x as f32;
                            down += y as f32;
                        }
                        count += 1.0;
                    }
                }
            }
            assert!(count > 20.0, "the leaf is in the picture: {count} pixels");
            [across / count, down / count]
        };
        // How far the leaf goes in the next two seconds.
        let wanders = |headless: &mut Headless| {
            let from = leaf(headless);
            let mut farthest = 0.0_f32;
            for _ in 0..12 {
                for _ in 0..10 {
                    headless.render(&camera).unwrap();
                }
                let now = leaf(headless);
                farthest = farthest.max((now[0] - from[0]).hypot(now[1] - from[1]));
            }
            farthest
        };

        // Held still, as pictures are to begin with, it stays where it is.
        headless.set_scene(&scene(Some(r#"{"wind": 1}"#))).unwrap();
        assert!(wanders(&mut headless) < 0.01);
        // With time passing it moves.
        headless.let_time_pass(true);
        let moved = wanders(&mut headless);
        assert!(moved > 0.3, "the leaf moved {moved} pixels");
        // And a leaf nobody said bends does not.
        headless.set_scene(&scene(None)).unwrap();
        let moved = wanders(&mut headless);
        assert!(moved < 0.01, "a stiff leaf moved {moved} pixels");
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    /// What the fluid tests draw: a grey floor, and with `ceiling` a
    /// solid sheet two metres over it; with `water` also a sheet of water
    /// two metres deep over the floor (the floor is then at -2).
    fn fluid_scene(ceiling: bool, water: bool) -> Scene {
        use lore_assets::{Instance, Material, Mesh, MeshPart};

        let square = |height: f32, material: u32| Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]
                    .map(|[x, z]| [x * 500.0, height, z * 500.0])
                    .to_vec(),
                normals: vec![[0.0, 1.0, 0.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                material,
                ..MeshPart::default()
            }],
        };
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        let placed = |mesh: u32, extras: Option<&str>| Instance {
            mesh,
            transform,
            mirrored: false,
            name: None,
            extras: extras.map(str::to_owned),
        };
        let mut scene = Scene::default();
        // A grey floor, so that milk, which is no brighter than white, can
        // be seen to be lighter than it.
        scene.materials.push(Material {
            base_color: [0.25, 0.25, 0.25, 1.0],
            ..Material::default()
        });
        scene.materials.push(Material {
            base_color: [0.02, 0.07, 0.08, 1.0],
            ..Material::default()
        });
        scene.meshes.push(square(if water { -2.0 } else { 0.0 }, 0));
        scene.instances.push(placed(0, None));
        if ceiling {
            scene.meshes.push(square(2.0, 0));
            scene.instances.push(placed(1, None));
        }
        if water {
            scene.meshes.push(square(0.0, 1));
            let mesh = scene.meshes.len() as u32 - 1;
            scene
                .instances
                .push(placed(mesh, Some(r#"{"water": true}"#)));
        }
        scene
    }

    /// Four metres up, looking straight down.
    fn looking_down() -> Camera {
        Camera {
            position: [0.0, 4.0, 0.0],
            pitch: -1.5,
            ..Camera::default()
        }
    }

    /// A round puddle about a metre and a half across, lying on the floor
    /// under the camera, of drops of `kind`.
    fn puddle(kind: u8) -> Vec<FluidDrop> {
        let mut drops = Vec::new();
        for x in -3..=3 {
            for z in -3..=3 {
                let place = [x as f32 * 0.25, 0.0, z as f32 * 0.25];
                if place[0].hypot(place[2]) < 0.8 {
                    drops.push(FluidDrop {
                        place,
                        radius: 0.4,
                        facing: [0.0, 1.0, 0.0],
                        flat: 1.0,
                        long: 1.0,
                        thick: 0.05,
                        kind,
                    });
                }
            }
        }
        drops
    }

    fn milk() -> FluidLook {
        FluidLook {
            absorbs: [0.1; 3],
            scatters: [40.0; 3],
            roughness: 0.4,
            ..FluidLook::default()
        }
    }

    fn tar() -> FluidLook {
        FluidLook {
            absorbs: [500.0; 3],
            scatters: [0.0; 3],
            roughness: 0.3,
            ..FluidLook::default()
        }
    }

    /// The brightness of the pixel at `x`, `y` of a 128 by 72 picture.
    fn fluid_light(picture: &[u8], [x, y]: [usize; 2]) -> i32 {
        let at = (y * 128 + x) * 4;
        i32::from(picture[at]) + i32::from(picture[at + 1]) + i32::from(picture[at + 2])
    }

    /// A floor of `roughness`, and a wall glowing red standing on it ten
    /// metres off, across the way the camera looks.
    fn floor_and_wall(roughness: f32) -> Scene {
        use lore_assets::{Instance, Material, Mesh, MeshPart};

        let quad = |corners: [[f32; 3]; 4], normal: [f32; 3], material: u32| Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: corners.to_vec(),
                normals: vec![normal; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                material,
                ..MeshPart::default()
            }],
        };
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        let mut scene = Scene::default();
        scene.materials.push(Material {
            base_color: [0.05, 0.05, 0.05, 1.0],
            roughness,
            ..Material::default()
        });
        scene.materials.push(Material {
            base_color: [0.0, 0.0, 0.0, 1.0],
            emissive: [1.0, 0.0, 0.0],
            emissive_strength: 8.0,
            double_sided: true,
            ..Material::default()
        });
        scene.meshes.push(quad(
            [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]
                .map(|[x, z]| [x * 100.0, 0.0, z * 100.0]),
            [0.0, 1.0, 0.0],
            0,
        ));
        scene.meshes.push(quad(
            [[-30.0, 0.0], [30.0, 0.0], [30.0, 12.0], [-30.0, 12.0]].map(|[x, y]| [x, y, -10.0]),
            [0.0, 0.0, 1.0],
            1,
        ));
        for mesh in 0..2 {
            scene.instances.push(Instance {
                mesh,
                transform,
                mirrored: false,
                name: None,
                extras: None,
            });
        }
        scene
    }

    #[test]
    fn a_smooth_floor_mirrors_the_wall_before_it_and_a_rough_one_does_not() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        let camera = Camera {
            position: [0.0, 1.5, 10.0],
            pitch: -0.15,
            ..Camera::default()
        };
        // How much redder than green the lower half of the picture is,
        // where only the floor is.
        let red = |picture: &[u8]| -> i64 {
            picture
                .as_chunks::<4>()
                .0
                .iter()
                .skip(128 * 40)
                .map(|pixel| i64::from(pixel[0]) - i64::from(pixel[1]))
                .sum()
        };
        headless.set_scene(&floor_and_wall(0.05)).unwrap();
        let mirrored = headless.screenshot(&camera).unwrap();
        // What it mirrors is drawn from the same camera, so the picture
        // does not change from one to the next.
        assert!(headless.screenshot(&camera).unwrap() == mirrored);
        headless.set_reflections(false);
        let unmirrored = headless.screenshot(&camera).unwrap();
        let (with, without) = (red(&mirrored), red(&unmirrored));
        assert!(
            with > without + 128 * 32 * 20,
            "the floor is no redder for mirroring the wall: {with} against {without}"
        );
        // A rough floor mirrors only the sky's light, so it looks the same
        // either way.
        headless.set_scene(&floor_and_wall(1.0)).unwrap();
        let rough = headless.screenshot(&camera).unwrap();
        headless.set_reflections(true);
        assert!(headless.screenshot(&camera).unwrap() == rough);
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    /// A pale floor with a pale wall standing on it three metres in front
    /// of [`at_the_wall`]'s camera, across the way it looks. `sheet` lays
    /// something over the open floor nearest the camera, a hand above it:
    /// 1 for a see-through pane, 2 for a sheet of which the alpha of its
    /// corners cuts the left half away.
    fn wall_on_a_floor(sheet: u8) -> Scene {
        use lore_assets::{AlphaMode, Instance, Material, Mesh, MeshPart};

        let quad =
            |corners: [[f32; 3]; 4], normal: [f32; 3], material: u32, alphas: [f32; 4]| Mesh {
                name: None,
                parts: vec![MeshPart {
                    positions: corners.to_vec(),
                    normals: vec![normal; 4],
                    colors: Some(alphas.map(|alpha| [1.0, 1.0, 1.0, alpha]).to_vec()),
                    indices: vec![0, 1, 2, 0, 2, 3],
                    material,
                    ..MeshPart::default()
                }],
            };
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        let mut scene = Scene::default();
        scene.materials.push(Material {
            base_color: [0.6, 0.6, 0.6, 1.0],
            roughness: 1.0,
            ..Material::default()
        });
        scene.materials.push(Material {
            base_color: [0.2, 0.4, 0.8, if sheet == 1 { 0.5 } else { 1.0 }],
            roughness: 1.0,
            alpha_mode: if sheet == 1 {
                AlphaMode::Blend
            } else {
                AlphaMode::Mask { cutoff: 0.5 }
            },
            double_sided: true,
            ..Material::default()
        });
        scene.meshes.push(quad(
            [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]
                .map(|[x, z]| [x * 100.0, 0.0, z * 100.0]),
            [0.0, 1.0, 0.0],
            0,
            [1.0; 4],
        ));
        scene.meshes.push(quad(
            [[-30.0, 0.0], [30.0, 0.0], [30.0, 12.0], [-30.0, 12.0]].map(|[x, y]| [x, y, -10.0]),
            [0.0, 0.0, 1.0],
            0,
            [1.0; 4],
        ));
        if sheet > 0 {
            // Its left corners have no alpha: cut out, that half is a hole.
            scene.meshes.push(quad(
                [[-3.0, -8.0], [3.0, -8.0], [3.0, -8.9], [-3.0, -8.9]].map(|[x, z]| [x, 0.1, z]),
                [0.0, 1.0, 0.0],
                1,
                [0.0, 1.0, 1.0, 0.0],
            ));
        }
        for mesh in 0..if sheet > 0 { 3 } else { 2 } {
            scene.instances.push(Instance {
                mesh,
                transform,
                mirrored: false,
                name: None,
                extras: None,
            });
        }
        scene
    }

    /// Standing three metres from [`wall_on_a_floor`]'s wall, looking at
    /// its foot. In a picture 128 by 72 the wall comes down to row 43; the
    /// floor in rows 44 to 46 is within 40 cm of it, row 56 is more than a
    /// metre from it, and the rows from 60 down are open floor.
    fn at_the_wall() -> Camera {
        Camera {
            position: [0.0, 1.5, -7.0],
            pitch: -0.3,
            ..Camera::default()
        }
    }

    /// How bright rows `from` to `to` of a picture 128 wide are, summed
    /// over the middle half of each.
    fn rows_light(picture: &[u8], from: usize, to: usize) -> i64 {
        (from..to)
            .flat_map(|y| (32..96).map(move |x| (y * 128 + x) * 4))
            .map(|at| {
                i64::from(picture[at]) + i64::from(picture[at + 1]) + i64::from(picture[at + 2])
            })
            .sum()
    }

    #[test]
    fn shade_gathers_at_the_foot_of_a_wall_and_open_ground_is_as_it_was() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        let camera = at_the_wall();
        headless.set_scene(&wall_on_a_floor(0)).unwrap();
        // Bounce light builds up over frames; this is about the shade alone.
        headless.set_bounce(Bounce::Off);
        // Under a sky that is all cloud the light comes from all round,
        // which is the light the shade dims: the sun's it leaves alone.
        headless.set_weather(Weather {
            cloud_cover: 1.0,
            ..Weather::default()
        });
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_ambient_occlusion(AmbientOcclusion::Medium);
        let shaded = headless.screenshot(&camera).unwrap();
        // The same view gives the same shade.
        assert!(headless.screenshot(&camera).unwrap() == shaded);
        // The floor at the wall's foot and the wall just above it are
        // darker; the open floor is not touched at all.
        let (floor, floor_plain) = (rows_light(&shaded, 44, 47), rows_light(&plain, 44, 47));
        let (wall, wall_plain) = (rows_light(&shaded, 41, 44), rows_light(&plain, 41, 44));
        assert!(
            floor * 100 < floor_plain * 98,
            "the floor at the wall's foot is no darker: {floor} against {floor_plain}"
        );
        assert!(
            wall * 100 < wall_plain * 96,
            "the foot of the wall is no darker: {wall} against {wall_plain}"
        );
        let open = 60 * 128 * 4;
        assert!(
            shaded[open..] == plain[open..],
            "open floor changed with nothing near it"
        );
        // A metre from the wall is beyond the search. A game that narrows
        // the search finds less shade at the wall's foot, and none half a
        // metre from it.
        assert_eq!(rows_light(&shaded, 56, 58), rows_light(&plain, 56, 58));
        assert!(rows_light(&shaded, 47, 48) < rows_light(&plain, 47, 48));
        headless.set_game_occlusion(1.0, 0.15);
        let narrow = headless.screenshot(&camera).unwrap();
        assert!(
            rows_light(&narrow, 44, 47) > floor,
            "a narrower search finds as much shade at the wall's foot"
        );
        assert_eq!(rows_light(&narrow, 47, 50), rows_light(&plain, 47, 50));
        // Stronger is darker, and no strength at all is the plain picture,
        // whatever the player's setting.
        headless.set_game_occlusion(2.0, 0.6);
        let strong = headless.screenshot(&camera).unwrap();
        assert!(rows_light(&strong, 44, 47) < floor);
        headless.set_game_occlusion(0.0, 0.6);
        assert!(headless.screenshot(&camera).unwrap() == plain);
        headless.set_game_occlusion(1.0, 0.6);
        headless.set_ambient_occlusion(AmbientOcclusion::Off);
        assert!(headless.screenshot(&camera).unwrap() == plain);
        // Each setting finds about as much shade as the others.
        for quality in [AmbientOcclusion::Low, AmbientOcclusion::High] {
            headless.set_ambient_occlusion(quality);
            let other = rows_light(&headless.screenshot(&camera).unwrap(), 44, 47);
            assert!(
                (other - floor).abs() * 3 < floor_plain - floor,
                "{quality:?} finds {other} at the wall's foot, medium {floor}, none {floor_plain}"
            );
        }
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn steps_far_from_their_pixel_are_read_from_smaller_copies_of_the_depth() {
        // Four times as wide and as high as the other pictures of this
        // wall: the search round the floor nearest the camera is then some
        // ninety pixels wide, and its far steps are read from the copies
        // of the depth an eighth of the picture's size.
        let Some(mut headless) = headless(512, 288) else {
            return;
        };
        headless.gpu().require_validation();
        let camera = at_the_wall();
        headless.set_scene(&wall_on_a_floor(0)).unwrap();
        headless.set_bounce(Bounce::Off);
        headless.set_weather(Weather {
            cloud_cover: 1.0,
            ..Weather::default()
        });
        let light = |picture: &[u8], from: usize, to: usize| -> i64 {
            (from..to)
                .flat_map(|y| (128..384).map(move |x| (y * 512 + x) * 4))
                .map(|at| i64::from(picture[at]) + i64::from(picture[at + 1]))
                .sum()
        };
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_ambient_occlusion(AmbientOcclusion::Medium);
        let shaded = headless.screenshot(&camera).unwrap();
        // The wall's foot is shaded, and a smaller copy's pixel stands for
        // a square of the floor without putting any of it above the floor:
        // open ground is still drawn bit for bit as it was.
        assert!(light(&shaded, 177, 186) * 100 < light(&plain, 177, 186) * 98);
        let open = 240 * 512 * 4;
        assert!(
            shaded[open..] == plain[open..],
            "open floor changed with nothing near it"
        );
        // More than a metre from the wall is beyond the search, until the
        // game widens it: then the wall is found from there, however many
        // pixels off it is.
        assert_eq!(light(&shaded, 224, 232), light(&plain, 224, 232));
        headless.set_game_occlusion(1.0, 2.5);
        let wide = headless.screenshot(&camera).unwrap();
        assert!(
            light(&wide, 224, 232) * 200 < light(&plain, 224, 232) * 199,
            "a wider search finds no shade farther from the wall"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn what_is_see_through_or_cut_out_is_drawn_the_same_with_shade_found() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        let camera = at_the_wall();
        headless.set_bounce(Bounce::Off);
        headless.set_weather(Weather {
            cloud_cover: 1.0,
            ..Weather::default()
        });
        for sheet in [1, 2] {
            headless.set_ambient_occlusion(AmbientOcclusion::Off);
            headless.set_scene(&wall_on_a_floor(sheet)).unwrap();
            let plain = headless.screenshot(&camera).unwrap();
            // The sheet is there, over the open floor.
            headless.set_scene(&wall_on_a_floor(0)).unwrap();
            let bare = headless.screenshot(&camera).unwrap();
            let open = 52 * 128 * 4;
            assert!(plain[open..] != bare[open..], "sheet {sheet} is not seen");
            if sheet == 2 {
                // Its left half is cut away, and the floor shows there.
                let left: Vec<usize> = (60..68)
                    .flat_map(|y| (8..40).map(move |x| (y * 128 + x) * 4))
                    .collect();
                assert!(
                    left.iter()
                        .all(|at| plain[*at..at + 4] == bare[*at..at + 4])
                );
            }
            // With shade found, what is solid is drawn for its depth first
            // and the picture only where that depth is: the pane is still
            // mixed over the floor, and the cut sheet keeps its holes. The
            // sheet is a hand above the floor, so shade lies under it and
            // round it, but not where the camera sees the sheet itself.
            headless.set_scene(&wall_on_a_floor(sheet)).unwrap();
            headless.set_ambient_occlusion(AmbientOcclusion::Medium);
            let shaded = headless.screenshot(&camera).unwrap();
            let middle: Vec<usize> = (61..65)
                .flat_map(|y| (72..120).map(move |x| (y * 128 + x) * 4))
                .collect();
            if sheet == 1 {
                // The pane hides nothing, so the floor under it has no
                // shade, and the pane itself is never shaded.
                assert!(
                    shaded[open..] == plain[open..],
                    "a see-through pane was shaded, or shaded the floor"
                );
            } else {
                // The cut sheet's own top, well inside its edge, sees the
                // whole sky: as it was.
                assert!(
                    middle
                        .iter()
                        .all(|at| shaded[*at..at + 4] == plain[*at..at + 4]),
                    "the top of the cut sheet changed"
                );
                // And the hole is still a hole: the floor, not the sheet.
                let hole = (63 * 128 + 20) * 4;
                // The sheet is blue and the floor grey: by how much bluer
                // than red a pixel is they are told apart.
                let bluer =
                    |picture: &[u8], at: usize| i32::from(picture[at + 2]) - i32::from(picture[at]);
                let on_sheet = (63 * 128 + 100) * 4;
                assert!(bluer(&plain, on_sheet) > bluer(&bare, hole) + 40);
                assert!(
                    (bluer(&shaded, hole) - bluer(&bare, hole)).abs() < 20,
                    "the hole in the cut sheet is not the floor"
                );
                assert!(bluer(&shaded, on_sheet) > bluer(&bare, hole) + 40);
            }
        }
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn the_picture_is_the_same_with_what_is_solid_drawn_for_its_depth_first() {
        // A floor, a wall behind it, and over the floor a see-through pane
        // or a sheet with half of it cut away: things in front of things,
        // holes, and what is mixed over them. Drawn with the depth first,
        // the picture is drawn only where that depth is; drawn without, as
        // it used to be, each part is tested against what came before it.
        // The two must be the same picture, bit for bit.
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        let camera = at_the_wall();
        headless.set_bounce(Bounce::Off);
        for sheet in [0, 1, 2] {
            headless.set_scene(&wall_on_a_floor(sheet)).unwrap();
            headless.view.depth_first = true;
            let first = headless.screenshot(&camera).unwrap();
            headless.view.depth_first = false;
            let without = headless.screenshot(&camera).unwrap();
            assert!(
                first == without,
                "sheet {sheet}: the picture differs with the depth drawn first"
            );
        }
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    /// A room from `low` to `high`, shut on every side, its faces turned
    /// inwards, standing in the open on the floor of [`fluid_scene`].
    fn shut_room(low: [f32; 3], high: [f32; 3]) -> Scene {
        let corner = |x: usize, y: usize, z: usize| {
            [
                [low[0], high[0]][x],
                [low[1], high[1]][y],
                [low[2], high[2]][z],
            ]
        };
        let mut part = lore_assets::MeshPart::default();
        // Each side: its four corners and the way it faces; its triangles
        // go round so that its front faces into the room.
        let sides: [([[usize; 3]; 4], [f32; 3]); 6] = [
            (
                [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
                [0.0, 1.0, 0.0],
            ),
            (
                [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]],
                [0.0, -1.0, 0.0],
            ),
            (
                [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
                [1.0, 0.0, 0.0],
            ),
            (
                [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]],
                [-1.0, 0.0, 0.0],
            ),
            (
                [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]],
                [0.0, 0.0, 1.0],
            ),
            (
                [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
                [0.0, 0.0, -1.0],
            ),
        ];
        for (corners, normal) in sides {
            let first = u32::try_from(part.positions.len()).unwrap();
            for [x, y, z] in corners {
                part.positions.push(corner(x, y, z));
                part.normals.push(normal);
            }
            part.indices
                .extend([0, 2, 1, 0, 3, 2].map(|number| first + number));
        }
        part.bounds = lore_assets::Bounds {
            min: low,
            max: high,
        };
        let mut scene = fluid_scene(false, false);
        // The sun's shadows span the level's bounds, and the room is in
        // them.
        scene.meshes[0].parts[0].bounds = lore_assets::Bounds {
            min: [-50.0, 0.0, -50.0],
            max: [50.0, high[1].max(3.0), 50.0],
        };
        // Seen from inside, whichever way its corners go round.
        part.material = u32::try_from(scene.materials.len()).unwrap();
        scene.materials.push(lore_assets::Material {
            base_color: [0.6, 0.6, 0.6, 1.0],
            double_sided: true,
            ..lore_assets::Material::default()
        });
        scene.meshes.push(lore_assets::Mesh {
            name: None,
            parts: vec![part],
        });
        let mut placed = scene.instances[0].clone();
        placed.mesh = 1;
        scene.instances.push(placed);
        scene
    }

    /// How much light a picture of `scene` from `camera` at a clear noon
    /// shows, with light bouncing and with the old guess in its place, as
    /// the sum of every pixel's red, green and blue; the pictures are saved
    /// as `name-bounced.raw` and `name-guessed.raw` where
    /// `LORE_SAVE_PICTURES` says.
    fn noon_inside(
        headless: &mut Headless,
        scene: &Scene,
        camera: &Camera,
        name: &str,
    ) -> (i64, i64) {
        headless.set_scene(scene).unwrap();
        headless.set_hour(Some(12.0));
        headless.set_weather(Weather {
            cloud_cover: 0.0,
            ..Weather::default()
        });
        let light = |picture: &[u8]| -> i64 {
            picture
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| i64::from(pixel[0]) + i64::from(pixel[1]) + i64::from(pixel[2]))
                .sum()
        };
        headless.set_bounce(Bounce::Off);
        let guessed_picture = headless.screenshot(camera).unwrap();
        let guessed = light(&guessed_picture);
        // Bounce light builds up over a few frames as the probes trace.
        headless.set_bounce(Bounce::Medium);
        headless.let_time_pass(true);
        for _ in 0..12 {
            headless.render(camera).unwrap();
        }
        headless.let_time_pass(false);
        let bounced = headless.screenshot(camera).unwrap();
        // Held still, the same view is the same picture.
        assert!(headless.screenshot(camera).unwrap() == bounced);
        if let Ok(folder) = std::env::var("LORE_SAVE_PICTURES") {
            let _ = std::fs::write(format!("{folder}/{name}-bounced.raw"), &bounced);
            let _ = std::fs::write(format!("{folder}/{name}-guessed.raw"), &guessed_picture);
        }
        (light(&bounced), guessed)
    }

    #[test]
    fn a_room_with_no_window_stays_dark_at_noon_when_light_bounces() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        // A room six metres square and three high, standing in the open at
        // noon.
        let scene = shut_room([-3.0, 0.0, -3.0], [3.0, 3.0, 3.0]);
        let camera = Camera {
            position: [0.0, 1.5, 1.5],
            ..Camera::default()
        };
        let (bounced, guessed) = noon_inside(&mut headless, &scene, &camera, "room");
        assert!(
            bounced * 3 < guessed,
            "the shut room glows: {bounced} with light bouncing against {guessed} guessed"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn a_hall_larger_than_the_copy_of_the_level_stays_dark_at_noon() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        // A hall ninety metres square and twenty high, shut on every side: a
        // ray from a probe goes farther than a ray may, or out of the copy
        // of the level, before it strikes a wall or the roof. Under the
        // roof that is not the sky.
        let scene = shut_room([-45.0, 0.0, -45.0], [45.0, 20.0, 45.0]);
        // Looking down at the floor round the eye, among the probes: the
        // far walls are beyond them, lit by the old guess either way.
        let camera = Camera {
            position: [0.0, 1.7, 0.0],
            pitch: -0.9,
            ..Camera::default()
        };
        let (bounced, guessed) = noon_inside(&mut headless, &scene, &camera, "hall");
        assert!(
            bounced * 3 < guessed,
            "the shut hall glows: {bounced} with light bouncing against {guessed} guessed"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    /// The grey floor, and over the square from `-half` to `half` along x
    /// and z a flat roof `high` up.
    fn roofed(half: f32, high: f32) -> Scene {
        let mut scene = fluid_scene(false, false);
        // The sun's shadows span the level's bounds, which a loaded level
        // knows: these are made here, so they are said here.
        scene.meshes[0].parts[0].bounds = lore_assets::Bounds {
            min: [-500.0, 0.0, -500.0],
            max: [500.0, 0.0, 500.0],
        };
        let roof = lore_assets::Mesh {
            name: None,
            parts: vec![lore_assets::MeshPart {
                positions: [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]
                    .map(|[x, z]| [x * half, high, z * half])
                    .to_vec(),
                normals: vec![[0.0, 1.0, 0.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                bounds: lore_assets::Bounds {
                    min: [-half, high, -half],
                    max: [half, high, half],
                },
                ..lore_assets::MeshPart::default()
            }],
        };
        scene.meshes.push(roof);
        let mut placed = scene.instances[0].clone();
        placed.mesh = 1;
        scene.instances.push(placed);
        scene
    }

    #[test]
    fn rain_and_snow_are_seen_in_a_lamps_light_on_a_dark_night() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        // A street lamp on a moonless night, three metres up and six ahead,
        // under a sky that is all cloud. The air's own glow round it is
        // left out, so that only what falls through its light is counted.
        headless.set_scene(&fluid_scene(false, false)).unwrap();
        headless.set_hour(Some(23.0 + 24.0 * 15.0));
        headless.set_light_shafts(false);
        headless.set_lamps(&[Lamp {
            at: [0.0, 3.0, -6.0],
            candela: 120.0,
            radius: 0.05,
            ..Lamp::default()
        }]);
        let camera = Camera {
            position: [0.0, 1.6, 0.0],
            pitch: 0.1,
            ..Camera::default()
        };
        let mut seen = |rain: f32, temperature: f32| {
            headless.set_weather(Weather {
                cloud_cover: 1.0,
                precipitation: rain,
                temperature,
                ..Weather::default()
            });
            headless.screenshot(&camera).unwrap()
        };
        let (dry, rained, snowed) = (seen(0.0, 10.0), seen(1.0, 10.0), seen(1.0, -5.0));
        if let Ok(folder) = std::env::var("LORE_SAVE_PICTURES") {
            let _ = std::fs::write(format!("{folder}/lamp-dry.raw"), &dry);
            let _ = std::fs::write(format!("{folder}/lamp-rain.raw"), &rained);
            let _ = std::fs::write(format!("{folder}/lamp-snow.raw"), &snowed);
        }
        // The air round the lamp, above the ground it lights.
        let light = |picture: &[u8]| -> i64 {
            (8..34)
                .flat_map(|row| (40..88).map(move |column| (row * 128 + column) * 4))
                .map(|at| {
                    (0..3)
                        .map(|colour| i64::from(picture[at + colour]))
                        .sum::<i64>()
                })
                .sum()
        };
        let (none, drops, flakes) = (light(&dry), light(&rained), light(&snowed));
        assert!(
            drops * 10 > none * 11,
            "no rain is seen in the lamp's light: {drops} against {none} with none"
        );
        assert!(
            flakes * 10 > none * 11,
            "no snow is seen in the lamp's light: {flakes} against {none} with none"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn snow_lies_in_the_open_and_not_under_a_roof() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        // A shelter twelve metres across with its roof three metres up,
        // seen from the open ground in front of it at noon. Snow falls
        // nearly straight down: under the roof, even just inside its edge
        // where most of the sky is still in sight, none lies.
        headless.set_scene(&roofed(6.0, 3.0)).unwrap();
        headless.set_hour(Some(12.0));
        let camera = Camera {
            position: [0.0, 1.6, 9.0],
            pitch: -0.25,
            ..Camera::default()
        };
        let mut snowed = |depth: f32| {
            headless.set_weather(Weather {
                cloud_cover: 0.3,
                temperature: -5.0,
                snow: depth,
                ..Weather::default()
            });
            headless.screenshot(&camera).unwrap()
        };
        let (bare, white) = (snowed(0.0), snowed(0.1));
        if let Ok(folder) = std::env::var("LORE_SAVE_PICTURES") {
            let _ = std::fs::write(format!("{folder}/roof-bare.raw"), &bare);
            let _ = std::fs::write(format!("{folder}/roof-snow.raw"), &white);
        }
        // How bright rows `from` to `to` are, across the middle half of
        // the picture.
        let light = |picture: &[u8], from: usize, to: usize| -> i64 {
            (from..to)
                .flat_map(|row| (32..96).map(move |column| (row * 128 + column) * 4))
                .map(|at| {
                    (0..3)
                        .map(|colour| i64::from(picture[at + colour]))
                        .sum::<i64>()
                })
                .sum()
        };
        // The open floor in front, from one to two metres out from the
        // roof's edge, is white; the floor under the roof, from a little
        // inside its edge to five metres in, is as it was.
        let (open, inside) = ((55, 70), (33, 44));
        let (lies, bare_open) = (light(&white, open.0, open.1), light(&bare, open.0, open.1));
        assert!(
            lies * 10 > bare_open * 15,
            "no snow lies in the open: {lies} against {bare_open} bare"
        );
        let (under, was) = (
            light(&white, inside.0, inside.1),
            light(&bare, inside.0, inside.1),
        );
        assert!(
            under * 10 < was * 12,
            "snow lies under the roof: {under} against {was} bare"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn air_in_the_shade_of_a_roof_does_not_glow_with_the_sun() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        // A roof forty metres across, four metres up, at noon in mist:
        // looking out from under it, the first twenty metres of air are in
        // its shadow, and the sun lights the air beyond.
        headless.set_scene(&roofed(20.0, 4.0)).unwrap();
        headless.set_hour(Some(12.0));
        headless.set_weather(Weather {
            cloud_cover: 0.0,
            fog: 0.6,
            ..Weather::default()
        });
        // Facing the sun's side: mist sends most of the sun's light on, so
        // that is where its share of the fog's light is greatest.
        let camera = Camera {
            position: [0.0, 1.5, 0.0],
            yaw: -2.47,
            ..Camera::default()
        };
        let light = |picture: &[u8]| -> i64 {
            picture
                .as_chunks::<4>()
                .0
                .iter()
                .skip(128 * 30)
                .take(128 * 12)
                .map(|pixel| i64::from(pixel[0]) + i64::from(pixel[1]) + i64::from(pixel[2]))
                .sum()
        };
        let lit = headless.screenshot(&camera).unwrap();
        // The same camera gives the same picture.
        assert!(headless.screenshot(&camera).unwrap() == lit);
        headless.set_light_shafts(false);
        let unlit = headless.screenshot(&camera).unwrap();
        let (shaded, glowing) = (light(&lit), light(&unlit));
        assert!(
            shaded + 128 * 12 * 3 < glowing,
            "the air under the roof glows as if it were in the sun: {shaded} against {glowing}"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn no_fluid_changes_nothing_even_after_some_was_drawn_and_cleared() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        headless.set_scene(&fluid_scene(false, false)).unwrap();
        let camera = looking_down();
        let never = headless.screenshot(&camera).unwrap();
        headless.set_fluid(&[]);
        assert!(headless.screenshot(&camera).unwrap() == never);
        headless.set_fluid(&puddle(0));
        let drawn = headless.screenshot(&camera).unwrap();
        assert!(drawn != never, "the puddle is not drawn at all");
        headless.set_fluid(&[]);
        assert!(headless.screenshot(&camera).unwrap() == never);
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn a_puddle_changes_the_pixels_where_it_lies_and_no_others() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        headless.set_scene(&fluid_scene(false, false)).unwrap();
        let camera = looking_down();
        let bare = headless.screenshot(&camera).unwrap();
        headless.set_fluid_looks(&[tar()]);
        headless.set_fluid(&puddle(0));
        let wet = headless.screenshot(&camera).unwrap();
        let differ = |[x, y]: [usize; 2]| fluid_light(&bare, [x, y]) != fluid_light(&wet, [x, y]);
        assert!(differ([64, 36]), "nothing changed under the puddle");
        for corner in [[2, 2], [125, 2], [2, 69], [125, 69], [4, 36], [123, 36]] {
            assert!(!differ(corner), "{corner:?} changed, far from the puddle");
        }
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn a_puddle_behind_a_solid_sheet_is_not_seen() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        headless.set_scene(&fluid_scene(true, false)).unwrap();
        let camera = looking_down();
        let bare = headless.screenshot(&camera).unwrap();
        headless.set_fluid_looks(&[tar()]);
        headless.set_fluid(&puddle(0));
        assert!(headless.screenshot(&camera).unwrap() == bare);
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn milk_is_lighter_than_the_floor_and_tar_is_darker() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        headless.set_scene(&fluid_scene(false, false)).unwrap();
        let camera = looking_down();
        let bare = fluid_light(&headless.screenshot(&camera).unwrap(), [64, 36]);
        headless.set_fluid_looks(&[milk(), tar()]);
        headless.set_fluid(&puddle(0));
        let white = fluid_light(&headless.screenshot(&camera).unwrap(), [64, 36]);
        headless.set_fluid(&puddle(1));
        let black = fluid_light(&headless.screenshot(&camera).unwrap(), [64, 36]);
        assert!(
            black < bare && bare < white,
            "tar {black}, floor {bare}, milk {white}"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    /// A lying drop of tar of `radius` metres at `x` on the floor.
    fn lying_at(x: f32, radius: f32) -> FluidDrop {
        FluidDrop {
            place: [x, 0.0, 0.0],
            radius,
            facing: [0.0, 1.0, 0.0],
            flat: 1.0,
            long: 1.0,
            thick: 0.004,
            kind: 0,
        }
    }

    #[test]
    fn lying_drops_near_each_other_melt_into_one_film_and_far_ones_do_not() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        headless.set_scene(&fluid_scene(false, false)).unwrap();
        let camera = looking_down();
        let bare = headless.screenshot(&camera).unwrap();
        headless.set_fluid_looks(&[tar()]);
        // Two drops 1.6 radii apart: the ground between them is covered.
        headless.set_fluid(&[lying_at(-0.24, 0.3), lying_at(0.24, 0.3)]);
        let near = headless.screenshot(&camera).unwrap();
        assert!(
            fluid_light(&near, [64, 36]) < fluid_light(&bare, [64, 36]),
            "the film does not join two drops close together"
        );
        // Six radii apart: each is its own drop, and the floor between is
        // dry.
        headless.set_fluid(&[lying_at(-0.9, 0.3), lying_at(0.9, 0.3)]);
        let far = headless.screenshot(&camera).unwrap();
        assert_eq!(fluid_light(&far, [64, 36]), fluid_light(&bare, [64, 36]));
        assert!(far != bare, "the drops far apart are not drawn");
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn a_stream_thinner_than_a_pixel_is_still_seen() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        headless.set_scene(&fluid_scene(false, false)).unwrap();
        let camera = looking_down();
        let bare = headless.screenshot(&camera).unwrap();
        headless.set_fluid_looks(&[tar()]);
        // Two millimetres across, two metres under the eye, where a pixel
        // is centimetres wide.
        let stream: Vec<FluidDrop> = (-40..=40)
            .map(|step| FluidDrop {
                place: [step as f32 * 0.002, 2.0, 0.0],
                radius: 0.001,
                facing: [1.0, 0.0, 0.0],
                flat: 0.0,
                long: 2.0,
                thick: 0.0,
                kind: 0,
            })
            .collect();
        headless.set_fluid(&stream);
        let seen = headless.screenshot(&camera).unwrap();
        assert!(seen != bare, "a hair-thin stream vanishes");
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn no_water_look_is_the_same_as_never_having_set_one() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        headless.set_scene(&fluid_scene(false, true)).unwrap();
        let camera = looking_down();
        let never = headless.screenshot(&camera).unwrap();
        headless.set_water_look(Some(tar()));
        let black = headless.screenshot(&camera).unwrap();
        assert!(black != never, "the water look changed nothing");
        headless.set_water_look(None);
        assert!(headless.screenshot(&camera).unwrap() == never);
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn the_same_drops_are_drawn_the_same_every_time() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        headless.set_scene(&fluid_scene(false, false)).unwrap();
        let camera = looking_down();
        headless.set_fluid_looks(&[milk()]);
        headless.set_fluid(&puddle(0));
        let first = headless.screenshot(&camera).unwrap();
        assert!(headless.screenshot(&camera).unwrap() == first);
        headless.set_fluid(&puddle(0));
        assert!(headless.screenshot(&camera).unwrap() == first);
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    /// A floor two hundred metres across with stripes a quarter of a metre
    /// wide, black and white by turns, running north to south.
    fn striped_floor() -> Scene {
        use lore_assets::{Image, Instance, Material, Mesh, MeshPart, Texture, TextureRef};

        let mut scene = Scene::default();
        let corners = [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]];
        scene.meshes.push(Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: corners.map(|[x, z]| [x * 100.0, 0.0, z * 100.0]).to_vec(),
                normals: vec![[0.0, 1.0, 0.0]; 4],
                // The picture is laid every half metre, both ways.
                uv0: Some(corners.map(|[x, z]| [x * 200.0, z * 200.0]).to_vec()),
                indices: vec![0, 1, 2, 0, 2, 3],
                ..MeshPart::default()
            }],
        });
        // Black on the left and white on the right: its one smaller copy
        // is a single grey pixel, so a stripe is seen only where the
        // picture is read at full size.
        scene.images.push(Image {
            name: None,
            width: 2,
            height: 2,
            pixels: [[0, 0, 0, 255], [255, 255, 255, 255]].repeat(2).concat(),
        });
        scene.textures.push(Texture {
            name: None,
            image: 0,
            sampler: lore_assets::Sampler::default(),
        });
        scene.materials.push(Material {
            base_color_texture: Some(TextureRef {
                texture: 0,
                uv_set: 0,
                transform: None,
            }),
            ..Material::default()
        });
        let mut transform = [[0.0; 4]; 4];
        for (axis, column) in transform.iter_mut().enumerate() {
            column[axis] = 1.0;
        }
        scene.instances.push(Instance {
            mesh: 0,
            transform,
            mirrored: false,
            name: None,
            extras: None,
        });
        scene
    }

    /// How much the stripes of the [`striped_floor`] show some ten metres
    /// ahead of an eye one metre over it that looks along it: how far each
    /// pixel of those rows is from the one beside it, added up.
    fn stripes_seen(headless: &mut Headless) -> i64 {
        const SIZE: [usize; 2] = [256, 144];
        let camera = Camera {
            position: [0.0, 1.0, 0.0],
            ..Camera::default()
        };
        let picture = headless.screenshot(&camera).unwrap();
        let light = |x: usize, y: usize| {
            let at = (y * SIZE[0] + x) * 4;
            picture[at..at + 3]
                .iter()
                .map(|&c| i64::from(c))
                .sum::<i64>()
        };
        (STRIPED_ROWS.0..STRIPED_ROWS.1)
            .flat_map(|y| (1..SIZE[0]).map(move |x| (x, y)))
            .map(|(x, y)| (light(x, y) - light(x - 1, y)).abs())
            .sum()
    }

    /// The rows of the picture [`stripes_seen`] looks at.
    const STRIPED_ROWS: (usize, usize) = (78, 88);

    #[test]
    fn a_floor_seen_along_its_length_is_sharper_with_anisotropic_filtering() {
        let Some(mut headless) = headless(256, 144) else {
            return;
        };
        headless.gpu().require_validation();
        // The stripes blur only where the picture has a smaller copy to
        // be read from, and sharpen only where the card reads at a slant.
        if !headless.gpu().anisotropy() || !crate::texture::shrinks(headless.gpu())[0] {
            eprintln!(
                "Skipping: this graphics card cannot read textures at a slant, or cannot \
                 make their smaller copies."
            );
            return;
        }
        // As a new player has it: sixteen samples.
        headless.set_scene(&striped_floor()).unwrap();
        let sharp = stripes_seen(&mut headless);
        // Off, with the scene already there: only the samplers are made again.
        headless.set_anisotropic_filtering(AnisotropicFiltering::Off);
        let plain = stripes_seen(&mut headless);
        assert!(
            sharp > 4 * plain.max(1),
            "sixteen samples show the stripes ({sharp}) and one blurs them to grey ({plain})"
        );
        // Two samples show more of them than one, four more than two, and
        // from there on they are all but all there.
        let mut seen = |filtering| {
            headless.set_anisotropic_filtering(filtering);
            stripes_seen(&mut headless)
        };
        let two = seen(AnisotropicFiltering::X2);
        let four = seen(AnisotropicFiltering::X4);
        let eight = seen(AnisotropicFiltering::X8);
        assert!(
            plain < two && two < four && four <= eight.min(sharp),
            "{plain}, {two}, {four}, {eight}, {sharp}"
        );
        // Back at sixteen, the picture is as it was.
        let close = |one: i64, other: i64| (one - other).abs() * 50 < one.max(other);
        let again = seen(AnisotropicFiltering::X16);
        assert!(close(again, sharp), "{again} and {sharp}");
        // A scene set after the setting is read as the setting says.
        headless.set_anisotropic_filtering(AnisotropicFiltering::Off);
        headless.set_scene(&striped_floor()).unwrap();
        let later = stripes_seen(&mut headless);
        assert!(close(later, plain), "{later} and {plain}");
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn frames_are_drawn_and_timed_with_no_window() {
        let Some(mut headless) = headless(96, 64) else {
            return;
        };
        headless.gpu().require_validation();
        let (width, height) = headless.drawn_size();
        assert_eq!((width, height), (96, 64));
        // With nothing to draw, and then with a scene.
        headless.render(&Camera::default()).unwrap();
        headless.set_scene(&Scene::default()).unwrap();
        // From the second of these frames on, a HUD is drawn over them, and
        // on the last a menu with letters the graphics card has not seen.
        let mut ui = lore_ui::Ui::default();
        ui.hud_text(lore_ui::Anchor::BottomRight, [0.0, 0.0], 32.0, "30 / 90");
        let menu = ui.add_screen("Paused", |screen| {
            screen.button("Resume");
        });
        for step in 0..4_u8 {
            if step == 3 {
                ui.open(menu.clone().unwrap()).unwrap();
            }
            if step > 0 {
                ui.update(&lore_ui::UiInput::default(), 1.0 / 60.0, [96, 64]);
                headless.set_ui(ui.draw());
            }
            let camera = Camera {
                yaw: f32::from(step) * 0.1,
                ..Camera::default()
            };
            let time = headless.render(&camera).unwrap();
            assert_eq!(time.total(), time.recording + time.drawing);
        }
        // The finished picture can be read back, and the menu is in it.
        let with_menu = headless.screenshot(&Camera::default()).unwrap();
        assert_eq!(with_menu.len(), 96 * 64 * 4);
        headless.set_ui(lore_ui::Ui::default().draw());
        let without = headless.screenshot(&Camera::default()).unwrap();
        assert_ne!(with_menu, without);
        assert_eq!(without, headless.screenshot(&Camera::default()).unwrap());
        // A frame can be drawn after one was read back.
        headless.render(&Camera::default()).unwrap();
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    /// The camera that looks straight at the usual sun.
    fn at_the_sun() -> Camera {
        let towards = [0.399_44_f32, 0.768_92, 0.499_3];
        Camera {
            yaw: (-towards[0]).atan2(-towards[2]),
            pitch: towards[1].asin(),
            ..Camera::default()
        }
    }

    /// Red, green and blue of the pixel at `x`, `y` of a `width`-wide picture.
    fn pixel(picture: &[u8], width: usize, x: usize, y: usize) -> [i32; 3] {
        let at = (y * width + x) * 4;
        [picture[at], picture[at + 1], picture[at + 2]].map(i32::from)
    }

    fn sum(colour: [i32; 3]) -> i32 {
        colour.iter().sum()
    }

    #[test]
    fn the_glow_spreads_the_suns_light_and_without_it_the_picture_is_as_it_was() {
        const WIDTH: usize = 128;
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        // Below the sun, where the sky is lit but the sun is not drawn.
        let camera = Camera {
            pitch: at_the_sun().pitch - 0.35,
            ..at_the_sun()
        };
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_look(
            Look {
                bloom: 0.0,
                ..Look::plain()
            },
            0.0,
        );
        assert_eq!(
            headless.screenshot(&camera).unwrap(),
            plain,
            "no glow, the same picture"
        );
        headless.set_look(
            Look {
                bloom: 1.0,
                ..Look::plain()
            },
            0.0,
        );
        let glowing = headless.screenshot(&camera).unwrap();
        // Round the brightest pixel, the sun, the picture glows brighter.
        let brightest = (0..plain.len() / 4)
            .max_by_key(|index| sum(pixel(&plain, WIDTH, index % WIDTH, index / WIDTH)))
            .unwrap_or_default();
        let (x, y) = (brightest % WIDTH, brightest / WIDTH);
        let ring = |picture: &[u8]| {
            [
                (x.saturating_sub(6), y),
                ((x + 6).min(WIDTH - 1), y),
                (x, (y + 6).min(71)),
            ]
            .into_iter()
            .map(|(x, y)| sum(pixel(picture, WIDTH, x, y)))
            .sum::<i32>()
        };
        let (before, after) = (ring(&plain), ring(&glowing));
        assert!(
            after > before + 6,
            "round the sun at {x}, {y}: {before} became {after}"
        );
        // The player can turn the glow off whatever the game says.
        headless.set_look_shares(LookShares {
            bloom: 0.0,
            ..LookShares::default()
        });
        assert_eq!(
            headless.screenshot(&camera).unwrap(),
            plain,
            "the player's share"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn the_vignette_darkens_the_corners_and_leaves_the_middle() {
        const WIDTH: usize = 128;
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        let camera = Camera {
            yaw: 0.9,
            pitch: 1.0,
            ..Camera::default()
        };
        let plain = headless.screenshot(&camera).unwrap();
        headless.set_look(
            Look {
                vignette: 1.0,
                ..Look::plain()
            },
            0.0,
        );
        let vignetted = headless.screenshot(&camera).unwrap();
        let (before, after) = (pixel(&plain, WIDTH, 0, 0), pixel(&vignetted, WIDTH, 0, 0));
        assert!(
            sum(after) * 10 < sum(before) * 8,
            "the corner: {before:?} became {after:?}"
        );
        let (before, after) = (
            pixel(&plain, WIDTH, 64, 36),
            pixel(&vignetted, WIDTH, 64, 36),
        );
        assert!(
            (sum(after) - sum(before)).abs() <= 6,
            "the middle: {before:?} became {after:?}"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn warmth_turns_the_sky_orange_and_no_saturation_turns_it_grey() {
        const WIDTH: usize = 128;
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        let camera = Camera {
            yaw: 0.9,
            pitch: 1.0,
            ..Camera::default()
        };
        let middle = |headless: &mut Headless, look: Look| {
            headless.set_look(look, 0.0);
            pixel(&headless.screenshot(&camera).unwrap(), WIDTH, 64, 36)
        };
        let [red, _, blue] = middle(&mut headless, Look::plain());
        let [warm_red, _, warm_blue] = middle(
            &mut headless,
            Look {
                warmth: 1.0,
                ..Look::plain()
            },
        );
        assert!(
            warm_red - warm_blue > red - blue + 20,
            "{red} {blue} became {warm_red} {warm_blue}"
        );
        let [r, g, b] = middle(
            &mut headless,
            Look {
                saturation: 0.0,
                ..Look::plain()
            },
        );
        assert!(
            (r - g).abs() <= 3 && (g - b).abs() <= 3,
            "grey: {r} {g} {b}"
        );
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }

    #[test]
    fn each_curve_grain_and_fringes_draw_a_picture_of_their_own() {
        let Some(mut headless) = headless(128, 72) else {
            return;
        };
        headless.gpu().require_validation();
        let camera = Camera {
            pitch: 0.1,
            ..Camera::default()
        };
        let plain = headless.screenshot(&camera).unwrap();
        let mean = |picture: &[u8]| {
            picture.iter().map(|byte| u64::from(*byte)).sum::<u64>() / picture.len() as u64
        };
        for look in [
            Look::default(),
            Look {
                tone_mapping: ToneMapping::AgX,
                ..Look::plain()
            },
            Look {
                grain: 1.0,
                ..Look::plain()
            },
            Look {
                chromatic_aberration: 1.0,
                ..Look::plain()
            },
            Look {
                contrast: 1.5,
                shadows: [0.5, 0.6, 1.0],
                highlights: [1.0, 0.8, 0.6],
                tint: 0.3,
                ..Look::plain()
            },
        ] {
            headless.set_look(look, 0.0);
            let picture = headless.screenshot(&camera).unwrap();
            assert_ne!(picture, plain, "{look:?}");
            // Nothing makes the picture black or white.
            let mean = mean(&picture);
            assert!((40..230).contains(&mean), "{look:?}: a mean of {mean}");
        }
        drop(headless);
        assert_eq!(validation_errors(), 0);
    }
}
