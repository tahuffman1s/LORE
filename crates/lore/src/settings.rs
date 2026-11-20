//! The player's own settings: how loud (everything, and music and effects
//! each), how fast the view turns, which way up, how hard the gamepad
//! shakes, how big the HUD and menus are, and how the picture is drawn.
//! One file on their computer that every game played with LORE reads when
//! it starts, and that the engine writes when a game's settings menu
//! changes one (`game.settings` in a script).
//!
//! The file is `settings.toml`, beside the bindings file (see
//! [`controls`](crate::controls)): in the `lore` folder of the place each
//! system keeps settings. Setting `LORE_SETTINGS` to a file's path uses
//! that file instead.
//!
//! ```
//! use lore::settings::Settings;
//!
//! let (settings, problems) = Settings::read("volume = 40\ninvert_look = true\n");
//! assert!(problems.is_empty());
//! assert_eq!(settings.volume, 40.0);
//! assert!(settings.invert_look);
//! // What the file leaves out keeps its usual value.
//! assert_eq!(settings.sensitivity, 1.0);
//! ```
//!
//! # Adding a setting
//!
//! Every setting is one entry in the `settings!` list in this file: its
//! name, what kind of thing it is, what a new player has, the heading and
//! the words a menu shows for it, and a sentence about it. The file, its
//! comments, what a script reads with `game.settings.list()` and every
//! menu built from that follow from the entry. What is left is to put the
//! new field into effect where the others are, in `App::apply_settings`.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lore_render::{AmbientOcclusion, AnisotropicFiltering, Bounce, LampShadows, Upscaling};
use lore_script::{Setting, SettingKind, SettingValue};
use toml::de::{DeTable, DeValue};

/// Names another settings file to use than the usual one.
pub const FILE_VARIABLE: &str = "LORE_SETTINGS";

/// A setting that is a number: the lowest and highest it can be, and how
/// far one press of left or right moves it in a settings menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    /// The lowest value.
    pub lowest: f32,
    /// The highest value.
    pub highest: f32,
    /// One step in a settings menu.
    pub step: f32,
}

/// What kind of thing a setting is, and what it can be.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Number(Range),
    /// A number that is 0 for no limit, and otherwise from the range's
    /// lowest to its highest. What lies between 0 and the lowest is no
    /// limit of its own: see [`limit`].
    Limit(Range),
    Toggle,
    /// One of these words.
    Choice(&'static [&'static str]),
}

/// What a setting is set to, whatever its kind.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Value {
    Number(f32),
    Toggle(bool),
    Choice(&'static str),
}

/// A type a setting is kept as in [`Settings`].
trait Stored: Sized {
    /// The value as any setting's.
    fn put(self) -> Value;
    /// The value back, if `value` is of this kind.
    fn take(value: Value) -> Option<Self>;
}

impl Stored for f32 {
    fn put(self) -> Value {
        Value::Number(self)
    }

    fn take(value: Value) -> Option<Self> {
        match value {
            Value::Number(number) => Some(number),
            _ => None,
        }
    }
}

impl Stored for bool {
    fn put(self) -> Value {
        Value::Toggle(self)
    }

    fn take(value: Value) -> Option<Self> {
        match value {
            Value::Toggle(on) => Some(on),
            _ => None,
        }
    }
}

/// Makes a setting of a type that is one of a few things, each with the
/// word a player and a script know it by: `$words` holds the words, in the
/// order a menu goes through them.
macro_rules! words {
    ($type:ty, $words:ident: $($variant:path => $word:literal),+ $(,)?) => {
        const $words: &[&str] = &[$($word),+];

        impl Stored for $type {
            fn put(self) -> Value {
                Value::Choice(match self {
                    $($variant => $word,)+
                })
            }

            fn take(value: Value) -> Option<Self> {
                match value {
                    $(Value::Choice($word) => Some($variant),)+
                    _ => None,
                }
            }
        }
    };
}

words!(Upscaling, UPSCALING:
    Upscaling::Off => "off",
    Upscaling::Native => "native",
    Upscaling::Quality => "quality",
    Upscaling::Balanced => "balanced",
    Upscaling::Performance => "performance",
    Upscaling::UltraPerformance => "ultra-performance",
);

words!(Bounce, BOUNCE:
    Bounce::Off => "off",
    Bounce::Low => "low",
    Bounce::Medium => "medium",
    Bounce::High => "high",
);

words!(AmbientOcclusion, AMBIENT_OCCLUSION:
    AmbientOcclusion::Off => "off",
    AmbientOcclusion::Low => "low",
    AmbientOcclusion::Medium => "medium",
    AmbientOcclusion::High => "high",
);

words!(AnisotropicFiltering, ANISOTROPIC_FILTERING:
    AnisotropicFiltering::Off => "off",
    AnisotropicFiltering::X2 => "2x",
    AnisotropicFiltering::X4 => "4x",
    AnisotropicFiltering::X8 => "8x",
    AnisotropicFiltering::X16 => "16x",
);

words!(LampShadows, LAMP_SHADOWS:
    LampShadows::Off => "off",
    LampShadows::Low => "low",
    LampShadows::Medium => "medium",
    LampShadows::High => "high",
);

/// The word a player and a script know `upscaling` by: what
/// `lore bench --upscaling` takes, and what the settings file holds.
#[must_use]
pub fn upscaling_name(upscaling: Upscaling) -> &'static str {
    match upscaling.put() {
        Value::Choice(word) => word,
        Value::Number(_) | Value::Toggle(_) => "",
    }
}

