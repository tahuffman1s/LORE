//! Where the world is seen from, for scripts: `game.camera`.
//!
//! The view is the player's own until a script says otherwise: from their
//! eyes or over their shoulder (`game.player:setView`). A script can put it
//! somewhere else with one line, a *shot*, and give it back with another:
//!
//! ```luau
//! game.camera.fixed(vector.create(0, 6, 12), game.player)   -- watch from a corner
//! game.camera.follow(guard, { distance = 5, seconds = 1 })  -- trail someone
//! game.camera.reset(1)                                      -- back to the player
//! ```
//!
//! Every shot can travel there over some seconds instead of cutting. What
//! the script says lives in [`CameraState`], outside the script's VM, so a
//! reload keeps it. A shot changes only what is drawn, so any part of a
//! script may say it, and the simulation never reads where the camera is.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{Lua, Table, Value};

use crate::CharacterId;
use crate::api::{Shared, describe, fail};
use crate::character_api::id_of_value;
use crate::error::closest;
use crate::weather_api::number;

/// Every function on `game.camera`.
pub(crate) const CAMERA_FUNCTIONS: &[&str] = &[
    "fixed", "follow", "topDown", "sideOn", "orbit", "path", "reset", "shot",
];

/// The most places a path shot goes through.
pub const MOST_PATH_POINTS: usize = 16;

/// The longest a shot may take to travel to, in seconds.
const LONGEST_TRAVEL: f32 = 600.0;
/// The fields of view a shot may have, in degrees from the bottom of the
/// picture to the top.
const FOV_RANGE: (f32, f32) = (10.0, 120.0);
/// How far behind a followed character the camera trails, by default, in metres.
const FOLLOW_DISTANCE: f32 = 4.0;
/// How far above a followed character's feet the camera rides, by default, in metres.
const FOLLOW_HEIGHT: f32 = 2.2;

/// What a shot looks at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CameraTarget {
    /// A place in the world, in metres.
    Point([f32; 3]),
    /// A character, wherever they go.
    Character(CharacterId),
}

/// Where the world is seen from.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ShotSaid {
    /// The player's own view: their eyes, or over their shoulder.
    #[default]
    Player,
    /// A camera that stays where it is put and looks at something.
    Fixed {
        /// Where the camera is, in metres.
        from: [f32; 3],
        /// What it looks at.
        at: CameraTarget,
    },
    /// A camera that trails a character, looking at them.
    Follow {
        /// Who it trails.
        who: CharacterId,
        /// How far from them it stays, in metres.
        distance: f32,
        /// How far above their feet it rides, in metres.
        height: f32,
    },
    /// A camera straight above a character, looking down, north (-Z) up.
    TopDown {
        /// Who it stays over.
        who: CharacterId,
        /// How far above them it is, in metres.
        height: f32,
    },
    /// A camera level with a character and to one side, as a
    /// side-scroller's is: it looks along `angle` and never turns.
    SideOn {
        /// Who it keeps in the middle of the picture.
        who: CharacterId,
        /// How far from them it is, in metres.
        distance: f32,
        /// How far above their chest it is, in metres.
        height: f32,
        /// Which way it looks, in radians turning left from -Z.
        angle: f32,
    },
    /// A camera that circles something, looking at it.
    Orbit {
        /// What it circles.
        about: CameraTarget,
        /// How far from it, in metres.
        distance: f32,
        /// How far above it, in metres.
        height: f32,
        /// How fast it goes round to the left, in radians a second.
        speed: f32,
    },
    /// A camera that travels through a list of places.
    Path {
        /// The places, in order; only the first `count` are used.
        points: [[f32; 3]; MOST_PATH_POINTS],
        /// How many places there are, at least 2.
        count: u8,
        /// What it looks at; `None` looks the way it is going.
        at: Option<CameraTarget>,
        /// How many seconds the whole path takes.
        over: f32,
        /// Whether it starts again at the end.
        looping: bool,
    },
}

