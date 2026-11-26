//! The world's clock for scripts: `game.clock`.
//!
//! The world has one idea of what time it is: a count of hours since
//! midnight at the start of the first day, kept here outside the script's
//! VM so a save of the script does not start it again. `game.clock` reads
//! and sets it in a game's words (`"8:00"`, the day, how long a day lasts),
//! and `game.sky` reads and sets the same count to place the sun.
//!
//! The hours pass on ticks, so a paused game's clock stands still and a game
//! run fast runs its days fast, with nothing to do here. Until a game says a
//! time or a day length the clock is unset: no day and night, and nothing
//! to pay.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{AnyUserData, Function, Lua, MetaMethod, UserData, UserDataMethods, Value};

use crate::api::{Shared, describe, fail};
use crate::character_api::{missing, shared_of};
use crate::time_api::rules_only;

/// Everything on `game.clock`. The type definitions are tested against it.
pub(crate) const CLOCK_FIELDS: &[&str] = &["dayLength", "time", "hour", "day", "skipTo", "onHour"];

/// The longest day a game can ask for, in minutes: about ten weeks.
const LONGEST_DAY: f64 = 100_000.0;

/// How many `game.clock.onHour` functions one game can have. Far more than
/// a game needs: reaching it means they are added over and over by mistake.
const MOST_HOUR_HANDLERS: usize = 1024;

/// The world's clock as plain data: what a save keeps, and what
/// [`ScriptHost::restore_world_clock`](crate::ScriptHost::restore_world_clock)
/// puts back.
///
/// The clock counts in two parts so that `game.sky` reads exactly as it did
/// before the clock was shared: `hours` is what `game.sky.hour()` gives,
/// counted from midnight at the start of the date the sky's calendar was
/// last given, and `days_before` the whole days the clock had counted
/// before that date. The day of `game.clock.day` is made of both.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WorldClock {
    /// Hours since midnight at the start of the sky's date: 30.5 is half
    /// past six in the morning of the day after. `None` until the game says
    /// a time or how long a day lasts.
    pub hours: Option<f64>,
    /// Whole days the clock counted before the sky's date: `game.sky.setDate`
    /// moves the days `hours` held into here, so the date said is today and
    /// the clock's day count goes on.
    pub days_before: i64,
    /// How many minutes of play a whole day and night last, or 0 while the
    /// hours stand still.
    pub day_minutes: f64,
    /// The last whole hour `game.clock.onHour` functions were told of,
    /// counted from midnight at the start of day 1.
    pub told: Option<i64>,
}

/// The world's clock, shared by `game.clock`, `game.sky` and the engine.
///
/// The rule for setting it: a time below 24 hours is a watch set within
/// today, and never moves the clock's day; only hours passing (ticks,
/// `skipTo`) and an hour of 24 or more said to `game.sky.setHour` move it
/// on. Setting tells `onHour` nothing; passing time tells each hour once.
#[derive(Debug, Default)]
pub(crate) struct DayClock {
    now: Cell<WorldClock>,
    /// Whether the hour has changed since the engine last took it.
    changed: Cell<bool>,
}

impl DayClock {
    pub(crate) fn state(&self) -> WorldClock {
        self.now.get()
    }

    pub(crate) fn restore(&self, clock: WorldClock) {
        self.now.set(clock);
        self.changed.set(true);
    }

    fn change(&self, change: impl FnOnce(&mut WorldClock)) {
        let mut now = self.now.get();
        change(&mut now);
        self.now.set(now);
        self.changed.set(true);
    }

    /// Hours since midnight at the start of the sky's date, if a time has
    /// been said: what `game.sky.hour()` gives and the sky is drawn at.
    pub(crate) fn hours(&self) -> Option<f64> {
        self.now.get().hours
    }

    /// Hours since midnight at the start of day 1, if a time has been said.
    fn clock_hours(&self) -> Option<f64> {
        let now = self.now.get();
        // Days are far below where an f64 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        now.hours.map(|hours| hours + 24.0 * now.days_before as f64)
    }

