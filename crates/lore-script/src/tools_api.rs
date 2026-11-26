//! What the engine tells a script about the screen, the frames and the
//! player, and what a script asks of them: `game.input.bind`, the pointer
//! and the wheel, `game.input.stepped` and `label`, `game.frames`, and the
//! moving of the player (`teleport`, `setMovement`, `setFlySpeed`).
//!
//! All of it is kept by the host, outside the script's VM, so saving the
//! script loses none of it.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;

use lore_input::{
    ActionId, ActionKind, ActionMap, Binding, Device, GamepadStyle, InputFrame, MapError, MouseMode,
};
use lore_ui::{EstimatedText, TextMeasure};
use mlua::{Function, Lua, MultiValue, Table, Value};

use crate::api::{Shared, describe, fail, unknown_action};
use crate::character_api::CharacterId;
use crate::error::closest;
use crate::time_api::rules_only;

/// Every function on `game.frames`.
pub(crate) const FRAMES_FUNCTIONS: &[&str] = &["rate", "lowRate", "lastTime", "slowestTime"];

/// How many frame times are kept: what `game.frames.slowestTime` looks back over.
pub const FRAME_HISTORY: usize = 1000;

/// How fast the player flies until a script says otherwise, in metres a second.
pub(crate) const DEFAULT_FLY_SPEED: f32 = 11.0;

/// The slowest and fastest the player can be told to fly, in metres a second.
const FLY_SPEEDS: (f32, f32) = (0.5, 200.0);

/// The furthest from the middle of the world, in metres on each axis, that
/// `game.player:teleport` goes: past it the physics loses its footing.
const FURTHEST_PLACE: f32 = 1_000_000.0;

/// How long a direction is held before a menu step repeats, in seconds. The
/// same as the engine's own menus. Seconds as the player lives them, whatever
/// the speed of time.
const REPEAT_DELAY: f64 = 0.4;

/// How often a held direction repeats after that, in seconds.
const REPEAT_EVERY: f64 = 0.1;

/// How far a stick or trigger must be pushed to count as a direction.
const PUSHED: f32 = 0.5;

/// The longest a name made with `game.input.bind` can be.
const LONGEST_NAME: usize = 40;

/// How a character is moving, as the engine says each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    /// How fast it is moving, in metres a second, with y up.
    pub velocity: [f32; 3],
    /// Where its eyes are, in metres.
    pub eye: [f32; 3],
    /// The direction its eyes look along, as a unit vector.
    pub aim: [f32; 3],
    /// Whether it stands on the ground.
    pub grounded: bool,
    /// Whether it is crouched.
    pub crouched: bool,
    /// Whether it is sprinting, on foot or swimming.
    pub sprinting: bool,
    /// Whether it is afloat in water too deep to stand in.
    pub swimming: bool,
    /// Whether it is on a ladder.
    pub on_ladder: bool,
    /// Whether it is pulling itself up onto a ledge, or over something.
    pub climbing: bool,
    /// Whether it is in the middle of a dodge.
    pub dodging: bool,
    /// Where a shot it fired now would land, in metres.
    pub aim_point: [f32; 3],
    /// Whether that is short of what the crosshair is on: cover in front
    /// of the body that the camera over its shoulder sees past.
    pub aim_blocked: bool,
}

/// Where the picture is drawn from, as the engine says before each one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTold {
    /// Where the camera is, in metres.
    pub position: [f32; 3],
    /// Which way it faces, in radians: 0 looks along -Z, growing to the left.
    pub yaw: f32,
    /// How far it looks up, in radians.
    pub pitch: f32,
    /// How many degrees it sees from the bottom of the picture to the top.
    pub fov_degrees: f32,
}

/// The numbers of how the player moves that a script can change, in metres
/// and metres a second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveSaid {
    /// Top speed on foot.
    pub walk_speed: f32,
    /// Top speed while sprinting.
    pub sprint_speed: f32,
    /// Top speed while crouched.
    pub crouch_speed: f32,
    /// How fast an unhurried swimmer goes.
    pub swim_speed: f32,
    /// How high a jump lifts the feet.
    pub jump_height: f32,
    /// How fast falling speeds up, in metres a second each second.
    pub gravity: f32,
    /// The highest thing vaulted, in metres above the feet.
    pub vault_height: f32,
    /// The highest ledge climbed onto, in metres above the feet.
    pub climb_height: f32,
    /// How fast a ladder is climbed.
    pub ladder_speed: f32,
    /// How long a dodge lasts, in seconds.
    pub dodge_seconds: f32,
    /// How far a dodge carries the player, in metres.
    pub dodge_distance: f32,
    /// Metres between footsteps when walking. Sprinting and crouching
    /// steps are spaced in the same proportion as the engine's own.
    pub stride: f32,
    /// What every step and landing is multiplied by in loudness: 1 is the
    /// engine's own, 0 is silent.
    pub step_loudness: f32,
}

/// What the player can do besides walk, jump and crouch: each is off
/// until a script turns it on, but swimming, which every game has had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abilities {
    /// Getting over low things with forward and jump.
    pub vault: bool,
    /// Pulling up onto ledges with forward and jump.
    pub climb: bool,
    /// Swimming in water too deep to stand in.
    pub swim: bool,
    /// Climbing the level's ladders.
    pub ladders: bool,
    /// Raising a guard with the `block` button.
    pub block: bool,
    /// A short dodge with the `dodge` button.
    pub dodge: bool,
    /// Holding the view on someone with the `lock_on` button.
    pub lock_on: bool,
    /// Changing the weapon in the hands with the `next_item` and
    /// `previous_item` buttons.
    pub switch_weapons: bool,
}

impl Default for Abilities {
    fn default() -> Self {
        Self {
            vault: false,
            climb: false,
            swim: true,
            ladders: false,
            block: false,
            dodge: false,
            lock_on: false,
            switch_weapons: true,
        }
    }
}

/// The names of the abilities, as a script gives them.
pub(crate) const ABILITIES: [&str; 8] = [
    "vault",
    "climb",
    "swim",
    "ladders",
    "block",
    "dodge",
    "lockon",
    "switchweapons",
];

/// `game.player:setAbility("climb", true)`.
pub(crate) fn set_ability(
    lua: &Lua,
    shared: &Rc<Shared>,
    name: &Value,
    on: &Value,
) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:setAbility(\"climb\", true)";
    let which = ability(lua, "game.player:setAbility", name, EXAMPLE)?;
    let Value::Boolean(on) = on else {
        return Err(fail(
            lua,
            format!(
                "game.player:setAbility takes true or false after the ability's name, but it \
                 was given {}. Write it like this: {EXAMPLE}",
                describe(on)
            ),
        ));
    };
    rules_only(lua, shared, "game.player:setAbility")?;
    shared.clock.note(ABILITIES[which]);
    let mut can = shared.tools.abilities.get();
    *[
        &mut can.vault,
        &mut can.climb,
        &mut can.swim,
        &mut can.ladders,
        &mut can.block,
        &mut can.dodge,
        &mut can.lock_on,
        &mut can.switch_weapons,
    ][which] = *on;
    shared.tools.abilities.set(can);
    Ok(())
}

