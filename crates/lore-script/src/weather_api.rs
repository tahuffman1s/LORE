//! The weather and the hour for scripts: `game.weather` and `game.sky`.
//!
//! A script says what the weather is, in one word (`game.weather.set`) or
//! a number at a time: how much cloud, how stormy, how hard it rains, how
//! warm the air is, the wind, the fog. Each can be said to change over
//! some seconds, and then moves there tick by tick, which is counted here
//! so that it is the same on every run. The engine reads what the weather
//! now is ([`WeatherSaid`]), lets the world follow from it, and says back
//! what that left at the player ([`WeatherHere`]): the snow that lies, the
//! ice on the water, what is falling and where the tornado is.
//!
//! The hour is the script's too: it says it, and may say how long a day
//! lasts, after which the hours pass by themselves.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use lore_sky::{Heavens, SkyPlace};
use mlua::{Function, Lua, Table, Value, Variadic, Vector};

use crate::api::{Handlers, Shared, describe, fail, strict};
use crate::time_api::rules_only;

/// Every function on `game.sky`.
pub(crate) const SKY_FUNCTIONS: &[&str] = &[
    "setHour",
    "hour",
    "setDayLength",
    "dayLength",
    "setLatitude",
    "latitude",
    "setNorth",
    "north",
    "setDate",
    "date",
    "sunDirection",
    "sunHeight",
    "moonDirection",
    "moonHeight",
    "moonPhase",
    "moonLit",
    "starDirection",
    "starHeight",
    "starNear",
    "starNames",
    "light",
    "setMoonSize",
    "moonSize",
    "setMoonColor",
    "moonColor",
    "setMilkyWayBrightness",
    "milkyWayBrightness",
    "setMilkyWayColor",
    "milkyWayColor",
    "setLightPollution",
    "lightPollution",
    "planetDirection",
    "planetHeight",
    "planetBrightness",
    "planetNames",
];

/// The words `game.sky.setLightPollution` takes, and the class on Bortle's
/// scale each stands for.
const LIGHT_POLLUTION_WORDS: [(&str, f32); 4] = [
    ("none", 1.0),
    ("village", 3.0),
    ("town", 5.0),
    ("city", 8.0),
];

/// A colour for the sky given to `function`, as the HUD takes colours: in
/// linear light, and as a hex code to give back to the script.
fn sky_color(lua: &Lua, function: &str, value: &Value) -> mlua::Result<([f32; 3], String)> {
    let (color, _) = crate::hud::color_of(lua, function, value)?;
    let linear = [color.r, color.g, color.b].map(|part| {
        if part <= 0.040_45 {
            part / 12.92
        } else {
            ((part + 0.055) / 1.055).powf(2.4)
        }
    });
    let byte = |part: f32| {
        // Rounded and brought into 0 to 255 first.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = (part.clamp(0.0, 1.0) * 255.0).round() as u8;
        byte
    };
    let said = format!(
        "#{:02x}{:02x}{:02x}",
        byte(color.r),
        byte(color.g),
        byte(color.b)
    );
    Ok((linear, said))
}

/// The real sky a game gets when it says only some of where and when:
/// 45 degrees north, north the level's -z, and 20 March 2000, an equinox
/// with a full moon.
fn usual_place() -> SkyPlace {
    SkyPlace {
        latitude: 45.0,
        north: 0.0,
        first_day: lore_sky::days_from_2000(2000, 3, 20),
    }
}

/// Every function on `game.weather`.
pub(crate) const WEATHER_FUNCTIONS: &[&str] = &[
    "set",
    "setCloudCover",
    "cloudCover",
    "setStorminess",
    "storminess",
    "setRain",
    "rain",
    "setTemperature",
    "temperature",
    "setWind",
    "wind",
    "setGusts",
    "gusts",
    "setFog",
    "fog",
    "setSnowDepth",
    "snowDepth",
    "setIce",
    "ice",
    "falling",
    "strike",
    "onLightning",
    "startTornado",
    "tornado",
    "stopTornado",
];

/// The kinds of weather `game.weather.set` knows by name.
pub(crate) const WEATHERS: &[&str] = &[
    "clear", "fair", "cloudy", "overcast", "fog", "drizzle", "rain", "storm", "sleet", "snow",
    "blizzard",
];

/// How long `game.weather.set` takes to bring the weather in when the
/// script does not say, in seconds.
const USUAL_CHANGE: f32 = 20.0;

/// The most strikes of lightning kept between two ticks: no more could be
/// seen.
const MOST_STRIKES: usize = 8;

/// The weather as a script has said it and as it now is, on the way to
/// what was last asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherSaid {
    /// How much of the sky is cloud, from 0 to 1.
    pub cloud_cover: f32,
    /// How stormy the sky is, from 0 to 1.
    pub storminess: f32,
    /// How hard it rains (or, in the cold, snows) from a sky that is all
    /// cloud, from 0 to 1.
    pub rain: f32,
    /// How warm the air is, in degrees Celsius.
    pub temperature: f32,
    /// The wind: how fast the air moves along x and z, in metres a second.
    pub wind: [f32; 2],
    /// How gusty the wind is, from 0 to 1.
    pub gusts: f32,
    /// How thick the fog is, from 0 to 1.
    pub fog: f32,
}

impl Default for WeatherSaid {
    /// What a level has until a script says otherwise: a fair, mild day
    /// with a fresh breeze.
    fn default() -> Self {
        Self {
            cloud_cover: 0.5,
            storminess: 0.0,
            rain: 0.0,
            temperature: 20.0,
            wind: [4.7, 1.71],
            gusts: 0.5,
            fog: 0.0,
        }
    }
}

/// What the weather has left where the player is, as the engine says it
/// back to the script.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WeatherHere {
    /// How deep the snow lies, in metres.
    pub snow_depth: f32,
    /// How thick the ice on still water is, in metres.
    pub ice: f32,
    /// How hard rain, sleet, snow and hail are falling, each from 0 to 1.
    pub rain: f32,
    /// See [`rain`](Self::rain).
    pub sleet: f32,
    /// See [`rain`](Self::rain).
    pub snow: f32,
    /// See [`rain`](Self::rain).
    pub hail: f32,
    /// Where the foot of the tornado is, if there is one.
    pub tornado: Option<[f32; 3]>,
}

/// What a script asked of the tornado.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TornadoAsk {
    /// Let one come down.
    Start {
        /// Where its foot is to be, in metres.
        at: [f32; 3],
        /// How strong it gets, from 0 to 1.
        strength: f32,
        /// The way it travels along x and z, if the script said: with
        /// none it goes with the wind.
        heading: Option<[f32; 2]>,
        /// How fast it travels, in metres a second.
        speed: f32,
        /// How long it lasts, in seconds.
        seconds: f32,
    },
    /// Let the one there is die away.
    Stop,
}

/// One number of the weather: what it is now, what it is on its way to
/// and how fast it goes there.
#[derive(Debug)]
struct Eased {
    now: Cell<f32>,
    target: Cell<f32>,
    /// How far it moves in a second.
    rate: Cell<f32>,
}

