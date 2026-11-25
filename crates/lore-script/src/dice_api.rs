//! The game's dice, for scripts: `math.random`, `game.chance`, `game.pick`,
//! `game.shuffle` and the seed they all come from.
//!
//! Luau seeds `math.random` from where its VM is in memory and from the
//! computer's clock, so a rule that rolled it would play differently each
//! time the game is started and again after each save of the script. Here
//! the dice are the game's own instead: a seed and the number of rolls made
//! from it, kept in [`Dice`] outside the script's VM. The same seed and the
//! same inputs give the same rolls on every computer, a save of the script
//! carries them on, and a save of the game needs only those two numbers.
//!
//! A picture is drawn more often on a faster computer, so what
//! `game.onDraw` rolls comes from a second set of dice: sparks may be
//! scattered by chance there without moving the rules' dice.

use std::cell::Cell;
use std::hash::{BuildHasher, Hasher};
use std::rc::Rc;

use mlua::{Lua, Table, Value};

use crate::api::{Shared, describe, fail};
use crate::time_api;

/// The largest whole number a script's number holds exactly, and so the
/// largest seed.
const LARGEST: f64 = 9_007_199_254_740_992.0;

thread_local! {
    /// The seed every game started on this thread gets, if one was said.
    static NEXT_SEED: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Says the seed of every game started on this thread from now on, or
/// with `None` that each is to get one by chance again.
///
/// `lore check` plays a game several times and compares what its rules
/// did, which needs the same dice each time; `lore run --seed` plays a
/// game with the dice of an earlier one.
pub fn seed_new_games(seed: Option<u64>) {
    NEXT_SEED.set(seed);
}

/// A seed by chance: the one thing a game takes from outside itself.
fn seed_by_chance() -> u64 {
    // The standard library keys each of these from the system's own dice.
    let chance = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    // Short enough to read out and type back in.
    chance % 1_000_000_000
}

/// Stirs `value` until every bit of it depends on every other (the
/// finishing step of SplitMix64). Whole-number sums only, so it gives the
/// same answer on every computer.
const fn stir(value: u64) -> u64 {
    let value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// The game's dice: a seed, and how many times each set has been rolled.
#[derive(Debug)]
pub struct Dice {
    seed: Cell<u64>,
    /// Rolls made by the rules.
    rolls: Cell<u64>,
    /// Rolls made while a picture was drawn, from the other set.
    shown: Cell<u64>,
}

impl Default for Dice {
    fn default() -> Self {
        Self::new(NEXT_SEED.get().unwrap_or_else(seed_by_chance))
    }
}

impl Dice {
    /// Unrolled dice of `seed`.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            seed: Cell::new(seed),
            rolls: Cell::new(0),
            shown: Cell::new(0),
        }
    }

    /// The seed the dice come from.
    pub fn seed(&self) -> u64 {
        self.seed.get()
    }

    /// How many times the rules have rolled since the seed was set. With
    /// the seed, it is all a save needs to carry the dice on.
    pub fn rolls(&self) -> u64 {
        self.rolls.get()
    }

    /// Starts the dice again from `seed`.
    pub fn set_seed(&self, seed: u64) {
        self.restore(seed, 0);
    }

    /// Puts the dice where a game with `seed` had them after `rolls`.
    pub fn restore(&self, seed: u64, rolls: u64) {
        self.seed.set(seed);
        self.rolls.set(rolls);
        self.shown.set(0);
    }

    /// The next roll: any of the 2^64 whole numbers, each as likely.
    fn roll(&self, drawing: bool) -> u64 {
        /// What tells the two sets of dice apart.
        const OTHER_SET: u64 = 0xD1CE_D1CE_D1CE_D1CE;
        /// The step of SplitMix64: odd, so every count gives another number.
        const STEP: u64 = 0x9E37_79B9_7F4A_7C15;
        let (count, set) = if drawing {
            (&self.shown, OTHER_SET)
        } else {
            (&self.rolls, 0)
        };
        let rolled = count.get().wrapping_add(1);
        count.set(rolled);
        // The seed is stirred first, so seeds 1 and 2 share nothing.
        stir(stir(self.seed.get() ^ set).wrapping_add(rolled.wrapping_mul(STEP)))
    }

    /// A number from 0 up to but not 1.
    fn share(&self, drawing: bool) -> f64 {
        // The top 53 bits: all a number holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let top = (self.roll(drawing) >> 11) as f64;
        top / LARGEST
    }

    /// A whole number from 0 up to but not `below`, each as likely: no
    /// leaning towards the low ones, as a remainder would give.
    fn under(&self, below: u64, drawing: bool) -> u64 {
        if below == 0 {
            return 0;
        }
        // Lemire's way: rolls that would make some numbers likelier are
        // thrown again.
        let unfair = below.wrapping_neg() % below;
        loop {
            let wide = u128::from(self.roll(drawing)) * u128::from(below);
            #[allow(clippy::cast_possible_truncation)]
            let (high, low) = ((wide >> 64) as u64, wide as u64);
            if low >= unfair {
                return high;
            }
        }
    }
}

