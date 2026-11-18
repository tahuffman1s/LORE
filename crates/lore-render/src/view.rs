//! One view of the scene: drawn, upscaled if asked, and ready to tone map.

use std::sync::Arc;

use ash::vk;

use crate::lamp_shadow::LampShadows;
use crate::camera::{Camera, Mat4, shifted};
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::mesh::{Contents, MeshPipeline, View};
use crate::scene::SceneTarget;
use crate::upscale::{Drawn, Upscale, Upscaling};

/// What a frame does with the picture that smooth things mirror: wet
/// ground, puddles and polished things show the world in the picture drawn
/// the frame before, which is kept for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mirroring {
    /// Nothing mirrors the world; smooth things mirror only the light of
    /// the sky and the ground.
    Off,
    /// Each frame mirrors the picture kept from the frame before and keeps
    /// its own for the next: a game drawn frame after frame.
    Running,
    /// This frame mirrors nothing, and keeps its picture for the next.
    Keeping,
    /// This frame mirrors the picture kept from the frame before and keeps
    /// nothing: after a frame that kept its picture from the same camera,
    /// it can be drawn again and come out the same.
    Reading,
}

/// Everything between the camera and the picture that is tone mapped: the
/// images the scene is drawn into, the upscaler, and what has to be
/// remembered from one frame to the next.
pub(crate) struct SceneView {
    // The upscaler goes before the images it reads.
    upscale: Option<Upscale>,
    target: SceneTarget,
    /// Where each point in the world was drawn in the last frame; `None`
    /// before the first frame and after a cut.
    before: Option<Mat4>,
    /// How many frames have been drawn, for the camera shift.
    frame: u32,
    /// How long this view has been drawn for, in seconds: what the clouds
    /// drift by.
    seconds: f64,
    /// Whether the clouds are held where they are, so that the same camera
    /// gives the same picture every time: for pictures drawn with no
    /// window, which tests compare.
    pub(crate) still: bool,
    /// What the next frame does with the picture smooth things mirror.
    pub(crate) mirroring: Mirroring,
    /// Where each point of the world was drawn in the picture the last
    /// frame kept for smooth things to mirror; `None` if it kept none, and
    /// after a cut.
    kept: Option<Mat4>,
    /// The time the clouds' clock shows, if the game says it.
    clock: Option<f64>,
    /// Where the eye was in the last frame, and how much its walking is
    /// stirring what it walks through, from 0 to 1.
    eye: Option<[f32; 3]>,
    stir: f32,
    /// How many sizes larger than usual textures are read; see [`sharper`].
    sharper: f32,
    /// Whether each frame's picture is measured for the eye to adapt to.
    pub(crate) measured: bool,
    /// Whether the air is lit: beams where the sun reaches it, the glow of
    /// lamps in it.
    pub(crate) shafts: bool,
    /// How many lamps cast shadows, and how sharp.
    pub(crate) lamp_shadows: LampShadows,
}

/// How many sizes larger than it otherwise would the graphics card should
/// read a texture when a scene `render` pixels wide is upscaled to `output`.
///
/// A scene drawn small picks small, blurry copies of its textures, which
/// upscaling cannot make sharp again. FSR's manual gives the rule: the
/// base-2 logarithm of how much smaller the scene is drawn, and one more.
fn sharper(render: u32, output: u32) -> f32 {
    // Sizes are far below the point where f32 stops being exact.
    #[allow(clippy::cast_precision_loss)]
    let ratio = output.max(1) as f32 / render.max(1) as f32;
    ratio.log2().max(0.0) + 1.0
}

impl SceneView {
    /// How long this view has been drawn for, in seconds: the clouds' clock.
    pub(crate) fn seconds(&self) -> f64 {
        self.clock.unwrap_or(self.seconds)
    }