impl ShotSaid {
    /// The shot's name, as `game.camera.shot()` gives it.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Fixed { .. } => "fixed",
            Self::Follow { .. } => "follow",
            Self::TopDown { .. } => "topDown",
            Self::SideOn { .. } => "sideOn",
            Self::Orbit { .. } => "orbit",
            Self::Path { .. } => "path",
        }
    }
}

/// The view a script asked for, as the engine reads it each frame.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CameraSaid {
    /// The shot.
    pub shot: ShotSaid,
    /// How many seconds the view takes to travel to the shot; 0 cuts.
    pub seconds: f32,
    /// The shot's own field of view in degrees, or `None` for the player's.
    pub fov: Option<f32>,
    /// Whether the player walks by the shot: forward is away from the
    /// camera, whichever way they themselves face.
    pub steer: bool,
    /// Goes up by one each time a script says a shot, so the engine can
    /// tell a shot said again from one that has not changed.
    pub said: u32,
}

/// What the script has said of the camera. It outlives a version of the
/// script.
#[derive(Default)]
pub(crate) struct CameraState {
    said: Cell<CameraSaid>,
}

impl CameraState {
    pub(crate) fn get(&self) -> CameraSaid {
        self.said.get()
    }

    fn say(&self, shot: ShotSaid, read: &Options) {
        let now = self.said.get();
        let (seconds, fov, steer) = (read.seconds, read.fov, read.steer);
        // The same shot said again, as a script may every tick, is not a
        // new one: its travel does not start over.
        if (now.shot, now.seconds, now.fov, now.steer) == (shot, seconds, fov, steer)
            && now.said > 0
        {
            return;
        }
        let said = now.said.wrapping_add(1);
        self.said.set(CameraSaid {
            shot,
            seconds,
            fov,
            steer,
            said,
        });
    }
}

/// The settings a shot was given in its last argument.
struct Options {
    seconds: f32,
    fov: Option<f32>,
    distance: f32,
    height: f32,
    /// Whether the script gave a distance or a height, or they are the
    /// follow shot's own.
    said_distance: bool,
    said_height: bool,
    steer: bool,
    /// Degrees: which way a side-on shot looks, how fast an orbit turns.
    angle: f32,
    speed: f32,
    /// A path's seconds from end to end, and whether it goes round again.
    over: f32,
    looping: bool,
    /// What a path looks at.
    at: Option<CameraTarget>,
}