    /// Says the clock has just been set, not passed: the hour it now reads
    /// is not told to `onHour`.
    fn set_told(&self) {
        let told = self.clock_hours().map(whole_hours);
        self.change(|now| now.told = told);
    }

    /// `game.sky.setHour`: `hours` from midnight at the start of the sky's
    /// date, as the sky has always read it. The clock's day is today plus
    /// the whole days in `hours`: an hour below 24 sets the watch within
    /// today. Tells nothing.
    pub(crate) fn set_sky_hour(&self, hours: f64) {
        let hours = hours.max(0.0);
        self.change(|now| {
            // The days the clock has counted to the start of today.
            let today = now
                .hours
                .map_or(0, |was| now.days_before.saturating_add(whole_days(was)));
            now.hours = Some(hours);
            now.days_before = today;
        });
        self.set_told();
    }

    /// `game.sky.setDate`: the date said becomes the sky's today. The hour
    /// keeps only its time of day, as the sky has always done; the days it
    /// held stay on the clock.
    pub(crate) fn start_calendar_today(&self) {
        if let Some(hours) = self.hours()
            && hours >= 24.0
        {
            self.change(|now| {
                now.days_before = now.days_before.saturating_add(whole_days(hours));
                now.hours = Some(hours.rem_euclid(24.0));
            });
        }
    }

    /// The whole days since the sky's date, for `game.sky.date()`.
    pub(crate) fn days_since_date(&self) -> i64 {
        self.hours().map_or(0, whole_days)
    }

    pub(crate) fn day_minutes(&self) -> f64 {
        self.now.get().day_minutes
    }

    /// Says how long a day lasts. The hours pass from some hour: noon, if
    /// none was said.
    pub(crate) fn set_day_minutes(&self, minutes: f64) {
        let minutes = minutes.clamp(0.0, LONGEST_DAY);
        self.now.set(WorldClock {
            day_minutes: minutes,
            ..self.now.get()
        });
        if minutes > 0.0 && self.hours().is_none() {
            self.start_at(12.0);
        }
    }

    /// Moves the hours on by one tick of `step` seconds, if they pass.
    pub(crate) fn step(&self, step: f32) {
        let now = self.now.get();
        if let (Some(hours), true) = (now.hours, now.day_minutes > 0.0) {
            self.change(|now| {
                now.hours = Some(hours + f64::from(step) * 24.0 / (now.day_minutes * 60.0));
            });
        }
    }

    /// The hour of the day, from 0 up to 24, if a time has been said.
    pub(crate) fn time_of_day(&self) -> Option<f64> {
        self.hours().map(|hours| hours.rem_euclid(24.0))
    }

    /// The day it is, counted from 1, if a time has been said.
    pub(crate) fn day(&self) -> Option<u32> {
        let now = self.now.get();
        let days = now.days_before.saturating_add(whole_days(now.hours?));
        // Held to what a u32 holds: an hour count of 1e300 is still a day.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(days.clamp(0, i64::from(u32::MAX - 1)) as u32 + 1)
    }

    /// Moves the clock forward to the next time it is `hour` o'clock (from
    /// 0 up to 24): later today, or tomorrow if that has gone by. It never
    /// goes back, and the hours it passes are told. An unset clock is set to
    /// that hour of day 1.
    pub(crate) fn move_to(&self, hour: f64) {
        let hour = hour.rem_euclid(24.0);
        let Some(hours) = self.hours() else {
            self.start_at(hour);
            return;
        };
        // Whole days are exact in an f64, so the hour arrived at is exactly
        // the one said and not a hair before it.
        let midnight = (hours / 24.0).floor() * 24.0;
        let to = if midnight + hour >= hours {
            midnight + hour
        } else {
            midnight + 24.0 + hour
        };
        self.change(|now| now.hours = Some(to));
    }

