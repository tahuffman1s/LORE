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
pub(crate) const CAMERA_FUNCTIONS: &[&str] = &["fixed", "follow", "reset", "shot"];

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
}

impl ShotSaid {
    /// The shot's name, as `game.camera.shot()` gives it.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Fixed { .. } => "fixed",
            Self::Follow { .. } => "follow",
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

    fn say(&self, shot: ShotSaid, seconds: f32, fov: Option<f32>) {
        let now = self.said.get();
        // The same shot said again, as a script may every tick, is not a
        // new one: its travel does not start over.
        if (now.shot, now.seconds, now.fov) == (shot, seconds, fov) && now.said > 0 {
            return;
        }
        let said = now.said.wrapping_add(1);
        self.said.set(CameraSaid {
            shot,
            seconds,
            fov,
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
            "distance" => {
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
                state
                    .camera
                    .say(ShotSaid::Fixed { from, at }, read.seconds, read.fov);
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
                read.seconds,
                read.fov,
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
            state.camera.say(ShotSaid::Player, seconds, None);
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
