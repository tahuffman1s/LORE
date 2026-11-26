//! `piece:animate`: moving numbers and colours of a HUD piece to new
//! values over a few seconds of the game's time.
//!
//! Animations are stepped once per game tick, counted in ticks and never in
//! wall time, so the same input gives the same values tick for tick.

use std::fmt::Write as _;

use lore_ui::Color;
use mlua::{Function, Lua, Value};

use super::{Entry, HudState, Kind, PieceRef, capitalised, color_of, hud_of, key_text};
use crate::api::{describe, fail};
use crate::error::closest;

/// The fields of a piece that hold a number and can be animated.
const NUMBERS: &[&str] = &[
    "right",
    "down",
    "width",
    "height",
    "opacity",
    "scale",
    "rotation",
    "borderRadius",
    "borderWidth",
    "letterSpacing",
    "value",
    "thickness",
    "toRight",
    "toDown",
];

/// The fields that hold a colour and can be animated.
const COLORS: &[&str] = &["color", "borderColor"];

/// What `animate` takes in its second table.
const OPTIONS: &[&str] = &["seconds", "ease", "delay", "onDone"];

/// The longest an animation or its delay can be, in seconds.
const LONGEST: f64 = 3600.0;

const DEFAULT_SECONDS: f64 = 0.3;

/// The shapes a change can take over time, named as in a style sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ease {
    Linear,
    Standard,
    In,
    Out,
    InOut,
}

const EASES: &[(&str, Ease)] = &[
    ("linear", Ease::Linear),
    ("ease", Ease::Standard),
    ("ease-in", Ease::In),
    ("ease-out", Ease::Out),
    ("ease-in-out", Ease::InOut),
];

impl Ease {
    /// The control points `[x1, y1, x2, y2]` of the curve, `None` for the
    /// straight line.
    fn points(self) -> Option<[f64; 4]> {
        match self {
            Self::Linear => None,
            Self::Standard => Some([0.25, 0.1, 0.25, 1.0]),
            Self::In => Some([0.42, 0.0, 1.0, 1.0]),
            Self::Out => Some([0.0, 0.0, 0.58, 1.0]),
            Self::InOut => Some([0.42, 0.0, 0.58, 1.0]),
        }
    }

    /// How far along the change is when `t` of the time has passed, both
    /// from 0 to 1. The ends are exact.
    pub(crate) fn at(self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        let Some([x1, y1, x2, y2]) = self.points() else {
            return t;
        };
        // One side of the cubic Bezier from (0, 0) to (1, 1).
        let side = |a: f64, b: f64, s: f64| {
            let u = 1.0 - s;
            3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
        };
        let slope = |a: f64, b: f64, s: f64| {
            let u = 1.0 - s;
            3.0 * u * u * a + 6.0 * u * s * (b - a) + 3.0 * s * s * (1.0 - b)
        };
        // Newton's method for the s whose x is `t`, then bisection if it
        // does not settle. Both run a fixed number of steps, so the answer
        // is the same on every machine and every run.
        let mut s = t;
        for _ in 0..8 {
            let error = side(x1, x2, s) - t;
            if error.abs() < 1e-12 {
                return side(y1, y2, s);
            }
            let rate = slope(x1, x2, s);
            if rate.abs() < 1e-6 {
                break;
            }
            s -= error / rate;
        }
        let (mut low, mut high) = (0.0, 1.0);
        s = t;
        for _ in 0..60 {
            if side(x1, x2, s) < t {
                low = s;
            } else {
                high = s;
            }
            s = (low + high) / 2.0;
        }
        side(y1, y2, s)
    }
}

#[derive(Debug, Clone, Copy)]
enum Target {
    Number { from: f32, to: f32 },
    Colour { from: Color, to: Color },
}

#[derive(Debug, Clone, Copy)]
struct Goal {
    field: &'static str,
    target: Target,
    /// False once a newer animation has taken the field over.
    live: bool,
}

/// One call of `piece:animate`, running.
#[derive(Debug)]
pub(crate) struct Animation {
    pub(super) id: u64,
    goals: Vec<Goal>,
    seconds: f64,
    delay: f64,
    ease: Ease,
    on_done: Option<Function>,
    /// How many ticks it has been stepped.
    elapsed: u64,
    /// The delay and the length in ticks, counted at the rate of its first
    /// step so a change of rate does not make it jump.
    ticks: Option<(u64, u64)>,
}

