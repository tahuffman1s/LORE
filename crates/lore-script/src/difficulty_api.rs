//! How hard the game is, for scripts: `game.difficulty`.
//!
//! Difficulty is one dial with named steps, easiest first: the engine's
//! own are "Story", "Standard" and "Hard", and a game can name its own.
//! Where the dial stands is game state, kept here outside the script's VM:
//! it changes only on a tick (a script sets it, or the player turns it in
//! the pause menu, which is a script too), so a replay of the same input
//! is the same game. The player's "difficulty" setting says where a new
//! game starts, and follows each turn of the dial, so the next game starts
//! where the player left it.
//!
//! What the dial changes is the game's to say, through
//! `game.difficulty.scale`; the engine's own people read
//! [`DifficultyScales`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mlua::{AnyUserData, Function, Lua, MetaMethod, UserData, UserDataMethods, Value};

use crate::api::{Shared, describe, fail};
use crate::character_api::{missing, shared_of};
use crate::settings_api::{SettingValue, SettingsState};
use crate::time_api::rules_only;

/// Everything on `game.difficulty`. The type definitions are tested
/// against it.
pub(crate) const DIFFICULTY_FIELDS: &[&str] = &["steps", "name", "level", "scale", "onChange"];

/// The engine's own steps, easiest first. "Story" names what that step is
/// for, a game played for its story, and not who would choose it.
pub(crate) const USUAL_STEPS: [&str; 3] = ["Story", "Standard", "Hard"];

/// The player's setting that says where a new game's dial starts, from 0
/// (the easiest step) to 100 (the hardest).
pub const DIFFICULTY_SETTING: &str = "difficulty";

/// The most steps a game can name: more than a player can tell apart.
const MOST_STEPS: usize = 10;

/// The longest name of a step, in bytes: it has to fit in a menu row.
const LONGEST_NAME: usize = 32;

/// What [`scale`](DifficultyState::scale) multiplies by at the easiest
/// step and at the hardest, unless a game says its own.
const USUAL_RANGE: (f64, f64) = (0.5, 1.5);

/// How many changes `game.difficulty.onChange` can wait to hear of at once.
const MOST_WAITING: usize = 16;

/// How hard the game is, as numbers the engine's own people read: each is
/// 1 at the middle of the dial, 0.5 at the easiest step and 1.5 at the
/// hardest, along [`DifficultyState::scale`]'s line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DifficultyScales {
    /// What the damage the game's people deal the player is multiplied by.
    pub damage: f32,
    /// How fast people notice the player: what the rate their awareness
    /// rises at is multiplied by.
    pub notice: f32,
    /// How well people aim: what their accuracy is multiplied by (a
    /// shooter's spread is divided by it).
    pub aim: f32,
}

/// Where the dial stands, and the steps it has.
#[derive(Debug)]
pub(crate) struct DifficultyState {
    steps: RefCell<Vec<Rc<str>>>,
    at: Cell<usize>,
    /// The level the player last chose, from 0 to 1: where the dial goes
    /// when a game names new steps that lack the one it was at.
    wanted: Cell<f64>,
    /// Whether the dial has been turned in this game. Until it has, new
    /// steps start nearest where the player wanted it, whatever they are
    /// called.
    turned: Cell<bool>,
    /// Changes `game.difficulty.onChange` has yet to hear of: now, before.
    waiting: RefCell<Vec<(Rc<str>, Rc<str>)>>,
}

impl DifficultyState {
    /// The engine's own steps, the dial where the player's setting says.
    pub(crate) fn new(settings: &SettingsState) -> Self {
        let wanted = match settings.value_of(DIFFICULTY_SETTING) {
            Some(SettingValue::Number(level)) if level.is_finite() => {
                (level / 100.0).clamp(0.0, 1.0)
            }
            _ => 0.5,
        };
        let state = Self {
            steps: RefCell::new(USUAL_STEPS.iter().map(|&step| Rc::from(step)).collect()),
            at: Cell::new(0),
            wanted: Cell::new(wanted),
            turned: Cell::new(false),
            waiting: RefCell::default(),
        };
        state.at.set(state.nearest(wanted));
        state
    }

