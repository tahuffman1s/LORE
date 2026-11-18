//! Tracks in the snow: where it has been trodden down round the eye.
//!
//! Snow that lies is drawn on the ground as it is built, with no points
//! moved: a level's ground is whatever triangles its maker gave it, and
//! lifting some of them opens cracks in what was never made for it. What
//! shows that the snow is deep is what is pressed into it. So a map is
//! kept of how far each bit of ground near the eye has been trodden, from
//! not at all to all the way, and the shader carves that out of the snow
//! where it draws it (`mesh.wgsl`): a print, or the furrow of someone
//! wading through.
//!
//! The way of the console versions of Batman: Arkham Origins, which had no
//! tessellation either (Barré-Brisebois, "Deformable Snow Rendering in
//! Batman: Arkham Origins", GDC 2014): a small map that follows the
//! player, stamped where feet come down, with a little taken off all of
//! it while it snows, so that tracks fill in again.
//!
//! The map is a square of points that follows the eye. A place in the
//! world always falls on the same point of it, whatever the eye has done
//! since (the square wraps round, as a globe's map does), so nothing is
//! moved when the eye moves: the points that come into the square as
//! others leave it are wiped, and that is all.

use std::ops::Range;
use std::sync::Arc;

use ash::vk;

use crate::buffer::{Buffer, once};
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::scene::Picture;
use crate::swapchain::COLOUR;

/// How many points the map has along a side, and how far apart they are,
/// in metres: sixty-four metres of tracks, to three centimetres, which is
/// fine enough for a print to have the shape of a boot.
pub(crate) const SIDE: u32 = 2048;
pub(crate) const APART: f32 = 0.03125;
/// How many points the square moves by at a time as it follows the eye:
/// four metres, so that a walker's map is wiped a strip at a time and not
/// at every step.
const MOVES_BY: i32 = 128;
const DOING: &str = "preparing the tracks in the snow";

/// Something pressing the snow down at a place: a foot, or a body wading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tread {
    /// Where, along x and z, in metres.
    pub place: [f32; 2],
    /// How far out from there the snow is pressed, in metres.
    pub wide: f32,
    /// How far down it is pressed, from 0 for not at all to 1 for as far
    /// as snow goes.
    pub deep: f32,
}

/// The map of trodden snow, as it is kept between frames.
pub(crate) struct Tracks {
    /// How far each point is trodden, a row of points along x for each
    /// step along z.
    map: Vec<u8>,
    /// The first point of the square along x and z, counted in points
    /// from the middle of the world: `None` until the eye is known.
    corner: Option<[i32; 2]>,
    /// The rows changed since the graphics card was last handed them.
    changed: Option<(u32, u32)>,
    /// Whether any of it is trodden.
    trodden: bool,
    /// How much filling in is owed that is less than one step of a point.
    owed: f32,
}

impl Tracks {
    /// Untrodden snow, with all the memory it will ever ask for.
    pub(crate) fn new() -> Self {
        Self {
            map: vec![0; (SIDE * SIDE) as usize],
            corner: None,
            changed: None,
            trodden: false,
            owed: 0.0,
        }
    }

    /// Smooths every track away.
    pub(crate) fn clear(&mut self) {
        if self.trodden {
            self.map.fill(0);
            self.changed = Some((0, SIDE - 1));
            self.trodden = false;
        }
        self.owed = 0.0;
    }

    /// Moves the square to be round `eye`, along x and z: what leaves it
    /// is forgotten.
    pub(crate) fn follow(&mut self, eye: [f32; 2]) {
        if !eye.iter().all(|part| part.is_finite() && part.abs() < 1e7) {
            return;
        }
        // Truncation is the point: whole points, and the bound above
        // keeps them far inside an i32.
        #[allow(clippy::cast_possible_truncation)]
        let wanted = eye.map(|part| {
            let point = (part / APART).floor() as i32 - SIDE.cast_signed() / 2;
            point.div_euclid(MOVES_BY) * MOVES_BY
        });
        let Some(corner) = self.corner.replace(wanted) else {
            return;
        };
        if corner == wanted || !self.trodden {
            return;
        }
        let side = SIDE.cast_signed();
        let moved = [wanted[0] - corner[0], wanted[1] - corner[1]];
        if moved.iter().any(|by| by.abs() >= side) {
            self.clear();
            return;
        }
        // The points that have come in along x: the same in every row.
        let entered = |by: i32, corner: i32| {
            let first = if by > 0 { corner + side } else { corner + by };
            (first..first + by.abs()).map(move |point| point.rem_euclid(side).cast_unsigned())
        };
        if moved[0] != 0 {
            for row in self.map.as_chunks_mut::<{ SIDE as usize }>().0 {
                for column in entered(moved[0], corner[0]) {
                    row[column as usize] = 0;
                }
            }
        }
        // And along z: whole rows.
        for row in entered(moved[1], corner[1]) {
            let first = (row * SIDE) as usize;
            self.map[first..first + SIDE as usize].fill(0);
        }
        self.changed = Some((0, SIDE - 1));
    }