/// `game.player:can("climb")`.
pub(crate) fn can(lua: &Lua, shared: &Rc<Shared>, name: &Value) -> mlua::Result<bool> {
    let which = ability(lua, "game.player:can", name, "game.player:can(\"climb\")")?;
    let can = shared.tools.abilities.get();
    Ok([
        can.vault,
        can.climb,
        can.swim,
        can.ladders,
        can.block,
        can.dodge,
        can.lock_on,
        can.switch_weapons,
    ][which])
}

/// Which of [`ABILITIES`] a script named.
fn ability(lua: &Lua, function: &str, name: &Value, example: &str) -> mlua::Result<usize> {
    let listed = "\"vault\", \"climb\", \"swim\", \"ladders\", \"block\", \"dodge\", \
                  \"lockOn\" and \"switchWeapons\"";
    let Value::String(name) = name else {
        return Err(fail(
            lua,
            format!(
                "{function} needs the name of an ability in quotes: {listed}. It was given \
                 {}. Write it like this: {example}",
                describe(name)
            ),
        ));
    };
    let name = name.to_string_lossy().trim().to_ascii_lowercase();
    // One ladder or many, it is the same ability; and "lock_on" is the
    // button's name for what the ability is called "lockOn".
    let name = match name.as_str() {
        "ladder" => "ladders",
        "lock_on" | "lock on" | "lock" => "lockon",
        "switch_weapons" | "switch weapons" | "switchweapon" => "switchweapons",
        other => other,
    };
    ABILITIES
        .iter()
        .position(|known| *known == name)
        .ok_or_else(|| {
            let mut message = format!("{function} was given \"{name}\", which is not an ability.");
            if let Some(near) = crate::error::closest(name, ABILITIES) {
                message.push_str(&format!(" Did you mean \"{near}\"?"));
            }
            message.push_str(&format!(
                " The abilities are {listed}. Write it like this: {example}"
            ));
            fail(lua, message)
        })
}

impl Default for MoveSaid {
    /// The engine's own, the same numbers as `lore_physics::MoveSettings`.
    fn default() -> Self {
        Self {
            walk_speed: 4.5,
            sprint_speed: 6.5,
            crouch_speed: 2.0,
            swim_speed: 1.0,
            jump_height: 1.1,
            gravity: 20.0,
            vault_height: 1.1,
            climb_height: 2.2,
            ladder_speed: 2.2,
            dodge_seconds: 0.35,
            dodge_distance: 3.2,
            // The same as `lore_audio::FootstepSettings::default().walk_stride`,
            // which a test in the `lore` crate holds it to.
            stride: 2.0,
            step_loudness: 1.0,
        }
    }
}

/// One of the numbers in [`MoveSaid`], as a script names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveNumber {
    Walk,
    Sprint,
    Crouch,
    Swim,
    Jump,
    Gravity,
    Vault,
    Climb,
    Ladder,
    DodgeSeconds,
    DodgeDistance,
    Stride,
    StepLoudness,
}

impl MoveNumber {
    /// The method that sets it, an example of calling it, what it takes and
    /// the range it is kept in.
    fn about(self) -> (&'static str, &'static str, &'static str, (f32, f32)) {
        match self {
            Self::Walk => (
                "setWalkSpeed",
                "game.player:setWalkSpeed(6)",
                "how fast the player walks, in metres a second",
                (0.0, 50.0),
            ),
            Self::Sprint => (
                "setSprintSpeed",
                "game.player:setSprintSpeed(9)",
                "how fast the player sprints, in metres a second",
                (0.0, 50.0),
            ),
            Self::Crouch => (
                "setCrouchSpeed",
                "game.player:setCrouchSpeed(3)",
                "how fast the player moves while crouched, in metres a second",
                (0.0, 50.0),
            ),
            Self::Swim => (
                "setSwimSpeed",
                "game.player:setSwimSpeed(2)",
                "how fast the player swims unhurried, in metres a second",
                (0.0, 20.0),
            ),
            Self::Jump => (
                "setJumpHeight",
                "game.player:setJumpHeight(2)",
                "how high a jump lifts the player's feet, in metres",
                (0.0, 50.0),
            ),
            Self::Gravity => (
                "setGravity",
                "game.player:setGravity(8)",
                "how fast the player's fall speeds up, in metres a second each second",
                (0.5, 100.0),
            ),
            Self::Vault => (
                "setVaultHeight",
                "game.player:setVaultHeight(1.3)",
                "the highest thing the player vaults, in metres above their feet",
                (0.3, 3.0),
            ),
            Self::Climb => (
                "setClimbHeight",
                "game.player:setClimbHeight(2.5)",
                "the highest ledge the player pulls themselves up onto, in metres above \
                 their feet",
                (0.3, 6.0),
            ),
            Self::Ladder => (
                "setLadderSpeed",
                "game.player:setLadderSpeed(3)",
                "how fast the player climbs a ladder, in metres a second",
                (0.1, 20.0),
            ),
            Self::DodgeSeconds => (
                "setDodgeSeconds",
                "game.player:setDodgeSeconds(0.5)",
                "how long a dodge lasts, in seconds",
                (0.05, 3.0),
            ),
            Self::DodgeDistance => (
                "setDodgeDistance",
                "game.player:setDodgeDistance(4)",
                "how far a dodge carries the player, in metres",
                (0.2, 30.0),
            ),
            Self::Stride => (
                "setStride",
                "game.player:setStride(1.5)",
                "how many metres the player goes between footsteps when walking",
                (0.1, 20.0),
            ),
            Self::StepLoudness => (
                "setStepLoudness",
                "game.player:setStepLoudness(0.5)",
                "how loud every step and landing is: 1 as now, 0 silent, 2 twice as loud",
                (0.0, 4.0),
            ),
        }
    }

    /// The number in `said`.
    pub(crate) fn of(self, said: &MoveSaid) -> f32 {
        match self {
            Self::Walk => said.walk_speed,
            Self::Sprint => said.sprint_speed,
            Self::Crouch => said.crouch_speed,
            Self::Swim => said.swim_speed,
            Self::Jump => said.jump_height,
            Self::Gravity => said.gravity,
            Self::Vault => said.vault_height,
            Self::Climb => said.climb_height,
            Self::Ladder => said.ladder_speed,
            Self::DodgeSeconds => said.dodge_seconds,
            Self::DodgeDistance => said.dodge_distance,
            Self::Stride => said.stride,
            Self::StepLoudness => said.step_loudness,
        }
    }
}