    /// How far along the steps `index` is, from 0 (the easiest) to 1 (the
    /// hardest). A dial of one step stands in the middle.
    fn level_of(&self, index: usize) -> f64 {
        let count = self.steps.borrow().len();
        if count <= 1 {
            return 0.5;
        }
        // Steps are few.
        #[allow(clippy::cast_precision_loss)]
        let level = index as f64 / (count - 1) as f64;
        level
    }

    /// The step nearest `level`; the easier of two as near.
    fn nearest(&self, level: f64) -> usize {
        let count = self.steps.borrow().len();
        let mut best = (0, f64::INFINITY);
        for index in 0..count {
            let off = (self.level_of(index) - level).abs();
            // A hair is allowed, so two as near by rounding count as near.
            if off < best.1 - 1e-9 {
                best = (index, off);
            }
        }
        best.0
    }

    /// How hard the game is now, from 0 (the easiest step) to 1 (the
    /// hardest).
    pub(crate) fn level(&self) -> f64 {
        self.level_of(self.at.get())
    }

    /// The name of the step the dial is at.
    pub(crate) fn name(&self) -> Rc<str> {
        let steps = self.steps.borrow();
        steps
            .get(self.at.get())
            .cloned()
            .unwrap_or_else(|| Rc::from(""))
    }

    pub(crate) fn steps(&self) -> Vec<Rc<str>> {
        self.steps.borrow().clone()
    }

    fn find(&self, name: &str) -> Option<usize> {
        let name = name.trim();
        self.steps
            .borrow()
            .iter()
            .position(|step| step.eq_ignore_ascii_case(name))
    }

    /// Names new steps. The dial stays on a step of the same name, or else
    /// goes to the step nearest where the player wanted it. Nobody is told:
    /// a game names its steps as it starts.
    fn set_steps(&self, steps: Vec<Rc<str>>) {
        let name = self.name();
        self.steps.replace(steps);
        let kept = if self.turned.get() {
            self.find(&name)
        } else {
            None
        };
        let at = kept.unwrap_or_else(|| self.nearest(self.wanted.get()));
        self.at.set(at);
    }

    /// Puts the engine's own steps back, as a new version of the script
    /// starts loading, and gives the ones there were.
    pub(crate) fn usual_steps(&self) -> Vec<Rc<str>> {
        let had = self.steps();
        self.set_steps(USUAL_STEPS.iter().map(|&step| Rc::from(step)).collect());
        had
    }

    /// Puts back steps [`usual_steps`](Self::usual_steps) gave, when the
    /// new version of the script failed to load.
    pub(crate) fn restore_steps(&self, steps: Vec<Rc<str>>) {
        self.set_steps(steps);
    }

    /// Turns the dial to follow the player's "difficulty" setting, after a
    /// script set it or reset it with `game.settings`.
    pub(crate) fn follow_setting(&self, settings: &SettingsState) {
        if let Some(SettingValue::Number(level)) = settings.value_of(DIFFICULTY_SETTING)
            && level.is_finite()
        {
            let level = (level / 100.0).clamp(0.0, 1.0);
            self.turn_to(self.nearest(level));
            self.wanted.set(level);
        }
    }

    /// Turns the dial to the step at `index`, for `onChange` to hear of.
    fn turn_to(&self, index: usize) {
        let before = self.name();
        if index == self.at.get() {
            return;
        }
        self.at.set(index);
        self.wanted.set(self.level());
        self.turned.set(true);
        let mut waiting = self.waiting.borrow_mut();
        if waiting.len() < MOST_WAITING {
            waiting.push((self.name(), before));
        }
    }

