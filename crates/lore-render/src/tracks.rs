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
//! others leave it are wiped, and that is all. Only the rows that have
//! changed are handed to the graphics card, and only the rows that are
//! trodden are gone over when snow fills them in: the map is four million
//! points, and a walker's trail is in a few hundred rows of it.

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

/// Fewer steps of filling in than this are saved up: each filling goes
/// over every trodden row and hands it to the graphics card again.
const FILLS_BY: f32 = 4.0;

/// The map of trodden snow, as it is kept between frames.
pub(crate) struct Tracks {
    /// How far each point is trodden, a row of points along x for each
    /// step along z.
    map: Vec<u8>,
    /// The first point of the square along x and z, counted in points
    /// from the middle of the world: `None` until the eye is known.
    corner: Option<[i32; 2]>,
    /// The rows changed since the graphics card was last handed them, and
    /// the rows that have anything trodden in them: each from a first to
    /// a last, counted from the square's corner, so that neither is ever
    /// in two pieces.
    changed: Option<(u32, u32)>,
    trodden: Option<(u32, u32)>,
    /// How much filling in is owed and not yet done.
    owed: f32,
}

/// `span` and the rows from `first` to `last`, as one span.
fn joined(span: Option<(u32, u32)>, first: u32, last: u32) -> Option<(u32, u32)> {
    Some(span.map_or((first, last), |(start, end)| {
        (start.min(first), end.max(last))
    }))
}

/// What is left of a span of rows inside the square once the square has
/// moved on by `by` rows.
fn moved(span: Option<(u32, u32)>, by: i32) -> Option<(u32, u32)> {
    let (first, last) = span?;
    let side = SIDE.cast_signed();
    let (first, last) = (first.cast_signed() - by, last.cast_signed() - by);
    (last >= 0 && first < side).then(|| {
        (
            first.max(0).cast_unsigned(),
            last.min(side - 1).cast_unsigned(),
        )
    })
}

impl Tracks {
    /// Untrodden snow, with all the memory it will ever ask for.
    pub(crate) fn new() -> Self {
        Self {
            map: vec![0; (SIDE * SIDE) as usize],
            corner: None,
            changed: None,
            trodden: None,
            owed: 0.0,
        }
    }

    /// Smooths every track away.
    pub(crate) fn clear(&mut self) {
        if self.trodden.take().is_some() {
            self.map.fill(0);
            self.changed = Some((0, SIDE - 1));
        }
        self.owed = 0.0;
    }

    /// Where in the map the row numbered `row` from the square's corner
    /// begins, when that corner is at `corner` along z.
    fn row_at(corner: i32, row: i32) -> usize {
        ((corner + row)
            .rem_euclid(SIDE.cast_signed())
            .cast_unsigned()
            * SIDE) as usize
    }

    /// Moves the square to be round `eye`, along x and z: what leaves it
    /// is forgotten. It moves a strip at a time, and not back again until
    /// the eye has gone back most of a strip: someone pacing up and down
    /// on one spot does not wipe and redraw it at every turn.
    pub(crate) fn follow(&mut self, eye: [f32; 2]) {
        if !eye.iter().all(|part| part.is_finite() && part.abs() < 1e6) {
            return;
        }
        let side = SIDE.cast_signed();
        // Truncation is the point: whole points, and the bound above
        // keeps them far inside an i32.
        #[allow(clippy::cast_possible_truncation)]
        let point = eye.map(|part| (part / APART).floor() as i32);
        let round = |point: i32| (point - side / 2).div_euclid(MOVES_BY) * MOVES_BY;
        let Some(corner) = self.corner else {
            self.corner = Some(point.map(round));
            return;
        };
        let mut wanted = corner;
        for axis in 0..2 {
            let from_middle = point[axis] - (corner[axis] + side / 2);
            if !(-MOVES_BY / 2..MOVES_BY * 3 / 2).contains(&from_middle) {
                wanted[axis] = round(point[axis]);
            }
        }
        if wanted == corner {
            return;
        }
        self.corner = Some(wanted);
        let by = [wanted[0] - corner[0], wanted[1] - corner[1]];
        if by.iter().any(|by| by.abs() >= side) {
            self.clear();
            return;
        }
        // Along z whole rows leave. Those that were trodden are wiped, so
        // that what comes into the square on the same points of the map
        // is untrodden; the rest are nought already.
        if by[1] != 0 {
            let leaving = if by[1] > 0 {
                0..by[1]
            } else {
                side + by[1]..side
            };
            let wiped = self.trodden.map_or(0..0, |(first, last)| {
                leaving.start.max(first.cast_signed())..leaving.end.min(last.cast_signed() + 1)
            });
            for row in wiped.clone() {
                let at = Self::row_at(corner[1], row);
                self.map[at..at + SIDE as usize].fill(0);
            }
            self.trodden = moved(self.trodden, by[1]);
            self.changed = moved(self.changed, by[1]);
            if !wiped.is_empty() {
                // Where those rows are counted from the new corner.
                let now = |row: i32| (row - by[1]).rem_euclid(side).cast_unsigned();
                self.changed = joined(self.changed, now(wiped.start), now(wiped.end - 1));
            }
        }
        // Along x a strip of every trodden row leaves.
        if by[0] != 0
            && let Some((first, last)) = self.trodden
        {
            let leaving = if by[0] > 0 {
                corner[0]..corner[0] + by[0]
            } else {
                corner[0] + side + by[0]..corner[0] + side
            };
            for row in first..=last {
                let at = Self::row_at(wanted[1], row.cast_signed());
                for point in leaving.clone() {
                    self.map[at + point.rem_euclid(side).cast_unsigned() as usize] = 0;
                }
            }
            self.changed = joined(self.changed, first, last);
        }
    }

