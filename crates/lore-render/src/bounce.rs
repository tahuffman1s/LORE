//! Bounce light (#287): light that has struck one thing and lights
//! another. A wall in the sun warms the shade across the street, a lamp
//! lights the ceiling above it by what its floor throws back, and a room
//! with no window stays dark however bright the day outside.
//!
//! It is worked out as Dynamic Diffuse Global Illumination does (Majercik
//! et al., JCGT 8(2), 2019). A grid of probes round the eye, a metre apart,
//! each keeps the light that reaches it from every direction, and how far
//! away what it sees is, as two small square pictures folded from a sphere
//! (octahedral maps). Each frame some of the probes send rays out through a
//! copy of the level kept as a grid of blocks half a metre across, and what
//! each ray strikes is lit by the sun, the lamps, and the light the probes
//! round it already hold; so light bounces once a frame, and as often as it
//! likes over a few. A ray that strikes nothing sees the sky and the
//! ground. A surface then takes its light from the eight probes round it,
//! leaving out any that cannot see it (Chebyshev's bound on the distances
//! the probe keeps), which is what stops light leaking through walls.
//!
//! The copy of the level is drawn by the graphics card each time the eye
//! has gone four metres, into a block of 128 × 64 × 128: each triangle is
//! drawn looking along the axis it faces most, and every pixel it covers
//! marks its block as solid, in the colour of its surface there.
//!
//! What is worked out here, on the processor, is which probes trace each
//! frame, which have newly come into the grid as it follows the eye, and
//! where the copy of the level is: the same on every card, and tested
//! without one.

use std::cell::RefCell;
use std::sync::Arc;

use ash::vk;

use crate::buffer::Buffer;
use crate::error::RenderError;
use crate::gpu::Gpu;
use crate::scene::Picture;

/// How many probes the grid has along x, y and z.
pub(crate) const PROBES: [u32; 3] = [24, 12, 24];

/// How many probes there are in all.
pub(crate) const PROBE_COUNT: usize = (PROBES[0] * PROBES[1] * PROBES[2]) as usize;

/// How far apart the probes are, in metres.
pub(crate) const SPACING: f32 = 1.0;

/// How many blocks the copy of the level has along x, y and z.
pub(crate) const VOXELS: [u32; 3] = [128, 64, 128];

/// How wide a block of the copy is, in metres.
pub(crate) const VOXEL: f32 = 0.5;

/// How far from the eye a lamp's light may reach and still be handed to
/// the probes when the lamp itself is out of view, in metres: from the eye
/// to the grid's farthest corner.
pub(crate) const LAMPS_AROUND: f32 = 18.0;

/// How many blocks the copy moves at a time as the eye moves.
const VOXEL_STEP: i32 = 8;

/// How many pixels wide and high each probe's pictures are: eight, and a
/// border of one all round that repeats the edge, so that the pictures
/// can be smoothed across their edges.
pub(crate) const TILE: u32 = 10;

/// How many probes' pictures are side by side in a row of the atlas.
pub(crate) const TILES_ACROSS: u32 = 96;

/// How many pixels wide and high the atlases are.
pub(crate) const ATLAS: [u32; 2] = [
    TILES_ACROSS * TILE,
    (PROBE_COUNT as u32).div_ceil(TILES_ACROSS) * TILE,
];

/// The most rays one probe sends in a frame.
pub(crate) const MOST_RAYS: u32 = 128;

/// The most probes newly in the grid that are traced in one frame. More
/// than this (the eye has jumped) clears the grid, and they are traced over
/// the next few frames, unread until they are.
const MOST_FRESH: usize = 1536;

/// The most probes traced in one frame: those of the highest setting's
/// turn and as many new ones as there can be.
pub(crate) const MOST_LISTED: usize = PROBE_COUNT / 4 + MOST_FRESH;

/// The bytes before the rays' directions in the shader's `Bounce`, and
/// before the list of probes: the head, then a direction for every ray.
const HEAD_BYTES: usize = 9 * 16;
const WAYS_BYTES: usize = MOST_RAYS as usize * 16;
const LIST_FROM: usize = HEAD_BYTES + WAYS_BYTES;

/// The bytes the shader's `Bounce` takes at most.
pub(crate) const BOUNCE_BYTES: usize = LIST_FROM + MOST_LISTED * 4;

/// The bit of a listed probe that says it is new to the grid, and its
/// light is to be written whole rather than blended with what it held.
const FRESH_BIT: u32 = 1 << 31;

/// How much light bounces, and how quickly it follows a change: a
/// player's setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Bounce {
    /// Light does not bounce: shade is lit by the sky and the ground, and
    /// guessed in narrow streets, as before.
    Off,
    /// 32 rays from each probe, each probe every sixteenth frame.
    Low,
    /// 64 rays, every eighth frame.
    #[default]
    Medium,
    /// 128 rays, every fourth frame.
    High,
}