/// `game.player:setWalkSpeed(6)` and the other five like it.
pub(crate) fn set_move(
    lua: &Lua,
    shared: &Rc<Shared>,
    which: MoveNumber,
    value: &Value,
) -> mlua::Result<()> {
    let (method, example, what, (least, most)) = which.about();
    let function = format!("game.player:{method}");
    let takes = format!(
        "{what}, from {least} to {most} (a number outside that is taken as the nearest end)"
    );
    let number = number(lua, &function, value, &takes, example)?.clamp(least, most);
    rules_only(lua, shared, &function)?;
    shared.clock.note(&number.to_string());
    let mut said = shared.tools.moves.get();
    match which {
        MoveNumber::Walk => said.walk_speed = number,
        MoveNumber::Sprint => said.sprint_speed = number,
        MoveNumber::Crouch => said.crouch_speed = number,
        MoveNumber::Swim => said.swim_speed = number,
        MoveNumber::Jump => said.jump_height = number,
        MoveNumber::Gravity => said.gravity = number,
        MoveNumber::Vault => said.vault_height = number,
        MoveNumber::Climb => said.climb_height = number,
        MoveNumber::Ladder => said.ladder_speed = number,
        MoveNumber::DodgeSeconds => said.dodge_seconds = number,
        MoveNumber::DodgeDistance => said.dodge_distance = number,
        MoveNumber::Stride => said.stride = number,
        MoveNumber::StepLoudness => said.step_loudness = number,
    }
    shared.tools.moves.set(said);
    Ok(())
}

/// A move the script asked for with `game.player:teleport`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Teleport {
    /// Where the player's feet go, in metres.
    pub position: [f32; 3],
    /// The direction to face afterwards, as a unit vector, if the script
    /// said one.
    pub facing: Option<[f32; 3]>,
}

/// How the script wants the player to move.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Movement {
    /// On foot: gravity pulls and walls stop the player.
    #[default]
    Walk,
    /// Flying: gravity does not pull, walls still stop the player.
    Fly,
    /// Flying through walls and floors.
    Noclip,
}

impl Movement {
    /// The word a script uses for it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Walk => "walk",
            Self::Fly => "fly",
            Self::Noclip => "noclip",
        }
    }

    const ALL: [Self; 3] = [Self::Walk, Self::Fly, Self::Noclip];
}

/// The last `FRAME_HISTORY` frame times, in a ring made once.
struct Frames {
    times: Vec<f32>,
    /// Where the next time goes.
    next: usize,
    /// How many times are held.
    count: usize,
}

impl Frames {
    fn new() -> Self {
        Self {
            times: vec![0.0; FRAME_HISTORY],
            next: 0,
            count: 0,
        }
    }

    fn push(&mut self, seconds: f32) {
        if !seconds.is_finite() || seconds < 0.0 {
            return;
        }
        self.times[self.next] = seconds;
        self.next = (self.next + 1) % FRAME_HISTORY;
        self.count = (self.count + 1).min(FRAME_HISTORY);
    }

    /// The time of the frame `back` frames ago: 0 is the newest.
    fn ago(&self, back: usize) -> f32 {
        let place = (self.next + FRAME_HISTORY - 1 - back) % FRAME_HISTORY;
        self.times[place]
    }

    fn each(&self, newest: usize, mut see: impl FnMut(f32)) {
        for back in (0..newest.min(self.count)).rev() {
            see(self.ago(back));
        }
    }

    /// Frames a second over the last second of frames.
    #[allow(clippy::cast_precision_loss)]
    fn rate(&self) -> f32 {
        let (mut seconds, mut frames) = (0.0_f32, 0_u32);
        for back in 0..self.count {
            seconds += self.ago(back);
            frames += 1;
            if seconds >= 1.0 {
                break;
            }
        }
        if seconds > 0.0 {
            frames as f32 / seconds
        } else {
            0.0
        }
    }

    fn last(&self) -> f32 {
        if self.count == 0 { 0.0 } else { self.ago(0) }
    }

    fn slowest(&self) -> f32 {
        (0..self.count)
            .map(|back| self.ago(back))
            .fold(0.0, f32::max)
    }

    /// Frames a second of the slowest hundredth of the frames held, at
    /// least one frame, averaged.
    #[allow(clippy::cast_precision_loss)]
    fn low(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let mut sorted = [0.0_f32; FRAME_HISTORY];
        sorted[..self.count].copy_from_slice(&self.times[..self.count]);
        let held = &mut sorted[..self.count];
        held.sort_unstable_by(|a, b| b.total_cmp(a));
        let worst = (self.count / 100).max(1);
        let average = held[..worst].iter().sum::<f32>() / worst as f32;
        if average > 0.0 { 1.0 / average } else { 0.0 }
    }
}

/// One action's menu stepping: the direction held and the ticks until it
/// repeats.
#[derive(Clone, Copy, Default)]
struct Held {
    direction: [i8; 2],
    wait: i32,
}

/// What `game.input.stepped` reads: the repeat state of every action,
/// advanced on every tick whether or not a script asks.
#[derive(Default)]
struct Steps {
    held: Vec<Held>,
    /// The step each action takes on this tick.
    now: Vec<[i8; 2]>,
}

impl Steps {
    /// The direction `state` points in, if it is pushed far enough to count.
    fn direction(kind: Option<ActionKind>, input: &InputFrame, action: ActionId) -> [i8; 2] {
        let state = input.get(action);
        match kind {
            Some(ActionKind::Button) => [i8::from(state.pressed), 0],
            Some(ActionKind::Axis1D) => {
                let x = state.axis[0];
                [i8::from(x > PUSHED) - i8::from(x < -PUSHED), 0]
            }
            Some(ActionKind::Axis2D) => {
                let [right, up] = state.axis;
                if right.abs() > up.abs() {
                    [i8::from(right > PUSHED) - i8::from(right < -PUSHED), 0]
                } else {
                    [0, i8::from(up > PUSHED) - i8::from(up < -PUSHED)]
                }
            }
            None => [0, 0],
        }
    }

    /// Moves every action on by one tick, `rate` ticks to a second, with the
    /// game running at `speed` times the usual (0 counts as 1: the ticks
    /// then run at the usual rate). The waits are in seconds the player
    /// lives, so at a faster speed they take more ticks and at a slower
    /// speed fewer.
    // Waits are a few dozen ticks.
    #[allow(clippy::cast_possible_truncation)]
    fn advance(&mut self, map: &ActionMap, input: &InputFrame, rate: f64, speed: f64) {
        if self.held.len() != map.len() {
            self.held.resize(map.len(), Held::default());
            self.now.resize(map.len(), [0, 0]);
        }
        let rate = if rate > 0.0 { rate } else { 60.0 };
        let speed = if speed.is_finite() && speed > 0.0 {
            speed
        } else {
            1.0
        };
        let delay = (REPEAT_DELAY * rate * speed).round().max(1.0) as i32;
        let every = (REPEAT_EVERY * rate * speed).round().max(1.0) as i32;
        for (place, action) in map.actions().enumerate() {
            let direction = Self::direction(map.kind(action), input, action);
            let held = &mut self.held[place];
            let mut step = [0, 0];
            if direction != held.direction {
                held.direction = direction;
                held.wait = delay;
                step = direction;
            } else if direction != [0, 0] {
                held.wait -= 1;
                if held.wait <= 0 {
                    held.wait += every;
                    step = direction;
                }
            }
            self.now[place] = step;
        }
    }
}