    /// Sets the clock to `hour` o'clock (from 0 up to 24) today, back or
    /// forward, as a watch is set: no hour is told as passed, and the day
    /// stays.
    pub(crate) fn start_at(&self, hour: f64) {
        let midnight = self
            .hours()
            .map_or(0.0, |hours| (hours / 24.0).floor() * 24.0);
        self.change(|now| now.hours = Some(midnight + hour.rem_euclid(24.0)));
        self.set_told();
    }

    /// The time of day as a game writes it: "8:00", "20:05".
    pub(crate) fn text(&self) -> Option<String> {
        let hour = self.time_of_day()?;
        // A hair is allowed for rounding, so that 8 o'clock is never 7:59.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let minutes = ((hour * 60.0 + 1e-6).floor() as u32).min(24 * 60 - 1);
        Some(format!("{}:{:02}", minutes / 60, minutes % 60))
    }

    /// The hour of the day, if it has changed since this was last asked.
    pub(crate) fn take_changed(&self) -> Option<f64> {
        self.changed.take().then(|| self.hours()).flatten()
    }

    /// The next whole hour of the day (0 to 23) the clock has come to and
    /// not yet told, marked as told. A clock moved forward tells each hour
    /// it passed, but no more than the last day of them; a clock set, back
    /// or forward, tells nothing. Asked one hour at a time, so a function
    /// told of one hour that sets the clock stops the hours after it.
    pub(crate) fn next_due(&self) -> Option<u8> {
        let now = whole_hours(self.clock_hours()?);
        let Some(told) = self.now.get().told else {
            // A clock restored without it: where it is has been told.
            self.set_told();
            return None;
        };
        let next = told.saturating_add(1).max(now.saturating_sub(23));
        if next > now {
            return None;
        }
        let mut state = self.now.get();
        state.told = Some(next);
        self.now.set(state);
        // From 0 to 23 by rem_euclid.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(next.rem_euclid(24) as u8)
    }
}

/// The whole days in `hours`, held to some millions of years either way so
/// an hour of 1e300 cannot carry the count past what an i64 holds.
fn whole_days(hours: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    let days = (hours / 24.0).floor().clamp(-1e9, 1e9) as i64;
    days
}

/// The whole hours in `hours`, held to what an i64 holds: no game lives
/// that long.
fn whole_hours(hours: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    let whole = hours.floor().clamp(-9e15, 9e15) as i64;
    whole
}

/// A time of day a script wrote for `game.clock.time` or `game.clock.hour`:
/// the hour, from 0 up to 24.
fn time_said(lua: &Lua, field: &str, value: &Value) -> mlua::Result<f64> {
    const TIME_FORMS: &str = "a time of day on a 24-hour clock, as text from \"0:00\" to \
         \"23:59\" (\"8:00\" is eight in the morning, \"20:30\" half past eight at night, \
         \"24:00\" midnight), or a number of hours from 0 to 24 (8.5 is half past eight)";
    let example = match field {
        "hour" => "game.clock.hour = 8.5",
        "skipTo" => "game.clock.skipTo(\"8:00\")",
        _ => "game.clock.time = \"8:00\"",
    };
    let wrong = |said: String| {
        fail(
            lua,
            format!(
                "game.clock.{field} was given {said}, but it takes {TIME_FORMS}. Write it like \
                 this: {example}"
            ),
        )
    };
    match value {
        // Hours are far below where an f64 stops being exact.
        #[allow(clippy::cast_precision_loss)]
        Value::Integer(number) if (0..=24).contains(number) => Ok(*number as f64),
        Value::Number(number) if (0.0..=24.0).contains(number) => Ok(*number),
        Value::Integer(number) => Err(wrong(number.to_string())),
        Value::Number(number) => Err(wrong(number.to_string())),
        Value::String(text) => {
            let text = text.to_string_lossy();
            parse_time(&text).ok_or_else(|| wrong(format!("\"{text}\"")))
        }
        other => Err(wrong(describe(other).to_owned())),
    }
}