impl Bounce {
    /// How many rays each probe sends when it traces.
    #[must_use]
    pub fn rays(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::Low => 32,
            Self::Medium => 64,
            Self::High => MOST_RAYS,
        }
    }

    /// One probe in how many traces each frame.
    fn turn(self) -> usize {
        match self {
            Self::Off | Self::Low => 16,
            Self::Medium => 8,
            Self::High => 4,
        }
    }

    /// How much of what a probe holds is the light it has just found: more
    /// for a probe that traces less often, so that light follows a change
    /// as quickly on every setting (in about two thirds of a second).
    fn weight(self) -> f32 {
        match self {
            Self::Off | Self::Low => 0.3,
            Self::Medium => 0.2,
            Self::High => 0.12,
        }
    }
}

/// What has to be done for bounce light this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Plan<'a> {
    /// The shader's `Bounce`: as much of it as this frame uses.
    pub(crate) bytes: &'a [u8],
    /// How many probes trace.
    pub(crate) listed: u32,
    /// How many rays each sends.
    pub(crate) rays: u32,
    /// Whether the copy of the level has to be drawn again first: it has
    /// moved, or the level is new.
    pub(crate) voxelize: bool,
    /// Whether every probe's light is to be forgotten first: the eye has
    /// jumped, or the level is new.
    pub(crate) clear: bool,
}

/// Where the probes and the copy of the level are, kept from frame to
/// frame.
#[derive(Debug)]
pub(crate) struct ProbeGrid {
    /// The probe at the grid's lowest corner, counted in probes from the
    /// middle of the world; `None` before the first frame.
    origin: Option<[i32; 3]>,
    /// The block at the copy's lowest corner, counted in blocks.
    voxels: Option<[i32; 3]>,
    /// The level the grid was last planned for.
    level: u64,
    /// For each probe, whether it is new to the grid and not traced yet.
    fresh: Vec<bool>,
    /// The first probe of the next frame's turn.
    next: usize,
    /// How many frames have been planned: it turns the rays.
    frame: u32,
    /// The eye and the setting the last frame was planned for.
    last: Option<([f32; 3], Bounce)>,
    /// The shader's `Bounce`, made again each frame in the same memory.
    bytes: Vec<u8>,
}

impl Default for ProbeGrid {
    fn default() -> Self {
        Self {
            origin: None,
            voxels: None,
            level: 0,
            fresh: vec![true; PROBE_COUNT],
            next: 0,
            frame: 0,
            last: None,
            bytes: Vec::with_capacity(BOUNCE_BYTES),
        }
    }
}

/// The probe slot (where in the atlas its pictures are) that holds the
/// probe at `cell` (counted in probes from the middle of the world): the
/// grid wraps round, so that as it follows the eye only the probes that
/// come into it are new. The shader's `probe_slot`.
#[cfg(test)]
fn slot_of(cell: [i32; 3]) -> usize {
    let [x, y, z] = [0, 1, 2].map(|axis| {
        // The grid is far smaller than an i32.
        #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
        let wrapped = cell[axis].rem_euclid(PROBES[axis] as i32) as usize;
        wrapped
    });
    x + PROBES[0] as usize * (y + PROBES[1] as usize * z)
}

/// The probe at `slot` in a grid whose lowest corner is the probe `origin`.
pub(crate) fn cell_of(slot: usize, origin: [i32; 3]) -> [i32; 3] {
    let dims = PROBES.map(|count| count as usize);
    let at = [
        slot % dims[0],
        slot / dims[0] % dims[1],
        slot / (dims[0] * dims[1]),
    ];
    std::array::from_fn(|axis| {
        // All of these are far smaller than an i32.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let (place, count) = (at[axis] as i32, dims[axis] as i32);
        origin[axis] + (place - origin[axis]).rem_euclid(count)
    })
}

impl ProbeGrid {
    /// Works out what bounce light needs this frame for an eye at `eye` in
    /// the level numbered `level` (0 for none), as `bounce` asks.
    ///
    /// With time held `still`, once every probe has traced, nothing is
    /// traced again unless the eye, the level or the setting changes, so
    /// that the same view gives the same picture however many times it is
    /// drawn.
    pub(crate) fn plan(
        &mut self,
        eye: [f32; 3],
        level: u64,
        bounce: Bounce,
        still: bool,
    ) -> Plan<'_> {
        self.bytes.clear();
        self.bytes.resize(LIST_FROM, 0);
        let sound = eye
            .iter()
            .all(|part| part.is_finite() && part.abs() < 1.0e6);
        if bounce == Bounce::Off || level == 0 || !sound {
            // Whatever the grid held is forgotten: when light bounces again
            // every probe starts afresh.
            self.origin = None;
            self.voxels = None;
            self.last = None;
            return Plan {
                bytes: &self.bytes[..HEAD_BYTES],
                listed: 0,
                rays: 0,
                voxelize: false,
                clear: false,
            };
        }
        let new_level = level != self.level;
        if still
            && !new_level
            && self.last == Some((eye, bounce))
            && self.origin.is_some()
            && self.settled()
        {
            // The head as it was, with nothing to trace.
            let (origin, voxels) = (
                self.origin.unwrap_or_default(),
                self.voxels.unwrap_or_default(),
            );
            let head = self.head(origin, origin, voxels, bounce, 0, 0);
            self.bytes[..HEAD_BYTES].copy_from_slice(&head);
            return Plan {
                bytes: &self.bytes[..HEAD_BYTES],
                listed: 0,
                rays: bounce.rays(),
                voxelize: false,
                clear: false,
            };
        }
        self.last = Some((eye, bounce));
        self.level = level;