/// One setting: everything the file, a script and a menu need to know of it.
struct Entry {
    /// What it is called in the file and by a script.
    name: &'static str,
    /// The heading a menu shows it under.
    group: &'static str,
    /// What a menu shows beside it.
    label: &'static str,
    /// A sentence or two for the player: the comment over it in the file,
    /// and what a script reads as its description.
    about: &'static str,
    shape: Shape,
    get: fn(&Settings) -> Value,
    set: fn(&mut Settings, Value),
}

/// Makes [`Settings`], what a new player has, and the list every reader
/// and writer of settings goes by, from one entry for each setting:
///
/// ```text
/// /// A sentence for the player, which is also the field's documentation.
/// name: Type = what a new player has, its shape, "Heading", "Label";
/// ```
macro_rules! settings {
    ($(
        $(#[doc = $about:literal])+
        $name:ident: $type:ty = $default:expr, $shape:expr, $group:literal, $label:literal;
    )+) => {
        /// What a player has chosen. `Settings::default()` is what a new
        /// player gets.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Settings {
            $(
                $(#[doc = $about])+
                pub $name: $type,
            )+
        }

        impl Default for Settings {
            /// Full volume, the game's own turning speed, up is up, rumble
            /// as the game asks for it, the HUD and menus at their designed
            /// size, and the picture as the engine draws it best on most
            /// computers.
            fn default() -> Self {
                Self {
                    $($name: $default,)+
                }
            }
        }

        /// Every setting, in the order the file and a menu have them.
        const ALL: &[Entry] = &[
            $(Entry {
                name: stringify!($name),
                group: $group,
                label: $label,
                about: concat!($($about),+),
                shape: $shape,
                get: |settings| settings.$name.put(),
                set: |settings, value| {
                    if let Some(value) = Stored::take(value) {
                        settings.$name = value;
                    }
                },
            },)+
        ];
    };
}

settings! {
    /// How loud everything is, from 0 (silent) to 100 (full).
    volume: f32 = 100.0, Shape::Number(Settings::VOLUME), "Sound", "Volume";
    /// How loud music is, from 0 (off) to 100 (full). It is turned down by
    /// `volume` as well.
    music_volume: f32 = 100.0, Shape::Number(Settings::VOLUME), "Sound", "Music volume";
    /// How loud everything that is not music is, from 0 (off) to 100
    /// (full). It is turned down by `volume` as well.
    effects_volume: f32 = 100.0, Shape::Number(Settings::VOLUME), "Sound", "Effects volume";
    /// How fast the view turns with the mouse and the stick: 1 is the
    /// game's own speed, 2 is twice as fast. From 0.1 to 3.
    sensitivity: f32 = 1.0, Shape::Number(Settings::SENSITIVITY), "Controls", "Look sensitivity";
    /// true makes pushing the mouse or stick up look down.
    invert_look: bool = false, Shape::Toggle, "Controls", "Invert look";
    /// How hard the gamepad shakes, in percent of what a game asks for,
    /// from 0 (never) to 100 (as the game asks).
    rumble: f32 = 100.0, Shape::Number(Settings::RUMBLE), "Controls", "Rumble";
    /// How big the HUD and menus are, in percent, from 75 to 150.
    ui_size: f32 = 100.0, Shape::Number(Settings::UI_SIZE), "Graphics", "HUD and menu size";
    /// How much is seen from the bottom of the picture to the top, in
    /// degrees, from 50 to 110. More shows more to the sides and makes
    /// what is ahead smaller.
    field_of_view: f32 = 74.0, Shape::Number(Settings::FIELD_OF_VIEW), "Graphics", "Field of view";
    /// How bright the picture is, from 0 (darkest) to 100 (brightest). 50
    /// is as the game was made.
    brightness: f32 = 50.0, Shape::Number(Settings::BRIGHTNESS), "Graphics", "Brightness";
    /// How much smaller than the window the game is drawn before FSR makes
    /// it full size: smaller is faster. "off" draws every pixel with
    /// nothing done to it, "native" draws every pixel and smooths the
    /// edges, and each one after that is faster and a little softer.
    upscaling: Upscaling = Upscaling::Quality, Shape::Choice(UPSCALING), "Graphics", "Upscaling";
    /// true waits for the display before showing each picture, so the
    /// picture never tears. false shows each at once, for the lowest delay.
    vsync: bool = true, Shape::Toggle, "Graphics", "Vsync";
    /// The most pictures drawn each second, from 30 to 1000, or 0 for no
    /// limit. It matters with `vsync` off, where nothing else holds the
    /// game back.
    frame_rate_limit: f32 = 400.0, Shape::Limit(Settings::FRAME_RATE_LIMIT), "Graphics",
        "Frame rate limit";
    /// true lets wet ground, puddles, glass and polished metal mirror what
    /// is round them. false shows only the colours of the sky and the
    /// ground in them, which is faster where much of the picture is wet.
    reflections: bool = true, Shape::Toggle, "Graphics", "Reflections";
    /// true lights the air: beams of sun through gaps and leaves where
    /// there is haze, mist or dust, and lamps that glow in fog. false draws
    /// the fog as if all of it were in sunlight, which is faster.
    light_shafts: bool = true, Shape::Toggle, "Graphics", "Light shafts";
    /// How many lamps near you cast shadows, so that their light stops at
    /// walls: "off" (none: lamplight goes through walls), "low" (4, with
    /// softer edges), "medium" (8) or "high" (16). More is slower where
    /// many lamps are near.
    lamp_shadows: LampShadows = LampShadows::Medium, Shape::Choice(LAMP_SHADOWS), "Graphics",
        "Lamp shadows";
    /// How light bounces off what it strikes (global illumination): a sunlit
    /// wall warms the shade across from it, a lamp lights the ceiling above
    /// it, and a room with no window stays dark. "off" lights shade from
    /// the sky alone, which is faster; "low", "medium" and "high" follow a
    /// change in the light more smoothly, each slower than the one before.
    bounce: Bounce = Bounce::Medium, Shape::Choice(BOUNCE), "Graphics", "Bounce light";
    /// The soft shade in corners, under things and where something stands
    /// on the ground (ambient occlusion). It only dims light that comes
    /// from all round, never the sun's or a lamp's. "off" draws none, which
    /// is faster; "low", "medium" and "high" look along more lines for
    /// smoother shade, each slower than the one before.
    ambient_occlusion: AmbientOcclusion = AmbientOcclusion::Medium,
        Shape::Choice(AMBIENT_OCCLUSION), "Graphics", "Ambient occlusion";
    /// true lets things touch their shadows in the sun: a foot on the
    /// ground, a cup on a table, a blade of grass, all finer than the
    /// sun's shadows can hold by themselves. false leaves a sliver of light
    /// between them, which is a little faster.
    contact_shadows: bool = true, Shape::Toggle, "Graphics", "Contact shadows";
    /// How sharp the ground, roads and walls look where they run away
    /// from you (anisotropic filtering). "off" blurs them a few steps
    /// ahead; "2x", "4x", "8x" and "16x" each keep them sharp about twice
    /// as far as the one before. It costs very little: turn it down only
    /// if the game is slow on a graphics card built into the processor.
    anisotropic_filtering: AnisotropicFiltering = AnisotropicFiltering::X16,
        Shape::Choice(ANISOTROPIC_FILTERING), "Graphics", "Anisotropic filtering";
    /// How much of the glow round bright things the game asks for you see,
    /// in percent, from 0 (none) to 100 (as the game made it).
    bloom: f32 = 100.0, Shape::Number(Settings::SHARE), "Graphics", "Bloom";
    /// How much of the darkened corners the game asks for you see, in
    /// percent, from 0 (none) to 100 (as the game made it).
    vignette: f32 = 100.0, Shape::Number(Settings::SHARE), "Graphics", "Vignette";
    /// How much of the film grain the game asks for you see, in percent,
    /// from 0 (none) to 100 (as the game made it).
    film_grain: f32 = 100.0, Shape::Number(Settings::SHARE), "Graphics", "Film grain";
    /// How much of the red and blue fringes towards the edges the game
    /// asks for you see (chromatic aberration), in percent, from 0 (none)
    /// to 100 (as the game made it).
    chromatic_aberration: f32 = 100.0, Shape::Number(Settings::SHARE), "Graphics",
        "Chromatic aberration";
    /// How much the upscaled picture is sharpened, from 0 (not at all) to
    /// 100. Much more than 30 draws bright rims round edges. With
    /// upscaling "off" it changes nothing.
    sharpening: f32 = 30.0, Shape::Number(Settings::SHARE), "Graphics", "Sharpening";
}

/// Settings older versions of LORE wrote that there no longer are. A file
/// that still has one is read without a word, and loses the line the next
/// time it is saved: `frame_meter` went when the frame meter became the
/// game's own to draw (`game.frames`, `game.hud.frameGraph`).
const RETIRED: &[&str] = &["frame_meter"];

/// The most the brightness setting brightens or darkens the picture, in
/// camera stops: at its ends the light is nearly tripled, or cut to nearly
/// a third.
const BRIGHTNESS_STOPS: f32 = 1.5;

impl Settings {
    /// What [`volume`](Self::volume), [`music_volume`](Self::music_volume)
    /// and [`effects_volume`](Self::effects_volume) can be.
    pub const VOLUME: Range = Range {
        lowest: 0.0,
        highest: 100.0,
        step: 5.0,
    };
    /// What [`sensitivity`](Self::sensitivity) can be.
    pub const SENSITIVITY: Range = Range {
        lowest: 0.1,
        highest: 3.0,
        step: 0.1,
    };
    /// What [`rumble`](Self::rumble) can be.
    pub const RUMBLE: Range = Range {
        lowest: 0.0,
        highest: 100.0,
        step: 5.0,
    };
    /// What [`ui_size`](Self::ui_size) can be.
    pub const UI_SIZE: Range = Range {
        lowest: 75.0,
        highest: 150.0,
        step: 5.0,
    };
    /// What [`field_of_view`](Self::field_of_view) can be.
    pub const FIELD_OF_VIEW: Range = Range {
        lowest: 50.0,
        highest: 110.0,
        step: 1.0,
    };
    /// What [`brightness`](Self::brightness) can be.
    pub const BRIGHTNESS: Range = Range {
        lowest: 0.0,
        highest: 100.0,
        step: 5.0,
    };
    /// What [`bloom`](Self::bloom), [`vignette`](Self::vignette),
    /// [`film_grain`](Self::film_grain),
    /// [`chromatic_aberration`](Self::chromatic_aberration) and
    /// [`sharpening`](Self::sharpening) can be: percentages.
    pub const SHARE: Range = Range {
        lowest: 0.0,
        highest: 100.0,
        step: 5.0,
    };
    /// What [`frame_rate_limit`](Self::frame_rate_limit) can be besides 0,
    /// which is no limit. The lowest is the slowest frames can come
    /// without leaving ticks behind; the highest is above what the fastest
    /// displays show.
    pub const FRAME_RATE_LIMIT: Range = Range {
        lowest: 30.0,
        highest: 1000.0,
        step: 10.0,
    };

    /// What sound is multiplied by, from 0 to 1.
    ///
    /// Ears hear loudness in ratios, so the slider's number is squared:
    /// half way along sounds about half as loud, where half the signal
    /// would sound only a little quieter.
    #[must_use]
    pub fn gain(&self) -> f32 {
        gain(self.volume)
    }

    /// What music is multiplied by on top of [`gain`](Self::gain), from 0
    /// to 1, squared in the same way.
    #[must_use]
    pub fn music_gain(&self) -> f32 {
        gain(self.music_volume)
    }

    /// What every sound that is not music is multiplied by on top of
    /// [`gain`](Self::gain), from 0 to 1, squared in the same way.
    #[must_use]
    pub fn effects_gain(&self) -> f32 {
        gain(self.effects_volume)
    }

    /// What the `look` action is multiplied by, sideways and then up and
    /// down, as `ActionMap::set_sensitivity` takes it.
    #[must_use]
    pub fn look(&self) -> [f32; 2] {
        let along = if self.invert_look { -1.0 } else { 1.0 };
        [self.sensitivity, self.sensitivity * along]
    }

    /// What the strength of every rumble is multiplied by, from 0 to 1.
    #[must_use]
    pub fn rumble_share(&self) -> f32 {
        (self.rumble / 100.0).clamp(0.0, 1.0)
    }

    /// What HUD and menu sizes are multiplied by: 1 is the designed size.
    #[must_use]
    pub fn ui_scale(&self) -> f32 {
        self.ui_size / 100.0
    }

    /// How much of the game's glow, vignette, grain and fringes the player
    /// wants to see, as `Renderer::set_look_shares` takes it.
    #[must_use]
    pub fn look_shares(&self) -> lore_render::LookShares {
        let share = |percent: f32| (percent / 100.0).clamp(0.0, 1.0);
        lore_render::LookShares {
            bloom: share(self.bloom),
            vignette: share(self.vignette),
            grain: share(self.film_grain),
            chromatic_aberration: share(self.chromatic_aberration),
        }
    }

    /// How much FSR sharpens what it upscales, from 0 to 1, as
    /// `Renderer::set_sharpness` takes it.
    #[must_use]
    pub fn sharpness(&self) -> f32 {
        (self.sharpening / 100.0).clamp(0.0, 1.0)
    }

    /// How much brighter (above 0) or darker the picture is drawn, in
    /// camera stops, as `Renderer::set_exposure` takes it: 0 at a
    /// brightness of 50.
    #[must_use]
    pub fn exposure_stops(&self) -> f32 {
        ((self.brightness - 50.0) / 50.0).clamp(-1.0, 1.0) * BRIGHTNESS_STOPS
    }

    /// The most pictures drawn each second, as a whole number: 0 for no
    /// limit.
    #[must_use]
    pub fn frames_per_second(&self) -> u32 {
        if self.frame_rate_limit <= 0.0 {
            return 0;
        }
        let Range {
            lowest, highest, ..
        } = Self::FRAME_RATE_LIMIT;
        // Held between 30 and 1000, which a `u32` holds.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let limit = self.frame_rate_limit.clamp(lowest, highest).round() as u32;
        limit
    }

    /// Every setting as a script is told of it (`game.settings.list()`),
    /// set as these are.
    ///
    /// ```
    /// use lore::settings::Settings;
    ///
    /// let told = Settings::default().described();
    /// assert_eq!(told[0].name, "volume");
    /// assert_eq!(told[0].label, "Volume");
    /// ```
    #[must_use]
    pub fn described(&self) -> Vec<Setting> {
        let usual = Self::default();
        ALL.iter()
            .map(|entry| Setting {
                name: entry.name.to_owned(),
                label: entry.label.to_owned(),
                description: about(entry),
                group: entry.group.to_owned(),
                kind: match entry.shape {
                    Shape::Number(range) => SettingKind::Number {
                        min: exactly(range.lowest),
                        max: exactly(range.highest),
                        step: exactly(range.step),
                    },
                    Shape::Limit(range) => SettingKind::Number {
                        min: 0.0,
                        max: exactly(range.highest),
                        step: exactly(range.step),
                    },
                    Shape::Toggle => SettingKind::Toggle,
                    Shape::Choice(words) => SettingKind::Choice {
                        options: words.iter().map(|word| (*word).to_owned()).collect(),
                    },
                },
                value: told((entry.get)(self)),
                default: told((entry.get)(&usual)),
            })
            .collect()
    }

    /// Sets the setting called `name` to `value`, as a script changed it
    /// (`ScriptHost::take_changed_settings`). A name that is no setting,
    /// or a value of the wrong kind, changes nothing; a number outside
    /// what the setting takes becomes the nearest one allowed.
    pub fn change(&mut self, name: &str, value: &SettingValue) {
        let Some(entry) = ALL.iter().find(|entry| entry.name == name) else {
            return;
        };
        // A setting needs no more precision than an `f32` holds.
        #[allow(clippy::cast_possible_truncation)]
        let value = match (entry.shape, value) {
            (Shape::Number(range), SettingValue::Number(number)) if number.is_finite() => {
                Value::Number((*number as f32).clamp(range.lowest, range.highest))
            }
            (Shape::Limit(range), SettingValue::Number(number)) if number.is_finite() => {
                let now = match (entry.get)(self) {
                    Value::Number(now) => now,
                    Value::Toggle(_) | Value::Choice(_) => 0.0,
                };
                Value::Number(limit(*number as f32, now, range))
            }
            (Shape::Toggle, SettingValue::Toggle(on)) => Value::Toggle(*on),
            (Shape::Choice(words), SettingValue::Choice(word)) => {
                match words.iter().find(|known| **known == word) {
                    Some(known) => Value::Choice(known),
                    None => return,
                }
            }
            _ => return,
        };
        (entry.set)(self, value);
    }

    /// Reads the text of a settings file. Returns the settings, and what
    /// the player should be told about the file; empty when all is well.
    ///
    /// A player's file never stops a game from starting: a line that
    /// cannot be used is left out and reported, and that setting keeps its
    /// usual value. A number outside its range becomes the nearest one
    /// allowed.
    #[must_use]
    pub fn read(text: &str) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let document = match DeTable::parse(text) {
            Ok(document) => document,
            Err(error) => {
                let line = line_of(text, error.span().map_or(0, |span| span.start));
                return (
                    settings,
                    vec![format!(
                        "line {line}: {}. None of the file was used, so the usual settings \
                         are in place.",
                        error.message()
                    )],
                );
            }
        };
        let mut problems: Vec<(usize, String)> = Vec::new();
        for (key, value) in document.get_ref() {
            let line = line_of(text, key.span().start);
            let name = key.get_ref().as_ref();
            let said = match ALL.iter().find(|entry| entry.name == name) {
                Some(entry) => read_one(entry, value.get_ref(), &mut settings),
                None if RETIRED.contains(&name) => None,
                None => Some(format!(
                    "`{name}` is not a setting, so the line was skipped. The settings are \
                     {}.",
                    listed(ALL.iter().map(|entry| entry.name), "and")
                )),
            };
            if let Some(said) = said {
                problems.push((line, said));
            }
        }
        // The reader hands lines over by name; a player reads top to bottom.
        problems.sort_by_key(|(line, _)| *line);
        let problems = problems
            .into_iter()
            .map(|(line, said)| format!("line {line}: {said}"))
            .collect();
        (settings, problems)
    }

    /// The text of a settings file holding these settings.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut text = String::from(
            "# Your settings for every game played with LORE.\n\
             # A game's settings menu writes this file. You can change a line here too,\n\
             # and save: games read it when they start.\n",
        );
        for entry in ALL {
            text.push('\n');
            let mut said = about(entry);
            if let Shape::Choice(words) = entry.shape {
                let _ = write!(said, " One of {}.", listed(words.iter().copied(), "or"));
            }
            for line in wrapped(&said) {
                // Writing to a `String` cannot fail.
                let _ = writeln!(text, "# {line}");
            }
            let _ = match (entry.get)(self) {
                Value::Number(number) => writeln!(text, "{} = {number}", entry.name),
                Value::Toggle(on) => writeln!(text, "{} = {on}", entry.name),
                Value::Choice(word) => writeln!(text, "{} = \"{word}\"", entry.name),
            };
        }
        text
    }
}