/// "8:00" or "20:30" as hours: hours from 0 to 23 (or "24:00", midnight)
/// and two digits of minutes.
fn parse_time(text: &str) -> Option<f64> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let digits = |part: &str, sizes: std::ops::RangeInclusive<usize>| {
        (sizes.contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let (hours, minutes) = (digits(hours, 1..=2)?, digits(minutes, 2..=2)?);
    if minutes > 59 || hours > 24 || (hours == 24 && minutes > 0) {
        return None;
    }
    Some(f64::from(hours) + f64::from(minutes) / 60.0)
}

/// What a script holds for `game.clock`.
struct ClockRef {
    on_hour: Function,
    skip_to: Function,
}

impl UserData for ClockRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(MetaMethod::ToString, |_, _: AnyUserData| Ok("game.clock"));
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| {
                let shared = shared_of(lua)?;
                let clock = &shared.weather.day;
                let name = match &key {
                    Value::String(name) => name.to_string_lossy(),
                    _ => String::new(),
                };
                Ok(match name.as_str() {
                    "dayLength" => Value::Number(clock.day_minutes()),
                    "time" => match clock.text() {
                        Some(text) => Value::String(lua.create_string(text)?),
                        None => Value::Nil,
                    },
                    "hour" => clock.time_of_day().map_or(Value::Nil, Value::Number),
                    "day" => clock
                        .day()
                        .map_or(Value::Nil, |day| Value::Integer(i64::from(day))),
                    "onHour" => Value::Function(this.borrow::<Self>()?.on_hour.clone()),
                    "skipTo" => Value::Function(this.borrow::<Self>()?.skip_to.clone()),
                    _ => return Err(missing(lua, "game.clock", &key, CLOCK_FIELDS, &[])),
                })
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (_, key, value): (AnyUserData, Value, Value)| {
                let shared = shared_of(lua)?;
                let name = match &key {
                    Value::String(name) => name.to_string_lossy(),
                    _ => String::new(),
                };
                match name.as_str() {
                    "dayLength" => {
                        let minutes = match &value {
                            Value::Number(minutes) if *minutes >= 0.0 => *minutes,
                            // Minutes are far below where an f64 stops being exact.
                            #[allow(clippy::cast_precision_loss)]
                            Value::Integer(minutes) if *minutes >= 0 => *minutes as f64,
                            other => {
                                let said = match other {
                                    Value::Number(n) => n.to_string(),
                                    Value::Integer(n) => n.to_string(),
                                    other => describe(other).to_owned(),
                                };
                                return Err(fail(
                                    lua,
                                    format!(
                                        "game.clock.dayLength was given {said}, but it takes \
                                         how many minutes of play a whole day and night last, \
                                         or 0 to stop the hours passing. Write it like this: \
                                         game.clock.dayLength = 20"
                                    ),
                                ));
                            }
                        };
                        rules_only(lua, &shared, "game.clock.dayLength")?;
                        shared.weather.day.set_day_minutes(minutes);
                    }
                    "time" | "hour" => {
                        let hour = time_said(lua, &name, &value)?;
                        rules_only(lua, &shared, &format!("game.clock.{name}"))?;
                        shared.clock.note(&hour.to_string());
                        shared.weather.day.start_at(hour);
                    }
                    "day" => {
                        return Err(fail(
                            lua,
                            "game.clock.day cannot be set: it counts the days since the game \
                             began, from 1, and moves on at midnight. To pass time, skip to a \
                             time of day, which is tomorrow if it has gone by today: \
                             game.clock.skipTo(\"8:00\")",
                        ));
                    }
                    "onHour" => {
                        return Err(fail(
                            lua,
                            "game.clock.onHour is a function to call, not something to set. \
                             Write it like this: game.clock.onHour(20, function() ... end)",
                        ));
                    }
                    "skipTo" => {
                        return Err(fail(
                            lua,
                            "game.clock.skipTo is a function to call, not something to set. \
                             Write it like this: game.clock.skipTo(\"8:00\")",
                        ));
                    }
                    _ => return Err(missing(lua, "game.clock", &key, CLOCK_FIELDS, &[])),
                }
                Ok(())
            },
        );
    }
}

