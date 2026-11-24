//! Health: anything in the world that can be hurt has it, and every hurt
//! goes through one path, whoever or whatever did it.
//!
//! ```lua
//! game.player:setMaxHealth(150)
//! game.onDamage(function(hurt)
//!     print(hurt.victim, "lost", hurt.amount, "to", hurt.how)
//! end)
//! game.onDeath(function(victim, killer)
//!     if victim == game.player then game.player:setHealth(game.player.maxHealth) end
//! end)
//! ```
//!
//! Weapons, falls and scripts all hurt through [`ScriptHost::damage`]: the
//! health changes at once, and the script hears of it at a fixed point of the
//! tick (before its `game.onTick`, and again after), so the order never
//! depends on who did the hurting. Nothing happens on death that the script
//! does not say.
//!
//! Research: Roblox's Humanoid (`Health`, `MaxHealth`, `TakeDamage`, `Died`),
//! Unreal's `ApplyDamage` and `OnTakeAnyDamage` (who did it, and how), and
//! Godot's absence of either, which every Godot game writes for itself.
//!
//! [`ScriptHost::damage`]: crate::ScriptHost::damage

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, Table, Value};

use crate::api::{Shared, describe, fail};
use crate::character_api::CharacterId;

/// A hurt or a death the script has not yet heard of.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hurt {
    pub victim: CharacterId,
    pub amount: f32,
    pub from: Option<CharacterId>,
    pub how: String,
    pub died: bool,
}

/// The hurts waiting for the script, shared by every version of it.
#[derive(Debug, Default)]
pub(crate) struct HealthState {
    waiting: RefCell<Vec<Hurt>>,
}

/// The most hurts kept for the script between two points it hears them.
const MOST_WAITING: usize = 256;

impl HealthState {
    pub(crate) fn take(&self) -> Vec<Hurt> {
        std::mem::take(&mut *self.waiting.borrow_mut())
    }

    fn push(&self, hurt: Hurt) {
        let mut waiting = self.waiting.borrow_mut();
        if waiting.len() < MOST_WAITING {
            waiting.push(hurt);
        }
    }
}

/// Takes `amount` of health from `victim`, as `how` ("fall", a weapon's
/// name, or what a script said), by `from` if someone did it. Gives back
/// whether it died of it. The script hears of it at the next fixed point.
pub(crate) fn hurt(
    shared_characters: &crate::character_api::Characters,
    health: &HealthState,
    victim: CharacterId,
    amount: f32,
    from: Option<CharacterId>,
    how: &str,
) -> bool {
    if !(amount.is_finite() && amount > 0.0) {
        return false;
    }
    let Some((now, _)) = shared_characters.health(victim) else {
        return false;
    };
    // The dead are hurt no more.
    if now <= 0.0 {
        return false;
    }
    let Some((before, after)) = shared_characters.set_health(victim, now - amount, None) else {
        return false;
    };
    let died = after <= 0.0;
    health.push(Hurt {
        victim,
        amount: before - after,
        from,
        how: how.to_owned(),
        died,
    });
    died
}

/// A number of health a script gave `method`.
fn amount(lua: &Lua, value: &Value, method: &str, example: &str) -> mlua::Result<f32> {
    let n = match value {
        Value::Number(n) => *n as f32,
        Value::Integer(n) => *n as f32,
        other => {
            return Err(fail(
                lua,
                format!(
                    "{method} needs a number of health points, but it was given {}. Write it \
                     like this: {example}",
                    describe(other)
                ),
            ));
        }
    };
    if !n.is_finite() || n < 0.0 {
        return Err(fail(
            lua,
            format!(
                "{method} needs a number of health points of 0 or more. Write it like this: \
                 {example}"
            ),
        ));
    }
    Ok(n)
}

/// `character:setHealth(points)`.
pub(crate) fn set_health(
    lua: &Lua,
    shared: &Rc<Shared>,
    id: CharacterId,
    value: &Value,
) -> mlua::Result<()> {
    let n = amount(lua, value, "setHealth", "game.player:setHealth(100)")?;
    shared.characters.set_health(id, n, None);
    Ok(())
}