/// The whole number `value` is, for `function`, or the error that says
/// what it takes.
fn whole(lua: &Lua, function: &str, value: &Value, example: &str) -> mlua::Result<i64> {
    let number = match value {
        Value::Integer(number) => return Ok(*number),
        Value::Number(number) => *number,
        other => {
            return Err(fail(
                lua,
                format!(
                    "{function} needs a whole number, but it was given {}. Write it like \
                     this: {example}",
                    describe(other)
                ),
            ));
        }
    };
    if !number.is_finite() || number.abs() > LARGEST {
        return Err(fail(
            lua,
            format!(
                "{function} was given {number}, which is too big to count to. It takes whole \
                 numbers up to 9007199254740992. Write it like this: {example}"
            ),
        ));
    }
    // As Luau does: 2.9 is 2. In range, as checked above.
    #[allow(clippy::cast_possible_truncation)]
    Ok(number.trunc() as i64)
}

/// A whole number from `low` to `high`, both included.
fn between(dice: &Dice, low: i64, high: i64, drawing: bool) -> i64 {
    // Both are within 2^53 of 0, so neither sum can overflow.
    let span = high.abs_diff(low).saturating_add(1);
    #[allow(clippy::cast_possible_wrap)]
    let add = dice.under(span, drawing) as i64;
    low.saturating_add(add)
}

/// `math.random`: with nothing, a number from 0 up to 1; with `n`, a whole
/// number from 1 to `n`; with `m, n`, one from `m` to `n`.
fn random(lua: &Lua, shared: &Shared, args: &[Value]) -> mlua::Result<f64> {
    const EXAMPLE: &str = "local roll = math.random(1, 6)";
    let drawing = shared.clock.is_drawing();
    let dice = &shared.dice;
    let (low, high) = match args {
        [] => return Ok(dice.share(drawing)),
        [high] => (1, whole(lua, "math.random", high, EXAMPLE)?),
        [low, high] => (
            whole(lua, "math.random", low, EXAMPLE)?,
            whole(lua, "math.random", high, EXAMPLE)?,
        ),
        _ => {
            return Err(fail(
                lua,
                format!(
                    "math.random takes at most two numbers, the lowest and the highest it may \
                     give, and it was given {}. Write it like this: {EXAMPLE}",
                    args.len()
                ),
            ));
        }
    };
    if low > high {
        let said = if args.len() == 1 {
            format!("math.random({high}) gives a whole number from 1 to {high}, and there is none")
        } else {
            format!(
                "math.random({low}, {high}) gives a whole number from {low} to {high}, and \
                 there is none: the lowest comes first"
            )
        };
        return Err(fail(
            lua,
            format!(
                "{said}. Write it like this: {EXAMPLE}. For a number from 0 up to 1, write \
                 math.random() with nothing in the brackets."
            ),
        ));
    }
    #[allow(clippy::cast_precision_loss)]
    Ok(between(dice, low, high, drawing) as f64)
}

/// The seed a script said, for `function`.
fn seed_said(lua: &Lua, function: &str, value: &Value) -> mlua::Result<u64> {
    let example = format!("{function}(1234)");
    let seed = whole(lua, function, value, &example)?;
    u64::try_from(seed).map_err(|_| {
        fail(
            lua,
            format!(
                "{function} was given {seed}, and a seed is a whole number from 0 up. Write \
                 it like this: {example}"
            ),
        )
    })
}

/// The list a script gave `function`, and how long it is.
fn list_said<'a>(
    lua: &Lua,
    function: &str,
    value: &'a Value,
    example: &str,
) -> mlua::Result<(&'a Table, i64)> {
    let Value::Table(list) = value else {
        return Err(fail(
            lua,
            format!(
                "{function} needs a list, but it was given {}. Write it like this: {example}",
                describe(value)
            ),
        ));
    };
    let length = i64::try_from(list.raw_len()).unwrap_or(i64::MAX);
    Ok((list, length))
}