/// `game.clock.onHour(hour, function)` and `game.clock.onHour(function)`.
fn on_hour(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    const EXAMPLE: &str = "game.clock.onHour(20, function() print(\"Night falls\") end)";
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, (first, second): (Value, Value)| {
        let (hour, handler) = match (first, second) {
            (Value::Function(handler), Value::Nil) => (None, handler),
            (hour, Value::Function(handler)) => {
                let whole = match &hour {
                    Value::Integer(hour) => Some(*hour),
                    // Checked to be whole and small first.
                    #[allow(clippy::cast_possible_truncation)]
                    Value::Number(hour) if hour.fract() == 0.0 && hour.abs() < 1e6 => {
                        Some(*hour as i64)
                    }
                    _ => None,
                };
                match whole {
                    // 24 o'clock is midnight, which is hour 0.
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    Some(hour @ 0..=24) => (Some((hour % 24) as u8), handler),
                    _ => {
                        let said = match &hour {
                            Value::Number(n) => n.to_string(),
                            Value::Integer(n) => n.to_string(),
                            other => describe(other).to_owned(),
                        };
                        return Err(fail(
                            lua,
                            format!(
                                "game.clock.onHour was given {said} for the hour, but it takes \
                                 a whole hour of the day from 0 (midnight) to 23, on a 24-hour \
                                 clock (24 is midnight too): 8 is eight in the morning and 20 \
                                 eight at night. Write \
                                 it like this: {EXAMPLE}. Leave the hour out to hear of every \
                                 hour: game.clock.onHour(function(hour) ... end)"
                            ),
                        ));
                    }
                }
            }
            (other, _) => {
                return Err(fail(
                    lua,
                    format!(
                        "game.clock.onHour needs an hour and then a function to call, but it \
                         was given {}. Write it like this: {EXAMPLE}, or with no hour to hear \
                         of every hour: game.clock.onHour(function(hour) ... end)",
                        describe(&other)
                    ),
                ));
            }
        };
        rules_only(lua, &shared, "game.clock.onHour")?;
        let mut handlers = shared.handlers.borrow_mut();
        if handlers.hour.len() >= MOST_HOUR_HANDLERS {
            return Err(fail(
                lua,
                format!(
                    "game.clock.onHour has been given {MOST_HOUR_HANDLERS} functions already. \
                     Call it once for each hour, at the top of the file, not inside another \
                     function that runs again and again."
                ),
            ));
        }
        handlers.hour.push((hour, handler));
        Ok(())
    })
}

/// `game.clock.skipTo(time)`.
fn skip_to(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, time: Value| {
        let hour = time_said(lua, "skipTo", &time)?;
        rules_only(lua, &shared, "game.clock.skipTo")?;
        shared.clock.note(&hour.to_string());
        shared.weather.day.move_to(hour);
        Ok(())
    })
}