/// Seconds as a number of ticks: the first tick that is not before the
/// time, as a timer counts, and at least `least`.
// Seconds are held to a few hours, so the cast cannot overflow.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn ticks(seconds: f64, rate: f64, least: u64) -> u64 {
    const ROUNDING: f64 = 1e-6;
    ((seconds * rate - ROUNDING).ceil().max(0.0) as u64).max(least)
}

/// A value part of the way from `from` to `to`.
#[allow(clippy::cast_possible_truncation)]
fn between(from: f32, to: f32, along: f64) -> f32 {
    (f64::from(from) + (f64::from(to) - f64::from(from)) * along) as f32
}

impl HudState {
    /// Starts an animation. A field it moves that another animation of the
    /// piece was moving is the new one's from now on.
    fn start_animation(&self, animation: Animation) {
        let mut running = self.animations.borrow_mut();
        for old in running.iter_mut().filter(|old| old.id == animation.id) {
            for goal in &mut old.goals {
                goal.live &= !animation.goals.iter().any(|new| new.field == goal.field);
            }
        }
        // One with nothing left to move is dropped, not finished.
        running.retain(|old| old.goals.iter().any(|goal| goal.live));
        running.push(animation);
    }

    /// Moves every animation on one tick, `rate` ticks to the second.
    /// Pieces that were removed lose their animations; the functions of
    /// those that finished wait for [`next_done`](Self::next_done).
    pub(crate) fn step_animations(&self, rate: f64) {
        let mut running = self.animations.borrow_mut();
        if running.is_empty() || rate <= 0.0 {
            return;
        }
        let mut done = self.done.borrow_mut();
        running.retain_mut(|animation| {
            animation.elapsed += 1;
            let (delay, total) = *animation.ticks.get_or_insert_with(|| {
                (
                    ticks(animation.delay, rate, 0),
                    ticks(animation.seconds, rate, 1),
                )
            });
            if animation.elapsed <= delay {
                return true;
            }
            let run = animation.elapsed - delay;
            let finished = run >= total;
            #[allow(clippy::cast_precision_loss)]
            let along = animation.ease.at(run as f64 / total as f64);
            let goals = &animation.goals;
            let found = self.with(animation.id, |entry| {
                let mut changed = false;
                for goal in goals.iter().filter(|goal| goal.live) {
                    changed |= match goal.target {
                        Target::Number { from, to } => entry.set_number(
                            goal.field,
                            if finished {
                                to
                            } else {
                                between(from, to, along)
                            },
                        ),
                        Target::Colour { from, to } => entry.set_color(
                            goal.field,
                            if finished {
                                to
                            } else {
                                Color {
                                    r: between(from.r, to.r, along),
                                    g: between(from.g, to.g, along),
                                    b: between(from.b, to.b, along),
                                    a: between(from.a, to.a, along),
                                }
                            },
                        ),
                    };
                }
                if changed {
                    entry.ruled.copy(&entry.piece);
                }
                ((), changed)
            });
            if found.is_none() {
                return false;
            }
            if finished && let Some(function) = &animation.on_done {
                done.push(function.clone());
            }
            !finished
        });
    }

    /// The oldest function of a finished animation that has not been
    /// called, taken off the list.
    pub(crate) fn next_done(&self) -> Option<Function> {
        let mut done = self.done.borrow_mut();
        (!done.is_empty()).then(|| done.remove(0))
    }
}

/// The fields `entry` can animate.
fn animatable(entry: &Entry) -> Vec<&'static str> {
    NUMBERS
        .iter()
        .filter(|field| entry.number(field).is_some())
        .chain(COLORS.iter().filter(|field| entry.colour(field).is_some()))
        .copied()
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn number_of(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(whole) => Some(*whole as f64),
        Value::Number(number) if number.is_finite() => Some(*number),
        _ => None,
    }
}

/// The pairs of a table, in the order of their names, so that a mistake is
/// the same one every run.
fn sorted(table: &mlua::Table) -> mlua::Result<Vec<(String, Value)>> {
    let mut pairs = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        pairs.push((key_text(&key), value));
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(pairs)
}