        // The probes round the eye, the eye at the middle.
        // The eye is well inside an i32 of metres (checked above).
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let origin: [i32; 3] = std::array::from_fn(|axis| {
            (eye[axis] / SPACING).floor() as i32 - PROBES[axis] as i32 / 2
        });
        let mut clear = new_level || self.origin.is_none();
        // Where the grid was: a probe new to it holds, until its rays are
        // traced, what the probe its slot held before saw, far away.
        let mut was = self.origin.unwrap_or(origin);
        if !clear && self.origin != Some(origin) {
            let was = self.origin.unwrap_or(origin);
            let mut moved = 0;
            for (slot, fresh) in self.fresh.iter_mut().enumerate() {
                if cell_of(slot, was) != cell_of(slot, origin) {
                    *fresh = true;
                    moved += 1;
                }
            }
            clear = moved > MOST_FRESH;
        }
        if clear {
            self.fresh.fill(true);
            was = origin;
        }
        self.origin = Some(origin);

        // The copy of the level round the eye, moved a step at a time.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let voxels: [i32; 3] = std::array::from_fn(|axis| {
            let middle = (eye[axis] / VOXEL).floor() as i32 - VOXELS[axis] as i32 / 2;
            middle.div_euclid(VOXEL_STEP) * VOXEL_STEP
        });
        let voxelize = new_level || self.voxels != Some(voxels);
        self.voxels = Some(voxels);

        // The probes new to the grid first, then this frame's turn of the
        // rest; with the grid cleared, the rest wait for the new ones.
        let mut listed = 0_usize;
        for slot in 0..PROBE_COUNT {
            if listed == MOST_FRESH {
                break;
            }
            if self.fresh[slot] {
                self.fresh[slot] = false;
                self.list(slot | FRESH_BIT as usize);
                listed += 1;
            }
        }
        let still_fresh = self.fresh.iter().any(|fresh| *fresh);
        let mut turn = 0;
        if !still_fresh {
            // The new probes take the place of as many of the turn, so that
            // a frame in which the grid moves costs no more than another.
            turn = (PROBE_COUNT / bounce.turn()).saturating_sub(listed);
            for number in 0..turn {
                self.list((self.next + number) % PROBE_COUNT);
            }
            self.next = (self.next + turn) % PROBE_COUNT;
            listed += turn;
        }
        self.frame = self.frame.wrapping_add(1);

        let head = self.head(origin, was, voxels, bounce, listed, turn);
        self.bytes[..HEAD_BYTES].copy_from_slice(&head);
        // Each ray's direction this frame, worked out once here rather than
        // in every pixel of every probe that reads it.
        let rotation = turned(self.frame.wrapping_sub(1));
        let rays = bounce.rays();
        for (number, slot) in self.bytes[HEAD_BYTES..LIST_FROM]
            .as_chunks_mut::<16>()
            .0
            .iter_mut()
            .enumerate()
            .take(rays as usize)
        {
            let way = ray_way(u32::try_from(number).unwrap_or(0), rays, &rotation);
            for (part, value) in slot
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(way.iter().chain(&[0.0]))
            {
                *part = value.to_ne_bytes();
            }
        }
        Plan {
            bytes: &self.bytes,
            listed: u32::try_from(listed).unwrap_or(0),
            rays: bounce.rays(),
            voxelize,
            clear,
        }
    }

    /// Whether every probe in the grid has traced at least once since it
    /// came into it (or light does not bounce).
    pub(crate) fn settled(&self) -> bool {
        self.origin.is_none() || !self.fresh.iter().any(|fresh| *fresh)
    }

    /// Where the copy of the level is: its lowest and highest corners, in
    /// metres; `None` while light does not bounce.
    pub(crate) fn voxel_box(&self) -> Option<[[f32; 3]; 2]> {
        let low = self.voxels?;
        // Block counts are far below where an f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let corner = |offset: [u32; 3]| {
            std::array::from_fn(|axis| (low[axis] as f32 + offset[axis] as f32) * VOXEL)
        };
        Some([corner([0; 3]), corner(VOXELS)])
    }

    /// Adds a probe to this frame's list.
    fn list(&mut self, entry: usize) {
        let entry = u32::try_from(entry).unwrap_or(0);
        self.bytes.extend_from_slice(&entry.to_ne_bytes());
    }

    /// The head of the shader's `Bounce`, for a grid at `origin` that was
    /// at `was` the frame before.
    fn head(
        &self,
        origin: [i32; 3],
        was: [i32; 3],
        voxels: [i32; 3],
        bounce: Bounce,
        listed: usize,
        turn: usize,
    ) -> [u8; HEAD_BYTES] {
        // Block counts are far below where an f32 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        let corner = voxels.map(|block| block as f32 * VOXEL);
        // A turn of the rays for each frame, so that over a few the probes
        // see in every direction; the same every run.
        let rotation = turned(self.frame.wrapping_sub(1));
        let mut rows = [[0_u32; 4]; 9];
        let float = |row: [f32; 4]| row.map(f32::to_bits);
        let int = |row: [i32; 4]| row.map(i32::cast_unsigned);
        rows[0] = float([corner[0], corner[1], corner[2], VOXEL]);
        rows[1] = [VOXELS[0], VOXELS[1], VOXELS[2], 0];
        rows[2] = int([origin[0], origin[1], origin[2], 0]);
        rows[3] = float([SPACING, bounce.weight(), 1.0, 0.0]);
        for (row, turn) in rows[4..7].iter_mut().zip(rotation) {
            *row = float([turn[0], turn[1], turn[2], 0.0]);
        }
        rows[7] = [
            u32::try_from(listed).unwrap_or(0),
            u32::try_from(turn).unwrap_or(0),
            bounce.rays(),
            self.frame,
        ];
        rows[8] = int([was[0], was[1], was[2], 0]);
        let mut bytes = [0_u8; HEAD_BYTES];
        for (slot, word) in bytes
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rows.as_flattened())
        {
            *slot = word.to_ne_bytes();
        }
        bytes
    }
}

