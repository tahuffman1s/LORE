//! Time for scripts: `game.time`, `game.after`, `game.every` and
//! `game.tween`, so a script never counts seconds or reads a clock itself.
//!
//! Everything here is counted in ticks, which are all the same length, and
//! never in real time. A timer fires on the tick its seconds come to, the
//! same tick on every computer and in every replay. Only `game.onDraw`,
//! which runs once per picture, is shown a time between two ticks, so what
//! it reads from a tween moves smoothly however fast pictures are drawn.

use std::cell::Cell;
use std::rc::Rc;

use mlua::{
    AnyUserData, Function, Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value,
};

use crate::api::{Shared, describe, fail};
use crate::character_api::{missing, shared_of};

/// Every function on a timer. The type definitions are tested against it.
#[cfg(test)]
pub(crate) const TIMER_METHODS: &[&str] = &["cancel"];

/// What can be read from a tween.
pub(crate) const TWEEN_FIELDS: &[&str] = &["value", "done"];

/// What a tween can be told to do.
pub(crate) const TWEEN_METHODS: &[&str] = &["to"];

/// How many timers can wait at once. Far more than a game needs: reaching
/// it means a timer is started over and over by mistake.
const MAX_TIMERS: usize = 10_000;

/// The shortest time between two calls of a `game.every` function. One
/// shorter than a tick is called more than once in a tick, so that it is
/// called as often at any tick rate; this keeps that to a handful.
const SHORTEST_REPEAT: f64 = 0.001;

/// The longest a timer or tween can last: about thirty years.
const LONGEST: f64 = 1.0e9;

/// The game's time. It is kept by the host, outside the script's VM, so
/// saving the script does not start it again.
#[derive(Default)]
pub(crate) struct Clock {
    /// How many ticks have been run to their end.
    ticks: Cell<u64>,
    /// How many ticks make a second. 0 until the first tick.
    rate: Cell<f64>,
    /// While `game.onDraw` functions run: how far the picture is from the
    /// tick before last (0) to the last one (1).
    drawing: Cell<Option<f64>>,
    /// Everything the script's rules have done so far, boiled down to one
    /// number: see [`note`](Self::note).
    done: Cell<u64>,
}

/// What [`Clock::note`] multiplies by: the FNV-1a hash's prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

impl Clock {
    /// Notes that the script's rules did `what` (printed a line, called a
    /// function that changes the game) on this tick. What is done while a
    /// picture is drawn is not noted: it is shown, and is no rule.
    ///
    /// Two runs of a game that are given the same input have the same
    /// [`done`](Self::done) after every tick, unless its rules depend on
    /// something they should not, such as how often a picture is drawn.
    pub fn note(&self, what: &str) {
        if self.is_drawing() {
            return;
        }
        let mut hash = self.done.get();
        let mut mix = |byte: u8| hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        self.ticks
            .get()
            .to_le_bytes()
            .into_iter()
            .for_each(&mut mix);
        // Where a table or a function is in memory ("table: 0x55d0c8a1")
        // is not the game's doing and differs from run to run, so the
        // digits of an address are left out.
        let (mut before, mut in_address) = (0, false);
        for &byte in what.as_bytes() {
            if in_address && byte.is_ascii_hexdigit() {
                continue;
            }
            in_address = before == b'0' && byte == b'x';
            before = byte;
            mix(byte);
        }
        // Keeps "ab" then "c" apart from "a" then "bc".
        mix(0xff);
        self.done.set(hash);
    }

    /// One number for everything [`note`](Self::note) was told, in order.
    pub fn done(&self) -> u64 {
        self.done.get()
    }

    /// The tick being run, counted from 0, or about to be.
    pub fn tick(&self) -> u64 {
        self.ticks.get()
    }

    /// How many ticks make a second, or 0 before the first tick.
    pub fn rate(&self) -> f64 {
        self.rate.get()
    }

    /// Notes the length in seconds of the tick that is starting.
    ///
    /// The engine hands the step over as an `f32`, where a sixtieth of a
    /// second is a little off. A step that is a whole number of ticks a
    /// second, as near as an `f32` can say, is taken to be just that, so
    /// that 60 ticks are 1 second and not 1.00000005.
    pub fn begin_tick(&self, step: f32) {
        let rate = 1.0 / f64::from(step);
        let whole = rate.round();
        self.rate.set(if !(rate.is_finite() && rate > 0.0) {
            0.0
        } else if whole >= 1.0 && (rate - whole).abs() <= whole * 1e-5 {
            whole
        } else {
            rate
        });
    }