/// The sentences about a setting, as its entry's documentation wrote them
/// over one line or several.
fn about(entry: &Entry) -> String {
    let words: Vec<&str> = entry.about.split_whitespace().collect();
    words.join(" ")
}

/// `text` in lines short enough for a comment in the settings file.
fn wrapped(text: &str) -> Vec<String> {
    const WIDE: usize = 74;
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.len() + 1 + word.len() <= WIDE => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    lines
}

/// Names in backquotes as a sentence lists them: "`a`, `b` and `c`".
fn listed<'a>(names: impl Iterator<Item = &'a str>, last: &str) -> String {
    let names: Vec<&str> = names.collect();
    let mut said = String::new();
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            said.push_str(if index + 1 == names.len() { " " } else { ", " });
            if index + 1 == names.len() {
                said.push_str(last);
                said.push(' ');
            }
        }
        let _ = write!(said, "`{name}`");
    }
    said
}

/// A setting's number as a script reads it: 0.1 kept as an `f32` is a
/// hair off a tenth, and a script should see the tenth.
fn exactly(number: f32) -> f64 {
    (f64::from(number) * 1e4).round() / 1e4
}

/// A value as a script is told of it.
fn told(value: Value) -> SettingValue {
    match value {
        Value::Number(number) => SettingValue::Number(exactly(number)),
        Value::Toggle(on) => SettingValue::Toggle(on),
        Value::Choice(word) => SettingValue::Choice(word.to_owned()),
    }
}