    /// Presses the snow down: steep at its edge, and no less than it was
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
        // square is nowhere near the bound of an i32 once it is clamped,
        // and a corner is a whole number far below where f32 stops being
        // exact.
        #[allow(clippy::cast_precision_loss)]
        let within =
            [0, 1].map(|axis| (place[axis] / APART - corner[axis] as f32).clamp(-1e6, 1e6));
        let reach = wide / APART;
        // Truncation is the point: whole points.
        #[allow(clippy::cast_possible_truncation)]
        let span = |middle: f32| {
            let first = ((middle - reach).floor() as i32).max(0);
            let last = ((middle + reach).ceil() as i32).min(side - 1);
            first..=last
        };
        for z in span(within[1]) {
            let at = Self::row_at(corner[1], z);
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
                let point = &mut self.map[at + column as usize];
                if pressed > *point {
                    *point = pressed;
                    touched = true;
                }
            }
            if touched {
                let row = z.cast_unsigned();
                self.trodden = joined(self.trodden, row, row);
                self.changed = joined(self.changed, row, row);
            }
        }
    }

    /// Fills every track in by `share` of the way from trodden to
    /// untrodden: what new snow does.
    pub(crate) fn fill(&mut self, share: f32) {
        let Some((first, last)) = self.trodden else {
            self.owed = 0.0;
            return;
        };
        if !share.is_finite() || share <= 0.0 {
            return;
        }
        self.owed += share.min(1.0) * 255.0;
        if self.owed < FILLS_BY {
            return;
        }
        // Truncation is the point: whole steps, at most 255 of them.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = self.owed.floor().min(255.0) as u8;
        self.owed -= f32::from(steps);
        let corner = self.corner.map_or(0, |corner| corner[1]);
        // Only the rows that have anything in them, and from now on only
        // those that still do.
        let mut left: Option<(u32, u32)> = None;
        for row in first..=last {
            let at = Self::row_at(corner, row.cast_signed());
            let mut any = 0;
            for point in &mut self.map[at..at + SIDE as usize] {
                *point = point.saturating_sub(steps);
                any |= *point;
            }
            if any != 0 {
                left = joined(left, row, row);
            }
        }
        self.trodden = left;
        self.changed = joined(self.changed, first, last);
    }

    /// The rows changed since this was last asked, from the first of
    /// them to the last, counted from the square's corner: `None` when
    /// nothing has changed. [`pieces`](Self::pieces) gives what is in
    /// them, and has to be asked before the square is moved again.
    pub(crate) fn take_changed(&mut self) -> Option<(u32, u32)> {
        self.changed.take()
    }

    /// What is in the rows from `first` to `last`, counted from the
    /// square's corner, as the graphics card takes it: one piece of the
    /// map, or two where the rows run off its end and on from its start.
    /// Each piece is the row of the map it begins at, and whole rows one
    /// after another.
    pub(crate) fn pieces(&self, (first, last): (u32, u32)) -> [Option<(u32, &[u8])>; 2] {
        let (first, last) = (first.min(SIDE - 1), last.min(SIDE - 1));
        if last < first {
            return [None; 2];
        }
        let corner = self.corner.map_or(0, |corner| corner[1]);
        let begins = (corner + first.cast_signed())
            .rem_euclid(SIDE.cast_signed())
            .cast_unsigned();
        let count = last - first + 1;
        let rows = |from: u32, count: u32| {
            (self.map)
                .get((from * SIDE) as usize..((from + count) * SIDE) as usize)
                .map(|bytes| (from, bytes))
        };
        if begins + count <= SIDE {
            [rows(begins, count), None]
        } else {
            [rows(begins, SIDE - begins), rows(0, begins + count - SIDE)]
        }
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

    /// Records the commands that hand the graphics card `pieces` of the
    /// map: each the row it begins at and whole rows one after another, as
    /// [`Tracks::pieces`] gives them.
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
        pieces: [Option<(u32, &[u8])>; 2],
    ) -> Result<(), RenderError> {
        let Some(buffer) = self.rows.get(frame % self.rows.len().max(1)) else {
            return Ok(());
        };
        // No more rows of each piece than the map has below its first, and
        // no more of both than the map has: that is what the buffer holds.
        let mut parts: [&[u8]; 2] = [&[]; 2];
        let mut copies = [vk::BufferImageCopy::default(); 2];
        let (mut pieces_kept, mut rows_kept, mut bytes_kept) = (0, 0, 0_usize);
        for (first, bytes) in pieces.into_iter().flatten() {
            let count = u32::try_from(bytes.len() / SIDE as usize)
                .unwrap_or(SIDE)
                .min(SIDE.saturating_sub(first))
                .min(SIDE - rows_kept);
            if count == 0 {
                continue;
            }
            let wanted = (count * SIDE) as usize;
            parts[pieces_kept] = &bytes[..wanted];
            copies[pieces_kept] = copy_of(first..first + count).buffer_offset(bytes_kept as u64);
            pieces_kept += 1;
            rows_kept += count;
            bytes_kept += wanted;
        }
        if pieces_kept == 0 {
            return Ok(());
        }
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
        // which the parts together are no more, and the caller guarantees
        // the commands that read it last have finished. The picture is
        // this map's own and was left for shaders to read when it was made
        // and after every call of this. Every struct passed lives until
        // its call returns.
        unsafe {
            buffer.write_parts(&parts[..pieces_kept])?;
            device.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&to_fill),
            );
            device.cmd_copy_buffer_to_image(
                commands,
                buffer.handle,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &copies[..pieces_kept],
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

    /// How many rows the graphics card would be handed, and whether what
    /// it is handed is whole rows.
    fn handed(tracks: &mut Tracks) -> u32 {
        let Some(changed) = tracks.take_changed() else {
            return 0;
        };
        let mut rows = 0;
        for (first, bytes) in tracks.pieces(changed).into_iter().flatten() {
            assert_eq!(bytes.len() % SIDE as usize, 0);
            let count = (bytes.len() / SIDE as usize) as u32;
            assert!(first + count <= SIDE, "{first} and {count} more");
            rows += count;
        }
        rows
    }

    #[test]
    fn a_foot_presses_the_snow_where_it_comes_down_and_nowhere_else() {
        let mut tracks = Tracks::new();
        // Before the eye is known there is nowhere to press.
        tracks.tread(foot(3.0, -7.0));
        assert_eq!(handed(&mut tracks), 0);
        tracks.follow([3.0, -7.0]);
        tracks.tread(foot(3.0, -7.0));
        assert!(tracks.at([3.0, -7.0]) > 0.99);
        // Steep at its edge, and gone a little past it.
        let edge = tracks.at([3.22, -7.0]);
        assert!(edge > 0.05 && edge < 0.9, "{edge}");
        assert_eq!(tracks.at([3.4, -7.0]), 0.0);
        // Only the rows it touched go to the graphics card.
        let rows = handed(&mut tracks);
        assert!((14..=18).contains(&rows), "{rows}");
        assert_eq!(handed(&mut tracks), 0);
        // A lighter tread on a deeper one changes nothing.
        tracks.tread(Tread {
            deep: 0.3,
            ..foot(3.0, -7.0)
        });
        assert_eq!(handed(&mut tracks), 0);
    }

    #[test]
    fn a_tread_where_the_map_wraps_is_handed_over_in_two_small_pieces() {
        // The map begins again every 64 metres, and at z = 0: a foot that
        // comes down across that line touches its last rows and its first.
        let mut tracks = Tracks::new();
        tracks.follow([10.0, 0.0]);
        tracks.tread(foot(10.0, 0.0));
        let changed = tracks.take_changed().unwrap();
        let pieces = tracks.pieces(changed);
        let [Some((first, before)), Some((0, after))] = pieces else {
            panic!("one piece, or none: {changed:?}");
        };
        let rows = (before.len() + after.len()) / SIDE as usize;
        assert!((14..=18).contains(&rows), "{rows}");
        assert_eq!(first as usize + before.len() / SIDE as usize, SIDE as usize);
        assert!(before.contains(&255));
        assert!(after.contains(&255));
        assert!(tracks.at([10.0, -0.1]) > 0.99 && tracks.at([10.0, 0.1]) > 0.99);
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
        assert!(
            tracks.at([-20.0, 5.0]) < 0.01,
            "that one is out of the square"
        );
        // What comes into the square in its place is untrodden.
        assert_eq!(tracks.at([44.0, 5.0]), 0.0);
        // And along z, there and back by another way.
        tracks.tread(foot(20.0, 25.0));
        for step in 0..=300 {
            tracks.follow([20.0, -(step as f32) * 0.1]);
        }
        assert!(tracks.at([0.0, 0.0]) > 0.99);
        assert_eq!(tracks.at([20.0, 25.0]), 0.0);
        assert_eq!(
            tracks.at([20.0, 25.0 - 64.0]),
            0.0,
            "nor where it would wrap to"
        );
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
    fn what_the_graphics_card_holds_is_what_the_map_holds_however_the_eye_wanders() {
        // A copy of the map kept only from what is handed over, as the
        // graphics card's is: after any walk the two are the same.
        let mut tracks = Tracks::new();
        let mut card = vec![0_u8; (SIDE * SIDE) as usize];
        let mut dice = 0x2545_F491_u32;
        let mut dealt = || {
            dice ^= dice << 13;
            dice ^= dice >> 17;
            dice ^= dice << 5;
            (dice >> 8) as f32 / 16_777_216.0
        };
        let mut eye = [3.0_f32, -2.0];
        for step in 0..600 {
            // A wandering walk with a jump now and then, treading as it
            // goes and to the side, with snow falling some of the time.
            let stride = if step % 97 == 96 { 40.0 } else { 1.7 };
            eye = [
                eye[0] + (dealt() - 0.45) * stride,
                eye[1] + (dealt() - 0.55) * stride,
            ];
            tracks.follow(eye);
            tracks.tread(foot(eye[0], eye[1]));
            tracks.tread(foot(
                eye[0] + (dealt() - 0.5) * 50.0,
                eye[1] + (dealt() - 0.5) * 50.0,
            ));
            if step % 5 == 0 {
                tracks.fill(0.02);
            }
            if step == 300 {
                tracks.clear();
            }
            if let Some(changed) = tracks.take_changed() {
                for (first, bytes) in tracks.pieces(changed).into_iter().flatten() {
                    let at = (first * SIDE) as usize;
                    card[at..at + bytes.len()].copy_from_slice(bytes);
                }
            }
            assert!(card == tracks.map, "after step {step}");
        }
        assert!(tracks.map.iter().any(|point| *point != 0));
    }

    #[test]
    fn pacing_up_and_down_on_one_spot_does_not_move_the_square() {
        let mut tracks = Tracks::new();
        tracks.follow([3.9, 0.0]);
        tracks.tread(foot(3.9, 0.0));
        let _ = tracks.take_changed();
        // Back and forth over where the square would move on, four
        // metres along.
        for step in 0..40 {
            tracks.follow([if step % 2 == 0 { 4.1 } else { 3.9 }, 0.0]);
        }
        assert_eq!(handed(&mut tracks), 0);
        // Going on is what moves it.
        tracks.follow([12.0, 0.0]);
        assert!(tracks.at([3.9, 0.0]) > 0.99);
    }

    #[test]
    fn falling_snow_fills_tracks_in() {
        let mut tracks = Tracks::new();
        tracks.follow([0.0, 0.0]);
        tracks.tread(foot(0.0, 0.0));
        let rows = handed(&mut tracks);
        // A thousandth at a time, a little at each of many frames.
        for _ in 0..500 {
            tracks.fill(0.001);
        }
        let half = tracks.at([0.0, 0.0]);
        assert!((half - 0.5).abs() < 0.02, "{half}");
        // Only the rows the track is in are gone over and handed on.
        assert_eq!(handed(&mut tracks), rows);
        for _ in 0..600 {
            tracks.fill(0.001);
        }
        assert_eq!(tracks.at([0.0, 0.0]), 0.0);
        // With nothing trodden there is nothing to hand over.
        let _ = tracks.take_changed();
        tracks.fill(0.5);
        assert_eq!(handed(&mut tracks), 0);
    }
}
