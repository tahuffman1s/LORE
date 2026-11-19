//! A game's own shaders: screen effects over the finished picture and surface
//! shaders that change what a material looks like.
//!
//! This part of the module needs no graphics card: the templates a game's text
//! is put into ([`template`]), the list of what the game wants against what is
//! drawn ([`book`]), the thread that compiles ([`worker`]) and the things kept
//! until the card is done with them ([`retire`]). [`GameShaders`] ties them
//! to the graphics card, and [`screen`] draws the screen effects.

// A shader error is a few strings, built once per mistake and never in a loop,
// so boxing it would only make every caller clumsier (as in `lore-shader`).
#![allow(clippy::result_large_err)]

mod book;
mod retire;
mod screen;
mod surface;
mod template;
#[cfg(test)]
mod tests;
mod worker;
mod works;

use std::sync::Arc;

use ash::vk;

use self::book::Book;
use self::retire::Retiring;
use self::screen::{Frame, Targets};
use self::template::fault;
use self::worker::Done;
use self::works::{Built, Works};
use crate::camera::Camera;
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::mesh::{MeshPipeline, Meshes};
use crate::view::SceneView;

pub(crate) use surface::SurfaceDraws;
pub use template::check_game_shader;

/// How many screen effects can be on at once.
pub const MOST_SCREEN_EFFECTS: usize = 8;

/// How many surface shaders can be on at once.
pub const MOST_SURFACE_SHADERS: usize = 8;

/// What a game shader is used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderUse<'a> {
    /// Drawn over the whole finished picture: `fn screen(in: ScreenIn) -> vec3f`.
    Screen,
    /// Changes the surface of every material of the level named `material`:
    /// `fn surface(in: SurfaceIn) -> Surface`.
    Surface {
        /// The name of the material in the level.
        material: &'a str,
    },
}

/// One shader the game wants drawn, as the script last gave it.
#[derive(Clone, Copy, Debug)]
pub struct GameShader<'a> {
    /// The game's name for it, such as `heat`. Reports carry it.
    pub name: &'a str,
    /// The file as errors show it, such as `shaders/heat.wgsl`.
    pub file: &'a str,
    /// The WGSL. It is read only when `revision` changed.
    pub text: &'a str,
    /// What the shader is for.
    pub used_as: ShaderUse<'a>,
    /// The values the script set, already checked against the shader's
    /// `//! param` lines.
    pub values: &'a [(String, lore_shader::ParamValue)],
    /// Raised by the script side whenever the text changes.
    pub revision: u64,
}

/// What became of a shader the game asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderReport {
    /// The game's name for the shader.
    pub name: String,
    /// The file as errors show it.
    pub file: String,
    /// The material a surface shader is for; `None` for a screen effect.
    pub material: Option<String>,
    /// The revision of the text this is about.
    pub revision: u64,
    /// `Ok`: this revision is drawn from now on. `Err`: what is wrong; its
    /// `note` says what is drawn instead.
    pub outcome: Result<(), lore_shader::ShaderError>,
}

/// How long the shader clock runs before it starts again, in seconds. An
/// `f32` still has a fine step after an hour, and a shader sees it as time.
const CLOCK_WRAPS_AT: f64 = 3600.0;

/// The most bytes a shader's values take: 32 values of 16 bytes.
const MOST_VALUE_BYTES: usize = 512;

/// The file reports name when the graphics driver fails.
const THIS_FILE: &str = "crates/lore-render/src/game_shaders/mod.rs";

/// What a renderer keeps of the game's shaders: what the game wants, what is
/// built of it, and what it takes to draw it. It makes nothing on the
/// graphics card until a game wants a shader, and then only what is needed.
pub(crate) struct GameShaders {
    /// What the game wants against what is drawn.
    book: Book<Built>,
    /// Made at the first frame that has a shader.
    works: Option<Works>,
    gpu: Arc<Gpu>,
    /// How many frames can be in flight.
    frames: usize,
    /// The shader clock, in seconds, which starts again each hour.
    clock: f64,
    /// How many frames were handed to the graphics card.
    submitted: u64,
    /// What was said about a failure of the graphics driver.
    faults: Vec<ShaderReport>,
    /// Set once the driver failed, so it is said once and not tried every frame.
    failed: bool,
}

impl GameShaders {
    /// Nothing on the graphics card yet. `frames` is how many frames can be
    /// in flight at once.
    pub(crate) fn new(gpu: &Arc<Gpu>, frames: usize) -> Self {
        Self {
            book: Book::new(),
            works: None,
            gpu: Arc::clone(gpu),
            frames: frames.max(1),
            clock: 0.0,
            submitted: 0,
            faults: Vec::new(),
            failed: false,
        }
    }

