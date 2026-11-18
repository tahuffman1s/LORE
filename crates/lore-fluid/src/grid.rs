//! A spatial hash of the lying drops, so that neighbours are found without
//! looking at every drop.

use crate::MOST_DROPS;
use crate::math::V3;
use crate::store::Particle;

/// Side of a cell, metres: a little over the widest merged patch's radius.
const CELL: f32 = 0.25;
/// How many buckets cells hash into; a power of two.
const BUCKETS: usize = 4096;
/// The longest run of cells a search covers on one axis.
const LONGEST_SPAN: i32 = 64;

/// The lying drops sorted into buckets by where they are.
///
/// Built by counting sort into arrays made once, so building and searching
/// allocate nothing. Within a bucket drops are in store order.
#[derive(Clone, Debug)]
pub(crate) struct Grid {
    /// Where each bucket's drops start in `items`; one extra at the end.
    starts: Vec<u32>,
    /// A working copy of `starts` while filling.
    cursor: Vec<u32>,
    /// Store indices of the lying drops, bucket by bucket.
    items: Vec<u32>,
}

/// The cell a coordinate falls in.
fn cell(x: f32) -> i32 {
    // Saturates, and a number that is not one becomes 0: both are harmless.
    libm::floorf(x * (1.0 / CELL)) as i32
}

/// The bucket of a cell: an integer hash with wrapping multiplies.
fn bucket(x: i32, y: i32, z: i32) -> usize {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ (z as u32).wrapping_mul(0xCB1A_B31F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 13;
    (h as usize) & (BUCKETS - 1)
}

impl Grid {
    /// An empty grid, with all its storage.
    pub(crate) fn new() -> Self {
        Self {
            starts: vec![0; BUCKETS + 1],
            cursor: vec![0; BUCKETS],
            items: Vec::with_capacity(MOST_DROPS),
        }
    }

    /// Sorts the lying drops of `parts` (every one that is alive) into
    /// buckets, forgetting what was there.
    pub(crate) fn build(&mut self, parts: &[Particle]) {
        self.starts.fill(0);
        let mut count = 0usize;
        for p in parts.iter().filter(|p| p.lying && !p.dead) {
            let b = bucket(cell(p.pos[0]), cell(p.pos[1]), cell(p.pos[2]));
            if let Some(s) = self.starts.get_mut(b + 1) {
                *s += 1;
            }
            count += 1;
        }
        let mut sum = 0u32;
        for s in &mut self.starts {
            sum += *s;
            *s = sum;
        }
        // Bucket `b` now runs from `starts[b]` to `starts[b + 1]`.
        for (cursor, window) in self.cursor.iter_mut().zip(self.starts.windows(2)) {
            *cursor = window.first().copied().unwrap_or(0);
        }
        self.items.clear();
        self.items.resize(count.min(MOST_DROPS), 0);
        for (index, p) in parts.iter().enumerate() {
            if !(p.lying && !p.dead) {
                continue;
            }
            let b = bucket(cell(p.pos[0]), cell(p.pos[1]), cell(p.pos[2]));
            if let Some(at) = self.cursor.get_mut(b) {
                if let Some(slot) = self.items.get_mut(*at as usize) {
                    *slot = index as u32;
                }
                *at += 1;
            }
        }
    }

    /// Calls `visit` with the store index of every lying drop in a cell that
    /// touches the box from `lo` to `hi`, each once, bucket by bucket. The
    /// caller still checks where the drop really is.
    pub(crate) fn near(&self, lo: V3, hi: V3, visit: &mut impl FnMut(usize)) {
        let (x0, x1) = span(lo[0], hi[0]);
        let (y0, y1) = span(lo[1], hi[1]);
        let (z0, z1) = span(lo[2], hi[2]);
        // Two cells can hash to one bucket: a drop must be visited once.
        let mut seen = [0u64; BUCKETS / 64];
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    let b = bucket(x, y, z);
                    let Some(word) = seen.get_mut(b / 64) else {
                        continue;
                    };
                    let bit = 1u64 << (b % 64);
                    if *word & bit != 0 {
                        continue;
                    }
                    *word |= bit;
                    let from = self.starts.get(b).copied().unwrap_or(0) as usize;
                    let to = self.starts.get(b + 1).copied().unwrap_or(0) as usize;
                    for &index in self.items.get(from..to).unwrap_or(&[]) {
                        visit(index as usize);
                    }
                }
            }
        }
    }
}

/// The first and last cell from `lo` to `hi`, no more than a sane number.
fn span(lo: f32, hi: f32) -> (i32, i32) {
    let first = cell(lo);
    let last = cell(hi).max(first).min(first.saturating_add(LONGEST_SPAN));
    (first, last)
}