/// Ray `number` of `count`, spread evenly over the sphere (a spherical
/// Fibonacci set) and turned by `rotation`. The shader's `ray_way`.
fn ray_way(number: u32, count: u32, rotation: &[[f32; 3]; 3]) -> [f32; 3] {
    // Counts are far below where an f32 stops being exact.
    #[allow(clippy::cast_precision_loss)]
    let (number, count) = (number as f32, count.max(1) as f32);
    let z = 1.0 - (2.0 * number + 1.0) / count;
    let out = (1.0 - z * z).max(0.0).sqrt();
    let round = number * 2.399_963_2;
    let way = [round.cos() * out, round.sin() * out, z];
    rotation.map(|row| row[0] * way[0] + row[1] * way[1] + row[2] * way[2])
}

/// A turn of the sphere for frame `frame`: about a direction and by an
/// angle that hop about from frame to frame in a way that covers every
/// turn evenly (the golden ratio's steps), as rows of a matrix.
fn turned(frame: u32) -> [[f32; 3]; 3] {
    let step = |scale: f64| ((f64::from(frame) * scale).fract()) as f32;
    let (a, b, c) = (step(0.618_034), step(0.754_878), step(0.569_840));
    // A random rotation from three numbers (Shoemake, "Uniform random
    // rotations", Graphics Gems III).
    let (s1, s2) = ((1.0 - a).sqrt(), a.sqrt());
    let (t1, t2) = (std::f32::consts::TAU * b, std::f32::consts::TAU * c);
    let [x, y, z, w] = [t1.sin() * s1, t1.cos() * s1, t2.sin() * s2, t2.cos() * s2];
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// The format of the copy of the level: each block's colour and whether it
/// is solid.
const VOXEL_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
/// The format of the probes' light, of how far away what they see is, and
/// of what this frame's rays found.
pub(crate) const LIGHT_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
pub(crate) const DEPTH_FORMAT: vk::Format = vk::Format::R16G16_SFLOAT;
pub(crate) const RAYS_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

/// How many pixels wide and high the picture the level is drawn into the
/// copy through: two a block along the copy's longest side.
pub(crate) const VOXEL_PICTURE: u32 = 2 * VOXELS[0];

/// Bounce light's pictures and pipelines on the graphics card, and the
/// grid it keeps on the processor.
pub(crate) struct BounceWorks {
    gpu: Arc<Gpu>,
    /// The copy of the level, always in the general layout: written by
    /// the voxel pass and read by the rays; and the way each block's
    /// surface faces, likewise.
    pub(crate) voxels: Picture,
    pub(crate) normals: Picture,
    /// Every probe's light, and how far away what it sees is: read by
    /// shaders except while the probes write them.
    pub(crate) light: Picture,
    pub(crate) depth: Picture,
    /// What this frame's rays found: read except while the rays are traced.
    pub(crate) rays: Picture,
    /// The shader's `Bounce` for each frame that can be in progress at once.
    pub(crate) buffers: Vec<Buffer>,
    /// Draws the level into the copy, traces the rays, and writes the
    /// probes: null where the card cannot write from a fragment shader, and
    /// light then does not bounce.
    pub(crate) voxel_pipeline: vk::Pipeline,
    pub(crate) trace_pipeline: vk::Pipeline,
    pub(crate) probe_pipeline: vk::Pipeline,
    /// Where the probes and the copy are.
    pub(crate) grid: RefCell<ProbeGrid>,
}

/// The whole of a picture with one level and one layer.
pub(crate) fn colour_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// A change of layout for the whole of `image`.
pub(crate) fn whole(
    image: vk::Image,
    old: vk::ImageLayout,
    new: vk::ImageLayout,
) -> vk::ImageMemoryBarrier2<'static> {
    vk::ImageMemoryBarrier2::default()
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(colour_range())
        .old_layout(old)
        .new_layout(new)
}