    /// Presses the snow down: soft at its edge, and no less than it was
    /// pressed there before.
    pub(crate) fn tread(&mut self, tread: Tread) {
        let Some(corner) = self.corner else {
            return;
        };
        let Tread { place, wide, deep } = tread;
        if !(place.iter().all(|part| part.is_finite()) && wide.is_finite() && deep.is_finite()) {
            return;
        }
        let (wide, deep) = (wide.clamp(APART, 4.0), deep.clamp(0.0, 1.0));
        if deep <= 0.0 {
            return;
        }
        let side = SIDE.cast_signed();
        // In points, from the square's corner. A place far outside the
        // square is nowhere near the bound of an i32 once it is clamped.
        let within = [0, 1].map(|axis| {
            (place[axis] / APART - corner[axis] as f32).clamp(-1e6, 1e6)
        });
        let reach = wide / APART;
        // Truncation is the point: whole points.
        #[allow(clippy::cast_possible_truncation)]
        let span = |middle: f32| {
            let first = ((middle - reach).floor() as i32).max(0);
            let last = ((middle + reach).ceil() as i32).min(side - 1);
            first..=last
        };
        for z in span(within[1]) {
            let row = (z + corner[1]).rem_euclid(side).cast_unsigned();
            let mut touched = false;
            for x in span(within[0]) {
                // The middle of the point. Small whole numbers are exact.
                #[allow(clippy::cast_precision_loss)]
                let away = (x as f32 + 0.5 - within[0]).hypot(z as f32 + 0.5 - within[1]) / reach;
                // Flat for most of the way out, with a steep side: a foot
                // goes straight down.
                let share = (1.0 - (away - 0.7).max(0.0) / 0.3).clamp(0.0, 1.0);
                let pressed = deep * share * share * (3.0 - 2.0 * share);
                // Truncation is the point: it is between 0 and 255.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let pressed = (pressed * 255.0).round() as u8;
                let column = (x + corner[0]).rem_euclid(side).cast_unsigned();
                let point = &mut self.map[(row * SIDE + column) as usize];
                if pressed > *point {
                    *point = pressed;
                    touched = true;
                }
            }
            if touched {
                self.trodden = true;
                self.changed = Some(match self.changed {
                    Some((first, last)) => (first.min(row), last.max(row)),
                    None => (row, row),
                });
            }
        }
    }

    /// Fills every track in by `share` of the way from trodden to
    /// untrodden: what new snow does.
    pub(crate) fn fill(&mut self, share: f32) {
        if !self.trodden || !share.is_finite() || share <= 0.0 {
            self.owed = 0.0;
            return;
        }
        self.owed += share.min(1.0) * 255.0;
        if self.owed < 1.0 {
            return;
        }
        // Truncation is the point: whole steps, at most 255 of them.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = self.owed.floor().min(255.0) as u8;
        self.owed -= f32::from(steps);
        let mut left = 0;
        for point in &mut self.map {
            *point = point.saturating_sub(steps);
            left |= *point;
        }
        self.trodden = left != 0;
        self.changed = Some((0, SIDE - 1));
    }

    /// The rows changed since this was last asked, from the first of
    /// them to the last: `None` when nothing has changed.
    pub(crate) fn take_changed(&mut self) -> Option<(u32, u32)> {
        self.changed.take()
    }

    /// What is in the rows from `first` to `last`, whole rows one after
    /// another: for the graphics card.
    pub(crate) fn rows(&self, first: u32, last: u32) -> &[u8] {
        let (first, last) = (first.min(SIDE - 1), last.min(SIDE - 1));
        (self.map).get((first * SIDE) as usize..((last + 1) * SIDE) as usize)
            .unwrap_or(&[])
    }

