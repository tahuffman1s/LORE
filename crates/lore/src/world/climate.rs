//! The weather as the world's rules have it.
//!
//! A script says what the weather is (`game.weather`); what follows from it
//! is counted here, tick by tick, so that it is the same on every run: how
//! far the wind has carried the clouds, how deep the snow that has fallen
//! lies and how fast it melts, how thick the ice on still water has grown,
//! how far rain has raised that water, and where the tornado has got to.
//! The picture is drawn from the same numbers ([`Climate::weather`]), so
//! the storm the rules count is the storm that is seen.
//!
//! All of it runs far faster than weather does, as a game's day is far
//! shorter than a day: a lake freezes hard enough to walk on in a minute
//! of deep frost, and a blizzard buries the land in five.

use lore_physics::{Level, Player};
use lore_render::{Falling, Tornado, Weather};
use lore_script::{ScriptHost, TornadoAsk, WeatherHere, WeatherSaid};

/// How fast a downpour raises the still water, and how fast it falls back
/// when the rain stops, in metres a second; and the most it rises: a storm
/// fills a lake to its brim in a little over two minutes, and it is seven
/// minutes going down.
const WATER_RISES: f32 = 0.006;
const WATER_FALLS: f32 = 0.002;
const MOST_RISE: f32 = 0.8;

/// How fast snow that falls as hard as it can settles, in metres a second:
/// the land is white in half a minute and knee-deep in four. Then how fast
/// what lies melts for each degree above freezing, how fast rain takes it
/// away, and the deepest it gets.
const SNOW_SETTLES: f32 = 0.002;
const SNOW_MELTS: f32 = 0.0003;
const RAIN_MELTS: f32 = 0.002;
const MOST_SNOW: f32 = 0.6;

/// How fast ice grows on still water for each degree below freezing, and
/// how fast it thaws for each degree above, in metres a second; and the
/// thickest it gets. At five below it bears a walker in under a minute.
const ICE_GROWS: f32 = 0.0002;
const ICE_THAWS: f32 = 0.0004;
const MOST_ICE: f32 = 0.5;

/// How much of the push of the air on a body becomes speed, for each metre
/// a second of wind squared: half the weight of the air, times the half
/// square metre a person turns to the wind, over the eighty kilograms they
/// weigh.
const DRAG: f32 = 0.0052;

/// The wind below which nobody is moved by it, in metres a second, and
/// the wind at which all of its push is felt: a fresh gale.
const WIND_FELT: [f32; 2] = [14.0, 24.0];

/// How long a tornado takes to come down and to die away, in seconds, at
/// the most: a quarter of its life, if that is shorter.
const TORNADO_GROWS: f32 = 12.0;
const TORNADO_DIES: f32 = 10.0;

/// `value` between `low` and `high`, as a share from 0 to 1 that starts
/// and ends gently.
fn eased(low: f32, high: f32, value: f32) -> f32 {
    let share = ((value - low) / (high - low)).clamp(0.0, 1.0);
    share * share * (3.0 - 2.0 * share)
}

/// The weather a script has said, as the picture and the rules take it,
/// before anything has followed from it: no snow lies and no wind has
/// carried the clouds anywhere.
pub(crate) fn said(said: &WeatherSaid) -> Weather {
    Weather {
        cloud_cover: said.cloud_cover,
        storminess: said.storminess,
        precipitation: said.rain,
        temperature: said.temperature,
        wind: said.wind,
        gusts: said.gusts,
        fog: said.fog,
        ..Weather::default()
    }
    .cleaned()
}

/// A tornado, from the tick it was asked for to the tick it is gone.
#[derive(Debug, Clone, Copy)]
struct Twister {
    /// Where its foot is.
    place: [f32; 3],
    /// The way it set off along x and z, and how fast it travels.
    heading: [f32; 2],
    speed: f32,
    /// How strong it gets.
    peak: f32,
    /// How long it has been there, and how long it lasts, in seconds.
    age: f32,
    lasts: f32,
    /// How far round it has turned, in radians.
    turned: f32,
}

impl Twister {
    /// How strong it is now: it comes down, holds, and dies away.
    fn strength(&self) -> f32 {
        let grows = TORNADO_GROWS.min(self.lasts * 0.25);
        let dies = TORNADO_DIES.min(self.lasts * 0.25);
        self.peak
            * eased(0.0, grows, self.age)
            * (1.0 - eased(self.lasts - dies, self.lasts, self.age))
    }