    /// Says what time the clouds' clock shows, in seconds, in place of how
    /// long this view has been drawn: the game's own time, so that the sky
    /// is where the game's rules have it. `None` gives it back to the view.
    pub(crate) fn set_clock(&mut self, clock: Option<f64>) {
        self.clock = clock.filter(|clock| clock.is_finite());
    }

    /// Makes a view whose finished picture is `output` pixels.
    ///
    /// If `upscaling` cannot be had on this graphics card, the reason is
    /// logged and the scene is drawn at full size.
    pub(crate) fn new(
        gpu: &Arc<Gpu>,
        output: vk::Extent2D,
        upscaling: Upscaling,
    ) -> Result<Self, RenderError> {
        let upscale = Upscale::new(gpu, output, upscaling)?;
        let render = upscale.as_ref().map_or(output, |upscale| upscale.render);
        Ok(Self {
            sharper: if upscale.is_some() {
                sharper(render.width, output.width)
            } else {
                0.0
            },
            target: SceneTarget::new(gpu, render)?,
            upscale,
            before: None,
            frame: 0,
            seconds: 0.0,
            still: false,
            mirroring: Mirroring::Running,
            kept: None,
            clock: None,
            eye: None,
            stir: 0.0,
            measured: false,
            shafts: true,
            lamp_shadows: LampShadows::default(),
        })
    }

    /// The finished picture, in high dynamic range, for the tone mapping to read.
    pub(crate) fn picture(&self) -> vk::ImageView {
        self.upscale
            .as_ref()
            .map_or(self.target.colour.view, |upscale| upscale.picture.view)
    }

    /// The size the scene is drawn at, which is smaller than the finished
    /// picture when upscaling.
    pub(crate) fn render_size(&self) -> vk::Extent2D {
        self.target.extent
    }

