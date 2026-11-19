//! Pools for scripts: `game.fluids.pool`, `game.fluids.pools` and what a
//! pool is to a script.
//!
//! A pool is a body of standing fluid in the level: a lake, a tank, a tub.
//! A script finds one by the name its sheet has in the level and reads what
//! the engine last said of it ([`PoolTold`](crate::PoolTold)): what it
//! holds, how high it stands, how full it is. What a script asks of it (fill,
//! drain, a new height, another kind) is queued with the sprays
//! ([`FluidAsk`]) and done at the next tick.
//!
//! A handle holds the pool's name, not its number: a script's first lines
//! run before the engine has said what pools the level has, and a name that
//! is wrong is then said when the pool is first used.

use std::fmt::Write as _;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use crate::api::{Shared, describe, fail};
use crate::character_api::shared_of;
use crate::error::closest;
use crate::fluid_api::{FluidAsk, FluidState, key_text, kind_of, plain, tidy, vector_of};
use crate::time_api::rules_only;
use crate::weather_api::number;

/// What can be read from a pool.
pub(crate) const POOL_FIELDS: &[&str] = &[
    "name",
    "fluid",
    "level",
    "depth",
    "litres",
    "capacity",
    "brim",
    "overflowing",
    "frozen",
];

/// What can be set on a pool.
const POOL_SETTINGS: &[&str] = &["fluid", "level", "litres"];

/// What a pool can be asked to do.
pub(crate) const POOL_METHODS: &[&str] = &["fill", "drain", "setLevel", "share", "splash"];

/// A pool, as a script holds one: the name it is found by.
#[derive(Clone)]
struct PoolRef(Rc<str>);

/// A pool for a script to hold: the one numbered `pool`.
pub(crate) fn handle(lua: &Lua, shared: &Shared, pool: u16) -> mlua::Result<AnyUserData> {
    let names = shared.fluids.pool_names();
    let name = names
        .get(usize::from(pool))
        .map_or_else(String::new, |name| tidy(name));
    lua.create_userdata(PoolRef(Rc::from(name)))
}

/// The error for a pool the level has not got.
fn unknown_pool(lua: &Lua, state: &FluidState, name: &str) -> mlua::Error {
    let names = state.pool_names();
    let mut message = format!("There is no pool called \"{name}\" in the level.");
    if names.is_empty() {
        message.push_str(
            " The level has no pools: a pool is a flat sheet with the custom property `water` \
             set to true, laid over a hollow at the height the fluid should stand. Add one in \
             your 3D tool, name it, and export the level again.",
        );
        return fail(lua, message);
    }
    match closest(name, names.iter().map(String::as_str)) {
        Some(near) => {
            let _ = write!(message, " Did you mean \"{near}\"?");
        }
        None => {
            let _ = write!(message, " The level's pools are: {}.", names.join(", "));
        }
    }
    fail(lua, message)
}

/// Which pool `this` is, once the engine has said what pools there are.
fn found(lua: &Lua, state: &FluidState, this: &PoolRef) -> mlua::Result<u16> {
    if !state.pools_known() {
        return Err(fail(
            lua,
            format!(
                "The pool \"{}\" cannot be used yet: the level is not in the world until the \
                 game starts. Keep the pool in a local as you do now, and use it from \
                 game.onStart(function() ... end) or game.onTick(function() ... end).",
                this.0
            ),
        ));
    }
    state
        .pool_named(&this.0)
        .ok_or_else(|| unknown_pool(lua, state, &this.0))
}

/// A number of litres a script wrote.
fn litres_of(lua: &Lua, function: &str, value: &Value, example: &str) -> mlua::Result<f32> {
    let litres = number(
        lua,
        function,
        Some(value),
        "a number of litres, 0 or more: a bucket is 10, a bath 150 and a cubic metre 1000",
        example,
    )?;
    Ok(litres.clamp(0.0, 1.0e9))
}

