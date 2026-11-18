//! Chance that rolls the same way every time a game is played.

/// A stream of rolls that follows from one starting number (splitmix64).
///
/// Only spraying and splashing roll, and they roll in store order, so the
/// same sprays on the same ticks always make the same drops.
#[derive(Clone, Debug)]
pub(crate) struct Rolls(u64);

impl Rolls {
    /// The stream that starts from `seed`.
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next roll, from 0 up to but not including 1.
    pub(crate) fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // 24 bits: every one of them fits a float exactly.
        (z >> 40) as f32 * (1.0 / 16_777_216.0)
    }
}