    fn seen(&self) -> Tornado {
        Tornado {
            place: self.place,
            strength: self.strength(),
            turned: self.turned,
        }
    }

    /// Moves it on by `step` seconds over `level`: along its heading,
    /// which wanders a little either way as real ones do, and on whatever
    /// ground is under it.
    fn step(&mut self, level: &Level, step: f32) {
        self.age += step;
        let wanders = 0.3 * libm::sinf(self.age * 0.21) + 0.14 * libm::sinf(self.age * 0.53 + 1.3);
        let (sin, cos) = (libm::sinf(wanders), libm::cosf(wanders));
        let [along_x, along_z] = self.heading;
        let way = [along_x * cos - along_z * sin, along_x * sin + along_z * cos];
        self.place[0] += way[0] * self.speed * step;
        self.place[2] += way[1] * self.speed * step;
        // The ground under it, if the level reaches so far.
        let [x, y, z] = self.place;
        if let Some(hit) = level.raycast([x, y + 150.0, z], [0.0, -1.0, 0.0], 600.0) {
            self.place[1] = hit.point[1];
        }
        self.turned += self.seen().turns() * step;
    }
}

/// The weather of one world.
#[derive(Debug)]
pub(crate) struct Climate {
    /// How far the wind has carried the air since the world began, along
    /// x and z, in metres.
    drift: [f64; 2],
    /// How deep the snow lies and how thick the ice on still water is, in
    /// metres.
    snow: f32,
    ice: f32,
    /// How far the rain that has fallen has raised the still water, in
    /// metres.
    rise: f32,
    twister: Option<Twister>,
    /// The weather of the newest tick, and what was falling on the player
    /// in it.
    weather: Weather,
    falling: Falling,
}

impl Default for Climate {
    fn default() -> Self {
        Self {
            drift: [0.0; 2],
            snow: 0.0,
            ice: 0.0,
            rise: 0.0,
            twister: None,
            weather: Weather {
                drift: Some([0.0; 2]),
                ..Weather::default()
            },
            falling: Falling::default(),
        }
    }
}

impl Climate {
    /// The weather of the newest tick: what the picture is drawn in.
    pub(crate) fn weather(&self) -> &Weather {
        &self.weather
    }

    /// How far the rain that has fallen has raised the level's still
    /// water, in metres.
    pub(crate) fn water_rise(&self) -> f32 {
        self.rise
    }

    /// How deep the snow lies, in metres.
    #[cfg(test)]
    pub(crate) fn snow(&self) -> f32 {
        self.snow
    }

    /// Moves the weather on by one tick of `step` seconds, `seconds` after
    /// the world began: what the script has said becomes the weather, and
    /// what follows from it at `eye`, where the player is, is counted. The
    /// level is told how high its water stands and how thick its ice is,
    /// and the script what the weather has left.
    pub(crate) fn step(
        &mut self,
        host: &mut ScriptHost,
        level: &Level,
        eye: [f32; 3],
        seconds: f64,
        step: f32,
    ) {
        let asked = host.weather();
        for (far, wind) in self.drift.iter_mut().zip(asked.wind) {
            *far += f64::from(wind) * f64::from(step);
        }
        for ask in host.take_tornado() {
            match ask {
                TornadoAsk::Start {
                    at,
                    strength,
                    heading,
                    speed,
                    seconds,
                } => {
                    let wind = said(&asked).wind_way();
                    self.twister = Some(Twister {
                        place: at,
                        heading: heading.unwrap_or(wind),
                        speed,
                        peak: strength,
                        age: 0.0,
                        lasts: seconds,
                        turned: 0.0,
                    });
                }
                TornadoAsk::Stop => {
                    if let Some(twister) = &mut self.twister {
                        // It dies away from where it has got to, and no
                        // stronger than it now is.
                        let dies = TORNADO_DIES.min(twister.lasts * 0.25);
                        twister.peak = twister.strength();
                        twister.lasts = twister.lasts.min(twister.age + dies);
                        twister.age = twister.age.max(twister.lasts - dies);
                    }
                }
            }
        }
        if let Some(twister) = &mut self.twister {
            twister.step(level, step);
            if twister.age >= twister.lasts {
                self.twister = None;
            }
        }
        if let Some(depth) = host.take_snow_depth() {
            self.snow = depth;
        }
        if let Some(thick) = host.take_ice() {
            self.ice = thick;
        }

        let mut weather = Weather {
            drift: Some(self.drift),
            tornado: self.twister.as_ref().map(Twister::seen),
            ..said(&asked)
        };
        let falling = weather.falling_at([eye[0], eye[2]], seconds);
        // Snow settles while it falls; warmth and rain take it away.
        let warm = weather.temperature;
        let melts = warm.max(0.0) * SNOW_MELTS + falling.rain * RAIN_MELTS;
        self.snow =
            (self.snow + step * (falling.snow * SNOW_SETTLES - melts)).clamp(0.0, MOST_SNOW);
        // Still water freezes in the cold and thaws in the warm.
        let freezes = (-warm).max(0.0) * ICE_GROWS - warm.max(0.0) * ICE_THAWS;
        self.ice = (self.ice + step * freezes).clamp(0.0, MOST_ICE);
        // Rain raises it, and it falls again after.
        let rain = falling.rain;
        self.rise = (self.rise + step * (rain * WATER_RISES - (1.0 - rain) * WATER_FALLS))
            .clamp(0.0, MOST_RISE);

        weather.snow = self.snow;
        weather.ice = self.ice;
        self.weather = weather.cleaned();
        self.falling = falling;
        // Said to the level before anything is moved in it: its shapes
        // are shared, and each world says its own.
        level.set_water_rise(self.rise);
        level.set_ice(self.ice);
        host.set_weather_here(WeatherHere {
            snow_depth: self.snow,
            ice: self.ice,
            rain: falling.rain,
            sleet: falling.sleet,
            snow: falling.snow,
            hail: falling.hail,
            tornado: self.twister.as_ref().map(|twister| twister.place),
        });
    }