    /// Whether the scene is being upscaled.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn upscales(&self) -> bool {
        self.upscale.is_some()
    }

    /// The images the scene was drawn into.
    #[cfg(test)]
    pub(crate) fn target(&self) -> &SceneTarget {
        &self.target
    }

    /// Forgets the frames before: the next one is not blended with them.
    pub(crate) fn cut(&mut self) {
        self.before = None;
        self.kept = None;
    }

    /// Records drawing `contents` as `camera` sees them, and upscaling the result.
    ///
    /// `seconds` is how long the last frame took.
    ///
    /// # Errors
    ///
    /// Fails if the graphics driver or the upscaler reports a failure.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this view's device that is
    /// being recorded, outside a render pass. `mesh` and the meshes in `contents` must
    /// belong to the same device, and they and this view must stay alive
    /// until the commands have run. The view must only be in use by earlier
    /// commands on the same queue. `frame` is as for [`MeshPipeline::record`].
    pub(crate) unsafe fn record(
        &mut self,
        commands: vk::CommandBuffer,
        frame: usize,
        mesh: &MeshPipeline,
        contents: Contents<'_>,
        camera: &Camera,
        seconds: f32,
    ) -> Result<(), RenderError> {
        let size = self.target.extent;
        let steady = camera.clip_from_world(size.width, size.height);
        let jitter = self
            .upscale
            .as_ref()
            .map_or((0.0, 0.0), |upscale| upscale.jitter(self.frame));
        // Sizes are far below the point where f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let (width, height) = (size.width as f32, size.height as f32);
        let [forward, right, up] = camera.rays(size.width, size.height);
        let (across, upwards) = (2.0 * jitter.0 / width, -2.0 * jitter.1 / height);
        // A picture kept only to be mirrored is drawn as if it had not
        // been: nothing that is remembered from frame to frame moves on.
        let keeping = self.mirroring == Mirroring::Keeping;
        // How fast the eye is moving over the ground, eased so that rings
        // die away after it stops and do not jump when it starts.
        if !keeping
            && let Some(was) = self.eye
            && seconds.is_finite()
            && seconds > 0.0001
        {
            let moved = (camera.position[0] - was[0]).hypot(camera.position[2] - was[2]);
            // A run is about five metres a second; a cut to somewhere new
            // is not walking.
            let speed = if moved < 2.0 { moved / seconds } else { 0.0 };
            let wanted = (speed / 5.0).clamp(0.0, 1.0);
            let ease = (seconds * 2.5).clamp(0.0, 1.0);
            self.stir += (wanted - self.stir) * ease;
        }
        let mut elapsed = 0.0;
        if !keeping {
            self.eye = Some(camera.position);
            if !self.still && seconds.is_finite() {
                elapsed = seconds.clamp(0.0, 1.0);
                self.seconds += f64::from(elapsed);
            }
        }
        let clip = shifted(&steady, across, upwards);
        // Only the picture the last frame kept is of where the eye is: not
        // one from before a cut, nor one from before a while when nothing
        // mirrored and nothing was kept.
        let kept = self.kept.take();
        let view = View {
            // The picture is 2 wide and 2 high with y running up, and the
            // shift is in pixels with y running down. The direction is the
            // FSR manual's; the offscreen test fails with either sign turned.
            clip,
            steady,
            before: self.before.unwrap_or(steady),
            eye: camera.position,
            sharper: self.sharper,
            rays: [
                [forward[0], forward[1], forward[2], height],
                [right[0], right[1], right[2], across],
                [up[0], up[1], up[2], upwards],
            ],
            // Clouds need no more of the time than an f32 holds.
            #[allow(clippy::cast_possible_truncation)]
            time: self.seconds() as f32,
            exact: self.seconds(),
            elapsed,
            stir: self.stir,
            // Only an upscaler mixes frames; without one a pattern that
            // moved every frame would be seen to crawl.
            #[allow(clippy::cast_precision_loss)]
            noise: if self.upscale.is_some() {
                (self.frame % 64) as f32
            } else {
                0.0
            },
            near: camera.near_metres(),
            reflecting: self.mirroring != Mirroring::Off,
            mirrors: kept.is_some()
                && matches!(self.mirroring, Mirroring::Running | Mirroring::Reading),
            kept: kept.unwrap_or(steady),
            keeps: matches!(self.mirroring, Mirroring::Running | Mirroring::Keeping),
            copied: false,
            shafts: self.shafts,
            lamp_shadows: self.lamp_shadows,
        };
        self.target.kept.set(false);
        // SAFETY: the caller's guarantees are the ones `MeshPipeline::record`
        // asks for, and the target is this view's own.
        unsafe { mesh.record(commands, frame, &self.target, &view, contents) }?;
        if self.measured && !keeping {
            // SAFETY: as for the drawing above, after which the picture is
            // ready to be read.
            unsafe { mesh.measure(commands, frame, &self.target) };
        }
        if self.target.kept.get() {
            self.kept = Some(clip);
        }
        if keeping {
            return Ok(());
        }
        let cut = self.before.is_none();
        self.before = Some(steady);
        self.frame = self.frame.wrapping_add(1);
        if let Some(upscale) = &mut self.upscale {
            let drawn = Drawn {
                jitter,
                seconds,
                near: camera.near_metres(),
                fov: camera.fov_radians(),
                cut,
            };
            // SAFETY: the mesh pass above drew the target, which was made
            // the size the upscaler draws at, and left it ready to be read.
            unsafe { upscale.record(commands, &self.target, &drawn) }
                .map_err(RenderError::Upscaler)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::sharper;

    #[test]
    fn a_scene_drawn_smaller_reads_larger_textures() {
        // Drawn full size and only smoothed: one size larger, as FSR asks.
        assert_eq!(sharper(1920, 1920), 1.0);
        // Half as wide: one more.
        assert_eq!(sharper(960, 1920), 2.0);
        assert!((sharper(1280, 1920) - 1.585).abs() < 1e-3);
        // Nothing to divide by is not a reason to fail.
        assert!(sharper(0, 0).is_finite());
    }
}
