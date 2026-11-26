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

use lore_physics::{Level, Medium, Player};

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

/// How a fluid changes the sounds water makes, by how thick and how heavy
/// it is: how high they are played (1 is as water) and how loud (1 is as
/// water). A thick fluid moves slowly and swallows its own splash, so it
/// is lower, longer and quieter: honey and tar glug. A heavy one is lower
/// too: mercury thuds. Water is exactly 1 and 1.
pub(crate) fn voice(medium: Medium) -> (f32, f32) {
    let medium = medium.cleaned();
    // From water (0) to a thousand and more times as thick (1).
    let thick =
        (libm::log10f((medium.viscosity / Medium::WATER.viscosity).max(1.0)) / 3.5).clamp(0.0, 1.0);
    // A fifth power root: mercury, 13.5 times as heavy, is 0.6.
    let heavy = libm::powf(
        (medium.density / Medium::WATER.density).clamp(0.2, 20.0),
        -0.2,
    );
    let pitch = ((1.0 - 0.45 * thick) * heavy).clamp(0.4, 1.3);
    (pitch, 1.0 - 0.5 * thick)
}

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
    // A head that was under and is now out of the water altogether (they
    // climbed out, or were put somewhere else) still came up.
    let was_under = before.head_under && !player.head_under;
    let surface = level.water_around(player.position);
    let Some(surface) = surface.filter(|_| player.submerged > 0.0) else {
        if was_under {
            noises.push(Noise::new(
                NoiseKind::Surface,
                player.position,
                0.8,
                1.0,
                0.0,
                tick,
            ));
        }
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
    // What they are in says how it sounds: honey glugs where water
    // splashes.
    let (pitch, quiet) = level
        .pool_around(player.position)
        .map_or((1.0, 1.0), |here| voice(here.medium));
    let noise = |kind: NoiseKind, loudness: f32| {
        Noise::new(
            kind,
            [x, surface, z],
            (loudness * quiet).clamp(0.0, 1.0),
            pitch,
            0.0,
            tick,
        )
    };

    // Into the water from the air, fast enough to be a splash and not a
    // step: the splash is the landing, with no thud of feet as well.
    // In water too shallow to stop them their feet find the bed a tick
    // or two later, still falling: that is the same splash, not a second.
    let dropped_in = before.submerged <= 0.0 && falling < -DROPPED_IN;
    let still_dropping = before.submerged > 0.0 && falling < -DROPPED_IN;
    if dropped_in || still_dropping {
        noises.retain(|noise| !(noise.tick == tick && noise.kind.is_footfall()));
    }

    // A foot that came down this tick came down in water.
    let mut stepped = false;
    if !player.swimming
        && !dropped_in
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
        footfall.loudness = (footfall.loudness * 1.3).clamp(0.2, 1.0) * quiet;
        footfall.pitch = pitch;
        marks.push(WaterMark::Splash {
            place: at(0.25, 0.0, surface),
            moving: [along_x * 0.9, -(1.4 + 0.5 * speed), along_z * 0.9],
            wide: if shallow { 0.12 } else { 0.22 },
        });
    }

    // The faster they came down and the deeper the water, the more of a
    // splash: a hop into a puddle wets the shins, a drop into a lake goes
    // right under.
    if dropped_in {
        // How deep the water is there, not how far in they are yet.
        let bed = level.water_depth(x, z);
        let shallow = bed < SPLASHED_THROUGH;
        // Water takes no more of a fall than there is water.
        let taken = falling.max(-(2.5 + 9.0 * bed));
        marks.push(WaterMark::Splash {
            place: [x, surface, z],
            moving: [along_x, taken, along_z],
            wide: if shallow { 0.2 } else { 0.5 },
        });
        if shallow {
            noises.push(noise(NoiseKind::SmallSplash, 0.9));
        } else {
            noises.push(noise(NoiseKind::Splash, (-falling / 9.0).clamp(0.25, 1.0)));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fluid_sounds_lower_the_thicker_and_the_heavier_it_is_and_water_as_water() {
        let of = |density: f32, viscosity: f32| voice(Medium { density, viscosity });
        // Water is played as it was recorded, to the last bit.
        assert_eq!(voice(Medium::WATER), (1.0, 1.0));
        // Brine is all but water; a light oil is a little thicker.
        let (brine, _) = of(1.2, 0.0015);
        assert!((0.9..1.0).contains(&brine), "{brine}");
        let (oil, oil_loud) = of(0.9, 0.08);
        assert!(
            (0.65..0.85).contains(&oil) && oil_loud < 0.8,
            "{oil} {oil_loud}"
        );
        // Honey and tar glug: about an octave down and half as loud.
        let (honey, honey_loud) = of(1.4, 6.0);
        assert!((0.45..0.6).contains(&honey), "{honey}");
        assert!((honey_loud - 0.5).abs() < 0.05, "{honey_loud}");
        // Mercury is as thin as water and thirteen times as heavy: lower,
        // and as loud.
        let (mercury, mercury_loud) = of(13.5, 0.0015);
        assert!((0.5..0.65).contains(&mercury), "{mercury}");
        assert!(mercury_loud > 0.95, "{mercury_loud}");
        // Never so low or so high as to be another sound, whatever is said.
        for (density, viscosity) in [(30.0, 1.0e6), (0.01, 0.0), (f32::NAN, f32::INFINITY)] {
            let (pitch, loud) = of(density, viscosity);
            assert!((0.4..=1.3).contains(&pitch) && (0.5..=1.0).contains(&loud));
        }
    }
}