/// State the engine and every version of the script both reach.
pub(crate) struct Tools {
    frames: RefCell<Frames>,
    steps: RefCell<Steps>,
    /// The player's own bindings as now in effect, for `game.input.label`.
    controls: RefCell<Option<ActionMap>>,
    device: Cell<Device>,
    style: Cell<GamepadStyle>,
    measure: RefCell<Box<dyn TextMeasure>>,
    teleport: Cell<Option<Teleport>>,
    movement: Cell<Movement>,
    fly_speed: Cell<f32>,
    /// How the script wants the player to walk, jump and fall.
    moves: Cell<MoveSaid>,
    /// What the script lets the player do: vault, climb, swim, ladders.
    abilities: Cell<Abilities>,
    /// Where this frame's picture is drawn from, for `game.hud.onScreen`.
    view: Cell<Option<ViewTold>>,
    /// Whether the engine has asked for the actions to run the game with.
    handed_out: Cell<bool>,
    /// Whether the script is taking typed text.
    pub(crate) typing: crate::typing_api::Typing,
    /// What the script asked of the bindings and of listening for a button.
    pub(crate) listen: crate::listen_api::Listen,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            frames: RefCell::new(Frames::new()),
            steps: RefCell::default(),
            controls: RefCell::new(None),
            device: Cell::new(Device::KeyboardAndMouse),
            style: Cell::new(GamepadStyle::default()),
            measure: RefCell::new(Box::new(EstimatedText)),
            teleport: Cell::new(None),
            movement: Cell::new(Movement::Walk),
            fly_speed: Cell::new(DEFAULT_FLY_SPEED),
            moves: Cell::new(MoveSaid::default()),
            abilities: Cell::new(Abilities::default()),
            view: Cell::new(None),
            typing: crate::typing_api::Typing::default(),
            listen: crate::listen_api::Listen::default(),
            handed_out: Cell::new(false),
        }
    }
}

impl Tools {
    pub fn frame_drawn(&self, seconds: f32) {
        self.frames.borrow_mut().push(seconds);
    }

    pub fn each_frame_time(&self, newest: usize, see: impl FnMut(f32)) {
        self.frames.borrow().each(newest, see);
    }

    /// Advances the menu stepping by one tick.
    pub fn advance_steps(&self, map: &ActionMap, input: &InputFrame, rate: f64, speed: f64) {
        self.steps.borrow_mut().advance(map, input, rate, speed);
    }

    /// Reads the player's controls as now in force, if the engine has said.
    pub fn with_controls<R>(&self, read: impl FnOnce(Option<&ActionMap>) -> R) -> R {
        read(self.controls.borrow().as_ref())
    }

    /// Changes the player's controls as the script reads them, starting from
    /// `own` when the engine has not said any.
    pub(crate) fn edit_controls(&self, own: &ActionMap, edit: impl FnOnce(&mut ActionMap)) {
        edit(
            self.controls
                .borrow_mut()
                .get_or_insert_with(|| own.clone()),
        );
    }

    pub fn set_controls(&self, controls: &ActionMap) {
        *self.controls.borrow_mut() = Some(controls.clone());
    }

    /// Whether the player last used a gamepad.
    pub fn gamepad_in_use(&self) -> bool {
        self.device.get() == Device::Gamepad
    }

    /// The device the tick says the player used, for what a script reads
    /// as it runs.
    pub fn set_device_kind(&self, device: Device) {
        self.device.set(device);
    }

    pub fn set_device(&self, device: Device, style: GamepadStyle) {
        self.device.set(device);
        self.style.set(style);
    }

    /// Reads with the engine's text measurer.
    pub(crate) fn with_measure<R>(&self, read: impl FnOnce(&dyn TextMeasure) -> R) -> R {
        read(&**self.measure.borrow())
    }

    pub fn set_measure(&self, measure: Box<dyn TextMeasure>) {
        *self.measure.borrow_mut() = measure;
    }

    /// The size of `text` drawn `height` high in `font`, with
    /// `letter_spacing` after every character, measured as the HUD draws it.
    pub fn text_size_styled(
        &self,
        text: &str,
        height: f32,
        font: lore_ui::FontFace,
        letter_spacing: f32,
    ) -> [f32; 2] {
        lore_ui::hud_text_size_styled(text, height, font, letter_spacing, &**self.measure.borrow())
    }

    pub fn take_teleport(&self) -> Option<Teleport> {
        self.teleport.take()
    }

    pub fn movement(&self) -> Movement {
        self.movement.get()
    }

    pub fn fly_speed(&self) -> f32 {
        self.fly_speed.get()
    }

    pub fn moves(&self) -> MoveSaid {
        self.moves.get()
    }

    pub fn abilities(&self) -> Abilities {
        self.abilities.get()
    }

    /// What to show for the action called `name` on the device the player
    /// last used, from their own bindings if the engine has said them and
    /// else from `own`. Empty for an action there is not.
    pub fn label(&self, own: &ActionMap, name: &str) -> String {
        let controls = self.controls.borrow();
        let map = controls.as_ref().unwrap_or(own);
        map.action(name).map_or_else(
            |_| String::new(),
            |action| map.label(action, self.device.get(), self.style.get()),
        )
    }

    pub fn view(&self) -> Option<ViewTold> {
        self.view.get()
    }

    pub fn set_view(&self, view: ViewTold) {
        self.view.set(Some(view));
    }

    /// Notes that the engine has the actions, and that a later one is not
    /// heard until the game is started again.
    pub fn hand_out(&self) {
        self.handed_out.set(true);
    }

    pub fn was_handed_out(&self) -> bool {
        self.handed_out.get()
    }
}

/// A number a script wrote for `function`, which `takes` such a number.
fn number(
    lua: &Lua,
    function: &str,
    value: &Value,
    takes: &str,
    example: &str,
) -> mlua::Result<f32> {
    // Speeds and places need no more precision than an f32 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    match value {
        Value::Number(number) if number.is_finite() => Ok(*number as f32),
        Value::Integer(number) => Ok(*number as f32),
        Value::Number(_) => Err(fail(
            lua,
            format!(
                "{function} was given a number with no size: infinity, or the answer to a sum \
                 such as 0 / 0. It takes {takes}. Write it like this: {example}"
            ),
        )),
        other => Err(fail(
            lua,
            format!(
                "{function} needs a number, but it was given {}. It takes {takes}. Write it \
                 like this: {example}",
                describe(other)
            ),
        )),
    }
}