/// What `piece:animate(goals, options)` does.
pub(super) fn animate(
    lua: &Lua,
    (this, goals, options): (Value, Value, Value),
) -> mlua::Result<()> {
    let piece = match &this {
        Value::UserData(this) => this.borrow::<PieceRef>().ok().map(|this| *this),
        _ => None,
    };
    let Some(PieceRef { id, kind }) = piece else {
        return Err(fail(
            lua,
            "animate needs a colon, not a dot, because it acts on one piece of the HUD. \
             Write it like this: health:animate({ opacity = 0 }, { seconds = 0.5 })",
        ));
    };
    let shared = hud_of(lua)?;
    crate::time_api::rules_only(lua, &shared, "piece:animate")?;
    let word = kind.word();
    let example = format!("{word}:animate({{ opacity = 0 }}, {{ seconds = 0.5 }})");
    let Value::Table(goals) = goals else {
        return Err(fail(
            lua,
            format!(
                "animate needs a table of what to move to, but it was given {}. Write it \
                 like this: {example}",
                describe(&goals)
            ),
        ));
    };
    let goals = sorted(&goals)?;
    if goals.is_empty() {
        return Err(fail(
            lua,
            format!("animate was given no fields to move. Write it like this: {example}"),
        ));
    }
    let can = shared
        .hud
        .with(id, |entry| (animatable(entry), false))
        .ok_or_else(|| super::removed(lua, kind))?;
    if can.is_empty() {
        return Err(fail(
            lua,
            format!(
                "{} has nothing to animate: it is drawn by the engine.",
                capitalised(kind.name())
            ),
        ));
    }
    let mut made = Vec::with_capacity(goals.len());
    for (key, value) in &goals {
        let Some(&field) = NUMBERS.iter().chain(COLORS).find(|field| **field == key) else {
            return Err(unknown_field(lua, kind, key, &can, &example));
        };
        if !can.contains(&field) {
            return Err(fail(
                lua,
                format!(
                    "{} has no `{field}` to animate. It can animate: {}.",
                    capitalised(kind.name()),
                    can.join(", ")
                ),
            ));
        }
        let what = format!("animate's `{field}`");
        let target = if COLORS.contains(&field) {
            let (to, _) = color_of(lua, &what, value)?;
            let from = shared
                .hud
                .with(id, |entry| (entry.colour(field), false))
                .flatten()
                .unwrap_or(to);
            Target::Colour { from, to }
        } else {
            let Some(to) = number_of(value) else {
                return Err(fail(
                    lua,
                    format!(
                        "{what} needs a number to move to, but it was given {}. Write it \
                         like this: {word}:animate({{ {field} = 1 }})",
                        describe(value)
                    ),
                ));
            };
            #[allow(clippy::cast_possible_truncation)]
            let to = to as f32;
            let from = shared
                .hud
                .with(id, |entry| (entry.number(field), false))
                .flatten()
                .unwrap_or(to);
            Target::Number { from, to }
        };
        made.push(Goal {
            field,
            target,
            live: true,
        });
    }
    let (mut seconds, mut delay, mut ease, mut on_done) =
        (DEFAULT_SECONDS, 0.0, Ease::Standard, None);
    match options {
        Value::Nil => {}
        Value::Table(options) => {
            for (key, value) in sorted(&options)? {
                match key.as_str() {
                    "seconds" | "delay" => {
                        let Some(number) =
                            number_of(&value).filter(|n| (0.0..=LONGEST).contains(n))
                        else {
                            return Err(fail(
                                lua,
                                format!(
                                    "animate's `{key}` needs a number of seconds from 0 to \
                                     {LONGEST}, but it was given {}. Write it like this: \
                                     {word}:animate({{ opacity = 0 }}, {{ {key} = 0.5 }})",
                                    describe(&value)
                                ),
                            ));
                        };
                        if key == "seconds" {
                            seconds = number;
                        } else {
                            delay = number;
                        }
                    }
                    "ease" => ease = ease_of(lua, &value, word)?,
                    "onDone" => match value {
                        Value::Function(function) => on_done = Some(function),
                        Value::Nil => {}
                        other => {
                            return Err(fail(
                                lua,
                                format!(
                                    "animate's `onDone` needs a function, but it was given \
                                     {}. Write it like this: {word}:animate({{ opacity = 0 }}, \
                                     {{ onDone = function() {word}:remove() end }})",
                                    describe(&other)
                                ),
                            ));
                        }
                    },
                    _ => {
                        let mut message = format!("animate has no option called `{key}`.");
                        match closest(&key, OPTIONS.iter().copied()) {
                            Some(near) => {
                                let _ = write!(message, " Did you mean `{near}`?");
                            }
                            None => {
                                let _ =
                                    write!(message, " Its options are: {}.", OPTIONS.join(", "));
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
                    "animate's second argument is a table of options, but it was given {}. \
                     Write it like this: {example}",
                    describe(&other)
                ),
            ));
        }
    }
    shared.hud.start_animation(Animation {
        id,
        goals: made,
        seconds,
        delay,
        ease,
        on_done,
        elapsed: 0,
        ticks: None,
    });
    Ok(())
}

fn ease_of(lua: &Lua, value: &Value, word: &str) -> mlua::Result<Ease> {
    let names = || {
        EASES
            .iter()
            .map(|(name, _)| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let Value::String(written) = value else {
        return Err(fail(
            lua,
            format!(
                "animate's `ease` needs a name in quotes, but it was given {}. The names \
                 are: {}.",
                describe(value),
                names()
            ),
        ));
    };
    let name = written.to_string_lossy();
    let wanted = name.trim().to_lowercase();
    if let Some((_, ease)) = EASES.iter().find(|(known, _)| *known == wanted) {
        return Ok(*ease);
    }
    let mut message = format!("animate has no ease called \"{name}\".");
    match closest(&wanted, EASES.iter().map(|(known, _)| *known)) {
        Some(near) => {
            let _ = write!(
                message,
                " Did you mean \"{near}\"? Write it like this: {word}:animate({{ opacity = 0 }}, \
                 {{ ease = \"{near}\" }})"
            );
        }
        None => {
            let _ = write!(message, " The names are: {}.", names());
        }
    }
    Err(fail(lua, message))
}

fn unknown_field(lua: &Lua, kind: Kind, key: &str, can: &[&str], example: &str) -> mlua::Error {
    let mut message = format!(
        "{} has nothing called `{key}` to animate.",
        capitalised(kind.name())
    );
    match closest(key, can.iter().copied()) {
        Some(near) => {
            let _ = write!(message, " Did you mean `{near}`?");
        }
        None if key == "visible" => message.push_str(
            " `visible` is on or off; to fade a piece in or out, animate `opacity` from 1 to 0.",
        ),
        None => {
            let _ = write!(
                message,
                " It can animate: {}. Write it like this: {example}",
                can.join(", ")
            );
        }
    }
    fail(lua, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Ease; 5] = [
        Ease::Linear,
        Ease::Standard,
        Ease::In,
        Ease::Out,
        Ease::InOut,
    ];

    #[test]
    fn every_curve_starts_at_0_and_ends_at_exactly_1() {
        for ease in ALL {
            assert_eq!(ease.at(0.0), 0.0);
            assert_eq!(ease.at(1.0), 1.0);
            assert_eq!(ease.at(-3.0), 0.0);
            assert_eq!(ease.at(7.0), 1.0);
        }
    }

    #[test]
    fn the_middles_are_where_the_css_curves_put_them() {
        let near = |got: f64, want: f64| assert!((got - want).abs() < 1e-3, "{got} vs {want}");
        near(Ease::Linear.at(0.5), 0.5);
        near(Ease::Standard.at(0.5), 0.8024);
        near(Ease::In.at(0.5), 0.3154);
        near(Ease::Out.at(0.5), 0.6846);
        near(Ease::InOut.at(0.5), 0.5);
        near(Ease::Standard.at(0.25), 0.4085);
    }

    #[test]
    fn curves_never_go_back() {
        for ease in ALL {
            let mut last = 0.0;
            for step in 0..=1000 {
                let value = ease.at(f64::from(step) / 1000.0);
                assert!(value >= last, "{ease:?} at {step}");
                last = value;
            }
        }
    }

    #[test]
    fn a_change_of_rate_does_not_make_a_running_animation_jump() {
        let state = HudState::default();
        let id = state
            .add(
                crate::hud::HudPiece {
                    id: 0,
                    shape: crate::hud::HudShape::Box { size: [1.0, 1.0] },
                    anchor: lore_ui::Anchor::TopLeft,
                    offset: [0.0; 2],
                    color: Color::WHITE,
                    visible: true,
                    layer: 0,
                    style: None,
                },
                String::new(),
                None,
            )
            .unwrap();
        state.start_animation(Animation {
            id,
            goals: vec![Goal {
                field: "right",
                target: Target::Number {
                    from: 0.0,
                    to: 100.0,
                },
                live: true,
            }],
            seconds: 1.0,
            delay: 0.0,
            ease: Ease::Linear,
            on_done: None,
            elapsed: 0,
            ticks: None,
        });
        for _ in 0..5 {
            state.step_animations(10.0);
        }
        state.step_animations(100.0);
        let right = state.with(id, |entry| (entry.piece.offset[0], false));
        assert_eq!(right, Some(60.0));
    }

    #[test]
    fn seconds_become_ticks_without_rounding_up_a_hair() {
        assert_eq!(ticks(0.5, 60.0, 1), 30);
        assert_eq!(ticks(0.1, 60.0, 1), 6);
        assert_eq!(ticks(0.0, 60.0, 1), 1);
        assert_eq!(ticks(0.0, 60.0, 0), 0);
    }
}
