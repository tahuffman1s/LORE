//! The colour of the sky, worked out from the sun and the air.
//!
//! Sunlight is scattered on its way down: by the air itself, which sends
//! blue light every way and so makes the sky blue and the low sun red
//! (Rayleigh scattering), and by haze, which sends light of every colour
//! mostly onwards and so makes the glow round the sun (Mie scattering).
//!
//! The sun is so far off that the angle between it and a line of sight is
//! the same all along that line. How much light is scattered into the line
//! therefore splits into two parts: one that depends only on how high the
//! line looks, which is slow to work out and is done here, once for a
//! scene, for a few heights; and one that depends only on the angle to the
//! sun, which the shader works out for each pixel.

use crate::mesh::Lighting;

/// How much of the sky the clouds cover, from 0 to 1, and how high they
/// are, in metres. Fair-weather clouds; weather will set these (#130).
pub(crate) const CLOUD_COVER: f32 = 0.5;
pub(crate) const CLOUD_HEIGHT: f32 = 1500.0;

/// The fog: how much of what is behind one metre of it is hidden, at the
/// height it is thickest (0, where levels are built), and how fast it
/// thins with each metre above that. A light haze: things 300 metres off
/// are a fifth gone and the far hills fade into the sky. A game cannot
/// set it yet.
pub(crate) const FOG: [f32; 3] = [0.0008, 0.012, 0.0];

/// The wind: the way it blows along x and z (length one), and how fast,
/// in metres a second. A fresh breeze. Clouds drift along it, the water's
/// ripples run with it and whatever a level says bends in the wind bends
/// to it. A game cannot set it yet.
pub(crate) const WIND: [f32; 3] = [0.94, 0.342, 5.0];

/// How many heights above the horizon the sky is worked out for.
pub(crate) const HEIGHTS: usize = 24;

/// The sizes of the Earth and its air, in metres.
const GROUND: f64 = 6_360_000.0;
const TOP: f64 = 6_460_000.0;
/// How far up the air and the haze thin out to about a third.
const AIR_HEIGHT: f64 = 8_000.0;
const HAZE_HEIGHT: f64 = 1_200.0;
/// How much red, green and blue light a metre of air at the ground
/// scatters, and how much the ozone high up takes away.
const AIR: [f64; 3] = [5.802e-6, 13.558e-6, 33.1e-6];
const OZONE: [f64; 3] = [0.650e-6, 1.881e-6, 0.085e-6];
/// How much light of any colour a metre of haze at the ground scatters,
/// and how much it stops.
const HAZE: f64 = 3.996e-6;
const HAZE_STOPS: f64 = 4.4e-6;
/// How strongly haze sends light onwards and not aside, from 0 to 1.
pub(crate) const HAZE_ONWARDS: f32 = 0.8;
/// Where the eye is taken to be: a little above the sea.
const EYE: f64 = GROUND + 200.0;
/// How far round a point, in metres, the light scattered there once is
/// gathered from to be scattered again.
const AGAIN_WITHIN: f64 = 12_000.0;
const STEPS: usize = 40;
const SUN_STEPS: usize = 8;

/// The sky of one scene: for each of [`HEIGHTS`] heights above the horizon,
/// evenly spaced in the square root of the angle (so more of them are near
/// the horizon, where the sky changes fastest), the light the air scatters
/// into a line of sight and the light the haze does, before either is
/// multiplied by how much of it is sent towards the eye.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sky {
    pub(crate) air: [[f32; 3]; HEIGHTS],
    pub(crate) haze: [[f32; 3]; HEIGHTS],
}

/// How much of the air, the haze and the ozone there is at `height` metres
/// above the ground, as shares of what there is at the ground.
fn thickness(height: f64) -> [f64; 3] {
    let height = height.max(0.0);
    [
        (-height / AIR_HEIGHT).exp(),
        (-height / HAZE_HEIGHT).exp(),
        (1.0 - (height - 25_000.0).abs() / 15_000.0).max(0.0),
    ]
}

/// What a metre of all three takes out of red, green and blue light.
fn stopped([air, haze, ozone]: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|colour| AIR[colour] * air + HAZE_STOPS * haze + OZONE[colour] * ozone)
}