/// Puts the game's dice behind `math.random` and `math.randomseed`, and
/// the functions that read better on `game`.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    let math: Table = lua.globals().get("math")?;
    let state = Rc::clone(shared);
    math.set(
        "random",
        lua.create_function(move |lua, args: mlua::Variadic<Value>| random(lua, &state, &args))?,
    )?;
    for (table, name, function) in [
        (&math, "randomseed", "math.randomseed"),
        (game, "setSeed", "game.setSeed"),
    ] {
        let state = Rc::clone(shared);
        table.set(
            name,
            lua.create_function(move |lua, seed: Value| {
                time_api::rules_only(lua, &state, function)?;
                state.dice.set_seed(seed_said(lua, function, &seed)?);
                Ok(())
            })?,
        )?;
    }
    let state = Rc::clone(shared);
    game.set(
        "seed",
        lua.create_function(move |_, ()| {
            // Never more than 2^53: exact.
            #[allow(clippy::cast_precision_loss)]
            Ok(state.dice.seed() as f64)
        })?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "chance",
        lua.create_function(move |lua, share: Value| {
            const EXAMPLE: &str = "if game.chance(0.25) then dropCoin() end";
            let share = match share {
                Value::Number(share) if share.is_finite() => share,
                #[allow(clippy::cast_precision_loss)]
                Value::Integer(share) => share as f64,
                other => {
                    return Err(fail(
                        lua,
                        format!(
                            "game.chance needs a number from 0 (never) to 1 (always), but it \
                             was given {}. Write it like this: {EXAMPLE}",
                            describe(&other)
                        ),
                    ));
                }
            };
            if !(0.0..=1.0).contains(&share) {
                return Err(fail(
                    lua,
                    format!(
                        "game.chance was given {share}, and it takes a number from 0 (never) \
                         to 1 (always): one time in four is 0.25, and 25 in a hundred is \
                         25 / 100. Write it like this: {EXAMPLE}"
                    ),
                ));
            }
            Ok(state.dice.share(state.clock.is_drawing()) < share)
        })?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "pick",
        lua.create_function(move |lua, list: Value| {
            const EXAMPLE: &str = "local prize = game.pick({ \"coin\", \"heart\", \"arrow\" })";
            let (list, length) = list_said(lua, "game.pick", &list, EXAMPLE)?;
            if length == 0 {
                return Ok(Value::Nil);
            }
            let at = between(&state.dice, 1, length, state.clock.is_drawing());
            list.raw_get::<Value>(at)
        })?,
    )?;
    let state = Rc::clone(shared);
    game.set(
        "shuffle",
        lua.create_function(move |lua, said: Value| {
            const EXAMPLE: &str = "local order = game.shuffle({ 1, 2, 3, 4 })";
            let (list, length) = list_said(lua, "game.shuffle", &said, EXAMPLE)?;
            if list.is_readonly() {
                return Err(fail(
                    lua,
                    format!(
                        "game.shuffle puts the list it is given in a new order, and this one \
                         cannot be changed. Give it a copy: game.shuffle(table.clone(list)). \
                         Or write it like this: {EXAMPLE}"
                    ),
                ));
            }
            let drawing = state.clock.is_drawing();
            // Fisher and Yates: every order is as likely as any other.
            for last in (2..=length).rev() {
                let other = between(&state.dice, 1, last, drawing);
                if other != last {
                    let (a, b): (Value, Value) = (list.raw_get(last)?, list.raw_get(other)?);
                    list.raw_set(last, b)?;
                    list.raw_set(other, a)?;
                }
            }
            Ok(said.clone())
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_rolls_the_same_on_every_computer() {
        // Written down once: any computer that gives other numbers has
        // other dice, and a save or a replay taken to it would differ.
        let dice = Dice::new(1234);
        let rolls: Vec<i64> = (0..8).map(|_| between(&dice, 1, 6, false)).collect();
        assert_eq!(rolls, ROLLS_OF_1234);
        assert_eq!(dice.rolls(), 8);
    }

    const ROLLS_OF_1234: [i64; 8] = [3, 4, 1, 5, 2, 1, 1, 6];

    #[test]
    fn every_side_comes_up_about_as_often() {
        let dice = Dice::new(7);
        let mut seen = [0_u32; 6];
        for _ in 0..60_000 {
            let side = usize::try_from(between(&dice, 1, 6, false) - 1).unwrap();
            seen[side] += 1;
        }
        for count in seen {
            assert!((9_500..10_500).contains(&count), "{seen:?}");
        }
    }

    #[test]
    fn a_share_is_never_one_and_neighbouring_seeds_share_nothing() {
        let (one, two) = (Dice::new(1), Dice::new(2));
        let mut same = 0;
        for _ in 0..1000 {
            let (a, b) = (one.share(false), two.share(false));
            assert!((0.0..1.0).contains(&a));
            same += u32::from((a < 0.5) == (b < 0.5));
        }
        assert!((400..600).contains(&same), "{same}");
    }

    #[test]
    fn the_dice_of_a_picture_do_not_move_the_rules_dice() {
        let (plain, drawn) = (Dice::new(99), Dice::new(99));
        let mut rolls = Vec::new();
        for _ in 0..20 {
            drawn.under(1000, true);
            drawn.share(true);
            rolls.push((plain.under(1000, false), drawn.under(1000, false)));
        }
        assert!(rolls.iter().all(|(a, b)| a == b));
        assert_eq!(plain.rolls(), drawn.rolls());
    }

    #[test]
    fn dice_carry_on_from_a_seed_and_a_count() {
        let dice = Dice::new(5);
        for _ in 0..10 {
            dice.roll(false);
        }
        let next = dice.roll(false);
        let again = Dice::new(0);
        again.restore(5, 10);
        assert_eq!(again.roll(false), next);
    }

    #[test]
    fn two_new_games_get_different_seeds() {
        seed_new_games(None);
        let seeds: Vec<u64> = (0..4).map(|_| Dice::default().seed()).collect();
        assert!(seeds.iter().any(|seed| *seed != seeds[0]), "{seeds:?}");
        seed_new_games(Some(42));
        assert_eq!(Dice::default().seed(), 42);
        seed_new_games(None);
    }
}