/// Reads one line of a settings file into `settings`, and says what the
/// player should know if it could not be used as written.
fn read_one(entry: &Entry, value: &DeValue<'_>, settings: &mut Settings) -> Option<String> {
    let name = entry.name;
    let now = (entry.get)(settings);
    match (entry.shape, now) {
        (Shape::Number(range), Value::Number(usual)) => {
            let (read, said) = number(name, value, range, usual);
            (entry.set)(settings, Value::Number(read));
            said
        }
        (Shape::Limit(range), Value::Number(usual)) => {
            let (read, said) = limit_in_file(name, value, range, usual);
            (entry.set)(settings, Value::Number(read));
            said
        }
        (Shape::Toggle, _) => match value {
            DeValue::Boolean(flag) => {
                (entry.set)(settings, Value::Toggle(*flag));
                None
            }
            _ => Some(format!(
                "`{name}` must be `true` or `false`, so it stays `{}`.",
                matches!(now, Value::Toggle(true))
            )),
        },
        (Shape::Choice(words), Value::Choice(usual)) => {
            let typed = match value {
                DeValue::String(word) => Some(word.trim().to_lowercase().replace('_', "-")),
                _ => None,
            };
            match typed.and_then(|typed| words.iter().find(|word| **word == typed)) {
                Some(word) => {
                    (entry.set)(settings, Value::Choice(word));
                    None
                }
                None => Some(format!(
                    "`{name}` must be one of {}, in quotes, so it stays \"{usual}\".",
                    listed(words.iter().copied(), "or")
                )),
            }
        }
        // An entry's shape and what it keeps are of one kind.
        _ => None,
    }
}