/// How far a line from `from` towards `direction` (length one) goes before
/// it leaves a ball of `radius` round the Earth's middle, if it is inside.
fn way_out(from: [f64; 2], direction: [f64; 2], radius: f64) -> Option<f64> {
    let along = from[0] * direction[0] + from[1] * direction[1];
    let inside = along * along - (from[0] * from[0] + from[1] * from[1] - radius * radius);
    (inside >= 0.0).then(|| -along + inside.sqrt())
}

/// How much of the sun's red, green and blue light reaches `from`, a point
/// in the air (how far sideways and up from the Earth's middle), when the
/// sun is found towards `sun` from there. Nothing, if the Earth is in the
/// way.
fn sunlight_at(from: [f64; 2], sun: [f64; 2]) -> [f64; 3] {
    let along = from[0] * sun[0] + from[1] * sun[1];
    let nearest = from[0] * from[0] + from[1] * from[1] - along * along;
    if along < 0.0 && nearest < GROUND * GROUND {
        return [0.0; 3];
    }
    let Some(far) = way_out(from, sun, TOP) else {
        return [1.0; 3];
    };
    let step = far / SUN_STEPS as f64;
    let mut depth = [0.0; 3];
    for number in 0..SUN_STEPS {
        let at = (number as f64 + 0.5) * step;
        let point = [from[0] + sun[0] * at, from[1] + sun[1] * at];
        let height = (point[0] * point[0] + point[1] * point[1]).sqrt() - GROUND;
        let lost = stopped(thickness(height));
        for colour in 0..3 {
            depth[colour] += lost[colour] * step;
        }
    }
    depth.map(|depth| (-depth).exp())
}