    /// What the air does to `player` in one tick of `step` seconds,
    /// `seconds` after the world began, under `gravity` metres a second
    /// each second: a gale pushes them, harder by the square of how fast
    /// it blows past them, and what rises faster than they fall lifts them
    /// off their feet. Below a gale nobody is moved: feet hold.
    pub(crate) fn blow(&self, player: &mut Player, gravity: f32, seconds: f64, step: f32) {
        let [x, y, z] = player.position;
        // The wind at their middle.
        let air = self.weather.wind_at([x, y + 1.0, z], seconds);
        let past = [0, 1, 2].map(|axis| air[axis] - player.velocity[axis]);
        let speed = past.iter().map(|part| part * part).sum::<f32>().sqrt();
        let felt = eased(WIND_FELT[0], WIND_FELT[1], speed);
        if !(speed.is_finite() && felt > 0.0) || player.swimming {
            return;
        }
        // No harder than six times their own weight, whatever blows.
        let push = (DRAG * speed * felt).min(60.0 / speed.max(1.0));
        player.velocity[0] += past[0] * push * step;
        player.velocity[2] += past[2] * push * step;
        let lift = past[1] * push;
        if !player.grounded {
            player.velocity[1] += lift * step;
        } else if lift > gravity {
            // It takes them off their feet.
            player.grounded = false;
            player.velocity[1] += lift * step;
        }
    }

    /// How much of their speed deep snow leaves whoever walks through it,
    /// from 1 on bare ground to under a half in snow to the knee.
    pub(crate) fn snow_slows(&self) -> f32 {
        1.0 - 0.55 * eased(0.08, 0.5, self.snow)
    }
}

#[cfg(test)]
mod tests {
    use lore_input::{ActionMap, InputFrame};
    use lore_physics::MoveSettings;

    use super::*;

    const STEP: f32 = 1.0 / 60.0;

    /// A world's weather under `script`, run for `ticks` on a level with
    /// nothing in it, the player at the middle of it.
    fn after(script: &str, ticks: u32) -> (Climate, ScriptHost) {
        let mut host = ScriptHost::from_source(script, ActionMap::default_fps());
        let mut climate = Climate::default();
        let level = Level::from_triangles(&[]);
        for tick in 0..ticks {
            host.tick(&InputFrame::default(), STEP);
            climate.step(
                &mut host,
                &level,
                [0.0; 3],
                f64::from(tick) * f64::from(STEP),
                STEP,
            );
        }
        (climate, host)
    }