impl Eased {
    fn at(value: f32) -> Self {
        Self {
            now: Cell::new(value),
            target: Cell::new(value),
            rate: Cell::new(0.0),
        }
    }

    /// Sends it to `value` over `seconds`; at once if that is no time.
    fn set(&self, value: f32, seconds: f32) {
        self.target.set(value);
        if seconds > 0.0 {
            self.rate.set((value - self.now.get()).abs() / seconds);
        } else {
            self.now.set(value);
        }
    }

    /// Moves it on by `step` seconds.
    fn step(&self, step: f32) {
        let (now, target) = (self.now.get(), self.target.get());
        let reach = self.rate.get() * step;
        self.now
            .set(now + (target - now).clamp(-reach.abs(), reach.abs()));
    }

    fn now(&self) -> f32 {
        self.now.get()
    }
}

/// What the script has said of the weather and the hour, and what the
/// engine has said back. It outlives a version of the script: the sky
/// stays as it was said to be when the script is read again.
#[derive(Debug)]
pub struct Climate {
    cloud_cover: Eased,
    storminess: Eased,
    rain: Eased,
    temperature: Eased,
    wind: [Eased; 2],
    gusts: Eased,
    fog: Eased,
    /// Where lightning was asked to strike and the engine has not yet
    /// taken, each with the number of the strike it is.
    strikes: RefCell<Vec<([f32; 3], u32)>>,
    /// How many strikes have been asked for.
    struck: Cell<u32>,
    /// The hour of the day, if the script has said one, and whether it
    /// has changed since the engine last took it.
    hour: Cell<Option<f64>>,
    hour_changed: Cell<bool>,
    /// How many minutes a day lasts: 0 if the hours do not pass by
    /// themselves.
    day_minutes: Cell<f64>,
    /// Where on the Earth the world lies, which way north is and on what
    /// day hour 0 was midnight, if the script has said any of it, and
    /// whether that has changed since the engine last took it.
    place: Cell<Option<SkyPlace>>,
    place_changed: Cell<bool>,
    /// How large the moon is drawn against its real size, if the script
    /// has said and the engine has not yet taken it.
    moon_size: Cell<f32>,
    moon_size_asked: Cell<Option<f32>>,
    /// What colour the moon is tinted, in linear light, as the script
    /// wrote it, and whether the engine has yet to take it.
    moon_tint: Cell<[f32; 3]>,
    moon_tint_said: RefCell<String>,
    moon_tint_asked: Cell<bool>,
    /// How many times its real brightness the Milky Way is drawn and what
    /// colour it is tinted (linear light, and as the script wrote it), and
    /// whether the engine has yet to take them.
    milky_way_brightness: Cell<f32>,
    milky_way_tint: Cell<[f32; 3]>,
    milky_way_tint_said: RefCell<String>,
    milky_way_asked: Cell<bool>,
    /// How much towns light the night sky, on Bortle's scale, and whether
    /// the engine has yet to take it.
    light_pollution: Cell<f32>,
    light_pollution_asked: Cell<bool>,
    /// The depth of snow and the thickness of ice the script asked for
    /// and the engine has not yet taken.
    snow_asked: Cell<Option<f32>>,
    ice_asked: Cell<Option<f32>>,
    /// What the script asked of the tornado and the engine has not yet
    /// taken, oldest first.
    tornado_asked: RefCell<Vec<TornadoAsk>>,
    /// What the engine last said the weather has left at the player.
    here: Cell<WeatherHere>,
}

impl Default for Climate {
    fn default() -> Self {
        let usual = WeatherSaid::default();
        Self {
            cloud_cover: Eased::at(usual.cloud_cover),
            storminess: Eased::at(usual.storminess),
            rain: Eased::at(usual.rain),
            temperature: Eased::at(usual.temperature),
            wind: usual.wind.map(Eased::at),
            gusts: Eased::at(usual.gusts),
            fog: Eased::at(usual.fog),
            strikes: RefCell::default(),
            struck: Cell::new(0),
            hour: Cell::new(None),
            hour_changed: Cell::new(false),
            place: Cell::new(None),
            place_changed: Cell::new(false),
            moon_size: Cell::new(1.0),
            moon_size_asked: Cell::new(None),
            moon_tint: Cell::new([1.0; 3]),
            moon_tint_said: RefCell::new("#ffffff".to_owned()),
            moon_tint_asked: Cell::new(false),
            milky_way_brightness: Cell::new(1.0),
            milky_way_tint: Cell::new([1.0; 3]),
            milky_way_tint_said: RefCell::new("#ffffff".to_owned()),
            milky_way_asked: Cell::new(false),
            light_pollution: Cell::new(1.0),
            light_pollution_asked: Cell::new(false),
            day_minutes: Cell::new(0.0),
            snow_asked: Cell::new(None),
            ice_asked: Cell::new(None),
            tornado_asked: RefCell::default(),
            here: Cell::default(),
        }
    }
}

impl Climate {
    /// Moves the weather on by one tick of `step` seconds towards what was
    /// asked for, and the hour with it if the hours pass.
    pub(crate) fn step(&self, step: f32) {
        if !(step.is_finite() && step > 0.0) {
            return;
        }
        for eased in [
            &self.cloud_cover,
            &self.storminess,
            &self.rain,
            &self.temperature,
            &self.wind[0],
            &self.wind[1],
            &self.gusts,
            &self.fog,
        ] {
            eased.step(step);
        }
        let minutes = self.day_minutes.get();
        if minutes > 0.0
            && let Some(hour) = self.hour.get()
        {
            self.hour
                .set(Some(hour + f64::from(step) * 24.0 / (minutes * 60.0)));
            self.hour_changed.set(true);
        }
    }

    /// The weather as it now is.
    pub(crate) fn said(&self) -> WeatherSaid {
        WeatherSaid {
            cloud_cover: self.cloud_cover.now(),
            storminess: self.storminess.now(),
            rain: self.rain.now(),
            temperature: self.temperature.now(),
            wind: [self.wind[0].now(), self.wind[1].now()],
            gusts: self.gusts.now(),
            fog: self.fog.now(),
        }
    }

    /// The hour of the day, if the script has said one.
    pub(crate) fn hour(&self) -> Option<f64> {
        self.hour.get()
    }

    /// Where the world lies and when its clock started, if it has changed
    /// since this was last asked.
    pub(crate) fn take_moon_tint(&self) -> Option<[f32; 3]> {
        self.moon_tint_asked.take().then(|| self.moon_tint.get())
    }

    pub(crate) fn take_moon_size(&self) -> Option<f32> {
        self.moon_size_asked.take()
    }

    /// How bright and what colour the Milky Way is drawn, if the script has
    /// said either since this was last asked.
    pub(crate) fn take_milky_way(&self) -> Option<(f32, [f32; 3])> {
        self.milky_way_asked
            .take()
            .then(|| (self.milky_way_brightness.get(), self.milky_way_tint.get()))
    }

    /// How much towns light the night sky, on Bortle's scale, if the script
    /// has said since this was last asked.
    pub(crate) fn take_light_pollution(&self) -> Option<f32> {
        self.light_pollution_asked
            .take()
            .then(|| self.light_pollution.get())
    }

