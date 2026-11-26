//! `game.canWalk` and `game.findWay`: whether a person can walk from one
//! place to another in the level, and the corners of the way, over the
//! walkable ground the engine works out from the level (`lore-nav`).
//!
//! The world hands the host its ground once a level is in place
//! ([`ScriptHost::set_walking_ground`](crate::ScriptHost::set_walking_ground))
//! and says which doors are open after every tick
//! ([`ScriptHost::set_openings`](crate::ScriptHost::set_openings)). A shut
//! door is a way shut.

use std::cell::RefCell;
use std::rc::Rc;

use lore_nav::{NoWay, Openings, Walkable};
use mlua::{Lua, Table, Value};

use crate::api::Shared;
use crate::fluid_api::vector_of;

/// Walkable ground, however the world keeps it: built, or being built and
/// waited for when asked.
pub trait WalkingGround {
    /// The ground.
    fn walkable(&self) -> &Walkable;
}

/// What the world said of its walkable ground. It outlives the script.
#[derive(Default)]
pub(crate) struct WalkState {
    pub(crate) ground: RefCell<Option<Rc<dyn WalkingGround>>>,
    pub(crate) openings: RefCell<Openings>,
    /// What each door is called in the map, in the order of their gates.
    pub(crate) doors: RefCell<Vec<String>>,
}

const CAN_EXAMPLE: &str =
    "if game.canWalk(game.player.position, vector.create(4, 0, -6)) then ... end";
const FIND_EXAMPLE: &str =
    "local corners, why = game.findWay(game.player.position, vector.create(4, 0, -6))";

/// Reads the two places `function` was given.
fn ends(
    lua: &Lua,
    function: &str,
    from: &Value,
    to: &Value,
    example: &str,
) -> mlua::Result<([f32; 3], [f32; 3])> {
    Ok((
        vector_of(lua, function, "where to start", from, example)?,
        vector_of(lua, function, "where to go", to, example)?,
    ))
}

/// Why there is no way, in words.
fn why(no: &NoWay, doors: &[String]) -> String {
    match no {
        NoWay::StartOff => "the place given as `from` is not on ground a person can stand on: \
                            there is none within 2 m of it. Start from a place on a floor, such \
                            as game.player.position"
            .to_owned(),
        NoWay::EndOff => "the place given as `to` is not on ground a person can stand on: there \
                          is none within 2 m of it. Choose a place on a floor"
            .to_owned(),
        NoWay::Shut { gates, .. } => {
            let named: Vec<String> = gates
                .iter()
                .map(|gate| match doors.get(gate.0 as usize) {
                    Some(name) => format!("the door \"{name}\" is shut"),
                    None => "a door is shut".to_owned(),
                })
                .collect();
            if named.is_empty() {
                "the way is shut".to_owned()
            } else {
                format!("{}: open it first", named.join(" and "))
            }
        }
        NoWay::Apart { .. } => "nothing a person can walk joins the two places: on the way there \
                                is a slope steeper than 45°, a step higher than 0.4 m or a gap \
                                narrower than 0.7 m. Add steps or a ramp, or widen the gap"
            .to_owned(),
    }
}

pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    let state = Rc::clone(shared);
    game.set(
        "canWalk",
        lua.create_function(move |lua, (from, to): (Value, Value)| {
            let (from, to) = ends(lua, "game.canWalk", &from, &to, CAN_EXAMPLE)?;
            let walk = &state.walk;
            let ground = walk.ground.borrow();
            Ok(ground
                .as_ref()
                .is_some_and(|g| g.walkable().can_reach(from, to, &walk.openings.borrow())))
        })?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "findWay",
        lua.create_function(move |lua, (from, to): (Value, Value)| {
            let (from, to) = ends(lua, "game.findWay", &from, &to, FIND_EXAMPLE)?;
            let walk = &state.walk;
            let ground = walk.ground.borrow();
            let Some(ground) = ground.as_ref() else {
                return Ok((
                    Value::Nil,
                    Value::String(lua.create_string("the game has no ground to walk on yet")?),
                ));
            };
            match ground.walkable().way(from, to, &walk.openings.borrow()) {
                Ok(way) => {
                    let corners = lua.create_table_with_capacity(way.points.len(), 0)?;
                    for point in &way.points {
                        let [x, y, z] = point.point;
                        corners.push(Value::Vector(mlua::Vector::new(x, y, z)))?;
                    }
                    Ok((Value::Table(corners), Value::Nil))
                }
                Err(no) => Ok((
                    Value::Nil,
                    Value::String(lua.create_string(why(&no, &walk.doors.borrow()))?),
                )),
            }
        })?,
    )?;
    Ok(())
}