    #[test]
    fn snow_settles_in_the_cold_and_melts_in_the_warm() {
        // Half a minute of heavy snow whitens the land.
        // The script reads what falls and what lies: it says so here by
        // letting the fog in, which the test can see.
        let (climate, host) = after(
            r#"game.weather.set("blizzard", 0)
            game.weather.setFog(0)
            game.onTick(function()
                local what, hard = game.weather.falling()
                if what == "snow" and hard > 0.9 and game.weather.snowDepth() > 0.04 then
                    game.weather.setFog(1)
                end
            end)"#,
            1800,
        );
        assert!((0.05..0.07).contains(&climate.snow()), "{}", climate.snow());
        assert!(climate.weather().snow_cover() > 0.9);
        assert_eq!(host.weather().fog, 1.0);
        assert!(climate.snow_slows() > 0.99, "a dusting slows nobody");
        // Four minutes of it and it is deep, and slows whoever walks.
        let (deep, _) = after(r#"game.weather.set("blizzard", 0)"#, 14_400);
        assert!((0.45..0.5).contains(&deep.snow()), "{}", deep.snow());
        assert!(deep.snow_slows() < 0.5, "{}", deep.snow_slows());
        // It lies while it is cold, and goes when it is warm: faster in rain.
        let lies = r"game.weather.setSnowDepth(0.3)
            game.weather.setTemperature(-2)";
        assert!((after(lies, 3600).0.snow() - 0.3).abs() < 1e-4);
        let thaw = r"game.weather.setSnowDepth(0.3)
            game.weather.setTemperature(10)";
        let left = after(thaw, 1800).0.snow();
        assert!((0.2..0.22).contains(&left), "{left}");
        let rained = r"game.weather.setSnowDepth(0.3)
            game.weather.setTemperature(10)
            game.weather.setRain(1)";
        let left = after(rained, 1800).0.snow();
        assert!((0.14..0.16).contains(&left), "{left}");
        assert_eq!(after(thaw, 7200).0.snow(), 0.0);
    }

    #[test]
    fn still_water_freezes_in_the_cold_until_it_bears_and_thaws_again() {
        let frost = "game.weather.setTemperature(-5)";
        // Half a minute at five below: ice, but not to walk on.
        let (thin, _) = after(frost, 1800);
        assert!(thin.weather().ice > 0.02 && thin.weather().ice < Level::ICE_BEARS);
        // A minute of it bears a walker, and the level is told so.
        let mut host = ScriptHost::from_source(frost, ActionMap::default_fps());
        let mut climate = Climate::default();
        let level = Level::from_triangles(&[]);
        for _ in 0..3600 {
            host.tick(&InputFrame::default(), STEP);
            climate.step(&mut host, &level, [0.0; 3], 0.0, STEP);
        }
        assert!(level.frozen(), "{}", level.ice());
        assert!((level.ice() - 0.06).abs() < 2e-3, "{}", level.ice());
        // A script can freeze it at once, and reads back how thick it is;
        // in the warm it thaws.
        let (kept, host) = after(
            r"game.weather.setIce(0.2)
            if math.abs(game.weather.ice() - 0.2) < 0.001 then
                game.weather.setTemperature(-1)
            end",
            60,
        );
        assert!(kept.weather().ice > 0.2);
        assert_eq!(host.weather().temperature, -1.0);
        let warm = r"game.weather.setIce(0.2)
            game.weather.setTemperature(10)";
        assert_eq!(after(warm, 3600).0.weather().ice, 0.0);
    }

    #[test]
    fn rain_raises_the_water_and_it_falls_again_and_snow_does_not() {
        let (rained, _) = after(r"game.weather.setRain(1)", 3600);
        assert!(
            (rained.water_rise() - 0.36).abs() < 0.01,
            "{}",
            rained.water_rise()
        );
        let (snowed, _) = after(
            r"game.weather.setRain(1)
            game.weather.setTemperature(-5)",
            3600,
        );
        assert_eq!(snowed.water_rise(), 0.0);
        assert_eq!(after("", 600).0.water_rise(), 0.0);
        // Three minutes of downpour fill it to its brim, and eight dry
        // ones after bring it back to where it was put.
        let filled = r"game.weather.setRain(1)
            game.after(180, function() game.weather.setRain(0) end)";
        assert_eq!(after(filled, 180 * 60).0.water_rise(), MOST_RISE);
        assert_eq!(after(filled, 660 * 60).0.water_rise(), 0.0);
    }

    #[test]
    fn the_wind_carries_the_clouds_as_far_as_it_has_blown() {
        // Ten seconds of a wind that turns half way through: 5 s at
        // 10 m/s along x, then 5 s at 10 m/s along z.
        let (climate, _) = after(
            r"game.weather.setWind(vector.create(10, 0, 0))
            game.after(5, function() game.weather.setWind(vector.create(0, 0, 10)) end)",
            600,
        );
        let [along_x, along_z] = climate.weather().drift.unwrap();
        assert!(
            (along_x - 50.0).abs() < 0.5 && (along_z - 50.0).abs() < 0.5,
            "{along_x} {along_z}"
        );
        assert_eq!(climate.weather().wind, [0.0, 10.0]);
    }

    #[test]
    fn a_tornado_comes_down_travels_and_dies_away() {
        let script = r"game.weather.startTornado(vector.create(100, 0, 0), {
                strength = 0.8, heading = vector.create(1, 0, 0), speed = 10, seconds = 60,
            })
            game.onTick(function()
                local at = game.weather.tornado()
                if at and at.x > 350 and at.x < 450 then
                    game.weather.setFog(1)
                end
            end)";
        // Just begun, it is weak; half way through it is at its strongest
        // and has travelled 300 metres, wandering a little off its line.
        let (young, _) = after(script, 60);
        let tornado = young.weather().tornado.unwrap();
        assert!(
            tornado.strength > 0.0 && tornado.strength < 0.1,
            "{tornado:?}"
        );
        let (grown, host) = after(script, 1830);
        let tornado = grown.weather().tornado.unwrap();
        assert!((tornado.strength - 0.8).abs() < 1e-3, "{tornado:?}");
        assert!((tornado.place[0] - 400.0).abs() < 40.0, "{tornado:?}");
        assert!(
            tornado.place[2].abs() < 80.0 && tornado.turned > 30.0,
            "{tornado:?}"
        );
        assert_eq!(host.weather().fog, 1.0, "the script is told where it is");
        // It brings a storm with it.
        let [x, _, z] = tornado.place;
        assert!(grown.weather().storm_at([x, z], 30.0) > 0.99);
        // A minute on it is gone.
        let (gone, _) = after(script, 3660);
        assert!(gone.weather().tornado.is_none());
        // Told to stop, it dies away in ten seconds from as strong as it was.
        let stopped = r"game.weather.startTornado(vector.create(100, 0, 0), { seconds = 600 })
            game.after(30, function() game.weather.stopTornado() end)";
        let (dying, _) = after(stopped, 2100);
        let tornado = dying.weather().tornado.unwrap();
        assert!(
            tornado.strength > 0.1 && tornado.strength < 0.4,
            "{tornado:?}"
        );
        assert!(after(stopped, 2460).0.weather().tornado.is_none());
    }

    #[test]
    fn a_gale_pushes_whoever_stands_in_it_and_a_tornado_lifts_them() {
        let rules = MoveSettings::default();
        // A breeze moves nobody.
        let (breeze, _) = after("", 2);
        let mut player = Player::at([0.0; 3], &rules);
        player.grounded = true;
        breeze.blow(&mut player, 9.81, 0.0, STEP);
        assert_eq!(player.velocity, [0.0; 3]);
        // A wind of thirty metres a second pushes them along with it.
        let (gale, _) = after("game.weather.setWind(vector.create(30, 0, 0))", 2);
        gale.blow(&mut player, 9.81, 0.0, STEP);
        let pushed = player.velocity[0] / STEP;
        assert!((4.0..5.5).contains(&pushed), "{pushed} m/s each second");
        assert!(player.grounded && player.velocity[1] == 0.0);
        // Whoever goes as fast as the wind is pushed no more.
        player.velocity = [30.0, 0.0, 0.0];
        gale.blow(&mut player, 9.81, 0.0, STEP);
        assert_eq!(player.velocity, [30.0, 0.0, 0.0]);
        // At the wall of a strong tornado they are taken off their feet.
        let (twister, _) = after(
            r"game.weather.setWind(vector.create(0, 0, 0))
            game.weather.startTornado(vector.create(0, 0, 0), { strength = 1, speed = 0, seconds = 600 })",
            1800,
        );
        let core = twister.weather().tornado.unwrap().core();
        let mut player = Player::at([core, 0.0, 0.0], &rules);
        player.grounded = true;
        twister.blow(&mut player, 9.81, 0.0, STEP);
        assert!(!player.grounded && player.velocity[1] > 0.0, "{player:?}");
        // Round it, and in towards it.
        assert!(
            player.velocity[2] < 0.0 && player.velocity[0] < 0.0,
            "{player:?}"
        );
    }
}
