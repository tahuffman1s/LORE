//! What the player does to water, and what it sounds like.
//!
//! Like a footstep, a splash is something that happened in the game: each
//! tick the world compares the player as they were with the player as
//! they are and works out whether they went into water, took a stroke,
//! put a foot down in the shallows or let their head go under. Each such
//! thing is a noise ([`World::noises`](super::World::noises)), which is
//! heard, and a mark on the water ([`World::water_marks`](super::World::water_marks)),
//! which is seen: drops, rings and bubbles. The same input makes the same
//! marks on every run.

use lore_physics::{Level, Player};

use crate::noise::{Noise, NoiseKind};

/// Something the player did to the water during one tick, for the picture
/// to show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaterMark {
    /// Something went into the water, or pushed through its surface: drops
    /// fly, and rings run out from it.
    Splash {
        /// Where it met the water, in metres: on the surface.
        place: [f32; 3],
        /// How fast it was moving, in metres a second.
        moving: [f32; 3],
        /// How wide it is, in metres.
        wide: f32,
    },
    /// Air let go under water: breath, or what a hand dragged down.
    Bubbles {
        /// Where they were let go, in metres.
        place: [f32; 3],
        /// How many.
        count: u32,
    },
}

/// Falling into water slower than this, in metres a second, is stepping
/// into it: the feet say so, not a splash.
const DROPPED_IN: f32 = 2.2;
/// Water shallower than this over the feet is splashed through; deeper,
/// it is waded.
const SPLASHED_THROUGH: f32 = 0.3;
/// How many seconds pass between breaths let go under water.
const BREATH_EVERY: f32 = 1.6;

/// Adds to `noises` and `marks` what `player` did to the water of `level`
/// in the tick numbered `tick`, which took `step` seconds and moved them
/// from where `before` has them. `yaw` is the way they face.
///
/// Call it every tick after the player has moved and their footsteps have
/// been heard: a step that came down in water is made the sound of one.
pub(crate) fn tick(
    before: &Player,
    player: &Player,
    level: &Level,
    yaw: f32,
    (tick, step): (u32, f32),
    noises: &mut Vec<Noise>,
    marks: &mut Vec<WaterMark>,
) {
    let [x, feet, z] = player.position;
    let Some(surface) = level.water_at(x, z).filter(|_| player.submerged > 0.0) else {
        return;
    };
    let deep = surface - feet;
    let [along_x, falling, along_z] = before.velocity;
    let speed = along_x.hypot(along_z);
    let (sin, cos) = yaw.sin_cos();
    let ahead = [-sin, -cos];
    let aside = [cos, -sin];
    let at = |forward: f32, right: f32, high: f32| {
        [
            x + ahead[0] * forward + aside[0] * right,
            high,
            z + ahead[1] * forward + aside[1] * right,
        ]
    };
    let noise = |kind: NoiseKind, loudness: f32| Noise {
        kind,
        position: [x, surface, z],
        loudness: loudness.clamp(0.0, 1.0),
        delay: 0.0,
        tick,
    };

    // A foot that came down this tick came down in water.
    let mut stepped = false;
    if !player.swimming
        && let Some(footfall) = noises
            .iter_mut()
            .rev()
            .find(|noise| noise.tick == tick && noise.kind.is_footfall())
    {
        stepped = true;
        let shallow = deep < SPLASHED_THROUGH;
        footfall.kind = if shallow {
            NoiseKind::SmallSplash
        } else {
            NoiseKind::Wade
        };
        footfall.loudness = (footfall.loudness * 1.3).clamp(0.2, 1.0);
        marks.push(WaterMark::Splash {
            place: at(0.25, 0.0, surface),
            moving: [along_x * 0.9, -(1.4 + 0.5 * speed), along_z * 0.9],
            wide: if shallow { 0.12 } else { 0.22 },
        });
    }

    // Into the water from the air: the faster, the more of a splash.
    if before.submerged <= 0.0 && falling < -DROPPED_IN {
        let hard = (-falling / 9.0).clamp(0.25, 1.0);
        marks.push(WaterMark::Splash {
            place: [x, surface, z],
            moving: before.velocity,
            wide: 0.5,
        });
        noises.push(noise(NoiseKind::Splash, hard));
        // The splash is the landing: no thud of feet as well.
        noises.retain(|noise| !(noise.tick == tick && noise.kind == NoiseKind::Landing));
    } else if before.submerged <= 0.0 && !stepped {
        // Let down into it gently.
        noises.push(noise(NoiseKind::SmallSplash, 0.5));
    }

    // A stroke: at the surface a hand goes in ahead, left and right in
    // turn; under water it only drags a little air with it.
    if player.pulls != before.pulls {
        let side = if player.pulls.is_multiple_of(2) {
            0.32
        } else {
            -0.32
        };
        if player.head_under {
            marks.push(WaterMark::Bubbles {
                place: at(0.4, side, feet + player.eye_height - 0.1),
                count: 3,
            });
        } else {
            let hard = if player.sprinting || speed > 1.4 {
                1.0
            } else {
                0.6
            };
            marks.push(WaterMark::Splash {
                place: at(0.5, side, surface),
                moving: [
                    along_x + ahead[0] * 1.6 * hard,
                    -1.9 * hard,
                    along_z + ahead[1] * 1.6 * hard,
                ],
                wide: 0.11,
            });
            noises.push(noise(NoiseKind::SwimStroke, 0.45 + 0.4 * hard));
        }
    }

    // The head going under, and coming up again.
    let mouth = [x, feet + player.eye_height - 0.08, z];
    if player.head_under && !before.head_under {
        marks.push(WaterMark::Bubbles {
            place: mouth,
            count: 14,
        });
        noises.push(noise(NoiseKind::Bubbles, 0.7));
    } else if !player.head_under && before.head_under {
        marks.push(WaterMark::Splash {
            place: [x, surface, z],
            moving: [along_x, 1.6 + falling.max(0.0), along_z],
            wide: 0.24,
        });
        noises.push(noise(NoiseKind::Surface, 0.8));
    } else if player.head_under && step > 0.0 {
        // Breath, let go a little at a time. Counted in ticks, so that it
        // comes on the same tick every run.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let every = ((BREATH_EVERY / step) as u32).max(1);
        if tick.is_multiple_of(every) {
            marks.push(WaterMark::Bubbles {
                place: mouth,
                count: 5,
            });
            noises.push(noise(NoiseKind::Bubbles, 0.35));
        }
    }
}