/// `character:setMaxHealth(points)`: the most it can have. Its health is
/// kept within the new most, and one at full health stays full.
pub(crate) fn set_max_health(
    lua: &Lua,
    shared: &Rc<Shared>,
    id: CharacterId,
    value: &Value,
) -> mlua::Result<()> {
    let n = amount(lua, value, "setMaxHealth", "game.player:setMaxHealth(150)")?;
    if let Some((now, max)) = shared.characters.health(id) {
        let full = now >= max;
        shared
            .characters
            .set_health(id, if full { n } else { now }, Some(n));
    }
    Ok(())
}

/// `character:heal(points)`: health back, up to its most. The dead stay dead
/// (`setHealth` brings them back).
pub(crate) fn heal(
    lua: &Lua,
    shared: &Rc<Shared>,
    id: CharacterId,
    value: &Value,
) -> mlua::Result<()> {
    let n = amount(lua, value, "heal", "game.player:heal(25)")?;
    if let Some((now, _)) = shared.characters.health(id)
        && now > 0.0
    {
        shared.characters.set_health(id, now + n, None);
    }
    Ok(())
}

/// `character:damage(points, { from = someone, how = "trap" })`.
pub(crate) fn damage(
    lua: &Lua,
    shared: &Rc<Shared>,
    id: CharacterId,
    value: &Value,
    options: &Value,
) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:damage(10, { how = \"spikes\" })";
    let n = amount(lua, value, "damage", EXAMPLE)?;
    let (mut from, mut how) = (None, "script".to_owned());
    match options {
        Value::Nil => {}
        Value::Table(table) => {
            for pair in table.pairs::<Value, Value>() {
                let (key, value) = pair?;
                let key = match key {
                    Value::String(k) => k.to_string_lossy(),
                    _ => {
                        return Err(fail(
                            lua,
                            format!("damage takes options by name. Write it like this: {EXAMPLE}"),
                        ));
                    }
                };
                match key.as_str() {
                    "from" => {
                        from = Some(crate::character_api::id_of_value(lua, &value).ok_or_else(
                            || {
                                fail(
                                    lua,
                                    format!(
                                        "damage's from needs a character, such as game.player, but \
                                     it was given {}. Write it like this: \
                                     enemy:damage(10, {{ from = game.player }})",
                                        describe(&value)
                                    ),
                                )
                            },
                        )?);
                    }
                    "how" => {
                        let Value::String(text) = &value else {
                            return Err(fail(
                                lua,
                                format!(
                                    "damage's how needs a word in quotes, such as \"spikes\", \
                                     but it was given {}. Write it like this: {EXAMPLE}",
                                    describe(&value)
                                ),
                            ));
                        };
                        how = text.to_string_lossy().trim().to_owned();
                    }
                    other => {
                        return Err(fail(
                            lua,
                            format!(
                                "damage has no option \"{other}\". It takes from (who did it) \
                                 and how (a word for what did it). Write it like this: {EXAMPLE}"
                            ),
                        ));
                    }
                }
            }
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "damage's second part is a table of options, but it was given {}. Write \
                     it like this: {EXAMPLE}",
                    describe(other)
                ),
            ));
        }
    }
    hurt(&shared.characters, &shared.health, id, n, from, &how);
    Ok(())
}

/// What the table handed to a `game.onDamage` function holds.
#[cfg(test)]
pub(crate) const HURT_FIELDS: &[&str] = &["victim", "amount", "from", "how", "died"];

/// The table a `game.onDamage` function is handed.
pub(crate) fn hurt_table(lua: &Lua, shared: &Shared, hurt: &Hurt) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.raw_set("victim", shared.handles.character(lua, hurt.victim)?)?;
    table.raw_set("amount", hurt.amount)?;
    if let Some(from) = hurt.from
        && shared.characters.has(from)
    {
        table.raw_set("from", shared.handles.character(lua, from)?)?;
    }
    table.raw_set("how", hurt.how.as_str())?;
    table.raw_set("died", hurt.died)?;
    table.set_readonly(true);
    Ok(table)
}