    /// How far the snow at a place is trodden, from 0 to 1: 0 outside the
    /// square.
    #[cfg(test)]
    fn at(&self, place: [f32; 2]) -> f32 {
        let Some(corner) = self.corner else {
            return 0.0;
        };
        let side = SIDE.cast_signed();
        #[allow(clippy::cast_possible_truncation)]
        let point = [0, 1].map(|axis| (place[axis] / APART).floor() as i32);
        if (0..2).any(|axis| !(corner[axis]..corner[axis] + side).contains(&point[axis])) {
            return 0.0;
        }
        let [column, row] = point.map(|point| point.rem_euclid(side).cast_unsigned());
        f32::from(self.map[(row * SIDE + column) as usize]) / 255.0
    }
}

/// The map on the graphics card, for the shader to read.
pub(crate) struct TrackMap {
    gpu: Arc<Gpu>,
    picture: Picture,
    /// Rows on their way to the graphics card, for each frame that can be
    /// in progress at once.
    rows: Vec<Buffer>,
}

impl TrackMap {
    /// An untrodden map, for up to `frames` frames in progress at once.
    pub(crate) fn new(gpu: &Arc<Gpu>, frames: usize) -> Result<Self, RenderError> {
        let mut map = Self {
            gpu: Arc::clone(gpu),
            picture: Picture::null(),
            rows: Vec::with_capacity(frames.max(1)),
        };
        // Dropping `map` cleans up whatever was created.
        let bytes = u64::from(SIDE * SIDE);
        for _ in 0..frames.max(1) {
            map.rows.push(Buffer::writable(
                gpu,
                bytes,
                vk::BufferUsageFlags::TRANSFER_SRC,
                DOING,
            )?);
        }
        let extent = vk::Extent2D {
            width: SIDE,
            height: SIDE,
        };
        map.picture.create(
            gpu,
            extent,
            vk::Format::R8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            COLOUR,
            DOING,
        )?;
        // Nothing is trodden yet: all of it is nought.
        let staging = Buffer::staging(gpu, bytes, DOING)?;
        staging.fill(&vec![0; (SIDE * SIDE) as usize], DOING)?;
        let device = gpu.device();
        let image = map.picture.image;
        // SAFETY: the image and the staging buffer are alive until `once`
        // has waited for the commands, and the image is written in the
        // layout the barrier before left it in.
        unsafe {
            once(gpu, DOING, |commands| {
                let to_fill = [barrier(image)
                    .dst_stage_mask(vk::PipelineStageFlags2::COPY)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_fill),
                );
                device.cmd_copy_buffer_to_image(
                    commands,
                    staging.handle,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[copy_of(0..SIDE)],
                );
                let to_read = [barrier(image)
                    .src_stage_mask(vk::PipelineStageFlags2::COPY)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            })
        }?;
        Ok(map)
    }

    /// What the shader reads the map through.
    pub(crate) fn view(&self) -> vk::ImageView {
        self.picture.view
    }

    /// Records the commands that hand the graphics card the rows of the
    /// map that `bytes` holds, whole rows from the one numbered `first`.
    ///
    /// # Safety
    ///
    /// `commands` must be a command buffer of this map's device that is
    /// being recorded, outside any pass, and the commands last recorded
    /// with the same `frame` must have finished.
    pub(crate) unsafe fn record(
        &self,
        commands: vk::CommandBuffer,
        frame: usize,
        first: u32,
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        let Some(buffer) = self.rows.get(frame % self.rows.len().max(1)) else {
            return Ok(());
        };
        // No more rows than the map has below the first.
        let count = u32::try_from(bytes.len() / SIDE as usize)
            .unwrap_or(SIDE)
            .min(SIDE.saturating_sub(first));
        if count == 0 {
            return Ok(());
        }
        let (rows, wanted) = (first..first + count, (count * SIDE) as usize);
        let device = self.gpu.device();
        let image = self.picture.image;
        let to_fill = [barrier(image)
            .src_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
            .src_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
            .dst_stage_mask(vk::PipelineStageFlags2::COPY)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)];
        let to_read = [barrier(image)
            .src_stage_mask(vk::PipelineStageFlags2::COPY)
            .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
            .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        // SAFETY: the buffer is writable and as large as the whole map, of
        // which `bytes` is a part, and the caller guarantees the commands
        // that read it last have finished. The picture is this map's own
        // and was left for shaders to read when it was made and after
        // every call of this. Every struct passed lives until its call
        // returns.
        unsafe {
            buffer.write(&bytes[..wanted])?;
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_fill),
            );
            device.cmd_copy_buffer_to_image(
                commands,
                buffer.handle,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[copy_of(rows)],
            );
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_read),
            );
        }
        Ok(())
    }
}