    pub(crate) fn take_sky_place(&self) -> Option<SkyPlace> {
        self.place_changed
            .take()
            .then(|| self.place.get())
            .flatten()
    }

    /// Where the sun, the moon and the stars are now, if the script has
    /// said the hour: as the renderer draws them.
    fn heavens(&self) -> Option<Heavens> {
        let hour = self.hour.get()?;
        Some(Heavens::at(hour, self.place.get().as_ref()))
    }

    /// Changes the place the script has said, starting from the usual one.
    fn change_place(&self, change: impl FnOnce(&mut SkyPlace)) {
        let mut place = self.place.get().unwrap_or_else(usual_place);
        change(&mut place);
        self.place.set(Some(place));
        self.place_changed.set(true);
    }

    /// The hour of the day, if it has changed since this was last asked.
    pub(crate) fn take_hour(&self) -> Option<f64> {
        self.hour_changed.take().then(|| self.hour.get()).flatten()
    }

    pub(crate) fn take_strikes(&self) -> Vec<([f32; 3], u32)> {
        std::mem::take(&mut *self.strikes.borrow_mut())
    }

    pub(crate) fn take_snow_depth(&self) -> Option<f32> {
        self.snow_asked.take()
    }

    pub(crate) fn take_ice(&self) -> Option<f32> {
        self.ice_asked.take()
    }

    pub(crate) fn take_tornado(&self) -> Vec<TornadoAsk> {
        std::mem::take(&mut *self.tornado_asked.borrow_mut())
    }

    pub(crate) fn set_here(&self, here: WeatherHere) {
        self.here.set(here);
    }

    /// Brings in the kind of weather called `name` over `seconds`. What a
    /// kind does not care about is left as it is: fog keeps the day's
    /// warmth, rain keeps the wind's direction.
    fn bring(&self, name: &str, seconds: f32) -> bool {
        // Cloud, storminess, rain, fog, the wind's speed, and the least
        // and most the air's warmth can be for it.
        let (cloud, storm, rain, fog, wind, coldest, warmest) = match name {
            "clear" => (0.05, 0.0, 0.0, 0.0, 3.0, -60.0, 60.0),
            "fair" => (0.5, 0.0, 0.0, 0.0, 5.0, -60.0, 60.0),
            "cloudy" => (0.8, 0.0, 0.0, 0.0, 6.0, -60.0, 60.0),
            "overcast" => (1.0, 0.0, 0.0, 0.05, 6.0, -60.0, 60.0),
            "fog" => (0.7, 0.0, 0.0, 0.75, 1.0, -60.0, 60.0),
            "drizzle" => (1.0, 0.0, 0.25, 0.2, 4.0, 6.0, 60.0),
            "rain" => (1.0, 0.1, 0.7, 0.1, 8.0, 6.0, 60.0),
            // Under a storm the wind is three times this: thirty metres a
            // second, which a walker can still stand in.
            "storm" => (1.0, 1.0, 0.85, 0.1, 10.0, 10.0, 60.0),
            "sleet" => (1.0, 0.0, 0.7, 0.15, 9.0, 1.5, 1.5),
            "snow" => (1.0, 0.0, 0.5, 0.1, 3.0, -60.0, -3.0),
            "blizzard" => (1.0, 0.2, 1.0, 0.2, 20.0, -60.0, -8.0),
            _ => return false,
        };
        self.cloud_cover.set(cloud, seconds);
        self.storminess.set(storm, seconds);
        self.rain.set(rain, seconds);
        self.fog.set(fog, seconds);
        // The wind keeps the way it blows, or takes the usual one if the
        // air was still.
        let [along_x, along_z] = [self.wind[0].target.get(), self.wind[1].target.get()];
        let speed = along_x.hypot(along_z);
        let usual = WeatherSaid::default().wind;
        let way = if speed > 0.01 {
            [along_x / speed, along_z / speed]
        } else {
            usual.map(|part| part / usual[0].hypot(usual[1]))
        };
        self.wind[0].set(way[0] * wind, seconds);
        self.wind[1].set(way[1] * wind, seconds);
        let warm = self.temperature.target.get();
        self.temperature.set(warm.clamp(coldest, warmest), seconds);
        true
    }
}

/// A number a script wrote for `function`, which `takes` such a number.
pub(crate) fn number(
    lua: &Lua,
    function: &str,
    value: Option<&Value>,
    takes: &str,
    example: &str,
) -> mlua::Result<f32> {
    // The weather needs no more precision than an f32 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    match value {
        Some(Value::Number(number)) if number.is_finite() => Ok(*number as f32),
        Some(Value::Integer(number)) => Ok(*number as f32),
        // Infinity, or what 0 / 0 gives: a number to Luau, and no amount
        // of anything.
        Some(Value::Number(_)) => Err(fail(
            lua,
            format!(
                "{function} was given a number with no size: infinity, or the answer to a sum \
                 such as 0 / 0. It takes {takes}. Write it like this: {example}"
            ),
        )),
        other => Err(fail(
            lua,
            format!(
                "{function} needs a number, but it was given {}. It takes {takes}. Write it \
                 like this: {example}",
                other.map_or("nothing", describe)
            ),
        )),
    }
}

/// How many seconds a change is to take: what the script wrote after the
/// value, or `usual` if it wrote nothing there.
fn change_time(
    lua: &Lua,
    function: &str,
    value: Option<&Value>,
    usual: f32,
    example: &str,
) -> mlua::Result<f32> {
    match value {
        None | Some(Value::Nil) => Ok(usual),
        Some(value) => {
            let seconds = number(
                lua,
                function,
                Some(value),
                "after the value how many seconds the change takes",
                example,
            )?;
            Ok(seconds.clamp(0.0, 86_400.0))
        }
    }
}

/// One of the weather's numbers, as the script reaches it.
#[derive(Clone, Copy)]
struct Dial {
    /// What it is called in `setName` and `name`.
    name: &'static str,
    /// What the setter takes, in words.
    takes: &'static str,
    example: &'static str,
    least: f32,
    most: f32,
    pick: fn(&Climate) -> &Eased,
}

const DIALS: [Dial; 6] = [
    Dial {
        name: "CloudCover",
        takes: "how much of the sky is cloud, from 0 (none) through 0.5 (a fair day) to 1 \
                (all of it)",
        example: "game.weather.setCloudCover(0.8)",
        least: 0.0,
        most: 1.0,
        pick: |climate| &climate.cloud_cover,
    },
    Dial {
        name: "Storminess",
        takes: "how stormy the sky is, from 0 (fair weather) to 1 (storms over most of it)",
        example: "game.weather.setStorminess(0.8)",
        least: 0.0,
        most: 1.0,
        pick: |climate| &climate.storminess,
    },
    Dial {
        name: "Rain",
        takes: "how hard it rains, from 0 (dry) to 1 (a downpour)",
        example: "game.weather.setRain(0.6)",
        least: 0.0,
        most: 1.0,
        pick: |climate| &climate.rain,
    },
    Dial {
        name: "Temperature",
        takes: "how warm the air is, in degrees Celsius: 20 is a mild day and 0 is freezing",
        example: "game.weather.setTemperature(-5)",
        least: -60.0,
        most: 60.0,
        pick: |climate| &climate.temperature,
    },
    Dial {
        name: "Gusts",
        takes: "how gusty the wind is, from 0 (steady) to 1 (lulls and squalls)",
        example: "game.weather.setGusts(0.9)",
        least: 0.0,
        most: 1.0,
        pick: |climate| &climate.gusts,
    },
    Dial {
        name: "Fog",
        takes: "how thick the fog is, from 0 (a clear day) to 1 (nothing seen past forty \
                metres)",
        example: "game.weather.setFog(0.7)",
        least: 0.0,
        most: 1.0,
        pick: |climate| &climate.fog,
    },
];