    /// Notes that a tick has been run.
    pub fn end_tick(&self) {
        self.ticks.set(self.ticks.get().saturating_add(1));
    }

    /// Whether `game.onDraw` functions are being run.
    pub fn is_drawing(&self) -> bool {
        self.drawing.get().is_some()
    }

    /// Starts (`Some`) or ends (`None`) the drawing of a picture that is
    /// `alpha` of the way from the tick before last to the last one.
    pub fn set_drawing(&self, alpha: Option<f32>) {
        self.drawing.set(alpha.map(|alpha| {
            if alpha.is_finite() {
                f64::from(alpha).clamp(0.0, 1.0)
            } else {
                1.0
            }
        }));
    }

    /// The seconds the game has run: a whole number of ticks in the game's
    /// rules, and the moment the picture shows in `game.onDraw`.
    // A game does not run for the 2^53 ticks it takes to lose precision.
    #[allow(clippy::cast_precision_loss)]
    pub fn now(&self) -> f64 {
        let rate = self.rate.get();
        if rate <= 0.0 {
            return 0.0;
        }
        let ticks = self.ticks.get() as f64;
        let ticks = match self.drawing.get() {
            // The picture is drawn between the last two ticks, like the view.
            Some(alpha) => (ticks - 1.0 + alpha).max(0.0),
            None => ticks,
        };
        ticks / rate
    }
}

/// A function waiting to be called by `game.after` or `game.every`.
pub(crate) struct Timer {
    id: u64,
    pub call: Function,
    seconds: f64,
    repeats: bool,
    /// The tick it was started on.
    made_at: u64,
    /// How many times it has been called.
    fired: u64,
    /// Set by `timer:cancel()`, and once a `game.after` function was called.
    pub finished: bool,
    /// Started while the file was loading. The next version of the script
    /// starts such a timer again when it loads; any other is lost by a save.
    pub from_loading: bool,
}

impl Timer {
    /// The tick of its next call, when `rate` ticks make a second: the first
    /// tick that is not before its time, so a timer is never early.
    ///
    /// Counted from the tick it was started on and not from its last call,
    /// so a repeating timer keeps its beat for ever: 0.5 seconds at 60 ticks
    /// a second is every 30th tick exactly. Never the tick it was started on.
    // Seconds are checked when the timer is made, and a number too large
    // for a tick count becomes the largest one.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn due(&self, rate: f64) -> u64 {
        /// How much of a tick is put down to rounding: 0.1 seconds at 60
        /// ticks a second comes to a hair over 6 ticks, and is 6.
        const ROUNDING: f64 = 1e-6;
        let calls = (self.fired + 1) as f64;
        let ticks = (calls * self.seconds * rate - ROUNDING).ceil().max(1.0) as u64;
        self.made_at.saturating_add(ticks)
    }

    /// Whether it is still waiting for its first call.
    pub fn never_called(&self) -> bool {
        self.fired == 0 && !self.finished
    }

    /// Notes that it is being called now.
    pub fn fire(&mut self) {
        self.fired += 1;
        self.finished = !self.repeats;
    }
}

/// The error for changing the game from `game.onDraw`, if that is where
/// `function` is being called from.
///
/// A picture is drawn more often on a faster computer,
/// so a rule run from there would make the game differ between them.
pub(crate) fn rules_only(lua: &Lua, shared: &Shared, function: &str) -> mlua::Result<()> {
    if !shared.clock.is_drawing() {
        shared.clock.note(function);
        return Ok(());
    }
    Err(fail(
        lua,
        format!(
            "{function} changes the game, and game.onDraw is only for showing it: it runs \
             once for every picture drawn, so more often on a faster computer. Move this line into game.onTick(function() ... end), or another \
             game.on... function. Reading is fine here: game.time(), tween.value, \
             game.player.ammo."
        ),
    ))
}

/// A number of seconds a script wrote, as `function` takes it.
fn seconds(lua: &Lua, function: &str, value: &Value, example: &str) -> mlua::Result<f64> {
    // Whole seconds this large are well within what an f64 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let seconds = match value {
        Value::Number(seconds) => *seconds,
        Value::Integer(seconds) => *seconds as f64,
        other => {
            return Err(fail(
                lua,
                format!(
                    "{function} needs a number of seconds, but it was given {}. Write it \
                     like this: {example}",
                    describe(other)
                ),
            ));
        }
    };
    if !(0.0..=LONGEST).contains(&seconds) {
        return Err(fail(
            lua,
            format!(
                "{function} needs a number of seconds from 0 up, but it was given \
                 {seconds}. Write it like this: {example}"
            ),
        ));
    }
    Ok(seconds)
}