/// `game.player:teleport(position, facing)`.
pub(crate) fn teleport(
    lua: &Lua,
    shared: &Rc<Shared>,
    place: &Value,
    facing: &Value,
) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:teleport(vector.create(0, 2, 0), vector.create(0, 0, -1))";
    let Value::Vector(place) = place else {
        return Err(fail(
            lua,
            format!(
                "game.player:teleport needs a place, but it was given {}. It takes where the \
                 player's feet go, and then, if you like, the way to face. Write it like \
                 this: {EXAMPLE}",
                describe(place)
            ),
        ));
    };
    let position = [place.x(), place.y(), place.z()];
    if !position.iter().all(|part| part.is_finite()) {
        return Err(fail(
            lua,
            format!(
                "game.player:teleport was given a place with a number in it that has no size \
                 (infinity, or the answer to 0 / 0). Write it like this: {EXAMPLE}"
            ),
        ));
    }
    if position.iter().any(|part| part.abs() > FURTHEST_PLACE) {
        return Err(fail(
            lua,
            format!(
                "game.player:teleport was given a place more than {FURTHEST_PLACE} metres from \
                 the middle of the world, which the physics cannot work with. Keep it inside \
                 the level. Write it like this: {EXAMPLE}"
            ),
        ));
    }
    let facing = match facing {
        Value::Nil => None,
        Value::Vector(way) => {
            let [x, y, z] = [way.x(), way.y(), way.z()];
            let length = (x * x + y * y + z * z).sqrt();
            if !(length.is_finite() && length > 0.0) {
                return Err(fail(
                    lua,
                    format!(
                        "game.player:teleport was given a way to face that has no direction: \
                         every part is 0, or one has no size. Give it the way to look \
                         along, such as vector.create(0, 0, -1). Write it like this: {EXAMPLE}"
                    ),
                ));
            }
            Some([x / length, y / length, z / length])
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.player:teleport needs a vector for the way to face, but it was \
                     given {}. Leave it out to keep facing as before. Write it like this: \
                     {EXAMPLE}",
                    describe(other)
                ),
            ));
        }
    };
    rules_only(lua, shared, "game.player:teleport")?;
    shared
        .clock
        .note(&format!("teleport {position:?} {facing:?}"));
    shared.characters.put(CharacterId::PLAYER, position);
    shared
        .tools
        .teleport
        .set(Some(Teleport { position, facing }));
    Ok(())
}

/// `game.player:setMovement(mode)`.
pub(crate) fn set_movement(lua: &Lua, shared: &Rc<Shared>, mode: &Value) -> mlua::Result<()> {
    let Value::String(written) = mode else {
        return Err(fail(
            lua,
            format!(
                "game.player:setMovement needs the way to move in quotes, but it was given \
                 {}. The ways are \"walk\", \"fly\" and \"noclip\" (flying through walls). \
                 Write it like this: game.player:setMovement(\"fly\")",
                describe(mode)
            ),
        ));
    };
    let written = written.to_string_lossy();
    let name = written.trim().to_lowercase();
    let Some(chosen) = Movement::ALL.into_iter().find(|way| way.name() == name) else {
        let mut message =
            format!("game.player:setMovement was given \"{written}\", which is not a way to move.");
        if let Some(near) = closest(&name, Movement::ALL.map(Movement::name)) {
            let _ = write!(message, " Did you mean \"{near}\"?");
        }
        message.push_str(
            " The ways are \"walk\", \"fly\" and \"noclip\". Write it like this: \
             game.player:setMovement(\"fly\")",
        );
        return Err(fail(lua, message));
    };
    rules_only(lua, shared, "game.player:setMovement")?;
    shared.clock.note(chosen.name());
    shared.tools.movement.set(chosen);
    Ok(())
}

/// `game.player:setFlySpeed(metresPerSecond)`.
pub(crate) fn set_fly_speed(lua: &Lua, shared: &Rc<Shared>, speed: &Value) -> mlua::Result<()> {
    const EXAMPLE: &str = "game.player:setFlySpeed(20)";
    let (slowest, fastest) = FLY_SPEEDS;
    let takes = format!(
        "metres a second, from {slowest} to {fastest} (a number outside that is taken as the \
         nearest end)"
    );
    let speed =
        number(lua, "game.player:setFlySpeed", speed, &takes, EXAMPLE)?.clamp(slowest, fastest);
    rules_only(lua, shared, "game.player:setFlySpeed")?;
    shared.clock.note(&speed.to_string());
    shared.tools.fly_speed.set(speed);
    Ok(())
}

/// What `game.frames` can only be read while drawing.
fn drawing_only(lua: &Lua, shared: &Shared, function: &str) -> mlua::Result<()> {
    if shared.clock.is_drawing() {
        return Ok(());
    }
    Err(fail(
        lua,
        format!(
            "game.frames.{function} is how fast this computer is drawing, which is different on \
             every computer, so the game's rules cannot read it. Read it in \
             game.onDraw(function() ... end) and show it with game.hud.text: \
             game.onDraw(function() fps.text = math.floor(game.frames.rate()) end)"
        ),
    ))
}

/// Builds the `game.frames` table.
pub(crate) fn frames(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    type Read = fn(&Frames) -> f64;
    let reads: [(&'static str, Read); 4] = [
        ("rate", |frames| f64::from(frames.rate())),
        ("lowRate", |frames| f64::from(frames.low())),
        ("lastTime", |frames| f64::from(frames.last())),
        ("slowestTime", |frames| f64::from(frames.slowest())),
    ];
    for (name, read) in reads {
        let shared = Rc::clone(shared);
        table.set(
            name,
            lua.create_function(move |lua, ()| {
                drawing_only(lua, &shared, name)?;
                Ok(read(&shared.tools.frames.borrow()))
            })?,
        )?;
    }
    Ok(table)
}

/// The fields of the table `game.input.onControlsChanged` is handed.
#[cfg(test)]
pub(crate) const CONTROLS_CHANGE_FIELDS: &[&str] =
    &["message", "changes", "problem", "file", "line"];

/// The fields of one of its `changes`.
#[cfg(test)]
pub(crate) const BINDING_CHANGE_FIELDS: &[&str] = &["action", "before", "after"];

/// One action whose bindings the player's file changed, as the player reads
/// them: the printed names of the buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingChange {
    /// The action's name, as the script knows it.
    pub action: String,
    /// What it was bound to.
    pub before: String,
    /// What it is bound to now.
    pub after: String,
}

/// What reading the player's bindings file again came to, as
/// `game.input.onControlsChanged` hears of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlsChange {
    /// Exactly what the engine's own notice says.
    pub message: String,
    /// The actions whose bindings changed. Empty when none did.
    pub changes: Vec<BindingChange>,
    /// What is wrong with the file, or `None` when it was good.
    pub problem: Option<String>,
    /// The file, with enough of its path to tell the player's from the game's.
    pub file: String,
    /// The line of the problem, when it has one.
    pub line: Option<usize>,
}

/// The table `game.input.onControlsChanged` functions are handed.
pub(crate) fn controls_table(lua: &Lua, change: &ControlsChange) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("message", change.message.as_str())?;
    let changes = lua.create_table_with_capacity(change.changes.len(), 0)?;
    for one in &change.changes {
        let entry = lua.create_table_with_capacity(0, 3)?;
        entry.set("action", one.action.as_str())?;
        entry.set("before", one.before.as_str())?;
        entry.set("after", one.after.as_str())?;
        changes.raw_push(entry)?;
    }
    table.set("changes", changes)?;
    table.set("problem", change.problem.as_deref())?;
    table.set("file", change.file.as_str())?;
    table.set("line", change.line)?;
    Ok(table)
}