    /// The changes `onChange` has yet to hear of, oldest first.
    pub(crate) fn take_waiting(&self) -> Vec<(Rc<str>, Rc<str>)> {
        std::mem::take(&mut *self.waiting.borrow_mut())
    }

    /// What `value` becomes at this level: multiplied by `easiest` at the
    /// easiest step, by 1 at the middle and by `hardest` at the hardest,
    /// along a straight line from each end to the middle.
    pub(crate) fn scale(&self, value: f64, (easiest, hardest): (f64, f64)) -> f64 {
        let level = self.level();
        let times = if level <= 0.5 {
            easiest + (1.0 - easiest) * level * 2.0
        } else {
            1.0 + (hardest - 1.0) * (level - 0.5) * 2.0
        };
        value * times
    }

    /// The numbers the engine's own people follow.
    pub(crate) fn scales(&self) -> DifficultyScales {
        // Near 1: an f32 holds it.
        #[allow(clippy::cast_possible_truncation)]
        let times = self.scale(1.0, USUAL_RANGE) as f32;
        DifficultyScales {
            damage: times,
            notice: times,
            aim: times,
        }
    }
}

/// The steps' names as a list for a message: "Story", "Standard" or "Hard".
fn listed(steps: &[Rc<str>]) -> String {
    let quoted: Vec<String> = steps.iter().map(|step| format!("\"{step}\"")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

/// The steps a script wrote for `game.difficulty.steps`.
fn steps_said(lua: &Lua, value: &Value) -> mlua::Result<Vec<Rc<str>>> {
    const EXAMPLE: &str = "game.difficulty.steps = { \"Gentle\", \"Fair\", \"Cruel\" }";
    let wrong = |why: String| {
        fail(
            lua,
            format!(
                "game.difficulty.steps {why}. It takes a list of names for the steps, easiest \
                 first, from 1 to {MOST_STEPS} of them. Write it like this: {EXAMPLE}"
            ),
        )
    };
    let Value::Table(list) = value else {
        return Err(wrong(format!("was given {}", describe(value))));
    };
    let mut steps: Vec<Rc<str>> = Vec::new();
    for entry in list.sequence_values::<Value>() {
        let entry = entry?;
        let Value::String(name) = &entry else {
            return Err(wrong(format!(
                "has {} in its list where a name should be",
                describe(&entry)
            )));
        };
        let name = name.to_string_lossy();
        let name = name.trim();
        if name.is_empty() || name.len() > LONGEST_NAME {
            return Err(wrong(format!(
                "has a name of {} letters; a name is 1 to {LONGEST_NAME}",
                name.len()
            )));
        }
        if steps.iter().any(|step| step.eq_ignore_ascii_case(name)) {
            return Err(wrong(format!("names \"{name}\" twice")));
        }
        if steps.len() == MOST_STEPS {
            return Err(wrong(format!("has more than {MOST_STEPS} steps")));
        }
        steps.push(Rc::from(name));
    }
    if steps.is_empty() {
        return Err(wrong("was given an empty list".to_owned()));
    }
    Ok(steps)
}

/// A number `game.difficulty.scale` was given as `what`.
fn number_said(lua: &Lua, what: &str, value: &Value) -> mlua::Result<f64> {
    const EXAMPLE: &str = "local hurt = game.difficulty.scale(20)";
    // A script's numbers are small enough to be exact.
    #[allow(clippy::cast_precision_loss)]
    let number = match value {
        Value::Integer(number) => *number as f64,
        Value::Number(number) if number.is_finite() => *number,
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.difficulty.scale needs a number for {what}, but it was given {}. \
                     Write it like this: {EXAMPLE}, or with your own ends: \
                     game.difficulty.scale(20, 0.25, 2)",
                    describe(other)
                ),
            ));
        }
    };
    Ok(number)
}