/// A plain number a script wrote for `name` in `game.tween` or `tween:to`.
fn number(
    lua: &Lua,
    function: &str,
    name: &str,
    value: &Value,
    example: &str,
) -> mlua::Result<f64> {
    // A script's whole numbers are small enough to be exact as an f64.
    #[allow(clippy::cast_precision_loss)]
    let number = match value {
        Value::Number(number) if number.is_finite() => *number,
        Value::Integer(number) => *number as f64,
        Value::Number(number) => {
            return Err(fail(
                lua,
                format!(
                    "{function} needs an ordinary number for `{name}`, but it was given \
                     {number}. That usually comes from dividing by zero a few lines \
                     earlier."
                ),
            ));
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "{function} needs a number for `{name}`, but it was given {}. Write \
                     it like this: {example}",
                    describe(other)
                ),
            ));
        }
    };
    Ok(number)
}

/// What a script holds for a timer it started.
#[derive(Clone, Copy)]
struct TimerRef(u64);

impl UserData for TimerRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("cancel", |lua, this: Value| {
            let id = match &this {
                Value::UserData(this) => this.borrow::<Self>().ok().map(|this| this.0),
                _ => None,
            };
            let Some(id) = id else {
                return Err(fail(
                    lua,
                    "timer.cancel needs a colon, not a dot, because it acts on one timer. \
                     Write it like this: timer:cancel()",
                ));
            };
            let shared = shared_of(lua)?;
            rules_only(lua, &shared, "timer:cancel")?;
            // Cancelling a timer that has finished does nothing, so a script
            // never has to ask whether it is still waiting.
            if let Some(timer) = shared
                .timers
                .borrow_mut()
                .iter_mut()
                .find(|timer| timer.id == id)
            {
                timer.finished = true;
            }
            Ok(())
        });
        methods.add_meta_function(MetaMethod::ToString, |_, _: AnyUserData| Ok("a timer"));
        methods.add_meta_function(MetaMethod::Index, |lua, (_, key): (Value, Value)| {
            Err::<(), _>(fail(
                lua,
                format!(
                    "timer.{} does not exist. A timer has one function, which calls it \
                     off: timer:cancel()",
                    key_text(&key)
                ),
            ))
        });
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (_, key, _): (Value, Value, Value)| {
                Err::<(), _>(fail(
                    lua,
                    format!(
                        "timer.{} cannot be set. A timer can only be stopped, with \
                         timer:cancel(); for another wait, start a new one with game.after.",
                        key_text(&key)
                    ),
                ))
            },
        );
    }
}

fn key_text(key: &Value) -> String {
    match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    }
}

/// `game.after(seconds, function)` and `game.every(seconds, function)`.
fn timer_function(lua: &Lua, shared: &Rc<Shared>, repeats: bool) -> mlua::Result<Function> {
    let shared = Rc::clone(shared);
    let (function, example) = if repeats {
        (
            "game.every",
            "game.every(0.5, function() print(\"tick tock\") end)",
        )
    } else {
        ("game.after", "game.after(3, function() print(\"Go!\") end)")
    };
    lua.create_function(move |lua, (wait, call): (Value, Value)| {
        rules_only(lua, &shared, function)?;
        // The two the other way round is the likeliest slip.
        if matches!(wait, Value::Function(_)) {
            return Err(fail(
                lua,
                format!(
                    "{function} takes the seconds first and the function second. Write it \
                     like this: {example}"
                ),
            ));
        }
        let wait = seconds(lua, function, &wait, example)?;
        if repeats && wait < SHORTEST_REPEAT {
            return Err(fail(
                lua,
                format!(
                    "game.every needs at least {SHORTEST_REPEAT} seconds between calls, but \
                     it was given {wait}. For something that happens on every tick, use \
                     game.onTick(function() ... end)."
                ),
            ));
        }
        let Value::Function(call) = call else {
            return Err(fail(
                lua,
                format!(
                    "{function} needs a function to call after the seconds, but it was \
                     given {}. Write it like this: {example}",
                    describe(&call)
                ),
            ));
        };
        let mut timers = shared.timers.borrow_mut();
        if timers.len() >= MAX_TIMERS {
            return Err(fail(
                lua,
                format!(
                    "{MAX_TIMERS} timers are already waiting, so {function} did not start \
                     another. This usually means a timer is started on every tick: start \
                     it once, when the thing it waits for happens."
                ),
            ));
        }
        let id = shared.next_timer.get();
        shared.next_timer.set(id + 1);
        timers.push(Timer {
            id,
            call,
            seconds: wait,
            repeats,
            made_at: shared.clock.tick(),
            fired: 0,
            finished: false,
            from_loading: shared.loading.get(),
        });
        drop(timers);
        lua.create_userdata(TimerRef(id))
    })
}