/// The names of the actions a game made for itself are written like the
/// engine's own ("menu_click"): lower case letters, digits and underscores,
/// one to forty of them.
fn name_problem(name: &str) -> Option<&'static str> {
    let characters = name.chars().count();
    if characters == 0 || characters > LONGEST_NAME {
        return Some("a name is from 1 to 40 characters long");
    }
    let plain = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
    if !name.chars().all(plain) {
        return Some("a name holds only lower case letters, digits and underscores");
    }
    None
}

/// `name` written the way an action's name must be: lower case, with
/// underscores where there were spaces or dashes.
fn fixed_name(name: &str) -> String {
    let fixed: String = name
        .trim()
        .chars()
        .filter_map(|c| match c {
            ' ' | '-' | '_' => Some('_'),
            c if c.is_ascii_alphanumeric() => Some(c.to_ascii_lowercase()),
            _ => None,
        })
        .take(LONGEST_NAME)
        .collect();
    if fixed.is_empty() {
        "open_map".to_owned()
    } else {
        fixed
    }
}

/// The engine's action for switching between first and third person.
pub(crate) const SWITCH_VIEW: &str = "switch_view";

/// The gamepad button to suggest for an action that has none. Every button
/// is in the standard map; this is the one the engine leaves for games
/// (`GamepadSelect` is `switch_view`).
const FREE_GAMEPAD_BUTTON: &str = "LeftBumper";

/// How a binding is told apart in an error: what it does.
pub(crate) const fn moves(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Button => "is a button",
        ActionKind::Axis1D => "moves in one direction",
        ActionKind::Axis2D => "moves in two directions",
    }
}

/// Checks what a script gave `game.input.bind` as bindings: spelt right, and
/// all of one kind. Gives that kind, or the whole error to show.
pub(crate) fn check_bindings(
    function: &str,
    name: &str,
    bindings: &[String],
    example: &str,
) -> Result<ActionKind, String> {
    let mut first: Option<(&str, ActionKind)> = None;
    for text in bindings {
        let binding: Binding = text.parse().map_err(|source| {
            format!(
                "game.input.{function}: {} Write it like this: {example}",
                MapError::UnknownBinding {
                    action: name.to_owned(),
                    source,
                }
            )
        })?;
        match first {
            None => first = Some((text, binding.kind())),
            Some((before, kind)) if kind != binding.kind() => {
                return Err(format!(
                    "game.input.{function}(\"{name}\", ...): \"{before}\" {} but \"{text}\" {}. \
                     Give an action buttons only, or directions only: \
                     {{ \"WASD\", \"LeftStick\" }}",
                    moves(kind),
                    moves(binding.kind()),
                ));
            }
            Some(_) => {}
        }
    }
    Ok(first.map_or(ActionKind::Button, |(_, kind)| kind))
}

/// The sets of a map, written for an error: `"gameplay", "menu"`.
pub(crate) fn set_names(map: &ActionMap) -> String {
    let names: Vec<String> = map
        .sets()
        .map(|set| format!("\"{}\"", map.set_name(set)))
        .collect();
    names.join(", ")
}

/// The sets the engine owns, which a game cannot name for its own.
const ENGINE_SETS: [&str; 2] = ["gameplay", "menu"];

/// The set the editor adds while a level is edited: not in a game's map, but
/// a game's own set by this name would stop the editor.
const EDITOR_SET: &str = "editor";

/// The set a script named in `{ set = "riding" }`, or the error that says
/// which sets there are and how to make one.
fn find_set(
    lua: &Lua,
    map: &ActionMap,
    function: &str,
    value: &Value,
) -> mlua::Result<lore_input::ActionSetId> {
    let Value::String(name) = value else {
        return Err(fail(
            lua,
            format!(
                "game.input.{function} needs the name of an action set in quotes, but it was \
                 given {}. The sets are {}. Make one of your own at the top of the file with \
                 game.input.addSet(\"riding\").",
                describe(value),
                set_names(map)
            ),
        ));
    };
    let name = name.to_str()?;
    map.set(&name).map_err(|unknown| {
        fail(
            lua,
            format!(
                "game.input.{function}: {unknown} The sets are {}. To make one of your own, \
                 write game.input.addSet(\"{name}\") above this line.",
                set_names(map)
            ),
        )
    })
}

/// `game.input.addSet(name, { mouse = "look" })`.
fn add_set(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    const EXAMPLE: &str = "game.input.addSet(\"riding\")";
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, (name, options): (Value, Value)| {
        if !shared.loading.get() {
            return Err(fail(
                lua,
                format!(
                    "game.input.addSet makes a set of actions for the whole game, so it belongs \
                     at the top of the file, outside any function: {EXAMPLE}. Then switch to it \
                     in a function with game.input.useSet(\"riding\")."
                ),
            ));
        }
        let Value::String(name) = &name else {
            return Err(fail(
                lua,
                format!(
                    "game.input.addSet needs a name for the set in quotes, but it was given \
                     {}. Write it like this: {EXAMPLE}",
                    describe(&name)
                ),
            ));
        };
        let name = name.to_string_lossy();
        if let Some(problem) = name_problem(&name) {
            return Err(fail(
                lua,
                format!(
                    "game.input.addSet was given the name \"{name}\", but {problem}. Write it \
                     like this: game.input.addSet(\"{}\")",
                    fixed_name(&name)
                ),
            ));
        }
        let mouse = match &options {
            Value::Nil => MouseMode::Look,
            Value::Table(table) => set_options(lua, table)?,
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.input.addSet takes its options in a table, but it was given {}. \
                         Write it like this: game.input.addSet(\"{name}\", {{ mouse = \
                         \"pointer\" }})",
                        describe(other)
                    ),
                ));
            }
        };
        let mut map = shared.actions.borrow_mut();
        if name == EDITOR_SET {
            return Err(fail(
                lua,
                format!(
                    "\"{name}\" is the name of the set the editor uses, so game.input.addSet \
                     cannot make it. Give yours a name of its own: {EXAMPLE}"
                ),
            ));
        }
        if ENGINE_SETS.contains(&name.as_str()) || map.set(&name).is_ok() {
            return Err(fail(
                lua,
                format!(
                    "\"{name}\" is already an action set ({}), so game.input.addSet cannot \
                     make it. Give yours a name of its own: {EXAMPLE}",
                    set_names(&map)
                ),
            ));
        }
        if let Err(problem) = map.add_set(&name, mouse) {
            return Err(fail(lua, format!("game.input.addSet: {problem}")));
        }
        shared.clock.note(&name);
        Ok(())
    })
}

