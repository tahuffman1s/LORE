//! The drops themselves, one record each.

use crate::lying::R_MAX;
use crate::math::V3;
use core::f32::consts::PI;

/// Radius of the ball of `volume` cubic metres.
pub(crate) fn ball_radius(volume: f32) -> f32 {
    libm::cbrtf(3.0 * volume / (4.0 * PI))
}

/// One drop in the store, in flight or lying on a surface.
///
/// The fields a neighbour search reads for every drop it passes come first,
/// so that they share a cache line (`repr(C)` keeps the order).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct Particle {
    /// Where it is: the ball's centre in flight, the point on the surface
    /// when it lies.
    pub pos: V3,
    /// Which way the surface faces, when it lies.
    pub normal: V3,
    /// Cubic metres.
    pub volume: f32,
    /// Thickness of the patch at its middle, when it lies.
    pub h: f32,
    /// Ticks it has lain without changing.
    pub still: u32,
    /// Which kind of fluid it is.
    pub kind: u8,
    /// Lying on a surface rather than flying.
    pub lying: bool,
    /// It touched down this tick and the others have not yet been told.
    pub landed_now: bool,
    /// To be removed at the end of the tick.
    pub dead: bool,
    /// Its thickness has changed enough that it must stay awake.
    pub changed: bool,
    /// It is awake but creeping, so it is only updated every few ticks.
    pub calm: bool,
    /// It was thrown off by a splash, and does not splash again where it
    /// lands: the droplets of a splash are small and slow, and a splash of
    /// splashes would fill the store with spatter.
    pub thrown: bool,
    /// It came out of a pool, over its rim or thrown up from it: part of
    /// a mix, which washes nothing away and covers nothing where it
    /// lands, as a jet of one clean kind does.
    pub of_a_mix: bool,
    /// A counter bumped for every drop made: the bigger, the newer.
    pub born: u64,
    /// Metres a second, in flight.
    pub vel: V3,
    /// How far, in metres, others have pushed it this tick.
    pub push: V3,
    /// The thickness it had when it last counted as having changed.
    pub h_ref: f32,
    /// Seconds before it has dried away, when its kind dries.
    pub dry_left: f32,
    /// Metres run since the last trail drop.
    pub run: f32,
    /// How much of its first tick it has already flown, 0 to 1: a spray is
    /// spread along its path, not bunched at the nozzle.
    pub head_start: f32,
    /// How many radii it is drawn out along its way, in flight.
    pub long: f32,
    /// Seconds in the air.
    pub flight_time: f32,
}

impl Particle {
    /// A drop in the air.
    pub(crate) fn flying(kind: u8, pos: V3, vel: V3, volume: f32, born: u64) -> Self {
        Self {
            pos,
            vel,
            normal: [0.0, 1.0, 0.0],
            push: [0.0; 3],
            volume,
            h: 0.0,
            h_ref: 0.0,
            dry_left: 0.0,
            run: 0.0,
            head_start: 0.0,
            long: 1.0,
            flight_time: 0.0,
            born,
            still: 0,
            kind,
            lying: false,
            landed_now: false,
            dead: false,
            changed: false,
            calm: false,
            thrown: false,
            of_a_mix: false,
        }
    }

    /// A drop lying on a surface at `pos` facing `normal`, `h` thick.
    pub(crate) fn lying(kind: u8, pos: V3, normal: V3, volume: f32, h: f32, born: u64) -> Self {
        // No patch is wider than R_MAX: every neighbour search relies on it,
        // so a big drop lies thick instead of wide.
        let h = h.max(1.0e-6).max(volume / (PI * R_MAX * R_MAX));
        Self {
            normal,
            h,
            h_ref: h,
            lying: true,
            ..Self::flying(kind, pos, [0.0; 3], volume, born)
        }
    }

    /// Radius of the ball this drop's volume would make.
    pub(crate) fn ball(&self) -> f32 {
        ball_radius(self.volume)
    }

    /// Radius of the patch it covers when it lies.
    pub(crate) fn patch(&self) -> f32 {
        (self.volume / (PI * self.h.max(1.0e-6))).sqrt()
    }
}