impl BounceWorks {
    /// Makes the pictures, empty, for up to `frames` frames in progress at
    /// once. The pipelines are made with the rest of the mesh pipeline's.
    pub(crate) fn new(gpu: &Arc<Gpu>, frames: usize) -> Result<Self, RenderError> {
        let doing = "making room for bounce light";
        let mut works = Self {
            gpu: Arc::clone(gpu),
            voxels: Picture::null(),
            normals: Picture::null(),
            light: Picture::null(),
            depth: Picture::null(),
            rays: Picture::null(),
            buffers: Vec::with_capacity(frames.max(1)),
            voxel_pipeline: vk::Pipeline::null(),
            trace_pipeline: vk::Pipeline::null(),
            probe_pipeline: vk::Pipeline::null(),
            grid: RefCell::new(ProbeGrid::default()),
        };
        for block in [&mut works.voxels, &mut works.normals] {
            block.create_block(
                gpu,
                vk::Extent3D {
                    width: VOXELS[0],
                    height: VOXELS[1],
                    depth: VOXELS[2],
                },
                VOXEL_FORMAT,
                vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_DST,
                colour_range(),
                doing,
            )?;
        }
        let atlas = vk::Extent2D {
            width: ATLAS[0],
            height: ATLAS[1],
        };
        let written = vk::ImageUsageFlags::COLOR_ATTACHMENT
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_DST;
        works
            .light
            .create(gpu, atlas, LIGHT_FORMAT, written, colour_range(), doing)?;
        works
            .depth
            .create(gpu, atlas, DEPTH_FORMAT, written, colour_range(), doing)?;
        works.rays.create(
            gpu,
            vk::Extent2D {
                width: MOST_RAYS,
                height: u32::try_from(MOST_LISTED).unwrap_or(u32::MAX),
            },
            RAYS_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            colour_range(),
            doing,
        )?;
        for _ in 0..frames.max(1) {
            works.buffers.push(Buffer::writable(
                gpu,
                BOUNCE_BYTES as u64,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                doing,
            )?);
            // Until a frame says otherwise, light does not bounce.
            if let Some(buffer) = works.buffers.last() {
                // SAFETY: the buffer is new, writable and longer than this.
                unsafe { buffer.write(&[0; HEAD_BYTES]) }?;
            }
        }
        // Every picture starts empty and in the layout it is read in, so
        // that a shader that only might read one finds it ready.
        let device = gpu.device();
        let (voxels, normals, light, depth, rays) = (
            works.voxels.image,
            works.normals.image,
            works.light.image,
            works.depth.image,
            works.rays.image,
        );
        // SAFETY: the pictures are this device's, new, and used by nothing.
        unsafe {
            crate::buffer::once(gpu, doing, |commands| {
                let to_clear = [
                    whole(voxels, vk::ImageLayout::UNDEFINED, vk::ImageLayout::GENERAL),
                    whole(
                        normals,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::GENERAL,
                    ),
                    whole(
                        light,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    ),
                    whole(
                        depth,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    ),
                    whole(
                        rays,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    ),
                ]
                .map(|barrier| {
                    barrier
                        .dst_stage_mask(vk::PipelineStageFlags2::CLEAR)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                });
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_clear),
                );
                let nothing = vk::ClearColorValue { float32: [0.0; 4] };
                for block in [voxels, normals] {
                    device.cmd_clear_color_image(
                        commands,
                        block,
                        vk::ImageLayout::GENERAL,
                        &nothing,
                        &[colour_range()],
                    );
                }
                for image in [light, depth] {
                    device.cmd_clear_color_image(
                        commands,
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &nothing,
                        &[colour_range()],
                    );
                }
                let to_read = [
                    whole(voxels, vk::ImageLayout::GENERAL, vk::ImageLayout::GENERAL),
                    whole(normals, vk::ImageLayout::GENERAL, vk::ImageLayout::GENERAL),
                    whole(
                        light,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    ),
                    whole(
                        depth,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    ),
                ]
                .map(|barrier| {
                    barrier
                        .src_stage_mask(vk::PipelineStageFlags2::CLEAR)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                        .dst_access_mask(vk::AccessFlags2::SHADER_READ)
                });
                device.cmd_pipeline_barrier2(
                    commands,
                    &vk::DependencyInfo::default().image_memory_barriers(&to_read),
                );
            })?;
        }
        Ok(works)
    }

    /// Whether light can bounce on this card: the pipelines were made.
    pub(crate) fn works(&self) -> bool {
        self.voxel_pipeline != vk::Pipeline::null()
            && self.trace_pipeline != vk::Pipeline::null()
            && self.probe_pipeline != vk::Pipeline::null()
    }

    /// Makes the three pipelines from the mesh pipeline's shader module and
    /// layout, and the states its own pipelines share.
    ///
    /// # Safety
    ///
    /// `module` and `layout` must be live and of this device, and hold the
    /// entry points and sets `bounce.wgsl` names.
    pub(crate) unsafe fn make_pipelines(
        &mut self,
        module: vk::ShaderModule,
        layout: vk::PipelineLayout,
        vertex_input: &vk::PipelineVertexInputStateCreateInfo<'_>,
        shared: &Shared<'_>,
    ) -> Result<(), vk::Result> {
        let device = self.gpu.device();
        let stage = |kind: vk::ShaderStageFlags, name: &'static std::ffi::CStr| {
            vk::PipelineShaderStageCreateInfo::default()
                .stage(kind)
                .module(module)
                .name(name)
        };
        let no_depth = vk::PipelineDepthStencilStateCreateInfo::default();
        let no_points = vk::PipelineVertexInputStateCreateInfo::default();
        let make = |info: &vk::GraphicsPipelineCreateInfo<'_>| {
            // SAFETY: the caller guarantees the module and layout; `info`
            // and what it points to live for the call.
            unsafe {
                device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(info),
                    None,
                )
            }
            .map_err(|(_, code)| code)
            .and_then(|made| made.first().copied().ok_or(vk::Result::ERROR_UNKNOWN))
        };

        // The level into the copy: no picture to draw into, only the blocks
        // the fragment shader writes; each triangle drawn three times.
        if self.gpu.fragment_stores() {
            let stages = [
                stage(vk::ShaderStageFlags::VERTEX, c"vertex_voxel"),
                stage(vk::ShaderStageFlags::FRAGMENT, c"fragment_voxel"),
            ];
            let mut rendering = vk::PipelineRenderingCreateInfo::default();
            let blend = vk::PipelineColorBlendStateCreateInfo::default();
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(vertex_input)
                .input_assembly_state(shared.assembly)
                .viewport_state(shared.viewport)
                .rasterization_state(shared.raster)
                .multisample_state(shared.multisample)
                .depth_stencil_state(&no_depth)
                .color_blend_state(&blend)
                .dynamic_state(shared.dynamic)
                .layout(layout)
                .push_next(&mut rendering);
            self.voxel_pipeline = make(&info)?;
        }

        // The rays: one pixel each.
        let stages = [
            stage(vk::ShaderStageFlags::VERTEX, c"vertex_whole"),
            stage(vk::ShaderStageFlags::FRAGMENT, c"fragment_trace"),
        ];
        let formats = [RAYS_FORMAT];
        let mut rendering =
            vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let attachments = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&no_points)
            .input_assembly_state(shared.assembly)
            .viewport_state(shared.viewport)
            .rasterization_state(shared.raster)
            .multisample_state(shared.multisample)
            .depth_stencil_state(&no_depth)
            .color_blend_state(&blend)
            .dynamic_state(shared.dynamic)
            .layout(layout)
            .push_next(&mut rendering);
        self.trace_pipeline = make(&info)?;

        // The probes: a square of each probe's two pictures, blended into
        // what they held by as much as the shader says in alpha; a probe
        // keeps 1 in its light's alpha once it has traced.
        let stages = [
            stage(vk::ShaderStageFlags::VERTEX, c"vertex_probe"),
            stage(vk::ShaderStageFlags::FRAGMENT, c"fragment_probe"),
        ];
        let formats = [LIGHT_FORMAT, DEPTH_FORMAT];
        let mut rendering =
            vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let blended = vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE)
            .alpha_blend_op(vk::BlendOp::MAX);
        // The same for both: a card need not offer blending each picture
        // its own way (`independentBlend`), and the distances' picture has
        // only red and green to write anyway.
        let attachments = [blended.color_write_mask(vk::ColorComponentFlags::RGBA); 2];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&no_points)
            .input_assembly_state(shared.assembly)
            .viewport_state(shared.viewport)
            .rasterization_state(shared.raster)
            .multisample_state(shared.multisample)
            .depth_stencil_state(&no_depth)
            .color_blend_state(&blend)
            .dynamic_state(shared.dynamic)
            .layout(layout)
            .push_next(&mut rendering);
        self.probe_pipeline = make(&info)?;
        Ok(())
    }

    /// The pictures and buffer a frame's set points at, as image and
    /// buffer infos for bindings 18 to 25 of the mesh set.
    pub(crate) fn infos(&self, frame: usize) -> BounceInfos {
        let picture = |view: vk::ImageView, layout: vk::ImageLayout| {
            [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(layout)]
        };
        let buffer = self
            .buffers
            .get(frame % self.buffers.len().max(1))
            .map_or(vk::Buffer::null(), |buffer| buffer.handle);
        BounceInfos {
            buffer: [vk::DescriptorBufferInfo::default()
                .buffer(buffer)
                .range(BOUNCE_BYTES as u64)],
            voxels: picture(self.voxels.view, vk::ImageLayout::GENERAL),
            normals: picture(self.normals.view, vk::ImageLayout::GENERAL),
            light: picture(self.light.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
            depth: picture(self.depth.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
            rays: picture(self.rays.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
        }
    }
}

/// The states the mesh pipeline's own pipelines share, lent to bounce
/// light's.
pub(crate) struct Shared<'a> {
    pub(crate) assembly: &'a vk::PipelineInputAssemblyStateCreateInfo<'a>,
    pub(crate) viewport: &'a vk::PipelineViewportStateCreateInfo<'a>,
    pub(crate) raster: &'a vk::PipelineRasterizationStateCreateInfo<'a>,
    pub(crate) multisample: &'a vk::PipelineMultisampleStateCreateInfo<'a>,
    pub(crate) dynamic: &'a vk::PipelineDynamicStateCreateInfo<'a>,
}

/// What [`BounceWorks::infos`] gives, kept alive while the writes that
/// point at it are made.
pub(crate) struct BounceInfos {
    pub(crate) buffer: [vk::DescriptorBufferInfo; 1],
    pub(crate) voxels: [vk::DescriptorImageInfo; 1],
    pub(crate) normals: [vk::DescriptorImageInfo; 1],
    pub(crate) light: [vk::DescriptorImageInfo; 1],
    pub(crate) depth: [vk::DescriptorImageInfo; 1],
    pub(crate) rays: [vk::DescriptorImageInfo; 1],
}

impl BounceInfos {
    /// The writes that point the set `set` at these.
    pub(crate) fn writes(&self, set: vk::DescriptorSet) -> [vk::WriteDescriptorSet<'_>; 8] {
        let image = |binding: u32, kind: vk::DescriptorType, info| {
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding)
                .descriptor_type(kind)
                .image_info(info)
        };
        [
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(18)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&self.buffer),
            image(19, vk::DescriptorType::SAMPLED_IMAGE, &self.voxels),
            image(20, vk::DescriptorType::STORAGE_IMAGE, &self.voxels),
            image(21, vk::DescriptorType::SAMPLED_IMAGE, &self.light),
            image(22, vk::DescriptorType::SAMPLED_IMAGE, &self.depth),
            image(23, vk::DescriptorType::SAMPLED_IMAGE, &self.rays),
            image(24, vk::DescriptorType::SAMPLED_IMAGE, &self.normals),
            image(25, vk::DescriptorType::STORAGE_IMAGE, &self.normals),
        ]
    }
}