/// What the mouse is for, from the options table of `game.input.addSet`.
fn set_options(lua: &Lua, table: &Table) -> mlua::Result<MouseMode> {
    let mut mouse = MouseMode::Look;
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = &key else {
            return Err(fail(
                lua,
                "game.input.addSet options are named, like { mouse = \"pointer\" }.".to_owned(),
            ));
        };
        if key.to_str()? != "mouse" {
            return Err(fail(
                lua,
                format!(
                    "game.input.addSet has no option called \"{}\". The only one is mouse, \
                     which says what the mouse is for while the set is live: {{ mouse = \
                     \"look\" }} to look around or {{ mouse = \"pointer\" }} to point at a menu.",
                    key.to_string_lossy()
                ),
            ));
        }
        mouse = match &value {
            Value::String(word) if word.to_str()? == "look" => MouseMode::Look,
            Value::String(word) if word.to_str()? == "pointer" => MouseMode::Pointer,
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.input.addSet needs mouse to be \"look\" or \"pointer\", but it \
                         was {}. Write {{ mouse = \"pointer\" }} for a menu the mouse points at.",
                        describe(other)
                    ),
                ));
            }
        };
    }
    Ok(mouse)
}

/// The set named in the options of `game.input.bind`, if any.
fn bind_set(
    lua: &Lua,
    map: &ActionMap,
    options: &Value,
) -> mlua::Result<Option<lore_input::ActionSetId>> {
    let table = match options {
        Value::Nil => return Ok(None),
        Value::Table(table) => table,
        other => {
            return Err(fail(
                lua,
                format!(
                    "game.input.bind takes its options in a table, but it was given {}. Write \
                     it like this: game.input.bind(\"gallop\", {{ \"H\", \"LeftBumper\" }}, \
                     {{ set = \"riding\" }})",
                    describe(other)
                ),
            ));
        }
    };
    let mut chosen = None;
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        match &key {
            Value::String(key) if key.to_str()? == "set" => {
                chosen = Some(find_set(lua, map, "bind", &value)?);
            }
            other => {
                let shown = match other {
                    Value::String(text) => text.to_string_lossy(),
                    other => describe(other).to_owned(),
                };
                return Err(fail(
                    lua,
                    format!(
                        "game.input.bind has no option called \"{shown}\". The only one is \
                         set, the action set that hears it: {{ set = \"riding\" }}."
                    ),
                ));
            }
        }
    }
    Ok(chosen)
}

/// Whether a game's buttons cover a keyboard or mouse and a gamepad: every
/// action must, so that nothing ships for one device only. Gives the mistake
/// to put right.
pub(crate) fn device_gap(
    function: &str,
    name: &str,
    kind: ActionKind,
    bindings: &[String],
) -> Option<String> {
    let parsed: Vec<Binding> = bindings.iter().filter_map(|b| b.parse().ok()).collect();
    // What is missing, and one example of it, for each device.
    let (pad, keys) = match kind {
        ActionKind::Button => (
            ("a gamepad button", FREE_GAMEPAD_BUTTON),
            ("a keyboard or mouse button", "F1\" or \"MouseMiddle"),
        ),
        ActionKind::Axis1D => (
            ("a gamepad direction", "LeftStickX"),
            ("a keyboard direction", "A/D"),
        ),
        ActionKind::Axis2D => (
            ("a gamepad direction", "LeftStick"),
            ("a keyboard or mouse direction", "WASD"),
        ),
    };
    if !parsed.iter().any(|binding| binding.is_gamepad()) {
        let (what, example) = pad;
        return Some(format!(
            "game.input.{function}(\"{name}\", ...) needs {what} too, so a player with a \
             controller can use it. Add one such as \"{example}\"."
        ));
    }
    if !parsed.iter().any(|binding| !binding.is_gamepad()) {
        let (what, example) = keys;
        return Some(format!(
            "game.input.{function}(\"{name}\", ...) needs {what} too, so a player without a \
             controller can use it. Add one such as \"{example}\"."
        ));
    }
    None
}

/// `game.input.bind(name, bindings, options)`.
fn bind(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    const EXAMPLE: &str = "game.input.bind(\"open_map\", { \"F1\", \"LeftBumper\" })";
    let shared = Rc::clone(shared);
    lua.create_function(move |lua, (name, list, options): (Value, Value, Value)| {
        if !shared.loading.get() {
            return Err(fail(
                lua,
                format!(
                    "game.input.bind adds an action the player can rebind, so it belongs at the \
                     top of the file, outside any function: {EXAMPLE}. Then read it in a \
                     function with game.input.justPressed(\"open_map\")."
                ),
            ));
        }
        let Value::String(name) = &name else {
            return Err(fail(
                lua,
                format!(
                    "game.input.bind needs a name for the action in quotes, but it was given \
                     {}. Write it like this: {EXAMPLE}",
                    describe(&name)
                ),
            ));
        };
        let name = name.to_string_lossy();
        if let Some(problem) = name_problem(&name) {
            let fixed = fixed_name(&name);
            return Err(fail(
                lua,
                format!(
                    "game.input.bind was given the name \"{name}\", but {problem}, like the \
                     engine's own \"menu_click\". Write it like this: \
                     game.input.bind(\"{fixed}\", {{ \"F1\", \"LeftBumper\" }})"
                ),
            ));
        }
        let bindings = bindings_of(lua, "bind", &list, EXAMPLE)?;
        let kind = match check_bindings("bind", &name, &bindings, EXAMPLE) {
            Ok(kind) => kind,
            Err(problem) => return Err(fail(lua, problem)),
        };
        if shared.bound.borrow().contains(&name) {
            return Err(fail(
                lua,
                format!(
                    "game.input.bind(\"{name}\") is written twice. Bind each action once; to \
                     change its buttons, edit the first one."
                ),
            ));
        }
        let mut map = shared.actions.borrow_mut();
        if let Ok(id) = map.action(&name)
            && !map.is_own(id)
        {
            return Err(fail(
                lua,
                format!(
                    "\"{name}\" is already one of the engine's actions, so game.input.bind \
                     cannot make it. To read it, write: game.input.pressed(\"{name}\"). \
                     To make an action of your own, give it a name of its own: {EXAMPLE}"
                ),
            ));
        }
        let set = bind_set(lua, &map, &options)?;
        if let Some(gap) = device_gap("bind", &name, kind, &bindings) {
            return Err(fail(lua, gap));
        }
        if map.action(&name).is_err() {
            let made = match set {
                Some(set) => map.own_action_in(&name, kind, &bindings, set),
                None => map.own_action(&name, kind, &bindings),
            };
            if let Err(problem) = made {
                return Err(fail(
                    lua,
                    format!("game.input.bind: {problem} Write it like this: {EXAMPLE}"),
                ));
            }
            shared.clock.note(&name);
            if let Some(set) = set {
                shared.clock.note(map.set_name(set));
            }
            // `switch_view` only does anything in a game that lets the
            // player switch views; that game is warned when it does.
            for clash in map.conflicts().iter().filter(|clash| {
                (clash.first == name || clash.second == name)
                    && clash.first != SWITCH_VIEW
                    && clash.second != SWITCH_VIEW
            }) {
                log::warn!("{clash}");
            }
        }
        shared.bound.borrow_mut().push(name);
        Ok(())
    })
}