/// Reads a shot's options table: any of `allowed`, each a number.
fn options(
    lua: &Lua,
    function: &str,
    value: Option<&Value>,
    allowed: &[&str],
    example: &str,
) -> mlua::Result<Options> {
    let mut read = Options {
        seconds: 0.0,
        fov: None,
        distance: FOLLOW_DISTANCE,
        height: FOLLOW_HEIGHT,
        said_distance: false,
        said_height: false,
        steer: false,
        angle: 90.0,
        speed: 20.0,
        over: 5.0,
        looping: false,
        at: None,
    };
    let settings = match value {
        None | Some(Value::Nil) => return Ok(read),
        Some(Value::Table(settings)) => settings,
        Some(other) => {
            return Err(fail(
                lua,
                format!(
                    "{function} takes its settings last, in a table, but it was given {}. \
                     Write it like this: {example}",
                    describe(other)
                ),
            ));
        }
    };
    for pair in settings.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = &key else {
            return Err(fail(
                lua,
                format!(
                    "{function}'s settings are named, but one was written as {}. Write it \
                     like this: {example}",
                    describe(&key)
                ),
            ));
        };
        let key = key.to_string_lossy();
        if !allowed.contains(&key.as_str()) {
            let mut message = format!("{function} has no setting \"{key}\".");
            if let Some(near) = closest(&key, allowed.iter().copied()) {
                message.push_str(&format!(" Did you mean \"{near}\"?"));
            }
            message.push_str(&format!(
                " It takes {}. Write it like this: {example}",
                allowed.join(", ")
            ));
            return Err(fail(lua, message));
        }
        let label = format!("{function}'s `{key}`");
        match key.as_str() {
            "seconds" => {
                read.seconds = number(
                    lua,
                    &label,
                    Some(&value),
                    "how many seconds the view takes to travel there; 0 cuts",
                    example,
                )?
                .clamp(0.0, LONGEST_TRAVEL);
            }
            "fov" => {
                read.fov = Some(
                    number(
                        lua,
                        &label,
                        Some(&value),
                        "how many degrees are seen from the bottom of the picture to the top, \
                         from 10 to 120",
                        example,
                    )?
                    .clamp(FOV_RANGE.0, FOV_RANGE.1),
                );
            }
            "steer" | "loop" => {
                let Value::Boolean(on) = value else {
                    return Err(fail(
                        lua,
                        format!(
                            "{label} is true or false, but it was given {}. Write it like \
                             this: {example}",
                            describe(&value)
                        ),
                    ));
                };
                if key == "steer" {
                    read.steer = on;
                } else {
                    read.looping = on;
                }
            }
            "angle" => {
                read.angle = number(
                    lua,
                    &label,
                    Some(&value),
                    "which way the camera looks, in degrees to the left of north (-Z): 90 \
                     looks west, along -X",
                    example,
                )?;
            }
            "speed" => {
                read.speed = number(
                    lua,
                    &label,
                    Some(&value),
                    "how fast the camera goes round, in degrees a second to the left (below \
                     0, to the right), from -360 to 360",
                    example,
                )?
                .clamp(-360.0, 360.0);
            }
            "over" => {
                read.over = number(
                    lua,
                    &label,
                    Some(&value),
                    "how many seconds the whole path takes, from 0.1 to 3600",
                    example,
                )?
                .clamp(0.1, 3600.0);
            }
            "at" => {
                read.at = Some(match &value {
                    Value::Vector(point) => CameraTarget::Point([point.x(), point.y(), point.z()]),
                    other => match id_of_value(lua, other) {
                        Some(who) => CameraTarget::Character(who),
                        None => {
                            return Err(fail(
                                lua,
                                format!(
                                    "{label} is what the camera looks at: a place as a \
                                     vector, or a character such as game.player. It was \
                                     given {}. Write it like this: {example}",
                                    describe(other)
                                ),
                            ));
                        }
                    },
                });
            }
            "distance" => {
                read.said_distance = true;
                read.distance = number(
                    lua,
                    &label,
                    Some(&value),
                    "how many metres from the character the camera stays, from 0.5 to 100",
                    example,
                )?
                .clamp(0.5, 100.0);
            }
            "height" => {
                read.said_height = true;
                read.height = number(
                    lua,
                    &label,
                    Some(&value),
                    "how many metres above the character's feet the camera rides, from -50 \
                     to 100",
                    example,
                )?
                .clamp(-50.0, 100.0);
            }
            // `allowed` holds only the names above.
            _ => {}
        }
    }
    Ok(read)
}

/// A character a script named, who must still be in the world.
fn character(
    lua: &Lua,
    shared: &Shared,
    function: &str,
    value: &Value,
    example: &str,
) -> mlua::Result<Option<CharacterId>> {
    let Some(id) = id_of_value(lua, value) else {
        return Ok(None);
    };
    if shared.characters.has(id) {
        Ok(Some(id))
    } else {
        Err(fail(
            lua,
            format!(
                "{function} was given a character who is no longer in the world. Give it one \
                 who is, such as game.player. Write it like this: {example}"
            ),
        ))
    }
}