/// `game.sky`: the hour of the day, and how long a day lasts.
pub(crate) fn sky(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let sky = lua.create_table()?;
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    sky.set(
        "setHour",
        lua.create_function(move |lua, value: Value| {
            let hour = match &value {
                Value::Number(number) if number.is_finite() => *number,
                // An hour is far below where an f64 stops being exact.
                #[allow(clippy::cast_precision_loss)]
                Value::Integer(number) => *number as f64,
                other => {
                    return Err(fail(
                        lua,
                        format!(
                            "game.sky.setHour needs a number, but it was given {}. It takes \
                             the hour of the day: 12 is noon, 18 is the sun setting and 24 \
                             is midnight. Write it like this: game.sky.setHour(18.5)",
                            describe(other)
                        ),
                    ));
                }
            };
            rules_only(lua, &rules, "game.sky.setHour")?;
            climate.hour.set(Some(hour.max(0.0)));
            climate.hour_changed.set(true);
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "hour",
        lua.create_function(move |_, ()| Ok(climate.hour.get()))?,
    )?;
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    sky.set(
        "setDayLength",
        lua.create_function(move |lua, value: Value| {
            let minutes = number(
                lua,
                "game.sky.setDayLength",
                Some(&value),
                "how many minutes of play a whole day and night last, or 0 to stop the hours \
                 passing",
                "game.sky.setDayLength(24)",
            )?;
            rules_only(lua, &rules, "game.sky.setDayLength")?;
            climate
                .day_minutes
                .set(f64::from(minutes.clamp(0.0, 100_000.0)));
            // The hours pass from some hour: noon, if none was said.
            if minutes > 0.0 && climate.hour.get().is_none() {
                climate.hour.set(Some(12.0));
                climate.hour_changed.set(true);
            }
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "dayLength",
        lua.create_function(move |_, ()| Ok(climate.day_minutes.get()))?,
    )?;
    place(lua, shared, &sky)?;
    lights(lua, shared, &sky)?;
    strict(lua, &sky, "game.sky", SKY_FUNCTIONS)?;
    Ok(sky)
}

/// `game.sky.setLatitude`, `setNorth` and `setDate`, and what they said:
/// where on the Earth the world lies and when, for the real sky there.
fn place(lua: &Lua, shared: &Rc<Shared>, sky: &Table) -> mlua::Result<()> {
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    sky.set(
        "setLatitude",
        lua.create_function(move |lua, value: Value| {
            const EXAMPLE: &str = "game.sky.setLatitude(61)";
            let latitude = degrees(
                lua,
                "game.sky.setLatitude",
                &value,
                "a number of degrees: how far north of the equator the world lies, 0 at the \
                 equator, 61 for the far north, -34 for the south",
                EXAMPLE,
            )?;
            if !(-90.0..=90.0).contains(&latitude) {
                return Err(fail(
                    lua,
                    format!(
                        "game.sky.setLatitude was given {latitude}, but a latitude is from -90 \
                         (the south pole) to 90 (the north pole). Write it like this: {EXAMPLE}"
                    ),
                ));
            }
            rules_only(lua, &rules, "game.sky.setLatitude")?;
            climate.change_place(|place| place.latitude = latitude);
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "latitude",
        lua.create_function(move |_, ()| Ok(climate.place.get().map(|place| place.latitude)))?,
    )?;
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    sky.set(
        "setNorth",
        lua.create_function(move |lua, value: Value| {
            let north = degrees(
                lua,
                "game.sky.setNorth",
                &value,
                "a number of degrees: which way north is in the level, as a compass bearing, 0 \
                 when north is the level's -z (Blender's +y), 90 when it is +x",
                "game.sky.setNorth(90)",
            )?;
            rules_only(lua, &rules, "game.sky.setNorth")?;
            climate.change_place(|place| place.north = north.rem_euclid(360.0));
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "north",
        lua.create_function(move |_, ()| Ok(climate.place.get().map(|place| place.north)))?,
    )?;
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    sky.set(
        "setDate",
        lua.create_function(move |lua, values: Variadic<Value>| {
            let [year, month, day] = date(lua, &values)?;
            rules_only(lua, &rules, "game.sky.setDate")?;
            // Small numbers: checked in `date`.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let first_day = lore_sky::days_from_2000(year, month as u32, day as u32);
            climate.change_place(|place| place.first_day = first_day);
            // The day said is today: the hour stays, without the days
            // that had passed.
            if let Some(hour) = climate.hour.get()
                && hour >= 24.0
            {
                climate.hour.set(Some(hour.rem_euclid(24.0)));
                climate.hour_changed.set(true);
            }
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "date",
        lua.create_function(move |_, ()| {
            let Some(place) = climate.place.get() else {
                return Ok((None, None, None));
            };
            // Whole days, held to some millions of years either way so an
            // hour of 1e300 cannot carry the count past what an i64 holds.
            #[allow(clippy::cast_possible_truncation)]
            let passed = (climate.hour.get().unwrap_or(0.0) / 24.0)
                .floor()
                .clamp(-1e9, 1e9) as i64;
            let (year, month, day) = lore_sky::date_from_2000(place.first_day + passed);
            Ok((Some(year), Some(month), Some(day)))
        })?,
    )?;
    Ok(())
}

/// A number of degrees for `function`, or an error that says what it
/// takes.
fn degrees(
    lua: &Lua,
    function: &str,
    value: &Value,
    takes: &str,
    example: &str,
) -> mlua::Result<f64> {
    match value {
        Value::Number(number) if number.is_finite() => Ok(*number),
        // Degrees are far below where an f64 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        Value::Integer(number) => Ok(*number as f64),
        other => Err(fail(
            lua,
            format!(
                "{function} needs a number, but it was given {}. It takes {takes}. Write it \
                 like this: {example}",
                describe(other)
            ),
        )),
    }
}

/// The year, month and day `game.sky.setDate` was given, checked.
fn date(lua: &Lua, values: &Variadic<Value>) -> mlua::Result<[i64; 3]> {
    const EXAMPLE: &str = "game.sky.setDate(2026, 3, 20)";
    const NAMES: [&str; 3] = ["year", "month", "day"];
    let mut said = [0_i64; 3];
    for (number, (slot, name)) in said.iter_mut().zip(NAMES).enumerate() {
        *slot = match values.get(number) {
            Some(Value::Integer(whole)) => *whole,
            // A whole number Luau keeps as a float.
            #[allow(clippy::cast_possible_truncation)]
            Some(Value::Number(value))
                if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e9 =>
            {
                *value as i64
            }
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.sky.setDate needs the {name} as a whole number (a number, not \
                         text, and no fraction), but it was given {}. It takes the year, the \
                         month (1 to 12) and the day, in that order. \
                         Write it like this: {EXAMPLE}",
                        other.map_or("nothing", describe)
                    ),
                ));
            }
        };
    }
    let [year, month, day] = said;
    if !(1..=12).contains(&month) {
        return Err(fail(
            lua,
            format!(
                "game.sky.setDate was given month {month}, but months go from 1 (January) to \
                 12 (December), and the order is year, month, day. Write it like this: {EXAMPLE}"
            ),
        ));
    }
    if !(-100_000..=100_000).contains(&year) {
        return Err(fail(
            lua,
            format!(
                "game.sky.setDate was given the year {year}, but the sky can be worked out \
                 only for years from -100000 to 100000. Write it like this: {EXAMPLE}"
            ),
        ));
    }
    // Checked as 1 to 12 just above.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let longest = {
        let next = if month == 12 {
            lore_sky::days_from_2000(year + 1, 1, 1)
        } else {
            lore_sky::days_from_2000(year, month as u32 + 1, 1)
        };
        next - lore_sky::days_from_2000(year, month as u32, 1)
    };
    if !(1..=longest).contains(&day) {
        return Err(fail(
            lua,
            format!(
                "game.sky.setDate was given day {day}, but that month has days 1 to {longest}. \
                 Write it like this: {EXAMPLE}"
            ),
        ));
    }
    Ok(said)
}

/// The star a script named for `function`, or an error that says which
/// names there are.
fn star(lua: &Lua, function: &str, value: &Value) -> mlua::Result<&'static lore_sky::stars::Star> {
    let example = format!("{function}(\"Sirius\")");
    let Value::String(name) = value else {
        return Err(fail(
            lua,
            format!(
                "{function} needs the name of a star as text, but it was given {}. Write it \
                 like this: {example}",
                describe(value)
            ),
        ));
    };
    let name = name.to_string_lossy();
    lore_sky::stars::named(&name).ok_or_else(|| {
        if let Some(index) = lore_sky::planets::named(&name) {
            let planet = lore_sky::planets::PLANETS[index].name;
            let asked = if function.ends_with("Height") {
                "game.sky.planetHeight"
            } else {
                "game.sky.planetDirection"
            };
            return fail(
                lua,
                format!("{planet} is a planet, not a star: write {asked}(\"{planet}\") instead."),
            );
        }
        let suggestion = lore_sky::stars::closest_name(&name)
            .map(|close| format!("Did you mean \"{close}\"? "))
            .unwrap_or_default();
        fail(
            lua,
            format!(
                "{function} knows no star called \"{name}\". {suggestion}Stars go by their own \
                 names, such as \"Sirius\", \"Polaris\" or \"Betelgeuse\", or by Greek-letter \
                 ones, such as \"Alpha Centauri\"; game.sky.starNames() lists them all. Write it \
                 like this: {example}"
            ),
        )
    })
}

/// The planet a script named for `function`, as its place in
/// [`lore_sky::planets::PLANETS`], or an error that says which there are.
fn planet(lua: &Lua, function: &str, value: &Value) -> mlua::Result<usize> {
    let example = format!("{function}(\"Venus\")");
    let Value::String(name) = value else {
        return Err(fail(
            lua,
            format!(
                "{function} needs the name of a planet as text, but it was given {}. Write it \
                 like this: {example}",
                describe(value)
            ),
        ));
    };
    let name = name.to_string_lossy();
    lore_sky::planets::named(&name).ok_or_else(|| {
        let for_stars = if function.ends_with("Height") {
            "; for a star, use game.sky.starHeight"
        } else if function.ends_with("Direction") {
            "; for a star, use game.sky.starDirection"
        } else {
            ""
        };
        fail(
            lua,
            format!(
                "{function} knows no planet called \"{name}\". The planets are \"Mercury\", \
                 \"Venus\", \"Mars\", \"Jupiter\", \"Saturn\", \"Uranus\" and \
                 \"Neptune\"{for_stars}. Write it like this: {example}"
            ),
        )
    })
}

/// How high above the horizon a direction in the level is, in degrees.
fn height_of(towards: [f32; 3]) -> f64 {
    f64::from(towards[1].clamp(-1.0, 1.0).asin().to_degrees())
}

/// `game.sky.sunDirection`, `sunHeight`, `moonDirection`, `moonHeight`,
/// `moonPhase` and `moonLit`: where the lights of the sky are, as they are
/// drawn. Each gives nil until the hour has been said.
fn lights(lua: &Lua, shared: &Rc<Shared>, sky: &Table) -> mlua::Result<()> {
    let towards = |pick: fn(&Heavens) -> [f32; 3]| {
        let climate = Rc::clone(&shared.weather);
        lua.create_function(move |_, ()| {
            Ok(climate.heavens().map(|heavens| {
                let [x, y, z] = pick(&heavens);
                Vector::new(x, y, z)
            }))
        })
    };
    let height = |pick: fn(&Heavens) -> [f32; 3]| {
        let climate = Rc::clone(&shared.weather);
        lua.create_function(move |_, ()| {
            Ok(climate
                .heavens()
                .map(|heavens| f64::from(pick(&heavens)[1].clamp(-1.0, 1.0).asin().to_degrees())))
        })
    };
    sky.set("sunDirection", towards(|heavens| heavens.sun)?)?;
    sky.set("sunHeight", height(|heavens| heavens.sun)?)?;
    sky.set("moonDirection", towards(|heavens| heavens.moon)?)?;
    sky.set("moonHeight", height(|heavens| heavens.moon)?)?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "moonPhase",
        lua.create_function(move |_, ()| {
            Ok(climate.heavens().map(|heavens| heavens.moon_phase()))
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "starDirection",
        lua.create_function(move |lua, value: Value| {
            let star = star(lua, "game.sky.starDirection", &value)?;
            Ok(climate.heavens().map(|heavens| {
                let [x, y, z] = heavens.world_of(star.towards);
                Vector::new(x, y, z)
            }))
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "starHeight",
        lua.create_function(move |lua, value: Value| {
            let star = star(lua, "game.sky.starHeight", &value)?;
            Ok(climate
                .heavens()
                .map(|heavens| height_of(heavens.world_of(star.towards))))
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "starNear",
        lua.create_function(move |lua, values: Variadic<Value>| {
            const EXAMPLE: &str = "game.sky.starNear(game.player.aim)";
            let Some(Value::Vector(towards)) = values.first() else {
                return Err(fail(
                    lua,
                    format!(
                        "game.sky.starNear needs a direction as a vector, but it was given {}. \
                         It gives the name of the brightest named star within a few degrees of \
                         that direction. Write it like this: {EXAMPLE}, or \
                         game.sky.starNear(vector.create(0, 1, 0)) for straight up",
                        values.first().map_or("nothing", describe)
                    ),
                ));
            };
            let degrees = match values.get(1) {
                None | Some(Value::Nil) => 2.0,
                Some(value) => number(
                    lua,
                    "game.sky.starNear",
                    Some(value),
                    "how many degrees from the direction a star may be, as its second number",
                    "game.sky.starNear(direction, 5)",
                )?
                .clamp(0.0, 180.0),
            };
            let Some(heavens) = climate.heavens() else {
                return Ok(None);
            };
            let [x, y, z] = [towards.x(), towards.y(), towards.z()];
            let long = (x * x + y * y + z * z).sqrt();
            if !(long > 0.0 && long.is_finite()) {
                return Ok(None);
            }
            let unit = [x / long, y / long, z / long];
            let among = heavens.among_stars(unit);
            let star = lore_sky::stars::name_near(among, degrees).map(|name| {
                let magnitude =
                    lore_sky::stars::named(name).map_or(f32::MAX, |star| star.magnitude);
                (magnitude, name)
            });
            // A planet as bright as it is, or brighter, is the one meant.
            let least = degrees.to_radians().cos();
            let planet = heavens
                .planets()
                .iter()
                .zip(&lore_sky::planets::PLANETS)
                .filter(|((at, _), _)| at[0] * unit[0] + at[1] * unit[1] + at[2] * unit[2] >= least)
                .map(|((_, magnitude), planet)| (*magnitude, planet.name))
                .min_by(|one, other| one.0.total_cmp(&other.0));
            Ok(match (star, planet) {
                (Some(star), Some(planet)) if star.0 < planet.0 => Some(star.1),
                (_, Some(planet)) => Some(planet.1),
                (star, None) => star.map(|(_, name)| name),
            })
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "planetDirection",
        lua.create_function(move |lua, value: Value| {
            let index = planet(lua, "game.sky.planetDirection", &value)?;
            Ok(climate.heavens().map(|heavens| {
                let [x, y, z] = heavens.planets()[index].0;
                Vector::new(x, y, z)
            }))
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "planetHeight",
        lua.create_function(move |lua, value: Value| {
            let index = planet(lua, "game.sky.planetHeight", &value)?;
            Ok(climate
                .heavens()
                .map(|heavens| height_of(heavens.planets()[index].0)))
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "planetBrightness",
        lua.create_function(move |lua, value: Value| {
            let index = planet(lua, "game.sky.planetBrightness", &value)?;
            Ok(climate
                .heavens()
                .map(|heavens| f64::from(heavens.planets()[index].1)))
        })?,
    )?;
    sky.set(
        "planetNames",
        lua.create_function(|_, ()| {
            Ok(lore_sky::planets::PLANETS
                .iter()
                .map(|planet| planet.name)
                .collect::<Vec<_>>())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "setMoonSize",
        lua.create_function(move |lua, value: Value| {
            let size = number(
                lua,
                "game.sky.setMoonSize",
                Some(&value),
                "how many times its real size the moon is drawn, from 1 (as it is, half a degree \
                 across, about eight pixels high on a 1080p screen) to 10",
                "game.sky.setMoonSize(3)",
            )?
            .clamp(1.0, 10.0);
            climate.moon_size.set(size);
            climate.moon_size_asked.set(Some(size));
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "setMoonColor",
        lua.create_function(move |lua, value: Value| {
            let (linear, said) = sky_color(lua, "game.sky.setMoonColor", &value)?;
            if linear.iter().all(|part| *part <= 0.0) {
                return Err(fail(
                    lua,
                    "game.sky.setMoonColor was given black, which would hide the moon. Give it \
                     a colour, or \"white\" for its own. Write it like this: \
                     game.sky.setMoonColor(\"#ff6040\")",
                ));
            }
            climate.moon_tint.set(linear);
            *climate.moon_tint_said.borrow_mut() = said;
            climate.moon_tint_asked.set(true);
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "moonColor",
        lua.create_function(move |_, ()| Ok(climate.moon_tint_said.borrow().clone()))?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "setMilkyWayBrightness",
        lua.create_function(move |lua, value: Value| {
            let times = number(
                lua,
                "game.sky.setMilkyWayBrightness",
                Some(&value),
                "how many times its real brightness the Milky Way is drawn, from 0 (not at all) \
                 to 10; 1 is as it really is",
                "game.sky.setMilkyWayBrightness(2)",
            )?
            .clamp(0.0, 10.0);
            climate.milky_way_brightness.set(times);
            climate.milky_way_asked.set(true);
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "milkyWayBrightness",
        lua.create_function(move |_, ()| Ok(f64::from(climate.milky_way_brightness.get())))?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "setMilkyWayColor",
        lua.create_function(move |lua, value: Value| {
            let (linear, said) = sky_color(lua, "game.sky.setMilkyWayColor", &value)?;
            if linear.iter().all(|part| *part <= 0.0) {
                return Err(fail(
                    lua,
                    "game.sky.setMilkyWayColor was given black. To hide the Milky Way, write \
                     game.sky.setMilkyWayBrightness(0); for a colour, write it like this: \
                     game.sky.setMilkyWayColor(\"#c0a0ff\")",
                ));
            }
            climate.milky_way_tint.set(linear);
            *climate.milky_way_tint_said.borrow_mut() = said;
            climate.milky_way_asked.set(true);
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "milkyWayColor",
        lua.create_function(move |_, ()| Ok(climate.milky_way_tint_said.borrow().clone()))?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "setLightPollution",
        lua.create_function(move |lua, value: Value| {
            let class = match &value {
                Value::String(word) => {
                    let word = word.to_string_lossy().trim().to_lowercase();
                    LIGHT_POLLUTION_WORDS
                        .iter()
                        .find(|(name, _)| *name == word)
                        .map(|(_, class)| *class)
                        .ok_or_else(|| {
                            fail(
                                lua,
                                format!(
                                    "game.sky.setLightPollution does not know \"{word}\". Give it \
                                     \"none\", \"village\", \"town\" or \"city\", or a number \
                                     from 1 (no town) to 9 (an inner city). Write it like this: \
                                     game.sky.setLightPollution(\"town\")"
                                ),
                            )
                        })?
                }
                _ => number(
                    lua,
                    "game.sky.setLightPollution",
                    Some(&value),
                    "how much towns light the night sky: \"none\", \"village\", \"town\" or \
                     \"city\", or a number on Bortle's scale from 1 (no town) to 9 (an inner city)",
                    "game.sky.setLightPollution(\"town\")",
                )?
                .clamp(1.0, 9.0),
            };
            climate.light_pollution.set(class);
            climate.light_pollution_asked.set(true);
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "lightPollution",
        lua.create_function(move |_, ()| Ok(f64::from(climate.light_pollution.get())))?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "moonSize",
        lua.create_function(move |_, ()| Ok(f64::from(climate.moon_size.get())))?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "light",
        lua.create_function(move |_, ()| {
            Ok(climate.heavens().map(|heavens| {
                f64::from(
                    heavens.ground_lux() + lore_sky::town_glow_lux(climate.light_pollution.get()),
                )
            }))
        })?,
    )?;
    sky.set(
        "starNames",
        lua.create_function(|_, ()| Ok(lore_sky::stars::star_names()))?,
    )?;
    let climate = Rc::clone(&shared.weather);
    sky.set(
        "moonLit",
        lua.create_function(move |_, ()| {
            Ok(climate
                .heavens()
                .map(|heavens| f64::from(heavens.moon_lit())))
        })?,
    )?;
    Ok(())
}

/// `game.weather`: the weather in a word or a number at a time, what it
/// has left behind, lightning, and the tornado.
pub(crate) fn weather(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let weather = lua.create_table()?;
    weather.set("set", set(lua, shared)?)?;
    for dial in DIALS {
        let setter = format!("set{}", dial.name);
        let getter = {
            let mut name = dial.name.to_owned();
            name[..1].make_ascii_lowercase();
            name
        };
        let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
        let function = format!("game.weather.{setter}");
        weather.set(
            setter,
            lua.create_function(move |lua, values: Variadic<Value>| {
                let amount = number(lua, &function, values.first(), dial.takes, dial.example)?;
                let seconds = change_time(lua, &function, values.get(1), 0.0, dial.example)?;
                rules_only(lua, &rules, &function)?;
                (dial.pick)(&climate).set(amount.clamp(dial.least, dial.most), seconds);
                Ok(())
            })?,
        )?;
        let climate = Rc::clone(&shared.weather);
        weather.set(
            getter,
            lua.create_function(move |_, ()| Ok(f64::from((dial.pick)(&climate).now())))?,
        )?;
    }
    wind(lua, shared, &weather)?;
    ground(lua, shared, &weather)?;
    lightning(lua, shared, &weather)?;
    tornado(lua, shared, &weather)?;
    strict(lua, &weather, "game.weather", WEATHER_FUNCTIONS)?;
    Ok(weather)
}

/// `game.weather.set(name, seconds)`: a kind of weather in one word.
fn set(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    const EXAMPLE: &str = "game.weather.set(\"rain\")";
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    lua.create_function(move |lua, values: Variadic<Value>| {
        let name = match values.first() {
            // "Rain" and "RAIN" are rain.
            Some(Value::String(name)) => name.to_string_lossy().trim().to_ascii_lowercase(),
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.weather.set needs the name of a kind of weather, but it was \
                         given {}. The kinds are: {}. Write it like this: {EXAMPLE}",
                        other.map_or("nothing", describe),
                        WEATHERS.join(", ")
                    ),
                ));
            }
        };
        let seconds = change_time(
            lua,
            "game.weather.set",
            values.get(1),
            USUAL_CHANGE,
            "game.weather.set(\"rain\", 60)",
        )?;
        rules_only(lua, &rules, "game.weather.set")?;
        if climate.bring(&name, seconds) {
            return Ok(());
        }
        let mut message = format!("There is no kind of weather called \"{name}\".");
        match crate::error::closest(&name, WEATHERS.iter().copied()) {
            Some(near) => message.push_str(&format!(" Did you mean \"{near}\"?")),
            None => message.push_str(&format!(" The kinds are: {}.", WEATHERS.join(", "))),
        }
        Err(fail(lua, message))
    })
}

/// `game.weather.setWind` and `wind`: the wind as the way the air moves.
fn wind(lua: &Lua, shared: &Rc<Shared>, weather: &Table) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.weather.setWind(vector.create(8, 0, 0))";
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    weather.set(
        "setWind",
        lua.create_function(move |lua, values: Variadic<Value>| {
            let Some(Value::Vector(wind)) = values.first() else {
                return Err(fail(
                    lua,
                    format!(
                        "game.weather.setWind needs a vector, but it was given {}. It takes \
                         the way the air moves, and the vector's length is how fast, in \
                         metres a second: 5 is a breeze, 20 a gale. Write it like this: \
                         {EXAMPLE}",
                        values.first().map_or("nothing", describe)
                    ),
                ));
            };
            let seconds = change_time(
                lua,
                "game.weather.setWind",
                values.get(1),
                0.0,
                "game.weather.setWind(vector.create(8, 0, 0), 30)",
            )?;
            rules_only(lua, &rules, "game.weather.setWind")?;
            // The wind blows along the ground; no wind is faster than a
            // hundred metres a second.
            let mut along =
                [wind.x(), wind.z()].map(|part| if part.is_finite() { part } else { 0.0 });
            let speed = along[0].hypot(along[1]);
            if speed > 100.0 {
                along = along.map(|part| part * 100.0 / speed);
            }
            climate.wind[0].set(along[0], seconds);
            climate.wind[1].set(along[1], seconds);
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    weather.set(
        "wind",
        lua.create_function(move |_, ()| {
            Ok(Vector::new(
                climate.wind[0].now(),
                0.0,
                climate.wind[1].now(),
            ))
        })?,
    )?;
    Ok(())
}

/// The deepest snow lies, in metres, whether it settles by itself or a
/// script says so.
pub const DEEPEST_SNOW: f32 = 0.6;
/// The thickest ice on still water grows, in metres, whether it freezes by
/// itself or a script says so.
pub const THICKEST_ICE: f32 = 0.5;

/// What the weather leaves behind and what is falling: `setSnowDepth`,
/// `snowDepth`, `setIce`, `ice` and `falling`.
fn ground(lua: &Lua, shared: &Rc<Shared>, weather: &Table) -> mlua::Result<()> {
    type Asked = fn(&Climate) -> &Cell<Option<f32>>;
    type Lies = fn(&WeatherHere) -> f32;
    let both: [(&str, &str, &str, &str, Asked, Lies); 2] = [
        (
            "setSnowDepth",
            "snowDepth",
            "how deep the snow lies, in metres: 0.05 whitens the land and 0.4 is hard to \
             walk through",
            "game.weather.setSnowDepth(0.2)",
            |climate| &climate.snow_asked,
            |here| here.snow_depth,
        ),
        (
            "setIce",
            "ice",
            "how thick the ice on lakes and ponds is, in metres: at 0.05 it bears someone \
             walking on it",
            "game.weather.setIce(0.1)",
            |climate| &climate.ice_asked,
            |here| here.ice,
        ),
    ];
    for (setter, getter, takes, example, asked, lies) in both {
        let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
        let function = format!("game.weather.{setter}");
        weather.set(
            setter,
            lua.create_function(move |lua, value: Value| {
                let depth = number(lua, &function, Some(&value), takes, example)?;
                rules_only(lua, &rules, &function)?;
                let most = if setter == "setIce" {
                    THICKEST_ICE
                } else {
                    DEEPEST_SNOW
                };
                let depth = depth.clamp(0.0, most);
                asked(&climate).set(Some(depth));
                // The script reads back what it wrote at once, before the
                // engine has taken it.
                let mut here = climate.here.get();
                if setter == "setIce" {
                    here.ice = depth;
                } else {
                    here.snow_depth = depth;
                }
                climate.here.set(here);
                Ok(())
            })?,
        )?;
        let climate = Rc::clone(&shared.weather);
        weather.set(
            getter,
            lua.create_function(move |_, ()| Ok(f64::from(lies(&climate.here.get()))))?,
        )?;
    }
    let climate = Rc::clone(&shared.weather);
    weather.set(
        "falling",
        lua.create_function(move |_, ()| {
            let here = climate.here.get();
            let kinds = [
                ("rain", here.rain),
                ("sleet", here.sleet),
                ("snow", here.snow),
                ("hail", here.hail),
            ];
            // The first of them that falls hardest; hail counts double,
            // since a little of it matters more than a lot of rain.
            let mut most = ("nothing", 0.0_f32);
            for (name, hard) in kinds {
                let weighed = if name == "hail" { hard * 2.0 } else { hard };
                if hard > 0.02 && weighed > most.1 {
                    most = (name, weighed);
                }
            }
            let hard = kinds
                .iter()
                .find(|(name, _)| *name == most.0)
                .map_or(0.0, |(_, hard)| *hard);
            Ok((most.0, f64::from(hard)))
        })?,
    )?;
    Ok(())
}

/// `game.weather.strike` and `onLightning`.
fn lightning(lua: &Lua, shared: &Rc<Shared>, weather: &Table) -> mlua::Result<()> {
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    weather.set(
        "strike",
        lua.create_function(move |lua, value: Value| {
            let Value::Vector(place) = &value else {
                return Err(fail(
                    lua,
                    format!(
                        "game.weather.strike needs a place, but it was given {}. It takes \
                         where on the ground the lightning strikes. Write it like this: \
                         game.weather.strike(game.player.position + vector.create(0, 0, -200))",
                        describe(&value)
                    ),
                ));
            };
            rules_only(lua, &rules, "game.weather.strike")?;
            let mut strikes = climate.strikes.borrow_mut();
            if strikes.len() < MOST_STRIKES {
                climate.struck.set(climate.struck.get().wrapping_add(1));
                strikes.push(([place.x(), place.y(), place.z()], climate.struck.get()));
            }
            Ok(())
        })?,
    )?;
    let list: fn(&mut Handlers) -> &mut Vec<Function> = |handlers| &mut handlers.lightning;
    weather.set(
        "onLightning",
        crate::api::on_event(
            lua,
            shared,
            "weather.onLightning",
            "game.weather.onLightning(function(at) ... end)",
            list,
        )?,
    )?;
    Ok(())
}

/// `game.weather.startTornado`, `tornado` and `stopTornado`.
fn tornado(lua: &Lua, shared: &Rc<Shared>, weather: &Table) -> mlua::Result<()> {
    const EXAMPLE: &str =
        "game.weather.startTornado(game.player.position + vector.create(0, 0, -300))";
    const OPTIONS: [&str; 4] = ["strength", "heading", "speed", "lasts"];
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    weather.set(
        "startTornado",
        lua.create_function(move |lua, values: Variadic<Value>| {
            let Some(Value::Vector(at)) = values.first() else {
                return Err(fail(
                    lua,
                    format!(
                        "game.weather.startTornado needs a place, but it was given {}. It \
                         takes where on the ground the tornado comes down. Write it like \
                         this: {EXAMPLE}",
                        values.first().map_or("nothing", describe)
                    ),
                ));
            };
            let (mut strength, mut heading, mut speed, mut seconds) = (0.5, None, 9.0, 120.0);
            match values.get(1) {
                None | Some(Value::Nil) => {}
                Some(Value::Table(options)) => {
                    for pair in options.pairs::<Value, Value>() {
                        let (key, value) = pair?;
                        let key = match &key {
                            Value::String(key) => key.to_string_lossy(),
                            other => other.to_string()?,
                        };
                        let function = "game.weather.startTornado";
                        match key.as_str() {
                            "strength" => {
                                strength = number(
                                    lua,
                                    function,
                                    Some(&value),
                                    "as strength how strong it gets, from 0.1 (a rope that \
                                     breaks branches) to 1 (a wedge that leaves nothing \
                                     standing)",
                                    "game.weather.startTornado(at, { strength = 0.8 })",
                                )?;
                            }
                            "speed" => {
                                speed = number(
                                    lua,
                                    function,
                                    Some(&value),
                                    "as speed how fast it travels over the land, in metres a \
                                     second",
                                    "game.weather.startTornado(at, { speed = 12 })",
                                )?;
                            }
                            "lasts" => {
                                seconds = number(
                                    lua,
                                    function,
                                    Some(&value),
                                    "as lasts how many seconds it is there for",
                                    "game.weather.startTornado(at, { lasts = 90 })",
                                )?;
                            }
                            "heading" => {
                                let Value::Vector(way) = &value else {
                                    return Err(fail(
                                        lua,
                                        format!(
                                            "game.weather.startTornado's heading is the way \
                                             it travels, a vector, but it was given {}. Write \
                                             it like this: game.weather.startTornado(at, \
                                             {{ heading = vector.create(1, 0, 0) }})",
                                            describe(&value)
                                        ),
                                    ));
                                };
                                let long = way.x().hypot(way.z());
                                if long.is_finite() && long > 1e-4 {
                                    heading = Some([way.x() / long, way.z() / long]);
                                }
                            }
                            other => {
                                let mut message = format!(
                                    "game.weather.startTornado has no option called \"{other}\"."
                                );
                                match crate::error::closest(other, OPTIONS.iter().copied()) {
                                    Some(near) => {
                                        message.push_str(&format!(" Did you mean {near}?"));
                                    }
                                    None => message.push_str(&format!(
                                        " Its options are: {}.",
                                        OPTIONS.join(", ")
                                    )),
                                }
                                return Err(fail(lua, message));
                            }
                        }
                    }
                }
                Some(other) => {
                    return Err(fail(
                        lua,
                        format!(
                            "game.weather.startTornado takes its options in a table after \
                             the place, but it was given {}. Write it like this: \
                             game.weather.startTornado(at, {{ strength = 0.8, lasts = 90 }})",
                            describe(other)
                        ),
                    ));
                }
            }
            rules_only(lua, &rules, "game.weather.startTornado")?;
            let mut asked = climate.tornado_asked.borrow_mut();
            // Only the newest of them can still matter.
            asked.clear();
            asked.push(TornadoAsk::Start {
                at: [at.x(), at.y(), at.z()],
                strength: strength.clamp(0.1, 1.0),
                heading,
                speed: speed.clamp(0.0, 40.0),
                seconds: seconds.clamp(5.0, 3_600.0),
            });
            Ok(())
        })?,
    )?;
    let (climate, rules) = (Rc::clone(&shared.weather), Rc::clone(shared));
    weather.set(
        "stopTornado",
        lua.create_function(move |lua, ()| {
            rules_only(lua, &rules, "game.weather.stopTornado")?;
            climate.tornado_asked.borrow_mut().push(TornadoAsk::Stop);
            Ok(())
        })?,
    )?;
    let climate = Rc::clone(&shared.weather);
    weather.set(
        "tornado",
        lua.create_function(move |_, ()| {
            Ok(climate
                .here
                .get()
                .tornado
                .map(|[x, y, z]| Vector::new(x, y, z)))
        })?,
    )?;
    Ok(())
}