/// A number on its way from one value to another: what a script holds for
/// `game.tween`. Where it is follows from the game's time alone, so it costs
/// nothing while nobody reads it and is the same on every computer.
struct Tween {
    from: Cell<f64>,
    to: Cell<f64>,
    /// The game time it set off at, in seconds.
    start: Cell<f64>,
    seconds: Cell<f64>,
}

impl Tween {
    /// Where the number is at the game time `now`.
    fn value(&self, now: f64) -> f64 {
        let (from, to) = (self.from.get(), self.to.get());
        let passed = now - self.start.get();
        if passed >= self.seconds.get() {
            return to;
        }
        let part = (passed / self.seconds.get()).clamp(0.0, 1.0);
        let value = from + (to - from) * part;
        if value.is_finite() {
            value
        } else {
            // The two are further apart than a number can hold.
            from * (1.0 - part) + to * part
        }
    }

    fn done(&self, now: f64) -> bool {
        now - self.start.get() >= self.seconds.get()
    }
}

impl UserData for Tween {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("value", |lua, this| {
            Ok(this.value(shared_of(lua)?.clock.now()))
        });
        fields.add_field_method_get("done", |lua, this| {
            Ok(this.done(shared_of(lua)?.clock.now()))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("to", |lua, (this, target, time): (Value, Value, Value)| {
            const EXAMPLE: &str = "tween:to(100, 0.5)";
            let this = match &this {
                Value::UserData(this) => this.borrow::<Self>().ok(),
                _ => None,
            };
            let Some(this) = this else {
                return Err(fail(
                    lua,
                    "tween.to needs a colon, not a dot, because it acts on one tween. \
                     Write it like this: tween:to(100, 0.5)",
                ));
            };
            let shared = shared_of(lua)?;
            rules_only(lua, &shared, "tween:to")?;
            let target = number(lua, "tween:to", "target", &target, EXAMPLE)?;
            let time = seconds(lua, "tween:to", &time, EXAMPLE)?;
            let now = shared.clock.now();
            // From wherever it has got to, so it never jumps.
            this.from.set(this.value(now));
            this.to.set(target);
            this.start.set(now);
            this.seconds.set(time);
            Ok(())
        });
        methods.add_meta_function(MetaMethod::ToString, |_, _: AnyUserData| Ok("a tween"));
        methods.add_meta_function(MetaMethod::Index, |lua, (_, key): (Value, Value)| {
            Err::<(), _>(missing(lua, "tween", &key, TWEEN_FIELDS, TWEEN_METHODS))
        });
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (_, key, _): (Value, Value, Value)| {
                Err::<(), _>(fail(
                    lua,
                    format!(
                        "tween.{} cannot be set. To send a tween somewhere else, write: \
                         tween:to(100, 0.5). With 0 seconds it gets there at once.",
                        key_text(&key)
                    ),
                ))
            },
        );
    }
}

/// `game.tween(from, to, seconds)`.
fn tween_function(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    const EXAMPLE: &str = "local shown = game.tween(0, 100, 1)";
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, (from, to, time): (Value, Value, Value)| {
        rules_only(lua, &shared, "game.tween")?;
        let from = number(lua, "game.tween", "from", &from, EXAMPLE)?;
        let to = number(lua, "game.tween", "to", &to, EXAMPLE)?;
        let time = seconds(lua, "game.tween", &time, EXAMPLE)?;
        lua.create_userdata(Tween {
            from: Cell::new(from),
            to: Cell::new(to),
            start: Cell::new(shared.clock.now()),
            seconds: Cell::new(time),
        })
    })
}