/// `game.camera`.
pub(crate) fn camera(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let camera = lua.create_table()?;

    let state = Rc::clone(shared);
    camera.set(
        "fixed",
        lua.create_function(
            move |lua, (from, at, settings): (Value, Value, Option<Value>)| {
                const EXAMPLE: &str =
                    "game.camera.fixed(vector.create(0, 6, 12), game.player, { seconds = 1 })";
                let Value::Vector(from) = from else {
                    return Err(fail(
                        lua,
                        format!(
                            "game.camera.fixed needs first where the camera goes, as a vector in \
                         metres, but it was given {}. Write it like this: {EXAMPLE}",
                            describe(&from)
                        ),
                    ));
                };
                let at = match &at {
                    Value::Vector(point) => CameraTarget::Point([point.x(), point.y(), point.z()]),
                    other => match character(lua, &state, "game.camera.fixed", other, EXAMPLE)? {
                        Some(who) => CameraTarget::Character(who),
                        None => {
                            return Err(fail(
                                lua,
                                format!(
                                    "game.camera.fixed needs, after the camera's place, what the camera looks at: a \
                                 place as a vector, or a character such as game.player. It was \
                                 given {}. Write it like this: {EXAMPLE}",
                                    describe(other)
                                ),
                            ));
                        }
                    },
                };
                let from = [from.x(), from.y(), from.z()];
                let aimed = match at {
                    CameraTarget::Point(point) => point,
                    CameraTarget::Character(_) => [0.0; 3],
                };
                if from.iter().chain(&aimed).any(|part| !part.is_finite()) {
                    return Err(fail(
                        lua,
                        format!(
                            "game.camera.fixed was given a place with no size: infinity, or the \
                         answer to a sum such as 0 / 0. Write it like this: {EXAMPLE}"
                        ),
                    ));
                }
                let read = options(
                    lua,
                    "game.camera.fixed",
                    settings.as_ref(),
                    &["seconds", "fov"],
                    EXAMPLE,
                )?;
                state.camera.say(ShotSaid::Fixed { from, at }, &read);
                Ok(())
            },
        )?,
    )?;

    let state = Rc::clone(shared);
    camera.set(
        "follow",
        lua.create_function(move |lua, (who, settings): (Value, Option<Value>)| {
            const EXAMPLE: &str = "game.camera.follow(game.player, { distance = 6, height = 3 })";
            let Some(who) = character(lua, &state, "game.camera.follow", &who, EXAMPLE)? else {
                return Err(fail(
                    lua,
                    format!(
                        "game.camera.follow needs the character to trail, such as game.player, \
                         but it was given {}. Write it like this: {EXAMPLE}",
                        describe(&who)
                    ),
                ));
            };
            let read = options(
                lua,
                "game.camera.follow",
                settings.as_ref(),
                &["distance", "height", "seconds", "fov"],
                EXAMPLE,
            )?;
            state.camera.say(
                ShotSaid::Follow {
                    who,
                    distance: read.distance,
                    height: read.height,
                },
                &read,
            );
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    camera.set(
        "topDown",
        lua.create_function(move |lua, (who, settings): (Value, Option<Value>)| {
            const EXAMPLE: &str = "game.camera.topDown(game.player, { height = 14, steer = true })";
            let Some(who) = character(lua, &state, "game.camera.topDown", &who, EXAMPLE)? else {
                return Err(fail(
                    lua,
                    format!(
                        "game.camera.topDown needs the character to stay over, such as \
                         game.player, but it was given {}. Write it like this: {EXAMPLE}",
                        describe(&who)
                    ),
                ));
            };
            let read = options(
                lua,
                "game.camera.topDown",
                settings.as_ref(),
                &["height", "seconds", "fov", "steer"],
                EXAMPLE,
            )?;
            let height = if read.said_height {
                read.height.max(1.0)
            } else {
                14.0
            };
            state.camera.say(ShotSaid::TopDown { who, height }, &read);
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    camera.set(
        "sideOn",
        lua.create_function(move |lua, (who, settings): (Value, Option<Value>)| {
            const EXAMPLE: &str =
                "game.camera.sideOn(game.player, { distance = 10, angle = 90, steer = true })";
            let Some(who) = character(lua, &state, "game.camera.sideOn", &who, EXAMPLE)? else {
                return Err(fail(
                    lua,
                    format!(
                        "game.camera.sideOn needs the character to keep in the middle of the \
                         picture, such as game.player, but it was given {}. Write it like \
                         this: {EXAMPLE}",
                        describe(&who)
                    ),
                ));
            };
            let read = options(
                lua,
                "game.camera.sideOn",
                settings.as_ref(),
                &["distance", "height", "angle", "seconds", "fov", "steer"],
                EXAMPLE,
            )?;
            state.camera.say(
                ShotSaid::SideOn {
                    who,
                    distance: if read.said_distance {
                        read.distance
                    } else {
                        10.0
                    },
                    height: if read.said_height { read.height } else { 0.5 },
                    angle: read.angle.to_radians(),
                },
                &read,
            );
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    camera.set(
        "orbit",
        lua.create_function(move |lua, (about, settings): (Value, Option<Value>)| {
            const EXAMPLE: &str =
                "game.camera.orbit(game.player, { distance = 6, height = 2, speed = 20 })";
            let about = match &about {
                Value::Vector(point) => CameraTarget::Point([point.x(), point.y(), point.z()]),
                other => match character(lua, &state, "game.camera.orbit", other, EXAMPLE)? {
                    Some(who) => CameraTarget::Character(who),
                    None => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.camera.orbit needs what the camera circles: a place as \
                                 a vector, or a character such as game.player. It was given \
                                 {}. Write it like this: {EXAMPLE}",
                                describe(other)
                            ),
                        ));
                    }
                },
            };
            let read = options(
                lua,
                "game.camera.orbit",
                settings.as_ref(),
                &["distance", "height", "speed", "seconds", "fov"],
                EXAMPLE,
            )?;
            state.camera.say(
                ShotSaid::Orbit {
                    about,
                    distance: if read.said_distance {
                        read.distance
                    } else {
                        6.0
                    },
                    height: if read.said_height { read.height } else { 2.0 },
                    speed: read.speed.to_radians(),
                },
                &read,
            );
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    camera.set(
        "path",
        lua.create_function(move |lua, (places, settings): (Value, Option<Value>)| {
            const EXAMPLE: &str = "game.camera.path({ vector.create(0, 3, 10), \
                                   vector.create(8, 3, 0), vector.create(0, 3, -10) }, { over \
                                   = 6, at = game.player })";
            let Value::Table(list) = &places else {
                return Err(fail(
                    lua,
                    format!(
                        "game.camera.path needs the places the camera goes through, in a \
                         list: vectors, or things placed in the map. It was given {}. Write \
                         it like this: {EXAMPLE}",
                        describe(&places)
                    ),
                ));
            };
            let mut points = [[0.0; 3]; MOST_PATH_POINTS];
            let mut count = 0usize;
            for place in list.sequence_values::<Value>() {
                let place = place?;
                let at = match &place {
                    Value::Vector(at) => [at.x(), at.y(), at.z()],
                    // A thing placed in the map: a marker put where the
                    // camera should pass, in `lore edit`.
                    Value::UserData(thing) => match crate::thing_api::place_of(&state, thing) {
                        Some(at) => at,
                        None => {
                            return Err(fail(
                                lua,
                                format!(
                                    "game.camera.path's places are vectors or things placed \
                                     in the map. Write it like this: {EXAMPLE}"
                                ),
                            ));
                        }
                    },
                    other => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.camera.path's places are vectors or things placed in \
                                 the map, but one was {}. Write it like this: {EXAMPLE}",
                                describe(other)
                            ),
                        ));
                    }
                };
                if !at.iter().all(|part| part.is_finite()) {
                    continue;
                }
                if count == MOST_PATH_POINTS {
                    return Err(fail(
                        lua,
                        format!(
                            "game.camera.path takes up to {MOST_PATH_POINTS} places, and was \
                             given more. Leave some out: the camera curves smoothly between \
                             those it has."
                        ),
                    ));
                }
                points[count] = at;
                count += 1;
            }
            if count < 2 {
                return Err(fail(
                    lua,
                    format!(
                        "game.camera.path needs at least two places to travel between, and \
                         was given {count}. Write it like this: {EXAMPLE}"
                    ),
                ));
            }
            let read = options(
                lua,
                "game.camera.path",
                settings.as_ref(),
                &["over", "at", "loop", "seconds", "fov"],
                EXAMPLE,
            )?;
            // No more than sixteen, so it fits.
            #[allow(clippy::cast_possible_truncation)]
            let count = count as u8;
            state.camera.say(
                ShotSaid::Path {
                    points,
                    count,
                    at: read.at,
                    over: read.over,
                    looping: read.looping,
                },
                &read,
            );
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    camera.set(
        "reset",
        lua.create_function(move |lua, seconds: Option<Value>| {
            let seconds = match &seconds {
                None | Some(Value::Nil) => 0.0,
                Some(value) => number(
                    lua,
                    "game.camera.reset",
                    Some(value),
                    "how many seconds the view takes to travel back to the player; nothing \
                     or 0 cuts",
                    "game.camera.reset(1)",
                )?
                .clamp(0.0, LONGEST_TRAVEL),
            };
            let read = Options {
                seconds,
                ..options(lua, "game.camera.reset", None, &[], "game.camera.reset(1)")?
            };
            state.camera.say(ShotSaid::Player, &read);
            Ok(())
        })?,
    )?;

    let state = Rc::clone(shared);
    camera.set(
        "shot",
        lua.create_function(move |_, ()| Ok(state.camera.get().shot.name()))?,
    )?;

    Ok(camera)
}

#[cfg(test)]
mod tests {
    use lore_input::ActionMap;

    use super::*;
    use crate::ScriptHost;

    fn host(script: &str) -> ScriptHost {
        ScriptHost::from_source(script, ActionMap::default())
    }

    #[test]
    fn the_view_is_the_players_until_a_script_says_a_shot() {
        let host = host("");
        assert_eq!(host.camera(), CameraSaid::default());
        assert_eq!(host.camera().shot, ShotSaid::Player);
    }

    #[test]
    fn a_fixed_shot_looks_at_a_place_or_a_character() {
        let host = host(
            "game.camera.fixed(vector.create(1, 2, 3), vector.create(0, 1, 0))\n\
             assert(game.camera.shot() == \"fixed\")",
        );
        assert_eq!(host.last_error(), None);
        let said = host.camera();
        assert_eq!(
            said.shot,
            ShotSaid::Fixed {
                from: [1.0, 2.0, 3.0],
                at: CameraTarget::Point([0.0, 1.0, 0.0]),
            }
        );
        assert_eq!((said.seconds, said.fov), (0.0, None));

        let host = self::host(
            "game.camera.fixed(vector.create(1, 2, 3), game.player, { seconds = 2, fov = 40 })",
        );
        assert_eq!(host.last_error(), None);
        let said = host.camera();
        assert_eq!(
            said.shot,
            ShotSaid::Fixed {
                from: [1.0, 2.0, 3.0],
                at: CameraTarget::Character(CharacterId::PLAYER),
            }
        );
        assert_eq!((said.seconds, said.fov), (2.0, Some(40.0)));
    }

    #[test]
    fn a_follow_shot_trails_a_character_and_reset_gives_the_view_back() {
        let host = host(
            "game.camera.follow(game.player, { distance = 6 })\n\
             assert(game.camera.shot() == \"follow\")",
        );
        assert_eq!(host.last_error(), None);
        let first = host.camera();
        assert_eq!(
            first.shot,
            ShotSaid::Follow {
                who: CharacterId::PLAYER,
                distance: 6.0,
                height: FOLLOW_HEIGHT,
            }
        );

        let host = self::host(
            "game.camera.follow(game.player)\ngame.camera.reset(1.5)\n\
             assert(game.camera.shot() == \"player\")",
        );
        assert_eq!(host.last_error(), None);
        let said = host.camera();
        assert_eq!((said.shot, said.seconds), (ShotSaid::Player, 1.5));
        // Each line was a shot of its own.
        assert_eq!(said.said, 2);
    }

    #[test]
    fn the_other_shots_are_said_with_their_own_settings() {
        let host = self::host(
            "game.camera.topDown(game.player)\nassert(game.camera.shot() == \"topDown\")",
        );
        assert_eq!(host.last_error(), None);
        assert_eq!(
            host.camera().shot,
            ShotSaid::TopDown {
                who: CharacterId::PLAYER,
                height: 14.0,
            }
        );
        assert!(!host.camera().steer);

        let host = self::host(
            "game.camera.sideOn(game.player, { distance = 8, angle = 180, steer = true })",
        );
        assert_eq!(host.last_error(), None);
        let said = host.camera();
        assert!(said.steer);
        let ShotSaid::SideOn {
            distance,
            height,
            angle,
            ..
        } = said.shot
        else {
            panic!("{:?}", said.shot);
        };
        assert_eq!((distance, height), (8.0, 0.5));
        assert!((angle - std::f32::consts::PI).abs() < 1e-5);

        let host = self::host("game.camera.orbit(vector.create(1, 2, 3), { speed = 90 })");
        assert_eq!(host.last_error(), None);
        let ShotSaid::Orbit {
            about,
            distance,
            speed,
            ..
        } = host.camera().shot
        else {
            panic!();
        };
        assert_eq!(about, CameraTarget::Point([1.0, 2.0, 3.0]));
        assert_eq!(distance, 6.0);
        assert!((speed - std::f32::consts::FRAC_PI_2).abs() < 1e-5);

        let host = self::host(
            "game.camera.path({ vector.create(0, 3, 10), vector.create(8, 3, 0) }, \
             { over = 6, at = game.player, loop = true })\n\
             assert(game.camera.shot() == \"path\")",
        );
        assert_eq!(host.last_error(), None);
        let ShotSaid::Path {
            points,
            count,
            at,
            over,
            looping,
        } = host.camera().shot
        else {
            panic!();
        };
        assert_eq!((count, over, looping), (2, 6.0, true));
        assert_eq!(points[1], [8.0, 3.0, 0.0]);
        assert_eq!(at, Some(CameraTarget::Character(CharacterId::PLAYER)));

        let message = |script: &str| self::host(script).last_error().unwrap().message.clone();
        let said = message("game.camera.path({ vector.create(0, 0, 0) })");
        assert!(said.contains("at least two places"), "{said}");
        let said = message("game.camera.path(\"round\")");
        assert!(said.contains("in a list"), "{said}");
        let said = message("game.camera.topDown(game.player, { steer = 1 })");
        assert!(said.contains("is true or false"), "{said}");
        let said = message("game.camera.orbit(7)");
        assert!(said.contains("what the camera circles"), "{said}");
        let said = message("game.camera.sideOn(game.player, { hieght = 2 })");
        assert!(said.contains("Did you mean \"height\"?"), "{said}");
    }

    #[test]
    fn mistakes_say_what_to_write() {
        let message = |script: &str| host(script).last_error().unwrap().message.clone();

        let said = message("game.camera.fixed(1, 2)");
        assert!(said.contains("as a vector in metres"), "{said}");
        assert!(said.contains("game.camera.fixed(vector.create"), "{said}");

        let said = message("game.camera.fixed(vector.create(0, 0, 0), \"door\")");
        assert!(said.contains("what the camera looks at"), "{said}");

        let said = message("game.camera.follow(game.player, { distanse = 3 })");
        assert!(said.contains("Did you mean \"distance\"?"), "{said}");

        let said = message("game.camera.follow(nil)");
        assert!(said.contains("the character to trail"), "{said}");

        let said = message("game.camera.reset(\"slowly\")");
        assert!(said.contains("needs a number"), "{said}");

        let said = message("game.camera.fixed(vector.create(0, 1, 0), game.player, { fov = {} })");
        assert!(said.contains("`fov` needs a number"), "{said}");
    }
}