/// `game.difficulty.scale(value, easiest, hardest)`, which `game.difficulty:scale`
/// reaches too.
fn scale_function(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let state = Rc::clone(&shared.difficulty);
    lua.create_function(move |lua, args: mlua::Variadic<Value>| {
        // A colon hands the dial itself over first.
        let args: Vec<&Value> = args
            .iter()
            .skip(usize::from(matches!(
                args.first(),
                Some(Value::UserData(_))
            )))
            .collect();
        let value = number_said(
            lua,
            "the number to scale",
            args.first().copied().unwrap_or(&Value::Nil),
        )?;
        let mut range = USUAL_RANGE;
        for (end, at) in [(&mut range.0, 1), (&mut range.1, 2)] {
            if let Some(given) = args.get(at).copied().filter(|given| !given.is_nil()) {
                let said = number_said(lua, "the end of its range", given)?;
                if said < 0.0 {
                    return Err(fail(
                        lua,
                        format!(
                            "game.difficulty.scale was given {said} for what to multiply by at \
                             the {} step, but that cannot be below 0. Write it like this: \
                             game.difficulty.scale(20, 0.25, 2)",
                            if at == 1 { "easiest" } else { "hardest" }
                        ),
                    ));
                }
                *end = said;
            }
        }
        Ok(state.scale(value, range))
    })
}

/// What a script holds for `game.difficulty`.
struct DifficultyRef {
    scale: Function,
    on_change: Function,
}

impl UserData for DifficultyRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(MetaMethod::ToString, |_, _: AnyUserData| {
            Ok("game.difficulty")
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| {
                let shared = shared_of(lua)?;
                let state = &shared.difficulty;
                let name = match &key {
                    Value::String(name) => name.to_string_lossy(),
                    _ => String::new(),
                };
                Ok(match name.as_str() {
                    "name" => Value::String(lua.create_string(&*state.name())?),
                    "level" => Value::Number(state.level()),
                    "steps" => {
                        let list = lua.create_table()?;
                        for step in state.steps() {
                            list.push(&*step)?;
                        }
                        Value::Table(list)
                    }
                    "scale" => Value::Function(this.borrow::<Self>()?.scale.clone()),
                    "onChange" => Value::Function(this.borrow::<Self>()?.on_change.clone()),
                    _ => {
                        return Err(missing(
                            lua,
                            "game.difficulty",
                            &key,
                            DIFFICULTY_FIELDS,
                            &[],
                        ));
                    }
                })
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (_, key, value): (AnyUserData, Value, Value)| {
                let shared = shared_of(lua)?;
                let state = &shared.difficulty;
                let name = match &key {
                    Value::String(name) => name.to_string_lossy(),
                    _ => String::new(),
                };
                match name.as_str() {
                    "name" => {
                        let steps = state.steps();
                        let said = match &value {
                            Value::String(said) => said.to_string_lossy(),
                            other => describe(other).to_owned(),
                        };
                        let Some(index) = state.find(&said) else {
                            return Err(fail(
                                lua,
                                format!(
                                    "game.difficulty.name was given {}, but this game's steps \
                                     are {}. Write it like this: game.difficulty.name = {}",
                                    if matches!(value, Value::String(_)) {
                                        format!("\"{said}\"")
                                    } else {
                                        said
                                    },
                                    listed(&steps),
                                    steps
                                        .first()
                                        .map(|step| format!("\"{step}\""))
                                        .unwrap_or_default()
                                ),
                            ));
                        };
                        rules_only(lua, &shared, "game.difficulty.name")?;
                        shared.clock.note(&said);
                        state.turn_to(index);
                        // The player's setting follows, for the next game.
                        let level = state.level() * 100.0;
                        let _ = shared
                            .settings
                            .set_from_screen(DIFFICULTY_SETTING, &SettingValue::Number(level));
                    }
                    "steps" => {
                        let steps = steps_said(lua, &value)?;
                        rules_only(lua, &shared, "game.difficulty.steps")?;
                        for step in &steps {
                            shared.clock.note(step);
                        }
                        state.set_steps(steps);
                    }
                    "level" => {
                        return Err(fail(
                            lua,
                            "game.difficulty.level cannot be set: it is where the step the \
                             game is at stands, from 0 (the easiest) to 1 (the hardest). Set \
                             the step by its name: game.difficulty.name = \"Hard\"",
                        ));
                    }
                    "scale" | "onChange" => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.difficulty.{name} is a function to call, not something \
                                 to set. Write it like this: {}",
                                if name == "scale" {
                                    "local hurt = game.difficulty.scale(20)"
                                } else {
                                    "game.difficulty.onChange(function(now, before) ... end)"
                                }
                            ),
                        ));
                    }
                    _ => {
                        return Err(missing(
                            lua,
                            "game.difficulty",
                            &key,
                            DIFFICULTY_FIELDS,
                            &[],
                        ));
                    }
                }
                Ok(())
            },
        );
    }
}