/// Where the player's settings file is kept, whether or not it exists yet.
///
/// `None` when the computer does not say where settings go (no home folder).
#[must_use]
pub fn file() -> Option<PathBuf> {
    crate::controls::player_file(FILE_VARIABLE, "settings.toml")
}

/// The settings in the file at `path`, and what the player should be told
/// about the file; empty when all is well.
///
/// A file that is not there gives the usual settings and is not a problem:
/// the player has not changed anything yet.
#[must_use]
pub fn load(path: &Path) -> (Settings, Vec<String>) {
    match fs::read_to_string(path) {
        Ok(text) => {
            let (settings, problems) = Settings::read(&text);
            let named = problems
                .iter()
                .map(|problem| format!("{}, {problem}", path.display()))
                .collect();
            (settings, named)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (Settings::default(), Vec::new()),
        Err(error) => (
            Settings::default(),
            vec![format!(
                "Your settings in {} could not be read ({error}), so the usual settings are \
                 used. Check that the file is yours to read and is plain text.",
                path.display()
            )],
        ),
    }
}

/// Writes `settings` to the file at `path`, making its folder if needed.
///
/// The file is written beside itself and then moved into place, so a crash
/// or a full disk never leaves half a file where the player's settings were.
///
/// # Errors
///
/// Fails if the folder cannot be made or the file cannot be written.
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    crate::controls::write_whole(path, &settings.to_text())
}