impl UserData for PoolRef {
    #[allow(clippy::too_many_lines)]
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("fill", |lua, this, (litres, fluid): (Value, Value)| {
            const EXAMPLE: &str = "pool:fill(50)";
            let shared = shared_of(lua)?;
            let state = &shared.fluids;
            rules_only(lua, &shared, "pool:fill")?;
            let pool = found(lua, state, this)?;
            let litres = litres_of(lua, "pool:fill", &litres, EXAMPLE)?;
            let kind = match &fluid {
                // More of what it holds.
                Value::Nil => state.pool(pool).map_or(0, |told| told.kind),
                other => kind_of(
                    lua,
                    state,
                    other,
                    true,
                    "pool:fill",
                    "pool:fill(50, \"honey\")",
                )?
                .unwrap_or(0),
            };
            state.queue(FluidAsk::PoolFill { pool, kind, litres });
            Ok(())
        });
        methods.add_method("drain", |lua, this, litres: Value| {
            let shared = shared_of(lua)?;
            rules_only(lua, &shared, "pool:drain")?;
            let pool = found(lua, &shared.fluids, this)?;
            let litres = litres_of(lua, "pool:drain", &litres, "pool:drain(50)")?;
            shared.fluids.queue(FluidAsk::PoolDrain { pool, litres });
            Ok(())
        });
        methods.add_method(
            "setLevel",
            |lua, this, (height, seconds): (Value, Value)| {
                const EXAMPLE: &str = "pool:setLevel(2.5, 10)";
                let shared = shared_of(lua)?;
                rules_only(lua, &shared, "pool:setLevel")?;
                let pool = found(lua, &shared.fluids, this)?;
                let height = number(
                    lua,
                    "pool:setLevel",
                    Some(&height),
                    "how high the surface is to stand, in metres, and then how many seconds it \
                 takes to get there",
                    EXAMPLE,
                )?;
                let seconds = match &seconds {
                    Value::Nil => 0.0,
                    other => number(
                        lua,
                        "pool:setLevel's seconds",
                        Some(other),
                        "how many seconds the surface takes to get there, 0 or more: leave it out \
                     to move it at once",
                        EXAMPLE,
                    )?
                    .clamp(0.0, 86_400.0),
                };
                shared.fluids.queue(FluidAsk::PoolLevel {
                    pool,
                    height,
                    seconds,
                });
                Ok(())
            },
        );
        methods.add_method("share", |lua, this, fluid: Value| {
            let shared = shared_of(lua)?;
            let state = &shared.fluids;
            let pool = found(lua, state, this)?;
            let kind = kind_of(
                lua,
                state,
                &fluid,
                false,
                "pool:share",
                "pool:share(\"honey\")",
            )?;
            let share = kind
                .zip(state.pool(pool))
                .and_then(|(kind, told)| told.shares.get(usize::from(kind)).copied())
                .unwrap_or(0.0);
            Ok(plain(share))
        });
        methods.add_method("splash", |lua, this, (place, options): (Value, Value)| {
            const EXAMPLE: &str = "pool:splash(game.player.position, { litres = 3, speed = 5 })";
            const OPTIONS: [&str; 2] = ["litres", "speed"];
            let shared = shared_of(lua)?;
            rules_only(lua, &shared, "pool:splash")?;
            let pool = found(lua, &shared.fluids, this)?;
            let place = vector_of(lua, "pool:splash", "where the splash is", &place, EXAMPLE)?;
            let (mut litres, mut speed) = (2.0_f32, 4.0_f32);
            match &options {
                Value::Nil => {}
                Value::Table(options) => {
                    for pair in options.pairs::<Value, Value>() {
                        let (key, value) = pair?;
                        let key = key_text(&key);
                        match key.as_str() {
                            "litres" => {
                                litres = litres_of(lua, "pool:splash's `litres`", &value, EXAMPLE)?
                                    .min(1000.0);
                            }
                            "speed" => {
                                speed = number(
                                    lua,
                                    "pool:splash's `speed`",
                                    Some(&value),
                                    "how fast the drops leave, in metres a second, from 0.5 \
                                     to 12: 4 is a body falling in",
                                    EXAMPLE,
                                )?
                                .clamp(0.5, 12.0);
                            }
                            _ => {
                                let mut message =
                                    format!("pool:splash has no option called {key}.");
                                match closest(&key, OPTIONS.iter().copied()) {
                                    Some(near) => {
                                        let _ = write!(message, " Did you mean {near}?");
                                    }
                                    None => {
                                        let _ = write!(
                                            message,
                                            " The options are: {}.",
                                            OPTIONS.join(", ")
                                        );
                                    }
                                }
                                return Err(fail(lua, message));
                            }
                        }
                    }
                }
                other => {
                    return Err(fail(
                        lua,
                        format!(
                            "pool:splash needs its options in a table, but it was given {}. \
                             Write it like this: {EXAMPLE}",
                            describe(other)
                        ),
                    ));
                }
            }
            shared.fluids.queue(FluidAsk::PoolSplash {
                pool,
                place,
                litres,
                speed,
            });
            Ok(())
        });
        methods.add_meta_function(MetaMethod::ToString, |lua, this: AnyUserData| {
            let name = this.borrow::<Self>()?.0.clone();
            let state = &shared_of(lua)?.fluids;
            let written = state
                .pool_named(&name)
                .and_then(|pool| state.pool_names().get(usize::from(pool)).cloned());
            Ok(format!("pool \"{}\"", written.unwrap_or(name.to_string())))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (AnyUserData, AnyUserData)| {
            Ok(match (a.borrow::<Self>(), b.borrow::<Self>()) {
                (Ok(a), Ok(b)) => a.0 == b.0,
                _ => false,
            })
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| -> mlua::Result<Value> {
                let this = this.borrow::<Self>()?.clone();
                let shared = shared_of(lua)?;
                let state = &shared.fluids;
                let key = key_text(&key);
                if !POOL_FIELDS.contains(&key.as_str()) {
                    let mut message = format!("pool.{key} does not exist.");
                    let all = POOL_FIELDS.iter().chain(POOL_METHODS).copied();
                    match closest(&key, all) {
                        Some(near) if POOL_METHODS.contains(&near) => {
                            let _ = write!(message, " Did you mean pool:{near}(...)?");
                        }
                        Some(near) => {
                            let _ = write!(message, " Did you mean pool.{near}?");
                        }
                        None => {
                            let _ = write!(
                                message,
                                " A pool has: {}; and can be asked to: {}.",
                                POOL_FIELDS.join(", "),
                                POOL_METHODS.join(", ")
                            );
                        }
                    }
                    return Err(fail(lua, message));
                }
                let pool = found(lua, state, &this)?;
                if key == "name" {
                    let names = state.pool_names();
                    let name = names.get(usize::from(pool)).map_or("", String::as_str);
                    return Ok(Value::String(lua.create_string(name)?));
                }
                // Before the first tick nothing has been said of it yet.
                let told = state.pool(pool);
                let read = |pick: fn(&crate::PoolTold) -> f32| {
                    Value::Number(plain(told.as_ref().map_or(0.0, pick)))
                };
                Ok(match key.as_str() {
                    "fluid" => {
                        Value::UserData(FluidState::handle(lua, told.map_or(0, |told| told.kind))?)
                    }
                    "level" => read(|told| told.level),
                    "depth" => read(|told| told.depth),
                    "litres" => read(|told| told.litres),
                    "capacity" => read(|told| told.capacity),
                    "brim" => read(|told| told.brim),
                    "overflowing" => Value::Boolean(told.is_some_and(|told| told.overflowing)),
                    _ => Value::Boolean(told.is_some_and(|told| told.frozen)),
                })
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, value): (AnyUserData, Value, Value)| -> mlua::Result<()> {
                let this = this.borrow::<Self>()?.clone();
                let shared = shared_of(lua)?;
                let state = &shared.fluids;
                let key = key_text(&key);
                if !POOL_SETTINGS.contains(&key.as_str()) {
                    let mut message = if POOL_FIELDS.contains(&key.as_str()) {
                        format!("pool.{key} can be read but not set.")
                    } else {
                        format!("pool.{key} cannot be set, there is no such setting.")
                    };
                    match closest(&key, POOL_SETTINGS.iter().copied()) {
                        Some(near) if near != key => {
                            let _ = write!(message, " Did you mean pool.{near}?");
                        }
                        _ => {
                            let _ = write!(
                                message,
                                " What can be set is: {}. To pour in or take out, write \
                                 pool:fill(50) or pool:drain(50).",
                                POOL_SETTINGS.join(", ")
                            );
                        }
                    }
                    return Err(fail(lua, message));
                }
                let label = format!("pool.{key}");
                rules_only(lua, &shared, &label)?;
                let pool = found(lua, state, &this)?;
                let ask = match key.as_str() {
                    "fluid" => {
                        let kind = kind_of(
                            lua,
                            state,
                            &value,
                            true,
                            "pool.fluid",
                            "pool.fluid = \"lava\"",
                        )?;
                        FluidAsk::PoolKind {
                            pool,
                            kind: kind.unwrap_or(0),
                        }
                    }
                    "level" => FluidAsk::PoolLevel {
                        pool,
                        height: number(
                            lua,
                            "pool.level",
                            Some(&value),
                            "how high the surface is to stand, in metres",
                            "pool.level = 2.5",
                        )?,
                        seconds: 0.0,
                    },
                    _ => FluidAsk::PoolLitres {
                        pool,
                        litres: litres_of(lua, "pool.litres", &value, "pool.litres = 500")?,
                    },
                };
                state.queue(ask);
                Ok(())
            },
        );
    }
}

/// Adds `pool` and `pools` to `game.fluids`.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, fluids: &Table) -> mlua::Result<()> {
    let finding = Rc::clone(shared);
    fluids.set(
        "pool",
        lua.create_function(move |lua, name: Value| {
            let Value::String(written) = &name else {
                return Err(fail(
                    lua,
                    format!(
                        "game.fluids.pool needs the pool's name in quotes, but it was given \
                         {}. A pool has the name its sheet has in the level. Write it like \
                         this: game.fluids.pool(\"Pond\")",
                        describe(&name)
                    ),
                ));
            };
            let state = &finding.fluids;
            let name = tidy(&written.to_string_lossy());
            // Once the level's pools are known, a wrong name is said here,
            // on the line that wrote it.
            if state.pools_known() && state.pool_named(&name).is_none() {
                return Err(unknown_pool(lua, state, &name));
            }
            lua.create_userdata(PoolRef(Rc::from(name)))
        })?,
    )?;
    let listing = Rc::clone(shared);
    fluids.set(
        "pools",
        lua.create_function(move |lua, ()| {
            let list = lua.create_table()?;
            for name in listing.fluids.pool_names() {
                list.raw_push(lua.create_userdata(PoolRef(Rc::from(tidy(&name))))?)?;
            }
            Ok(list)
        })?,
    )?;
    Ok(())
}