impl Drop for BounceWorks {
    fn drop(&mut self) {
        // SAFETY: the owner waits for the graphics card to go idle before
        // dropping these, so none is in use; null handles are allowed.
        unsafe {
            let device = self.gpu.device();
            for pipeline in [
                self.voxel_pipeline,
                self.trace_pipeline,
                self.probe_pipeline,
            ] {
                device.destroy_pipeline(pipeline, None);
            }
            for picture in [
                &self.voxels,
                &self.normals,
                &self.light,
                &self.depth,
                &self.rays,
            ] {
                picture.destroy(&self.gpu);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(plan: &Plan<'_>) -> Vec<u32> {
        plan.bytes[LIST_FROM..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_ne_bytes(*word))
            .collect()
    }

    #[test]
    fn each_probe_has_its_own_slot_and_the_grid_wraps_round() {
        let origin = [-12, -3, 40];
        for slot in 0..PROBE_COUNT {
            let cell = cell_of(slot, origin);
            assert_eq!(slot_of(cell), slot);
            for axis in 0..3 {
                assert!(cell[axis] >= origin[axis]);
                assert!(cell[axis] < origin[axis] + PROBES[axis] as i32);
            }
        }
        // The atlas has a tile for every probe, and is the size the shader
        // says (`PROBE_ATLAS`).
        assert!(ATLAS[0] / TILE * (ATLAS[1] / TILE) >= PROBE_COUNT as u32);
        assert_eq!(ATLAS, [960, 720]);
    }

    #[test]
    fn the_probes_are_trusted_only_where_the_grid_reaches_wherever_the_eye_is() {
        // What the eye sees trusts the probes out to this far from it along
        // each axis (`probes_trusted` in `bounce.wgsl`), and the shader
        // finds a probe's place `PROBE_OFFSET` from its cell's middle.
        let reach = PROBES.map(|count| (count as f32 * 0.5 - 1.5) * SPACING);
        let offset = 0.125;
        let mut grid = ProbeGrid::default();
        for step in 0..400 {
            // Across whole metres and between them, below nought and above.
            let along = step as f32 * 0.137 - 27.0;
            let eye = [along, along * 0.61 + 1.7, -along * 1.3];
            let plan = grid.plan(eye, 1, Bounce::Medium, false);
            let origin = [0, 1, 2].map(|part| {
                let at = 2 * 16 + part * 4;
                i32::from_ne_bytes([
                    plan.bytes[at],
                    plan.bytes[at + 1],
                    plan.bytes[at + 2],
                    plan.bytes[at + 3],
                ])
            });
            for axis in 0..3 {
                for side in [-1.0, 1.0] {
                    // In probes from the lowest corner of the grid, as the
                    // shader counts.
                    let place = eye[axis] + side * reach[axis];
                    let grid_at = (place - offset) / SPACING - 0.5 - origin[axis] as f32;
                    let last = (PROBES[axis] - 1) as f32;
                    assert!(
                        grid_at > 0.0 && grid_at < last,
                        "at {eye:?} the probes are trusted {place} along axis {axis}, \
                         outside the grid ({grid_at} of 0 to {last})"
                    );
                }
            }
        }
    }

    #[test]
    fn the_first_frame_traces_every_probe_and_draws_the_level_then_takes_turns() {
        let mut grid = ProbeGrid::default();
        let first = grid.plan([0.5, 1.7, 0.5], 1, Bounce::Medium, false);
        assert!(first.voxelize && first.clear);
        // More probes than are traced in a frame are all new: the first
        // MOST_FRESH of them now, and no turn yet.
        assert_eq!(first.listed as usize, MOST_FRESH);
        let entries = listed(&first);
        assert!(entries.iter().all(|entry| entry & FRESH_BIT != 0));

        let mut frames = 1;
        loop {
            let plan = grid.plan([0.5, 1.7, 0.5], 1, Bounce::Medium, false);
            frames += 1;
            assert!(!plan.voxelize && !plan.clear);
            let entries = listed(&plan);
            if entries.iter().all(|entry| entry & FRESH_BIT == 0) {
                // Settled: a turn of an eighth of the probes, none new.
                assert_eq!(plan.listed as usize, PROBE_COUNT / 8);
                break;
            }
            assert!(frames < 10, "the new probes were never all traced");
        }
        // Every probe comes round in eight frames.
        let mut traced = vec![0; PROBE_COUNT];
        for _ in 0..8 {
            let plan = grid.plan([0.5, 1.7, 0.5], 1, Bounce::Medium, false);
            for entry in listed(&plan) {
                traced[entry as usize] += 1;
            }
        }
        assert!(traced.iter().all(|count| *count == 1));
    }

    #[test]
    fn a_step_brings_in_one_side_of_new_probes_and_a_jump_clears_the_grid() {
        let mut grid = ProbeGrid::default();
        for _ in 0..10 {
            grid.plan([0.5, 1.7, 0.5], 1, Bounce::Medium, false);
        }
        // A metre along x: one side of the grid is new.
        let step = grid.plan([1.5, 1.7, 0.5], 1, Bounce::Medium, false);
        assert!(!step.clear && !step.voxelize);
        let fresh = listed(&step)
            .iter()
            .filter(|entry| **entry & FRESH_BIT != 0)
            .count();
        assert_eq!(fresh, (PROBES[1] * PROBES[2]) as usize);
        // They take the place of as many of the turn: no more is traced.
        assert_eq!(step.listed as usize, PROBE_COUNT / 8);
        // The head says where the grid was, so that the rays traced this
        // frame do not read what the new probes' slots held before.
        let int = |row: usize| {
            [0, 1, 2].map(|part| {
                let at = row * 16 + part * 4;
                i32::from_ne_bytes([
                    step.bytes[at],
                    step.bytes[at + 1],
                    step.bytes[at + 2],
                    step.bytes[at + 3],
                ])
            })
        };
        assert_eq!(int(8)[0] + 1, int(2)[0]);
        assert_eq!(int(8)[1..], int(2)[1..]);
        // Four metres on, the copy of the level moves.
        let far = grid.plan([4.5, 1.7, 0.5], 1, Bounce::Medium, false);
        assert!(far.voxelize);
        // Fifty metres away, everything starts again.
        let jump = grid.plan([54.5, 1.7, 0.5], 1, Bounce::Medium, false);
        assert!(jump.clear && jump.voxelize);
        // Everything was forgotten: nothing is left to mistake for the new.
        assert_eq!(
            jump.bytes[2 * 16..2 * 16 + 12],
            jump.bytes[8 * 16..8 * 16 + 12]
        );
        // Another level too.
        let other = grid.plan([54.5, 1.7, 0.5], 2, Bounce::Medium, false);
        assert!(other.clear && other.voxelize);
    }

    #[test]
    fn off_or_no_level_traces_nothing_and_starts_afresh_after() {
        let mut grid = ProbeGrid::default();
        grid.plan([0.0; 3], 1, Bounce::Medium, false);
        let off = grid.plan([0.0; 3], 1, Bounce::Off, false);
        assert_eq!(off.listed, 0);
        assert_eq!(off.bytes.len(), HEAD_BYTES);
        // The head says light does not bounce.
        assert_eq!(off.bytes[3 * 16 + 8..3 * 16 + 12], [0; 4]);
        let again = grid.plan([0.0; 3], 1, Bounce::Medium, false);
        assert!(again.clear && again.voxelize);
        assert_eq!(
            grid.plan([f32::NAN, 0.0, 0.0], 1, Bounce::Medium, false)
                .listed,
            0
        );
    }

    #[test]
    fn held_still_the_same_view_traces_nothing_more() {
        let mut grid = ProbeGrid::default();
        let first = grid.plan([0.5, 1.7, 0.5], 1, Bounce::Medium, true);
        assert!(first.listed > 0);
        let head = first.bytes[..HEAD_BYTES].to_vec();
        // Until every probe has traced, held still or not, the new ones
        // are traced.
        let mut frames = 1;
        while !grid.settled() {
            assert!(grid.plan([0.5, 1.7, 0.5], 1, Bounce::Medium, true).listed > 0);
            frames += 1;
            assert!(frames < 10);
        }
        let again = grid.plan([0.5, 1.7, 0.5], 1, Bounce::Medium, true);
        assert_eq!(again.listed, 0);
        assert!(!again.voxelize && !again.clear);
        // Light still bounces: the head says where the probes are, as before.
        assert_eq!(again.bytes[..3 * 16 + 12], head[..3 * 16 + 12]);
        // Moving the eye, or another level, traces again.
        assert!(grid.plan([0.6, 1.7, 0.5], 1, Bounce::Medium, true).listed > 0);
        assert!(grid.plan([0.6, 1.7, 0.5], 2, Bounce::Medium, true).listed > 0);
    }

    #[test]
    fn the_rays_turn_each_frame_and_each_turn_is_a_rotation() {
        let one = turned(1);
        let two = turned(2);
        assert_ne!(one, two);
        for matrix in [one, two, turned(1000)] {
            for row in 0..3 {
                for other in 0..3 {
                    let dot: f32 = (0..3).map(|k| matrix[row][k] * matrix[other][k]).sum();
                    let want = if row == other { 1.0 } else { 0.0 };
                    assert!((dot - want).abs() < 1e-4, "{matrix:?}");
                }
            }
        }
    }
}