/// The number a line of a settings file holds, if it holds one.
fn number_in(value: &DeValue<'_>) -> Option<f32> {
    let read = match value {
        DeValue::Integer(whole) => i64::from_str_radix(whole.as_str(), whole.radix())
            .ok()
            // Any whole number a setting has a use for is exact as `f32`.
            .map(|whole| {
                #[allow(clippy::cast_precision_loss)]
                let read = whole as f32;
                read
            }),
        DeValue::Float(float) => float.as_str().parse::<f32>().ok(),
        _ => None,
    };
    read.filter(|read| read.is_finite())
}

/// A limit set to `asked` while it is `now`, as [`Shape::Limit`] holds it.
/// 0 is no limit, and anything between that and the lowest limit becomes
/// the lowest, since frames slower than it are not held to; except that a
/// step down from the lowest limit itself is no limit, so a menu stepping
/// left reaches it, as a step up from no limit reaches the lowest.
fn limit(asked: f32, now: f32, range: Range) -> f32 {
    let asked = asked.clamp(0.0, range.highest);
    if asked > 0.0 && asked < range.lowest {
        if now > 0.0 && now <= range.lowest && asked < now {
            0.0
        } else {
            range.lowest
        }
    } else {
        asked
    }
}

/// Reads a limit setting that is `usual` unless the file says otherwise,
/// as [`number`] reads a number.
fn limit_in_file(
    name: &str,
    value: &DeValue<'_>,
    range: Range,
    usual: f32,
) -> (f32, Option<String>) {
    let can = format!(
        "0 for no limit, or from {} to {}",
        range.lowest, range.highest
    );
    let Some(read) = number_in(value) else {
        return (
            usual,
            Some(format!(
                "`{name}` must be {can}. It keeps its usual value, {usual}."
            )),
        );
    };
    // A file is not a step in a menu: whatever it says between 0 and the
    // lowest limit is the lowest.
    let held = limit(read, range.highest, range);
    let said =
        (held != read).then(|| format!("`{name}` is {can}, so {read} was changed to {held}."));
    (held, said)
}

/// Reads a number setting that is `usual` unless the file says otherwise.
/// Gives the number to use, and what the player should know if the file's
/// could not be used as written.
fn number(name: &str, value: &DeValue<'_>, range: Range, usual: f32) -> (f32, Option<String>) {
    let Range {
        lowest, highest, ..
    } = range;
    let Some(read) = number_in(value) else {
        return (
            usual,
            Some(format!(
                "`{name}` must be a number from {lowest} to {highest}. It keeps its usual \
                 value, {usual}."
            )),
        );
    };
    let held = read.clamp(lowest, highest);
    let said = (held != read).then(|| {
        format!("`{name}` goes from {lowest} to {highest}, so {read} was changed to {held}.")
    });
    (held, said)
}

/// What sound is multiplied by for a volume setting from 0 to 100.
fn gain(volume: f32) -> f32 {
    let share = (volume / 100.0).clamp(0.0, 1.0);
    share * share
}