/// `game.clock`.
pub(crate) fn clock(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<AnyUserData> {
    lua.create_userdata(ClockRef {
        on_hour: on_hour(lua, shared)?,
        skip_to: skip_to(lua, shared)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hours: f64) -> DayClock {
        let clock = DayClock::default();
        clock.set_sky_hour(hours);
        clock
    }

    fn due(clock: &DayClock) -> Vec<u8> {
        std::iter::from_fn(|| clock.next_due()).collect()
    }

    #[test]
    fn times_are_read_as_a_24_hour_clock_and_nothing_else() {
        assert_eq!(parse_time("8:00"), Some(8.0));
        assert_eq!(parse_time(" 20:30 "), Some(20.5));
        assert_eq!(parse_time("08:15"), Some(8.25));
        assert_eq!(parse_time("24:00"), Some(24.0));
        for wrong in [
            "25:00", "24:01", "8:60", "8", "eight", "8:5", "8:000", ":30", "-1:00", "8:00pm", "",
        ] {
            assert_eq!(parse_time(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn setting_the_time_moves_forward_to_the_next_time_it_is_that_hour() {
        let clock = DayClock::default();
        assert_eq!(clock.day(), None);
        clock.move_to(20.0);
        assert_eq!((clock.hours(), clock.day()), (Some(20.0), Some(1)));
        // 8:00 has gone by today: it is tomorrow morning, exactly.
        clock.move_to(8.0);
        assert_eq!(clock.hours(), Some(32.0));
        assert_eq!(
            (clock.text().as_deref(), clock.day()),
            (Some("8:00"), Some(2))
        );
        // The same time again goes nowhere.
        clock.move_to(8.0);
        assert_eq!(clock.hours(), Some(32.0));
        // Midnight said as 24 is the start of the next day.
        clock.move_to(24.0);
        assert_eq!((clock.hours(), clock.day()), (Some(48.0), Some(3)));
        let clock = at(20.3);
        clock.move_to(20.5);
        assert_eq!(clock.text().as_deref(), Some("20:30"));
    }

    #[test]
    fn a_start_is_today_at_that_time_and_tells_nothing() {
        let clock = at(30.0);
        clock.start_at(20.0);
        assert_eq!((clock.hours(), clock.day()), (Some(44.0), Some(2)));
        clock.start_at(8.0);
        assert_eq!(clock.hours(), Some(32.0));
        assert!(due(&clock).is_empty());
    }

    #[test]
    fn each_hour_passed_is_told_once_in_order_even_over_midnight() {
        let clock = at(22.5);
        assert!(
            due(&clock).is_empty(),
            "setting a time is not an hour passing"
        );
        clock.move_to(23.9);
        assert_eq!(due(&clock), [23]);
        clock.move_to(2.0);
        assert_eq!(due(&clock), [0, 1, 2]);
        assert!(due(&clock).is_empty());
        // Setting the watch, back or forward, tells nothing, and the hours
        // go on from there.
        clock.start_at(10.2);
        assert!(due(&clock).is_empty());
        clock.set_sky_hour(20.0);
        assert!(due(&clock).is_empty());
        clock.start_at(10.2);
        clock.move_to(11.0);
        assert_eq!(due(&clock), [11]);
        // A week passing in one go tells the last day of hours, once each.
        clock.set_day_minutes(1.0);
        clock.step(7.0 * 60.0);
        assert_eq!(due(&clock), (12..24).chain(0..12).collect::<Vec<u8>>());
    }

    #[test]
    fn an_hour_below_24_said_to_the_sky_sets_the_watch_and_keeps_the_day() {
        let clock = at(30.0);
        assert_eq!(
            (clock.text().as_deref(), clock.day()),
            (Some("6:00"), Some(2))
        );
        // The sky reads its hour as it always has, from its own date.
        clock.set_sky_hour(7.0);
        assert_eq!((clock.hours(), clock.day()), (Some(7.0), Some(2)));
        assert_eq!(clock.days_since_date(), 0);
        // Past 24 runs on into the days after today.
        clock.set_sky_hour(25.0);
        assert_eq!((clock.hours(), clock.day()), (Some(25.0), Some(3)));
        // A new date takes the days the hour held; the clock keeps them.
        clock.start_calendar_today();
        assert_eq!((clock.hours(), clock.day()), (Some(1.0), Some(3)));
        assert!(due(&clock).is_empty(), "setting tells nothing");
    }

    #[test]
    fn the_hours_pass_by_ticks_from_noon_unless_a_time_was_said() {
        let clock = DayClock::default();
        clock.step(1.0);
        assert_eq!(
            clock.hours(),
            None,
            "an unset clock costs and moves nothing"
        );
        // A day of one minute is an hour every 2.5 seconds.
        clock.set_day_minutes(1.0);
        assert_eq!(clock.hours(), Some(12.0));
        for _ in 0..60 {
            clock.step(2.5 / 60.0);
        }
        // A step is an f32, a little off a sixtieth of 2.5 seconds.
        assert!((clock.hours().unwrap_or_default() - 13.0).abs() < 1e-5);
    }
}