    /// Takes the game's whole list, in drawing order. Cheap when nothing
    /// changed. A shader that is gone is retired, not dropped, since a frame
    /// in flight may still draw it.
    pub(crate) fn want(&mut self, shaders: &[GameShader<'_>]) {
        let now = self.submitted;
        let (worker, retiring) = match &mut self.works {
            Some(works) => (Some(&works.worker), Some(&mut works.retiring)),
            None => (None, None),
        };
        let mut retiring = retiring;
        self.book.want(
            shaders,
            |id| {
                if let Some(worker) = worker {
                    worker.forget(id);
                }
            },
            |built| {
                if let Some(retiring) = retiring.as_deref_mut() {
                    retiring.push(now, built);
                }
            },
        );
    }

    /// Reports since the last call.
    pub(crate) fn take_reports(&mut self) -> Vec<ShaderReport> {
        let mut reports = self.book.take_reports();
        reports.append(&mut self.faults);
        reports
    }

    /// Says that the graphics driver failed to make something. The frame goes
    /// on without effects.
    fn failed_with(&mut self, error: &RenderError) {
        if self.failed {
            return;
        }
        log::warn!("Game shaders are not drawn: {error}");
        self.failed = true;
        self.faults.push(ShaderReport {
            name: "game shaders".to_owned(),
            file: THIS_FILE.to_owned(),
            material: None,
            revision: 0,
            outcome: Err(fault(THIS_FILE, error.to_string())
                .with_note("Nothing of the game's shaders is drawn.")),
        });
    }

    /// A level was set: which material each surface shader draws is
    /// worked out again, and what is wrong with it is said again.
    pub(crate) fn scene_changed(&mut self) {
        if let Some(works) = &mut self.works {
            works.surfaces.scene_changed();
        }
    }

    /// Makes the thread and what it builds with, if a game wants anything.
    fn ensure_works(&mut self, mesh: &MeshPipeline) {
        if self.works.is_some() || self.failed || self.book.is_empty() {
            return;
        }
        match Works::new(&self.gpu, self.frames, mesh) {
            Ok(works) => self.works = Some(works),
            Err(error) => self.failed_with(&error),
        }
    }

    /// Gives the thread what it has not been asked to build, and installs
    /// what it finished. `wait` holds on until it has finished everything.
    fn collect(&mut self, wait: bool) {
        let now = self.submitted;
        let Some(works) = &mut self.works else {
            return;
        };
        self.book.asks(|job| works.worker.ask(job));
        if wait {
            works.worker.wait_idle();
        }
        while let Some(done) = works.worker.done() {
            install(&mut self.book, &mut works.retiring, now, done);
        }
    }

    /// Waits for every shader the game wants to be built, and installs them:
    /// for pictures that must come out the same every time.
    pub(crate) fn settle(&mut self, mesh: &MeshPipeline) {
        self.ensure_works(mesh);
        self.collect(true);
    }

    /// Gets ready to record a frame in slot `slot`, whose last frame has
    /// finished: installs what was built, lets go of what the card is done
    /// with, writes the values and makes the pictures the effects need, for a
    /// picture `output` pixels in size. `seconds` is how long the frame took.
    /// `meshes` is the level, whose materials the surface shaders are matched
    /// to; `mesh` is what draws it.
    ///
    /// Never fails: if the graphics driver does, it is said, and the frame
    /// goes on without effects.
    pub(crate) fn start_frame(
        &mut self,
        slot: usize,
        seconds: f32,
        output: vk::Extent2D,
        mesh: &MeshPipeline,
        meshes: Option<&Meshes>,
    ) {
        if self.book.is_empty() && self.works.is_none() {
            return;
        }
        self.ensure_works(mesh);
        self.collect(false);
        let now = self.submitted;
        if seconds.is_finite() && seconds > 0.0 {
            self.clock = (self.clock + f64::from(seconds)).rem_euclid(CLOCK_WRAPS_AT);
        }
        let Some(works) = &mut self.works else {
            return;
        };
        works.retiring.release(now);
        works.old_targets.release(now);

        let mut failure = None;
        let mut live = 0;
        let mut bytes = [0_u8; MOST_VALUE_BYTES];
        for entry in self.book.screens() {
            let Some(drawn) = entry.live() else {
                continue;
            };
            let Some(shader) = drawn.built.screen() else {
                continue;
            };
            live += 1;
            let Some(out) = bytes.get_mut(..drawn.block.size()) else {
                continue;
            };
            drawn.block.write(entry.values(), out);
            let Some(buffer) = shader.buffers.get(slot) else {
                continue;
            };
            // SAFETY: this slot's last frame has finished, so the card is not
            // reading the buffer, and it is as long as the block.
            if let Err(error) = unsafe { buffer.write(out) } {
                failure = Some(error);
            }
        }

        let mut surfaces = false;
        for entry in self.book.surfaces() {
            let Some(drawn) = entry.live() else {
                continue;
            };
            let Some(shader) = drawn.built.surface() else {
                continue;
            };
            surfaces = true;
            let Some(out) = bytes.get_mut(..drawn.block.size()) else {
                continue;
            };
            drawn.block.write(entry.values(), out);
            let Some(buffer) = shader.buffers.get(slot) else {
                continue;
            };
            // SAFETY: as for the screen effects' buffers above.
            if let Err(error) = unsafe { buffer.write(out) } {
                failure = Some(error);
            }
        }
        if surfaces {
            // A shader sees a fraction of a second of an hour, which an f32 holds.
            #[allow(clippy::cast_possible_truncation)]
            let time = (self.clock as f32).to_ne_bytes();
            let mut clock = [0_u8; 16];
            clock[..4].copy_from_slice(&time);
            if let Some(buffer) = works.shared.clocks.get(slot) {
                // SAFETY: this slot's last frame has finished, so the card is
                // not reading the clock, and it is 16 bytes long.
                if let Err(error) = unsafe { buffer.write(&clock) } {
                    failure = Some(error);
                }
            }
        }
        works.surfaces.update(&mut self.book, meshes, slot);

        // The pictures exist while an effect is live, at the picture's size.
        let stale = live == 0 || works.targets.as_ref().is_some_and(|t| t.extent != output);
        if stale && let Some(old) = works.targets.take() {
            works.old_targets.push(now, old);
        }
        if live > 0 && works.targets.is_none() {
            match Targets::new(&self.gpu, output) {
                Ok(targets) => works.targets = Some(targets),
                Err(error) => failure = Some(error),
            }
        }
        if let Some(error) = failure {
            self.failed_with(&error);
        }
    }

    /// Records the effects over the picture `view` drew, and gives the set
    /// the tone mapping should read in place of the scene's; `None` when no
    /// effect is drawn, and nothing was recorded.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this device that is being
    /// recorded, outside a render pass, after `view` drew its scene and left
    /// its picture and depth ready to be read by fragment shaders. `slot`
    /// must be the one given to [`start_frame`](Self::start_frame) for this
    /// frame, and `view` the one with the output size it was given.
    pub(crate) unsafe fn record_screen(
        &mut self,
        commands: vk::CommandBuffer,
        slot: usize,
        view: &SceneView,
        camera: &Camera,
    ) -> Option<vk::DescriptorSet> {
        let works = self.works.as_mut()?;
        let targets = works.targets.as_ref()?;
        let frame = Frame {
            commands,
            slot,
            view,
            camera,
            // A shader sees a fraction of a second of an hour, which an f32 holds.
            #[allow(clippy::cast_possible_truncation)]
            time: self.clock as f32,
        };
        // SAFETY: the caller's guarantees are the pass's; the targets were
        // made for this frame's output size in `start_frame`.
        unsafe {
            works
                .pass
                .record(&works.shared, &self.book, targets, &frame)
        }
    }

    /// What the mesh pass draws the surface shaders with, for the frame given
    /// to [`start_frame`](Self::start_frame): `None` when no shader draws
    /// anything, so that the pass is exactly what it is without them.
    pub(crate) fn surface_draws(&self) -> Option<&SurfaceDraws> {
        self.works.as_ref()?.surfaces.draws()
    }

    /// Says that a frame was handed to the graphics card.
    pub(crate) fn submitted(&mut self) {
        self.submitted += 1;
    }

    /// Lets go of everything retired and of the screen pictures: the
    /// graphics card is idle, so nothing is in use.
    pub(crate) fn idle(&mut self) {
        self.failed = false;
        if let Some(works) = &mut self.works {
            works.retiring.clear();
            works.old_targets.clear();
            works.targets = None;
            works.pass.forget();
        }
    }
}

/// Hands a built shader to the book, and logs what it cost.
fn install(book: &mut Book<Built>, retiring: &mut Retiring<Built>, now: u64, done: Done<Built>) {
    let (id, revision, compiling, building) =
        (done.id, done.revision, done.compiling, done.building);
    let installed = done.outcome.is_ok();
    book.done(done, |built| retiring.push(now, built));
    if let (true, Some(file)) = (installed, book.file_of(id)) {
        log::info!(
            "Shader {file} (revision {revision}) is ready: compiled in {} ms, built in {} ms.",
            compiling.as_millis(),
            building.as_millis(),
        );
    }
}