/// The button names of a table a script gave `game.input.bind`.
pub(crate) fn bindings_of(
    lua: &Lua,
    function: &str,
    list: &Value,
    example: &str,
) -> mlua::Result<Vec<String>> {
    let Value::Table(table) = list else {
        return Err(fail(
            lua,
            format!(
                "game.input.{function} needs a list of button names in a table, but it was given \
                 {}. Write it like this: {example}",
                describe(list)
            ),
        ));
    };
    let mut names = Vec::new();
    for value in table.sequence_values::<Value>() {
        match value? {
            Value::String(text) => names.push(text.to_string_lossy()),
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "game.input.{function} needs button names in quotes, but the list holds {}. \
                         Write it like this: {example}",
                        describe(&other)
                    ),
                ));
            }
        }
    }
    if names.is_empty() {
        return Err(fail(
            lua,
            format!(
                "game.input.{function} was given an empty list, so nothing would press the action. \
                 Name at least one button. Write it like this: {example}"
            ),
        ));
    }
    Ok(names)
}

/// The id and place in the map of the action a script named, or the error
/// that says what the actions are.
pub(crate) fn find(
    lua: &Lua,
    map: &ActionMap,
    function: &str,
    value: &Value,
) -> mlua::Result<(ActionId, usize)> {
    let Value::String(action) = value else {
        return Err(fail(
            lua,
            format!(
                "game.input.{function} needs the name of an action in quotes, but it was \
                 given {}. Write it like this: game.input.{function}(\"jump\")",
                describe(value)
            ),
        ));
    };
    let action = action.to_str()?;
    match map.action(&action) {
        Ok(id) => {
            let place = map.actions().position(|other| other == id).unwrap_or(0);
            Ok((id, place))
        }
        Err(unknown) => Err(fail(lua, unknown_action(map, &unknown))),
    }
}

/// Puts `bind`, `pointer`, `wheel`, `stepped` and `label` on `game.input`.
pub(crate) fn install_input(lua: &Lua, shared: &Rc<Shared>, input: &Table) -> mlua::Result<()> {
    input.set("bind", bind(lua, shared)?)?;
    let list: fn(&mut crate::api::Handlers) -> &mut Vec<Function> =
        |handlers| &mut handlers.controls_changed;
    input.set(
        "onControlsChanged",
        crate::api::on_event(
            lua,
            shared,
            "input.onControlsChanged",
            "game.input.onControlsChanged(function(change) ... end)",
            list,
        )?,
    )?;
    let notice = Rc::clone(shared);
    input.set(
        "showControlsNotice",
        lua.create_function(move |lua, value: Value| match value {
            Value::Boolean(shown) => {
                notice.controls_notice.set(shown);
                Ok(())
            }
            other => Err(fail(
                lua,
                format!(
                    "game.input.showControlsNotice needs true or false, but it was given {}. \
                     Write it like this: game.input.showControlsNotice(false)",
                    describe(&other)
                ),
            )),
        })?,
    )?;
    input.set("addSet", add_set(lua, shared)?)?;

    let rules = Rc::clone(shared);
    input.set(
        "sets",
        lua.create_function(move |lua, ()| {
            let map = rules.actions.borrow();
            lua.create_sequence_from(map.sets().map(|set| map.set_name(set).to_owned()))
        })?,
    )?;

    let rules = Rc::clone(shared);
    input.set(
        "actions",
        lua.create_function(move |lua, set: Value| {
            let map = rules.actions.borrow();
            let only = match &set {
                Value::Nil => None,
                other => Some(find_set(lua, &map, "actions", other)?),
            };
            lua.create_sequence_from(
                map.actions()
                    .filter(|action| only.is_none_or(|set| map.is_heard_in(*action, set)))
                    .map(|action| map.name(action).to_owned()),
            )
        })?,
    )?;

    let rules = Rc::clone(shared);
    input.set(
        "bindings",
        lua.create_function(move |lua, value: Value| {
            let own = rules.actions.borrow();
            let (id, _) = find(lua, &own, "bindings", &value)?;
            // The player's own file may have changed them: ask what is in force.
            let names: Vec<String> = rules.tools.with_controls(|controls| {
                let in_force =
                    controls.and_then(|map| map.action(own.name(id)).ok().map(|id| (map, id)));
                match in_force {
                    Some((map, id)) => map.bindings(id).iter().map(ToString::to_string).collect(),
                    None => own.bindings(id).iter().map(ToString::to_string).collect(),
                }
            });
            lua.create_sequence_from(names)
        })?,
    )?;

    let rules = Rc::clone(shared);
    input.set(
        "pointer",
        lua.create_function(move |_, ()| {
            Ok(match rules.input.borrow().pointer {
                Some([x, y]) => MultiValue::from_iter([
                    Value::Number(f64::from(x)),
                    Value::Number(f64::from(y)),
                ]),
                None => MultiValue::from_iter([Value::Nil]),
            })
        })?,
    )?;

    let rules = Rc::clone(shared);
    input.set(
        "wheel",
        lua.create_function(move |_, ()| Ok(f64::from(rules.input.borrow().wheel)))?,
    )?;

    let rules = Rc::clone(shared);
    input.set(
        "stepped",
        lua.create_function(move |lua, value: Value| {
            let (_, place) = find(lua, &rules.actions.borrow(), "stepped", &value)?;
            let [x, y] = rules
                .tools
                .steps
                .borrow()
                .now
                .get(place)
                .copied()
                .unwrap_or([0, 0]);
            Ok((f64::from(x), f64::from(y)))
        })?,
    )?;

    let rules = Rc::clone(shared);
    input.set(
        "label",
        lua.create_function(move |lua, (value, device): (Value, Value)| {
            if !rules.clock.is_drawing() {
                return Err(fail(
                    lua,
                    "game.input.label is for showing which key does what, and the key can \
                     change while the game runs, so read it in game.onDraw(function() ... \
                     end), for example: game.onDraw(function() hint.text = \"Press \" .. \
                     game.input.label(\"interact\") end)",
                ));
            }
            let tools = &rules.tools;
            let controls = tools.controls.borrow();
            let own;
            let map: &ActionMap = match controls.as_ref() {
                Some(controls) => controls,
                None => {
                    own = rules.actions.borrow();
                    &own
                }
            };
            let (id, _) = find(lua, map, "label", &value)?;
            let gamepad = match &device {
                Value::Nil => return Ok(map.label(id, tools.device.get(), tools.style.get())),
                Value::String(text) => match &*text.to_str()? {
                    "keyboard" => false,
                    "gamepad" => true,
                    other => {
                        return Err(fail(
                            lua,
                            format!(
                                "game.input.label's device is \"keyboard\" or \"gamepad\", but it \
                                 was given \"{other}\". Write it like this: \
                                 game.input.label(\"jump\", \"gamepad\")"
                            ),
                        ));
                    }
                },
                other => {
                    return Err(fail(
                        lua,
                        format!(
                            "game.input.label's device is \"keyboard\" or \"gamepad\" in quotes, \
                             but it was given {}. Write it like this: \
                             game.input.label(\"jump\", \"gamepad\")",
                            describe(other)
                        ),
                    ));
                }
            };
            Ok(map.label_on(id, gamepad, tools.style.get()))
        })?,
    )?;
    Ok(())
}
