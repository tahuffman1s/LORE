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
use crate::fluid::{FluidDrop, FluidLook};
use crate::font::Font;
use crate::gpu::Gpu;
use crate::lamp_shadow::LampShadows;
use crate::lamps::Lamp;
use crate::mesh::{Contents, MeshPipeline, Meshes};
use crate::overlay::Overlay;
use crate::swapchain::COLOUR;
use crate::tonemap::{Canvas, Over, Then, TonemapPipeline};
use crate::tracks::Tread;
use crate::ui_pass::UiPass;
use crate::upscale::Upscaling;
use crate::view::{Mirroring, SceneView};
use crate::water::Stir;
use crate::weather::Weather;

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
        let tonemap = TonemapPipeline::new(&gpu, FORMAT)?;
        let ui = UiPass::new(&gpu, FORMAT, 1)?;
        // SAFETY: the picture is `view`'s, which lives as long as `tonemap`
        // does, and nothing is being drawn yet.
        unsafe { tonemap.read_from(view.picture()) };

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
        self.meshes = None;
        self.meshes = Some(Meshes::new(&self.gpu, scene)?);
        self.lamps.clear();
        self.lamps
            .extend(scene.lights.iter().filter_map(Lamp::from_light));
        // Nothing that flew in the old level is in the new one.
        self.effects.clear();
        // Nothing of the old level belongs in the new one's first frame.
        self.view.cut();
        Ok(())
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

    /// Says how wet the ground is, from 0 for dry to 1 for soaked: rain
    /// takes half a minute to soak it, and a picture of a wet street
    /// should not have to wait.
    pub fn set_wet_ground(&mut self, wet: f32) {
        self.effects.soak(wet);
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
            && (weather.rows[0][2] > 0.0 || self.meshes.as_ref().is_some_and(Meshes::glossy));
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
            self.view.measured = finished && self.eye.adaptation.adapts();
            self.view.shafts = self.light_shafts;
            self.view.lamp_shadows = self.lamp_shadows;
            self.view.bounce = self.bounce;
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
                },
                camera,
                FRAME_SECONDS,
            )?;
            if finished {
                self.ui.prepare(self.commands, 0, &mut self.overlay)?;
                self.tonemap.record(
                    self.commands,
                    Canvas {
                        image: self.image,
                        view: self.image_view,
                        extent: self.extent,
                    },
                    self.eye.multiplier(),
                    Over {
                        ui: Some(&self.ui),
                        #[cfg(feature = "panels")]
                        panels: Some(&mut self.panels),
                    },
                    Then::Copy,
                )?;
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
            origin: [-5.0, -5.0],
            cell: 0.25,
            size: [points, points],
            depths,
            colour: [0.0, 0.02, 0.02, 0.3],
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
}