impl Drop for TrackMap {
    fn drop(&mut self) {
        // SAFETY: whoever owns the map waits for the graphics card to be
        // idle before dropping what it draws with, as for the pipeline
        // this belongs to.
        unsafe { self.picture.destroy(&self.gpu) };
    }
}

/// A barrier for the whole of `image`, with nothing said yet of before
/// and after.
fn barrier<'a>(image: vk::Image) -> vk::ImageMemoryBarrier2<'a> {
    vk::ImageMemoryBarrier2::default()
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(COLOUR)
}

/// Whole rows of the map, from the start of a buffer.
fn copy_of(rows: Range<u32>) -> vk::BufferImageCopy {
    vk::BufferImageCopy::default()
        .image_subresource(vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        })
        .image_offset(vk::Offset3D {
            x: 0,
            y: rows.start.cast_signed(),
            z: 0,
        })
        .image_extent(vk::Extent3D {
            width: SIDE,
            height: rows.end - rows.start,
            depth: 1,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn foot(x: f32, z: f32) -> Tread {
        Tread {
            place: [x, z],
            wide: 0.25,
            deep: 1.0,
        }
    }

    #[test]
    fn a_foot_presses_the_snow_where_it_comes_down_and_nowhere_else() {
        let mut tracks = Tracks::new();
        // Before the eye is known there is nowhere to press.
        tracks.tread(foot(3.0, -7.0));
        assert!(tracks.take_changed().is_none());
        tracks.follow([3.0, -7.0]);
        tracks.tread(foot(3.0, -7.0));
        assert!(tracks.at([3.0, -7.0]) > 0.99);
        // Soft at its edge, and gone a little past it.
        let edge = tracks.at([3.22, -7.0]);
        assert!(edge > 0.05 && edge < 0.9, "{edge}");
        assert_eq!(tracks.at([3.4, -7.0]), 0.0);
        // Only the rows it touched go to the graphics card.
        let (first, last) = tracks.take_changed().unwrap();
        assert!(last - first < 20 && last < SIDE, "{first} to {last}");
        assert_eq!(tracks.rows(first, last).len(), ((last - first + 1) * SIDE) as usize);
        assert!(tracks.take_changed().is_none());
        // A lighter tread on a deeper one changes nothing.
        tracks.tread(Tread {
            deep: 0.3,
            ..foot(3.0, -7.0)
        });
        assert!(tracks.take_changed().is_none());
    }

    #[test]
    fn tracks_stay_where_they_were_made_as_the_eye_moves_and_are_lost_far_behind() {
        let mut tracks = Tracks::new();
        tracks.follow([0.0, 0.0]);
        tracks.tread(foot(0.0, 0.0));
        tracks.tread(foot(-20.0, 5.0));
        // Twenty metres on, both are where they were.
        for step in 0..=200 {
            tracks.follow([step as f32 * 0.1, 0.0]);
        }
        assert!(tracks.at([0.0, 0.0]) > 0.99);
        assert!(tracks.at([-20.0, 5.0]) < 0.01, "that one is out of the square");
        // What comes into the square in its place is untrodden.
        assert_eq!(tracks.at([44.0, 5.0]), 0.0);
        // A long way off, and back: the square was wiped as it went.
        tracks.follow([500.0, 300.0]);
        tracks.follow([0.0, 0.0]);
        assert_eq!(tracks.at([0.0, 0.0]), 0.0);
        // A place that is not a place moves nothing.
        tracks.tread(foot(1.0, 1.0));
        tracks.follow([f32::NAN, 0.0]);
        assert!(tracks.at([1.0, 1.0]) > 0.99);
    }

    #[test]
    fn falling_snow_fills_tracks_in() {
        let mut tracks = Tracks::new();
        tracks.follow([0.0, 0.0]);
        tracks.tread(foot(0.0, 0.0));
        let _ = tracks.take_changed();
        // A hundredth at a time, a little at each of many frames.
        for _ in 0..500 {
            tracks.fill(0.001);
        }
        let half = tracks.at([0.0, 0.0]);
        assert!((half - 0.5).abs() < 0.02, "{half}");
        assert!(tracks.take_changed().is_some());
        for _ in 0..600 {
            tracks.fill(0.001);
        }
        assert_eq!(tracks.at([0.0, 0.0]), 0.0);
        // With nothing trodden there is nothing to hand over.
        let _ = tracks.take_changed();
        tracks.fill(0.5);
        assert!(tracks.take_changed().is_none());
    }
}