/// `game.difficulty`.
pub(crate) fn difficulty(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<AnyUserData> {
    let list: fn(&mut crate::api::Handlers) -> &mut Vec<Function> =
        |handlers| &mut handlers.difficulty;
    lua.create_userdata(DifficultyRef {
        scale: scale_function(lua, shared)?,
        on_change: crate::api::on_event(
            lua,
            shared,
            "difficulty.onChange",
            "game.difficulty.onChange(function(now, before) ... end)",
            list,
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dial(setting: Option<f64>) -> DifficultyState {
        let settings = setting.map_or_else(Vec::new, |level| {
            vec![crate::Setting {
                name: DIFFICULTY_SETTING.to_owned(),
                label: "Difficulty".to_owned(),
                description: String::new(),
                group: "Gameplay".to_owned(),
                kind: crate::SettingKind::Number {
                    min: 0.0,
                    max: 100.0,
                    step: 50.0,
                },
                value: SettingValue::Number(level),
                default: SettingValue::Number(50.0),
            }]
        });
        DifficultyState::new(&SettingsState::new(settings))
    }

    #[test]
    fn a_new_game_starts_where_the_player_left_the_dial() {
        assert_eq!(&*dial(None).name(), "Standard");
        assert_eq!(&*dial(Some(0.0)).name(), "Story");
        assert_eq!(&*dial(Some(80.0)).name(), "Hard");
        // A game's own steps: the nearest, the easier of two as near.
        let state = dial(Some(50.0));
        state.set_steps(["A", "B", "C", "D"].map(Rc::from).to_vec());
        assert_eq!(&*state.name(), "B");
        let state = dial(Some(100.0));
        state.set_steps(["Gentle", "Fair", "Cruel"].map(Rc::from).to_vec());
        assert_eq!(&*state.name(), "Cruel");
        assert!(state.take_waiting().is_empty(), "naming steps tells nobody");
    }

    #[test]
    fn scale_runs_from_half_through_one_to_one_and_a_half() {
        let state = dial(Some(0.0));
        let at = |state: &DifficultyState| state.scale(10.0, USUAL_RANGE);
        assert!((at(&state) - 5.0).abs() < 1e-12);
        state.turn_to(1);
        assert!((at(&state) - 10.0).abs() < 1e-12);
        state.turn_to(2);
        assert!((at(&state) - 15.0).abs() < 1e-12);
        assert!((state.scale(10.0, (0.25, 3.0)) - 30.0).abs() < 1e-12);
        assert_eq!(
            state.scales(),
            DifficultyScales {
                damage: 1.5,
                notice: 1.5,
                aim: 1.5
            }
        );
        assert_eq!(
            state.take_waiting(),
            [
                (Rc::from("Standard"), Rc::from("Story")),
                (Rc::from("Hard"), Rc::from("Standard"))
            ]
        );
    }
}