impl Sky {
    /// The sky under the sun of `lighting`, as bright as the light
    /// `lighting` says a surface gets from it.
    pub(crate) fn of(lighting: &Lighting) -> Self {
        // The sun's height is what matters; which way round it stands does
        // not change how thick the air is.
        let sun_height = f64::from(lighting.towards_sun[1]);
        let sun_height = if sun_height.is_finite() {
            sun_height.clamp(-1.0, 1.0)
        } else {
            1.0
        };
        let sun = [(1.0 - sun_height * sun_height).sqrt(), sun_height];
        let mut sky = Self {
            air: [[0.0; 3]; HEIGHTS],
            haze: [[0.0; 3]; HEIGHTS],
        };
        for number in 0..HEIGHTS {
            let angle = Self::angle_of(number);
            // The line of sight runs in a plane at right angles to the
            // sun's, which is as typical as any.
            let direction = [angle.cos(), angle.sin()];
            let from = [0.0, EYE];
            let far = way_out(from, direction, TOP).unwrap_or(0.0);
            let step = far / STEPS as f64;
            let mut depth = [0.0; 3];
            let (mut air, mut haze) = ([0.0; 3], [0.0; 3]);
            for at in 0..STEPS {
                let along = (at as f64 + 0.5) * step;
                // Sideways from the sun's plane: the height is what counts.
                let point = [direction[0] * along, EYE + direction[1] * along];
                let height = (point[0] * point[0] + point[1] * point[1]).sqrt() - GROUND;
                let thick = thickness(height);
                let lost = stopped(thick);
                // The sun as seen from this point of the line, which stands
                // straight over the eye near enough.
                let lit = sunlight_at([0.0, GROUND + height], sun);
                for colour in 0..3 {
                    // Half this step's own loss counts before the light
                    // scattered in its middle sets off.
                    let reaches = (-(depth[colour] + lost[colour] * step * 0.5)).exp();
                    air[colour] += reaches * lit[colour] * AIR[colour] * thick[0] * step;
                    haze[colour] += reaches * lit[colour] * HAZE * thick[1] * step;
                    // Light that was scattered more than once comes from
                    // all round, and there is more of it the more the air
                    // here scatters: so more of it is blue, and it is what
                    // keeps the horizon from turning green. It is counted
                    // with the air's, which is the nearer to even-handed
                    // of the two. (Each further bounce is the same share
                    // again, after Hillaire, "A Scalable and Production
                    // Ready Sky and Atmosphere Rendering Technique", 2020.)
                    let scatters = AIR[colour] * thick[0] + HAZE * thick[1];
                    let again = (scatters * AGAIN_WITHIN).min(0.6);
                    air[colour] += reaches * lit[colour] * scatters * step * again / (1.0 - again);
                    depth[colour] += lost[colour] * step;
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            {
                sky.air[number] = air.map(|part| part as f32);
                sky.haze[number] = haze.map(|part| part as f32);
            }
        }
        sky.matched_to(lighting)
    }

    /// The angle above the horizon of height `number`, in radians.
    fn angle_of(number: usize) -> f64 {
        let share = number as f64 / (HEIGHTS - 1) as f64;
        share * share * std::f64::consts::FRAC_PI_2
    }

    /// The same sky, made as bright as `lighting` says the sky is: so that
    /// what a matte white surface facing up gets from this sky is
    /// `lighting.sky`, as near as its colour allows. A level's light is in
    /// its author's own units, and the sky has to be in the same ones.
    fn matched_to(mut self, lighting: &Lighting) -> Self {
        let sun = lighting.towards_sun;
        let mut from_above = [0.0_f32; 3];
        let (rings, round) = (12, 24);
        for ring in 0..rings {
            #[allow(clippy::cast_precision_loss)]
            let angle = (ring as f32 + 0.5) / rings as f32 * std::f32::consts::FRAC_PI_2;
            let (up, out) = angle.sin_cos();
            for step in 0..round {
                #[allow(clippy::cast_precision_loss)]
                let turn = (step as f32 + 0.5) / round as f32 * std::f32::consts::TAU;
                let light = self.colour([out * turn.cos(), up, out * turn.sin()], sun);
                // Each patch counts by its size, which shrinks towards
                // straight up, and by how squarely it shines on the ground.
                for colour in 0..3 {
                    from_above[colour] += light[colour] * up * out;
                }
            }
        }
        // Each patch is this wide and this tall; a matte surface shows the
        // light on it divided by pi.
        #[allow(clippy::cast_precision_loss)]
        let patch = std::f32::consts::FRAC_PI_2 / rings as f32 * std::f32::consts::TAU
            / round as f32
            / std::f32::consts::PI;
        let brightness =
            |[red, green, blue]: [f32; 3]| 0.2126 * red + 0.7152 * green + 0.0722 * blue;
        let now = brightness(from_above) * patch;
        let wanted = brightness(lighting.sky);
        let scale = if now > 1e-12 && now.is_finite() {
            (wanted / now).min(1e9)
        } else {
            0.0
        };
        for row in self.air.iter_mut().chain(&mut self.haze) {
            *row = row.map(|part| part * scale);
        }
        self
    }

    /// The light that comes from `direction` (length one) when the sun
    /// stands towards `towards_sun`: what the shader's `sky_colour` draws.
    pub(crate) fn colour(&self, direction: [f32; 3], towards_sun: [f32; 3]) -> [f32; 3] {
        let angle = direction[1].clamp(0.0, 1.0).asin();
        #[allow(clippy::cast_precision_loss)]
        let place = (angle / std::f32::consts::FRAC_PI_2).sqrt() * (HEIGHTS - 1) as f32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let below = (place.floor() as usize).min(HEIGHTS - 2);
        #[allow(clippy::cast_precision_loss)]
        let share = (place - below as f32).clamp(0.0, 1.0);
        let to_sun: f32 = (0..3).map(|axis| direction[axis] * towards_sun[axis]).sum();
        let (by_air, by_haze) = (air_towards(to_sun), haze_towards(to_sun));
        [0, 1, 2].map(|colour| {
            let between = |rows: &[[f32; 3]; HEIGHTS]| {
                rows[below][colour] + (rows[below + 1][colour] - rows[below][colour]) * share
            };
            between(&self.air) * by_air + between(&self.haze) * by_haze
        })
    }
}

/// How much of the light the air scatters goes off at an angle whose cosine
/// is `to_sun` from the way it came.
fn air_towards(to_sun: f32) -> f32 {
    3.0 / (16.0 * std::f32::consts::PI) * (1.0 + to_sun * to_sun)
}

/// The same for haze (Henyey and Greenstein's shape).
fn haze_towards(to_sun: f32) -> f32 {
    let g = HAZE_ONWARDS;
    (1.0 - g * g) / (4.0 * std::f32::consts::PI * (1.0 + g * g - 2.0 * g * to_sun).powf(1.5))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit([x, y, z]: [f32; 3]) -> [f32; 3] {
        let length = (x * x + y * y + z * z).sqrt();
        [x / length, y / length, z / length]
    }

    fn under(towards_sun: [f32; 3]) -> (Sky, [f32; 3]) {
        let towards_sun = unit(towards_sun);
        let lighting = Lighting {
            towards_sun,
            ..Lighting::default()
        };
        (Sky::of(&lighting), towards_sun)
    }

    #[test]
    fn the_sky_is_blue_overhead_and_paler_at_the_horizon() {
        let (sky, sun) = under([0.3, 0.8, 0.5]);
        let [red, green, blue] = sky.colour([0.0, 1.0, 0.0], sun);
        assert!(blue > green && green > red, "{red} {green} {blue}");
        assert!(blue > 1.8 * red, "{red} {blue}");
        // Away from the sun, low down: brighter, and less blue for its
        // brightness.
        let low = sky.colour(unit([-0.3, 0.05, -0.5]), sun);
        assert!(low[2] > blue, "{low:?}");
        assert!(low[0] / low[2] > red / blue, "{low:?}");
    }

    #[test]
    fn a_low_sun_reddens_the_sky_round_it_and_glows() {
        let (sky, sun) = under([1.0, 0.07, 0.0]);
        let beside = sky.colour(unit([1.0, 0.07, 0.25]), sun);
        let opposite = sky.colour(unit([-1.0, 0.07, 0.25]), sun);
        assert!(beside[0] > beside[2], "red by the sun: {beside:?}");
        assert!(
            beside[0] > 1.5 * opposite[0],
            "{beside:?} against {opposite:?}"
        );
        let overhead = sky.colour([0.0, 1.0, 0.0], sun);
        assert!(overhead[2] > overhead[0], "still blue above: {overhead:?}");
    }

    #[test]
    fn the_sky_is_as_bright_as_the_light_it_is_said_to_give() {
        for sun in [[0.3, 0.8, 0.5], [1.0, 0.07, 0.0], [0.0, 1.0, 0.0]] {
            let (sky, sun) = under(sun);
            // Added up over every direction above, as `matched_to` does but
            // more finely.
            let mut total = [0.0_f32; 3];
            let (rings, round) = (60, 120);
            for ring in 0..rings {
                let angle = (ring as f32 + 0.5) / rings as f32 * std::f32::consts::FRAC_PI_2;
                for step in 0..round {
                    let turn = (step as f32 + 0.5) / round as f32 * std::f32::consts::TAU;
                    let (up, out) = angle.sin_cos();
                    let light = sky.colour([out * turn.cos(), up, out * turn.sin()], sun);
                    for colour in 0..3 {
                        total[colour] += light[colour] * up * out;
                    }
                }
            }
            let patch = std::f32::consts::FRAC_PI_2 / rings as f32 * std::f32::consts::TAU
                / round as f32
                / std::f32::consts::PI;
            let brightness = (0.2126 * total[0] + 0.7152 * total[1] + 0.0722 * total[2]) * patch;
            let [red, green, blue] = Lighting::default().sky;
            let wanted = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
            assert!(
                (brightness - wanted).abs() < 0.08 * wanted,
                "{brightness} against {wanted}"
            );
        }
    }

    #[test]
    fn a_sun_below_the_horizon_or_one_that_is_not_a_number_gives_no_nonsense() {
        for sun in [[0.2, -0.5, 0.1], [f32::NAN, 1.0, 0.0], [0.0, 0.0, 0.0]] {
            let lighting = Lighting {
                towards_sun: sun,
                ..Lighting::default()
            };
            let sky = Sky::of(&lighting);
            assert!(
                sky.air
                    .iter()
                    .chain(&sky.haze)
                    .flatten()
                    .all(|part| part.is_finite() && *part >= 0.0),
                "{sky:?}"
            );
        }
    }
}