/// Puts `time`, `after`, `every` and `tween` on `game`, and takes the
/// computer's clock away from the script.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, game: &Table) -> mlua::Result<()> {
    let clock = Rc::clone(&shared.clock);
    game.set("time", lua.create_function(move |_, ()| Ok(clock.now()))?)?;
    game.set("after", timer_function(lua, shared, false)?)?;
    game.set("every", timer_function(lua, shared, true)?)?;
    game.set("tween", tween_function(lua, shared)?)?;

    // Luau's `os` holds nothing but the computer's clock and calendar. A
    // rule that read them would play out differently on every computer and
    // in every replay, so each says what to use instead.
    let os = lua.create_table()?;
    for name in ["clock", "time", "date", "difftime"] {
        os.set(
            name,
            lua.create_function(move |lua, _: mlua::MultiValue| -> mlua::Result<()> {
                let instead = if name == "date" || name == "time" {
                    "A game cannot read the real date or time of day. For how long the \
                     game has been running, use game.time()."
                } else {
                    "Use game.time(), the seconds the game has been running, or a timer: \
                     game.after(3, function() ... end)."
                };
                Err(fail(
                    lua,
                    format!(
                        "os.{name} reads this computer's clock, which a game's rules cannot \
                         use: the game would play differently on every computer and every \
                         time it is replayed. {instead}"
                    ),
                ))
            })?,
        )?;
    }
    lua.globals().set("os", os)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_rules_did_is_one_number_that_leaves_out_pictures_and_addresses() {
        let done = |notes: &[&str]| {
            let clock = clock_at(3, 1.0 / 60.0);
            notes.iter().for_each(|note| clock.note(note));
            clock.done()
        };
        assert_eq!(done(&[]), 0);
        assert_ne!(done(&["game.after"]), 0);
        assert_eq!(done(&["a", "b"]), done(&["a", "b"]));
        assert_ne!(done(&["a", "b"]), done(&["b", "a"]));
        assert_ne!(done(&["ab", "c"]), done(&["a", "bc"]));
        // The same thing done on another tick is not the same game.
        let later = clock_at(4, 1.0 / 60.0);
        later.note("a");
        assert_ne!(later.done(), done(&["a"]));

        // A table printed twice is in two places in memory.
        assert_eq!(
            done(&["it is table: 0x000055d0c8a1f3b0 now"]),
            done(&["it is table: 0x00007f11aa now"])
        );
        assert_ne!(done(&["table: 0x55d0"]), done(&["function: 0x55d0"]));
        assert_ne!(done(&["10 x 20"]), done(&["10 x 30"]));

        // What is done while a picture is drawn is not a rule.
        let clock = clock_at(3, 1.0 / 60.0);
        clock.note("a");
        clock.set_drawing(Some(0.5));
        clock.note("b");
        clock.set_drawing(None);
        assert_eq!(clock.done(), done(&["a"]));
    }

    fn clock_at(ticks: u64, step: f32) -> Clock {
        let clock = Clock::default();
        clock.begin_tick(step);
        for _ in 0..ticks {
            clock.end_tick();
        }
        clock
    }

    #[test]
    fn the_time_in_a_picture_is_between_the_last_two_ticks() {
        let clock = clock_at(10, 0.5);
        assert!((clock.now() - 5.0).abs() < 1e-12);
        clock.set_drawing(Some(0.5));
        assert!((clock.now() - 4.75).abs() < 1e-12);
        clock.set_drawing(Some(f32::NAN));
        assert!((clock.now() - 5.0).abs() < 1e-12);
        clock.set_drawing(None);
        assert!((clock.now() - 5.0).abs() < 1e-12);
        // Before the first tick there is nothing earlier to show.
        let clock = clock_at(0, 0.5);
        clock.set_drawing(Some(0.25));
        assert!(clock.now().abs() < 1e-12);
    }

    #[test]
    fn a_tween_ends_exactly_on_its_target() {
        let tween = Tween {
            from: Cell::new(0.0),
            to: Cell::new(0.3),
            start: Cell::new(1.0),
            seconds: Cell::new(2.0),
        };
        assert!(tween.value(0.5).abs() < 1e-12);
        assert!((tween.value(2.0) - 0.15).abs() < 1e-12);
        assert!(!tween.done(2.9));
        assert!(tween.done(3.0));
        assert_eq!(tween.value(3.0).to_bits(), 0.3_f64.to_bits());
        // Ends too far apart to subtract are still a number in between.
        let wide = Tween {
            from: Cell::new(-1e308),
            to: Cell::new(1e308),
            start: Cell::new(0.0),
            seconds: Cell::new(1.0),
        };
        assert!(wide.value(0.0).is_finite() && wide.value(0.5).abs() < 1e300);
        // No time at all is not a division by zero.
        tween.seconds.set(0.0);
        assert_eq!(tween.value(1.0).to_bits(), 0.3_f64.to_bits());
    }
}