/// The line, counted from 1, of the byte at `offset`.
fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset).unwrap_or(text).matches('\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_player_gets_the_games_own_feel() {
        let settings = Settings::default();
        assert_eq!(settings.gain(), 1.0);
        assert_eq!(settings.look(), [1.0, 1.0]);
        assert_eq!(settings.ui_scale(), 1.0);
        assert_eq!(settings.exposure_stops(), 0.0);
        assert_eq!(settings.frames_per_second(), 400);
        assert_eq!(settings.upscaling, Upscaling::default());
        assert_eq!(Settings::read(""), (settings, Vec::new()));
    }

    #[test]
    fn what_is_written_is_read_back_the_same() {
        let chosen = Settings {
            volume: 35.0,
            music_volume: 0.0,
            effects_volume: 80.0,
            sensitivity: 1.7,
            invert_look: true,
            rumble: 40.0,
            ui_size: 125.0,
            field_of_view: 95.0,
            brightness: 65.0,
            upscaling: Upscaling::UltraPerformance,
            vsync: false,
            frame_rate_limit: 144.0,
            reflections: false,
            light_shafts: false,
            lamp_shadows: LampShadows::High,
            bounce: Bounce::Low,
            ambient_occlusion: AmbientOcclusion::Low,
            contact_shadows: false,
            anisotropic_filtering: AnisotropicFiltering::X4,
            bloom: 50.0,
            vignette: 0.0,
            film_grain: 25.0,
            chromatic_aberration: 0.0,
            sharpening: 60.0,
        };
        let text = chosen.to_text();
        let (read, problems) = Settings::read(&text);
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(read, chosen, "{text}");
        assert_eq!(read.look()[1], -read.sensitivity);
        assert_eq!(read.ui_scale(), 1.25);
        assert_eq!(read.rumble_share(), 0.4);
        assert_eq!(read.look_shares().bloom, 0.5);
        assert_eq!(read.look_shares().grain, 0.25);
        assert!((read.sharpness() - 0.6).abs() < 1e-6);
        assert_eq!(Settings::default().rumble_share(), 1.0);
        assert_eq!(read.frames_per_second(), 144);
        assert!((read.exposure_stops() - 0.45).abs() < 1e-6);
        // Half way along the slider is a quarter of the signal.
        let half = Settings {
            volume: 50.0,
            ..chosen
        };
        assert_eq!(half.gain(), 0.25);
        // Music and effects are squared the same way, each by itself.
        assert_eq!(half.music_gain(), 0.0);
        assert!((half.effects_gain() - 0.64).abs() < 1e-6);
        assert_eq!(Settings::default().music_gain(), 1.0);
    }

    #[test]
    fn the_file_says_what_each_setting_is_in_lines_that_fit() {
        let text = Settings::default().to_text();
        assert!(
            text.contains(
                "\n# How loud everything is, from 0 (silent) to 100 (full).\nvolume = 100\n"
            ),
            "{text}"
        );
        assert!(text.contains("\nsensitivity = 1\n"), "{text}");
        assert!(
            text.contains(
                "softer. One of `off`, `native`, `quality`,\n\
                 # `balanced`, `performance` or `ultra-performance`.\nupscaling = \"quality\"\n"
            ),
            "{text}"
        );
        for line in text.lines() {
            assert!(line.len() <= 76, "{line}");
        }
        // Every setting is in it once, in the order a menu has them.
        let names: Vec<&str> = text
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split_once(" = "))
            .map(|(name, _)| name)
            .collect();
        let all: Vec<&str> = ALL.iter().map(|entry| entry.name).collect();
        assert_eq!(names, all);
    }

    #[test]
    fn a_line_that_cannot_be_used_is_reported_and_the_rest_still_counts() {
        let (read, problems) = Settings::read(
            "volume = \"loud\"\nsensitivity = 9\nfov = 100\ninvert_look = 1\nui_size = 90\n\
             upscaling = \"best\"\nvsync = \"no\"\nfield_of_view = 95.5\n",
        );
        assert_eq!(
            read,
            Settings {
                sensitivity: 3.0,
                ui_size: 90.0,
                field_of_view: 95.5,
                ..Settings::default()
            }
        );
        assert_eq!(
            problems,
            [
                "line 1: `volume` must be a number from 0 to 100. It keeps its usual value, 100.",
                "line 2: `sensitivity` goes from 0.1 to 3, so 9 was changed to 3.",
                "line 3: `fov` is not a setting, so the line was skipped. The settings are \
                 `volume`, `music_volume`, `effects_volume`, `sensitivity`, `invert_look`, \
                 `rumble`, `ui_size`, `field_of_view`, `brightness`, `upscaling`, `vsync`, \
                 `frame_rate_limit`, `reflections`, `light_shafts`, `lamp_shadows`, `bounce`, \
                 `ambient_occlusion`, `contact_shadows`, `anisotropic_filtering`, `bloom`, \
                 `vignette`, `film_grain`, `chromatic_aberration` and `sharpening`.",
                "line 4: `invert_look` must be `true` or `false`, so it stays `false`.",
                "line 6: `upscaling` must be one of `off`, `native`, `quality`, `balanced`, \
                 `performance` or `ultra-performance`, in quotes, so it stays \"quality\".",
                "line 7: `vsync` must be `true` or `false`, so it stays `true`.",
            ]
        );
    }

    #[test]
    fn a_file_from_before_the_frame_meter_went_is_read_without_a_word() {
        let (read, problems) = Settings::read("volume = 40\nframe_meter = true\n");
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(read.volume, 40.0);
        assert!(!read.to_text().contains("frame_meter"));
    }

    #[test]
    fn an_upscaling_is_written_as_players_know_it_and_read_however_it_is_typed() {
        let (read, problems) = Settings::read("upscaling = \" Ultra_Performance\"\n");
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(read.upscaling, Upscaling::UltraPerformance);
        assert_eq!(upscaling_name(Upscaling::Native), "native");
    }

    #[test]
    fn a_script_is_told_every_setting_and_what_it_changes_is_taken() {
        let mut settings = Settings {
            sensitivity: 0.1,
            ..Settings::default()
        };
        let told = settings.described();
        assert_eq!(told.len(), ALL.len());
        let named = |name: &str| told.iter().find(|setting| setting.name == name).unwrap();
        // A tenth is a tenth, not the hair off it that an `f32` keeps.
        assert_eq!(named("sensitivity").value, SettingValue::Number(0.1));
        assert_eq!(named("sensitivity").default, SettingValue::Number(1.0));
        assert_eq!(
            named("sensitivity").kind,
            SettingKind::Number {
                min: 0.1,
                max: 3.0,
                step: 0.1
            }
        );
        assert_eq!(named("vsync").kind, SettingKind::Toggle);
        assert_eq!(named("vsync").group, "Graphics");
        assert_eq!(
            named("upscaling").value,
            SettingValue::Choice("quality".to_owned())
        );
        assert_eq!(
            named("music_volume").description,
            "How loud music is, from 0 (off) to 100 (full). It is turned down by `volume` as \
             well."
        );

        settings.change("upscaling", &SettingValue::Choice("performance".to_owned()));
        settings.change("vsync", &SettingValue::Toggle(false));
        settings.change("field_of_view", &SettingValue::Number(500.0));
        assert_eq!(settings.upscaling, Upscaling::Performance);
        assert!(!settings.vsync);
        assert_eq!(settings.field_of_view, 110.0);
        // What cannot be meant changes nothing.
        let before = settings;
        settings.change("upscaling", &SettingValue::Choice("best".to_owned()));
        settings.change("vsync", &SettingValue::Number(1.0));
        settings.change("volume", &SettingValue::Number(f64::NAN));
        settings.change("frame_meter", &SettingValue::Toggle(true));
        assert_eq!(settings, before);
    }

    #[test]
    fn a_frame_rate_limit_of_0_is_no_limit_and_one_below_30_is_30() {
        let (read, problems) = Settings::read("frame_rate_limit = 0\n");
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(read.frames_per_second(), 0);
        let (read, problems) = Settings::read("frame_rate_limit = 20\n");
        assert_eq!(read.frames_per_second(), 30);
        assert_eq!(
            problems,
            [
                "line 1: `frame_rate_limit` is 0 for no limit, or from 30 to 1000, so 20 was \
              changed to 30."
            ]
        );
        // The fastest displays are not held back.
        let (read, problems) = Settings::read("frame_rate_limit = 480\n");
        assert_eq!((read.frames_per_second(), problems), (480, Vec::new()));
        assert_eq!(
            Settings::read("frame_rate_limit = 5000\n")
                .0
                .frame_rate_limit,
            1000.0
        );

        // A script is told it goes from 0, and said what 0 is.
        let told = Settings::default().described();
        let limit = told
            .iter()
            .find(|setting| setting.name == "frame_rate_limit")
            .unwrap();
        assert_eq!(
            limit.kind,
            SettingKind::Number {
                min: 0.0,
                max: 1000.0,
                step: 10.0
            }
        );
        assert!(
            limit.description.contains("or 0 for no limit"),
            "{}",
            limit.description
        );

        // A menu steps down from 30 to no limit, and back up to 30; a
        // script that asks for 20 from higher up gets the lowest limit.
        let mut settings = Settings {
            frame_rate_limit: 30.0,
            reflections: false,
            ..Settings::default()
        };
        settings.change("frame_rate_limit", &SettingValue::Number(20.0));
        assert_eq!(settings.frame_rate_limit, 0.0);
        settings.change("frame_rate_limit", &SettingValue::Number(10.0));
        assert_eq!(settings.frame_rate_limit, 30.0);
        settings.change("frame_rate_limit", &SettingValue::Number(240.0));
        settings.change("frame_rate_limit", &SettingValue::Number(20.0));
        assert_eq!(settings.frame_rate_limit, 30.0);
        settings.change("frame_rate_limit", &SettingValue::Number(0.0));
        assert_eq!(settings.frames_per_second(), 0);
    }

    #[test]
    fn a_file_that_is_not_toml_changes_nothing_and_says_where() {
        let (read, problems) = Settings::read("volume = 40\nsensitivity = = 2\n");
        assert_eq!(read, Settings::default());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("line 2: "), "{}", problems[0]);
        assert!(problems[0].ends_with("the usual settings are in place."));
    }

    #[test]
    fn settings_are_saved_to_a_file_and_loaded_from_it() {
        let folder =
            std::env::temp_dir().join(format!("lore-settings-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let path = folder.join("deeper").join("settings.toml");

        // No file yet: nothing to say.
        assert_eq!(load(&path), (Settings::default(), Vec::new()));

        let chosen = Settings {
            volume: 0.0,
            invert_look: true,
            ..Settings::default()
        };
        save(&path, &chosen).unwrap();
        assert_eq!(load(&path), (chosen, Vec::new()));
        assert!(!path.with_extension("toml.part").exists());

        fs::write(&path, "volume = 500\n").unwrap();
        let (read, problems) = load(&path);
        assert_eq!(read.volume, 100.0);
        assert_eq!(
            problems,
            [format!(
                "{}, line 1: `volume` goes from 0 to 100, so 500 was changed to 100.",
                path.display()
            )]
        );
        let _ = fs::remove_dir_all(&folder);
    }
}
