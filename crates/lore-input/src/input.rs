//! Collects device events and turns them into one [`InputFrame`] per tick.

use std::fmt;

use crate::binding::{ActionKind, Binding, Button};
use crate::devices::{GamepadButton, Key, Stick, StickAxis};
use crate::frame::{ActionState, InputFrame};
use crate::map::{Action, ActionId, ActionMap, ActionSetId, Conflict, Device};
use crate::shaping::{PRESS_AT, RELEASE_AT, clamp_length, length, sanitize};

/// How far the mouse must move, in counts or pixels, to count as picked
/// up again after a gamepad was used. A bumped desk moves it less.
const MOUSE_TAKES_OVER_AT: f32 = 8.0;

/// The live state of every device, resolved through an [`ActionMap`].
///
/// A window layer reports what the player does (`press`, `release`,
/// `move_mouse`, `set_stick`...) whenever events arrive. The simulation calls
/// [`tick`](Self::tick) once per fixed step to get an [`InputFrame`].
///
/// A press is never lost and never counted twice: a tap that starts and ends
/// between two ticks still shows up as pressed for exactly one tick, and when
/// one rendered frame runs several ticks, only the first sees the new presses
/// and mouse movement.
#[derive(Debug, Clone)]
pub struct Input {
    map: ActionMap,
    /// Buttons physically down right now.
    held: [bool; Button::COUNT],
    /// Buttons that went down since the last tick, even if already released.
    latched: [bool; Button::COUNT],
    /// What the last tick reported for each button.
    reported: [bool; Button::COUNT],
    /// Buttons pressed again this tick while the last tick already reported them.
    repressed: [bool; Button::COUNT],
    /// How far each gamepad button is pushed, 0 to 1.
    amount: [f32; GamepadButton::COUNT],
    /// Raw stick positions, before the dead zone.
    sticks: [[f32; 2]; Stick::COUNT],
    /// Mouse movement accumulated since the last tick.
    mouse: [f32; 2],
    /// Where the free pointer is, if there is one.
    pointer: Option<[f32; 2]>,
    /// The size of the screen, in the pointer's units.
    screen: [f32; 2],
    /// Wheel notches rolled over the pointer since the last tick.
    wheel: f32,
    frame: InputFrame,
    /// [`ActionMap::layout`] of `map`, kept so a tick does not compute it.
    layout: u64,
    /// The action waiting for a new key or button, and the one it replaces.
    listening: Option<(ActionId, Button)>,
    /// The last finished rebind, until it is taken.
    rebound: Option<Rebound>,
    /// Buttons pressed while listening; each is ignored until it comes back up.
    swallowed: [bool; Button::COUNT],
    /// The action set whose actions are live.
    active: ActionSetId,
    /// Buttons that were down when the set last changed. Until each comes
    /// back up it presses no button action: it was pressed for the old set.
    stale: [bool; Button::COUNT],
    /// What the player last used on purpose.
    device: Device,
    /// How far the mouse has gone since a gamepad was last used, in counts.
    mouse_travel: f32,
}

/// A finished rebind: what [`Input::start_rebind`] heard and did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebound {
    /// The action that was rebound.
    pub action: ActionId,
    /// The key or button it no longer uses.
    pub from: Button,
    /// The key or button the player pressed, which it uses now.
    pub to: Button,
    /// The clashes this made: other actions that already used the new key or
    /// button. The rebind was made anyway; each prints as a sentence to show
    /// the player.
    pub conflicts: Vec<Conflict>,
}

/// Why [`Input::start_rebind`] did not start listening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebindError {
    /// The action's name, or `""` for an id from a different map.
    pub action: String,
    /// The key or button that was to be replaced.
    pub replacing: Button,
    /// What the action is bound to, as written in a bindings file.
    pub bound_to: Vec<String>,
}

impl fmt::Display for RebindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.action.is_empty() {
            return f.write_str(
                "That action is not in this action map. \
                 Look it up again with `map.action(\"name\")` after changing the map.",
            );
        }
        write!(
            f,
            "Action \"{}\" does not use \"{}\", so there is nothing to replace. ",
            self.action, self.replacing
        )?;
        if self.bound_to.is_empty() {
            f.write_str("It has nothing bound; give it bindings with `set_bindings`.")
        } else {
            write!(f, "It is bound to: {}.", self.bound_to.join(", "))
        }
    }
}

impl std::error::Error for RebindError {}

impl Input {
    /// Starts with every device at rest.
    #[must_use]
    pub fn new(map: ActionMap) -> Self {
        let mut frame = InputFrame::idle(map.len());
        let layout = map.layout();
        frame.layout = layout;
        Self {
            layout,
            map,
            held: [false; Button::COUNT],
            latched: [false; Button::COUNT],
            reported: [false; Button::COUNT],
            repressed: [false; Button::COUNT],
            amount: [0.0; GamepadButton::COUNT],
            sticks: [[0.0; 2]; Stick::COUNT],
            mouse: [0.0; 2],
            pointer: None,
            screen: crate::frame::DEFAULT_SCREEN,
            wheel: 0.0,
            frame,
            listening: None,
            rebound: None,
            swallowed: [false; Button::COUNT],
            active: ActionMap::GAMEPLAY,
            stale: [false; Button::COUNT],
            device: Device::KeyboardAndMouse,
            mouse_travel: 0.0,
        }
    }

    /// What the player last used on purpose: the keyboard and mouse, or a
    /// gamepad. Menus show the names of that device's buttons.
    ///
    /// A key, a mouse button, a wheel notch or a gamepad button changes it
    /// at once. A stick or trigger must be pushed half way and the mouse
    /// moved a little, so a stick that drifts or a desk that is bumped
    /// changes nothing. It starts as the keyboard and mouse.
    ///
    /// ```
    /// use lore_input::{ActionMap, Device, GamepadButton, Input, Key};
    ///
    /// let mut input = Input::new(ActionMap::standard());
    /// assert_eq!(input.last_device(), Device::KeyboardAndMouse);
    /// input.press(GamepadButton::GamepadSouth);
    /// assert_eq!(input.last_device(), Device::Gamepad);
    /// input.press(Key::Enter);
    /// assert_eq!(input.last_device(), Device::KeyboardAndMouse);
    /// ```
    #[must_use]
    pub fn last_device(&self) -> Device {
        self.device
    }

    /// Reports that the mouse pointer moved `distance` pixels over the
    /// window, for [`last_device`](Self::last_device). Movement that turns
    /// the view goes to [`move_mouse`](Self::move_mouse), which counts it
    /// by itself.
    pub fn pointer_moved(&mut self, distance: f32) {
        if !distance.is_finite() {
            return;
        }
        self.mouse_travel += distance.abs();
        if self.mouse_travel >= MOUSE_TAKES_OVER_AT {
            self.device = Device::KeyboardAndMouse;
            self.mouse_travel = 0.0;
        }
    }

    /// Notes that a gamepad was used on purpose.
    fn gamepad_used(&mut self) {
        self.device = Device::Gamepad;
        self.mouse_travel = 0.0;
    }

    /// The action set whose actions are live. It starts as
    /// [`ActionMap::GAMEPLAY`].
    #[must_use]
    pub fn active_set(&self) -> ActionSetId {
        self.active
    }

    /// Makes `set` the live action set, for example when a menu opens.
    ///
    /// Actions in every other set read as not pressed; one that was held
    /// reports `just_released` on the next tick.
    ///
    /// A key or button that is down at this moment was pressed for the old
    /// set, so it presses no button action in the new one until it is let go
    /// and pressed again. Without this, the Escape that opens a menu would
    /// close it again on the same press, and the click on "Resume" would fire
    /// a shot. Sticks and held direction keys carry over, so a player who
    /// holds W while closing a menu is walking as soon as it is closed.
    /// Presses that no tick has seen yet are dropped.
    ///
    /// Switching to the set that is already live does nothing, so this can be
    /// called every frame. A set from a different map is ignored.
    ///
    /// ```
    /// use lore_input::{ActionMap, Input, Key, MouseMode};
    ///
    /// let mut map = ActionMap::new();
    /// let pause = map.button("pause", ["Escape", "GamepadStart"])?;
    /// let menu = map.add_set("menu", MouseMode::Pointer)?;
    /// let resume = map.button("resume", ["Escape", "GamepadStart"])?;
    /// let mut input = Input::new(map);
    ///
    /// input.press(Key::Escape);
    /// assert!(input.tick().just_pressed(pause));
    ///
    /// input.set_active_set(menu);
    /// assert!(!input.tick().pressed(resume)); // still the same press
    ///
    /// input.release(Key::Escape);
    /// input.press(Key::Escape);
    /// assert!(input.tick().just_pressed(resume));
    /// # Ok::<(), lore_input::MapError>(())
    /// ```
    pub fn set_active_set(&mut self, set: ActionSetId) {
        if set == self.active || self.map.mouse_mode(set).is_none() {
            return;
        }
        self.switch_to(set);
    }

    /// Makes `set` live; what is down now belongs to the set before it.
    fn switch_to(&mut self, set: ActionSetId) {
        self.active = set;
        self.stale = self.held;
        self.latched = [false; Button::COUNT];
    }

    /// The action map in use.
    #[must_use]
    pub fn map(&self) -> &ActionMap {
        &self.map
    }

    /// Changes what `action` is multiplied by from the next tick on, with
    /// everything that is held staying held: see
    /// [`ActionMap::set_sensitivity`]. This is for a settings menu, where
    /// the player is holding the very button that changes it.
    pub fn set_sensitivity(&mut self, action: ActionId, sensitivity: [f32; 2]) {
        self.map.set_sensitivity(action, sensitivity);
    }

    /// Changes how much an action's mouse movement and its direction are
    /// multiplied by, each by its own amount, from the next tick on: see
    /// [`ActionMap::set_sensitivities`].
    pub fn set_sensitivities(&mut self, action: ActionId, mouse: [f32; 2], stick: [f32; 2]) {
        self.map.set_sensitivities(action, mouse, stick);
    }

    /// Replaces the action map. Ids from the old map must be looked up again.
    ///
    /// What is held stays held: an action the new map has under the same name
    /// as the old one keeps its state, so a key that is still down does not
    /// report `just_pressed` again. An action with a new name starts idle.
    ///
    /// The live action set stays the one with the same name, or becomes
    /// [`ActionMap::GAMEPLAY`] if the new map has none by that name.
    pub fn set_map(&mut self, map: ActionMap) {
        let live = self.map.set_name(self.active);
        match map.set(live) {
            Ok(same) => self.active = same,
            Err(_) => self.switch_to(ActionMap::GAMEPLAY),
        }
        let mut frame = InputFrame::idle(map.len());
        for (state, id) in frame.actions.iter_mut().zip(map.actions()) {
            if let Ok(old) = self.map.action(map.name(id)) {
                *state = self.frame.get(old);
            }
        }
        frame.tick = self.frame.tick;
        self.layout = map.layout();
        frame.layout = self.layout;
        self.frame = frame;
        self.map = map;
        self.listening = None;
        self.rebound = None;
    }

    /// Gives the same actions other bindings, as when the player's bindings
    /// file is read again while the game runs. Ids stay as they are.
    ///
    /// What is held stays held: an action whose key is still down does not
    /// report `just_pressed` again, and one whose key no longer presses it
    /// reports `just_released` on the next tick. A rebind that is listening
    /// is cancelled.
    ///
    /// `map` must have the actions of the map it replaces. One with another
    /// number of actions is taken as a new map: see [`set_map`](Self::set_map).
    pub fn set_bindings(&mut self, map: ActionMap) {
        if map.len() != self.map.len() {
            self.set_map(map);
            return;
        }
        if map.mouse_mode(self.active).is_none() {
            self.switch_to(ActionMap::GAMEPLAY);
        }
        self.layout = map.layout();
        self.frame.layout = self.layout;
        self.map = map;
        self.listening = None;
        self.rebound = None;
    }

    /// Starts listening: the next key or button the player presses replaces
    /// `replacing` in the action's bindings.
    ///
    /// Keep feeding events as usual. While listening, presses reach no action.
    /// The first new press of the same kind of device as `replacing` is taken:
    /// a key, mouse button or wheel notch for a keyboard or mouse binding, a
    /// gamepad button or trigger for a gamepad one, so an action never loses
    /// its binding on either device. Presses on the other device are dropped.
    /// Nothing pressed while listening, the chosen key included, reaches an
    /// action until it is let go. Choosing a key the action already uses
    /// elsewhere swaps the two.
    ///
    /// `Escape` (for keyboard and mouse) or `GamepadStart` (for a gamepad)
    /// cancels instead, so neither can be chosen this way; bind them in the
    /// bindings file. [`cancel_rebind`](Self::cancel_rebind) cancels too, for
    /// a time limit or a button on screen.
    ///
    /// Check [`is_rebinding`](Self::is_rebinding) each frame; once it is
    /// false, [`take_rebound`](Self::take_rebound) says what happened.
    ///
    /// ```
    /// use lore_input::{ActionMap, Input, Key};
    ///
    /// let mut input = Input::new(ActionMap::standard());
    /// let jump = input.map().action("jump")?;
    ///
    /// input.start_rebind(jump, Key::Space.into())?;
    /// input.press(Key::H); // the player presses the key they want
    ///
    /// let rebound = input.take_rebound().ok_or("still listening")?;
    /// assert_eq!(rebound.to, Key::H.into());
    /// assert!(rebound.conflicts.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Fails if the action does not use `replacing`; the error lists what it
    /// is bound to.
    pub fn start_rebind(&mut self, action: ActionId, replacing: Button) -> Result<(), RebindError> {
        // Swapping a button for itself changes nothing, so this only asks
        // whether the action uses it.
        let uses = Binding::buttons_of(self.map.bindings(action)).any(|button| button == replacing);
        if !uses {
            return Err(RebindError {
                action: self.map.name(action).to_owned(),
                replacing,
                bound_to: (self.map.bindings(action).iter())
                    .map(ToString::to_string)
                    .collect(),
            });
        }
        self.listening = Some((action, replacing));
        self.rebound = None;
        Ok(())
    }

    /// Stops listening without changing any binding.
    pub fn cancel_rebind(&mut self) {
        self.listening = None;
    }

    /// Whether a rebind is waiting for the player to press something.
    #[must_use]
    pub fn is_rebinding(&self) -> bool {
        self.listening.is_some()
    }

    /// The rebind that just finished, once. `None` while still listening,
    /// and `None` after listening ended if it was cancelled.
    pub fn take_rebound(&mut self) -> Option<Rebound> {
        self.rebound.take()
    }

    /// Decides whether a fresh press of `button` belongs to a rebind instead
    /// of to the game. Returns `true` when the press must reach no action.
    fn intercept(&mut self, button: Button) -> bool {
        let index = button.index();
        if self.swallowed[index] {
            return true;
        }
        let Some((action, from)) = self.listening else {
            return false;
        };
        let wheel = matches!(button, Button::Wheel(_));
        if !wheel && self.held[index] {
            // Key repeat of something held since before listening began.
            return true;
        }
        // Whatever goes down while listening belongs to the rebind, not the
        // game, until it comes back up. A wheel notch never comes back up.
        self.swallowed[index] = !wheel;
        if button.is_gamepad() != from.is_gamepad() {
            return true;
        }
        self.listening = None;
        let cancel = if from.is_gamepad() {
            Button::Gamepad(GamepadButton::GamepadStart)
        } else {
            Button::Key(Key::Escape)
        };
        if button != cancel {
            let before = self.map.conflicts();
            self.map.replace_button(action, from, button);
            let mut conflicts = self.map.conflicts();
            conflicts.retain(|conflict| !before.contains(conflict));
            self.rebound = Some(Rebound {
                action,
                from,
                to: button,
                conflicts,
            });
        }
        true
    }

    /// The frame produced by the most recent [`tick`](Self::tick).
    #[must_use]
    pub fn frame(&self) -> &InputFrame {
        &self.frame
    }

    /// Reports that a key or button went down. Key-repeat events are harmless.
    pub fn press(&mut self, button: impl Into<Button>) {
        let button = button.into();
        let index = button.index();
        if button.is_gamepad() {
            self.gamepad_used();
        } else {
            self.device = Device::KeyboardAndMouse;
        }
        if self.intercept(button) {
            return;
        }
        match button {
            // A wheel notch has no "held" state: it is a press that is already over.
            Button::Wheel(_) => self.latched[index] = true,
            Button::Key(_) | Button::Mouse(_) | Button::Gamepad(_) => {
                if !self.held[index] {
                    self.latched[index] = true;
                }
                self.held[index] = true;
                if let Button::Gamepad(pad) = button {
                    self.amount[pad.index()] = 1.0;
                }
            }
        }
    }

    /// Reports that a key or button came up.
    pub fn release(&mut self, button: impl Into<Button>) {
        let button = button.into();
        self.swallowed[button.index()] = false;
        self.stale[button.index()] = false;
        self.held[button.index()] = false;
        if let Button::Gamepad(pad) = button {
            self.amount[pad.index()] = 0.0;
        }
    }

    /// Reports how far an analog gamepad button, such as a trigger, is pushed: 0 to 1.
    ///
    /// The button counts as pressed from half way, and as released once it
    /// falls back below 0.4, so a trigger held near the middle does not flicker.
    pub fn set_button_amount(&mut self, button: GamepadButton, amount: f32) {
        let amount = sanitize(amount).max(0.0);
        let index = Button::Gamepad(button).index();
        if amount >= PRESS_AT {
            self.gamepad_used();
        }
        if self.swallowed[index] {
            if amount < RELEASE_AT {
                self.swallowed[index] = false;
            }
            return;
        }
        if !self.held[index] && amount >= PRESS_AT && self.intercept(Button::Gamepad(button)) {
            return;
        }
        self.amount[button.index()] = amount;
        if self.held[index] {
            if amount < RELEASE_AT {
                self.held[index] = false;
                self.stale[index] = false;
            }
        } else if amount >= PRESS_AT {
            self.held[index] = true;
            self.latched[index] = true;
        }
    }

    /// Reports one axis of a thumbstick, from -1 to 1. Right and up are positive.
    pub fn set_stick(&mut self, stick: Stick, axis: StickAxis, value: f32) {
        let slot = match axis {
            StickAxis::X => 0,
            StickAxis::Y => 1,
        };
        let value = sanitize(value);
        if value.abs() >= PRESS_AT {
            self.gamepad_used();
        }
        self.sticks[stick.index()][slot] = value;
    }

    /// Reports raw mouse movement in counts. Right and up are positive.
    pub fn move_mouse(&mut self, right: f32, up: f32) {
        if right.is_finite() && up.is_finite() {
            self.mouse[0] += right;
            self.mouse[1] += up;
            self.pointer_moved(right.abs() + up.abs());
        }
    }

    /// Lets go of everything, for example when the window loses focus.
    ///
    /// Held actions report `just_released` on the next tick. Presses and mouse
    /// movement that no tick has seen yet are discarded.
    ///
    /// A rebind that is listening is cancelled: the player has looked away.
    pub fn release_all(&mut self) {
        self.listening = None;
        self.swallowed = [false; Button::COUNT];
        self.stale = [false; Button::COUNT];
        self.held = [false; Button::COUNT];
        self.latched = [false; Button::COUNT];
        self.mouse = [0.0; 2];
        self.wheel = 0.0;
        self.release_gamepad();
    }

    /// Reports where the free pointer is and how big the screen is, both in
    /// whatever units the window layer uses (LORE's engine uses HUD units:
    /// pixels on a screen 1080 high). `at` is measured from the top-left
    /// corner, right and down, and is `None` when there is no free pointer:
    /// the mouse looks around, it is outside the window, or there is no window.
    ///
    /// The latest report wins, and every tick carries it in
    /// [`InputFrame::pointer`] and [`InputFrame::screen`]. Numbers that are
    /// not finite are ignored: a bad `at` becomes `None`, and a bad `screen`
    /// leaves the last one in place.
    pub fn set_pointer(&mut self, at: Option<[f32; 2]>, screen: [f32; 2]) {
        self.pointer = at.filter(|at| at.iter().all(|n| n.is_finite()));
        if screen.iter().all(|n| n.is_finite() && *n > 0.0) {
            self.screen = screen;
        }
    }

    /// Reports wheel notches rolled over the free pointer: above zero is away
    /// from the player, and a touchpad gives parts of a notch.
    ///
    /// Rolls add up until the next tick shows the total in
    /// [`InputFrame::wheel`], then start again from zero. The total is kept
    /// within 1000 notches either way, and numbers that are not finite are ignored.
    pub fn roll_wheel(&mut self, notches: f32) {
        if notches.is_finite() {
            self.wheel = (self.wheel + notches).clamp(-1000.0, 1000.0);
        }
    }

    /// Lets go of every gamepad control, for example when the gamepad disconnects.
    pub fn release_gamepad(&mut self) {
        for button in GamepadButton::ALL {
            let index = Button::Gamepad(*button).index();
            self.held[index] = false;
            self.latched[index] = false;
            self.swallowed[index] = false;
            self.stale[index] = false;
        }
        self.amount = [0.0; GamepadButton::COUNT];
        self.sticks = [[0.0; 2]; Stick::COUNT];
    }

    /// Reports that the gamepad in use was unplugged: its controls are let
    /// go, and the keyboard and mouse are what the player has left, so
    /// [`last_device`](Self::last_device) says so.
    pub fn gamepad_disconnected(&mut self) {
        self.release_gamepad();
        self.device = Device::KeyboardAndMouse;
        self.mouse_travel = 0.0;
    }

    /// Advances one simulation tick and returns the state of every action.
    ///
    /// This never allocates.
    pub fn tick(&mut self) -> &InputFrame {
        for index in 0..Button::COUNT {
            let now = self.held[index] || self.latched[index];
            self.repressed[index] = self.latched[index] && self.reported[index];
            self.reported[index] = now;
            self.latched[index] = false;
        }
        let mouse = std::mem::take(&mut self.mouse);
        self.frame.pointer = self.pointer;
        self.frame.screen = self.screen;
        self.frame.wheel = std::mem::take(&mut self.wheel);
        self.frame.layout = self.layout;

        // Taken out while it is filled, so the rest of `self` can be read.
        let mut states = std::mem::take(&mut self.frame.actions);
        let (actions, _) = self.map.parts();
        for (action, state) in actions.iter().zip(&mut states) {
            *state = self.state_of(action, *state, &self.reported, &self.repressed, mouse);
        }
        self.frame.actions = states;

        self.frame.tick += 1;
        &self.frame
    }

    /// Fills `frame` with what [`tick`](Self::tick) would report if it ran
    /// now, and uses nothing up: the next tick still sees every press and
    /// all of the mouse movement.
    ///
    /// This is for drawing, between ticks: the mouse movement and stick
    /// position no tick has seen yet are what turn the view on every drawn
    /// frame. The game's rules must read ticks only. `frame.tick` is the
    /// number of the last tick that ran.
    ///
    /// Filling the same frame every time does not allocate.
    pub fn peek(&self, frame: &mut InputFrame) {
        let mut reported = [false; Button::COUNT];
        let mut repressed = [false; Button::COUNT];
        for index in 0..Button::COUNT {
            reported[index] = self.held[index] || self.latched[index];
            repressed[index] = self.latched[index] && self.reported[index];
        }
        let (actions, _) = self.map.parts();
        frame.tick = self.frame.tick;
        frame.pointer = self.pointer;
        frame.screen = self.screen;
        frame.wheel = self.wheel;
        frame.layout = self.layout;
        frame.actions.clear();
        frame.actions.extend(
            actions
                .iter()
                .zip(&self.frame.actions)
                .map(|(action, last)| {
                    self.state_of(action, *last, &reported, &repressed, self.mouse)
                }),
        );
    }

    /// What `action` is doing, given what it did on the tick before
    /// (`last`), which buttons count as down (`reported`) or as pressed
    /// again (`repressed`), and how far the mouse moved.
    fn state_of(
        &self,
        action: &Action,
        last: ActionState,
        reported: &[bool; Button::COUNT],
        repressed: &[bool; Button::COUNT],
        mouse: [f32; 2],
    ) -> ActionState {
        if action.set != self.active && !action.everywhere {
            return ActionState {
                just_released: last.pressed,
                ..ActionState::default()
            };
        }
        let (_, bindings) = self.map.parts();
        let deadzone = self.map.deadzone();
        // A button held since before the set changed belongs to the old set.
        let fresh = action.kind != ActionKind::Button;
        let bindings = bindings.get(action.bindings.clone()).unwrap_or_default();
        let mut axis = [0.0_f32; 2];
        let mut delta = [0.0_f32; 2];
        let mut pressed_again = false;
        let amount = |button: Button| button_amount(reported, &self.amount, button);

        for binding in bindings {
            let candidate = match *binding {
                Binding::Button(button) => {
                    if fresh || !self.stale[button.index()] {
                        pressed_again |= repressed[button.index()];
                        [amount(button), 0.0]
                    } else {
                        [0.0, 0.0]
                    }
                }
                Binding::Axis { negative, positive } => [amount(positive) - amount(negative), 0.0],
                Binding::StickAxis(stick, stick_axis) => {
                    let shaped = deadzone.apply(self.sticks[stick.index()], action.stick_curve);
                    match stick_axis {
                        StickAxis::X => [shaped[0], 0.0],
                        StickAxis::Y => [shaped[1], 0.0],
                    }
                }
                Binding::Directions {
                    up,
                    down,
                    left,
                    right,
                } => clamp_length([amount(right) - amount(left), amount(up) - amount(down)]),
                Binding::Stick(stick) => {
                    deadzone.apply(self.sticks[stick.index()], action.stick_curve)
                }
                Binding::MouseMotion => {
                    delta = mouse;
                    [0.0, 0.0]
                }
            };
            // When several bindings are active the strongest wins. Adding
            // them would make keyboard plus stick faster than either alone.
            if length(candidate) > length(axis) {
                axis = candidate;
            }
        }
        if action.kind != ActionKind::Axis2D {
            axis[1] = 0.0;
        }

        let pressed = axis != [0.0, 0.0];
        let [across, along] = action.sensitivity;
        let delta = [delta[0] * across, delta[1] * along];
        let [across, along] = action.stick_sensitivity;
        let axis = [axis[0] * across, axis[1] * along];
        ActionState {
            pressed,
            just_pressed: pressed && (!last.pressed || pressed_again),
            just_released: !pressed && last.pressed,
            axis,
            delta,
        }
    }
}

/// How far a button is pushed this tick: 0 when up, 1 for keys, 0 to 1 for triggers.
fn button_amount(
    reported: &[bool; Button::COUNT],
    amounts: &[f32; GamepadButton::COUNT],
    button: Button,
) -> f32 {
    if !reported[button.index()] {
        return 0.0;
    }
    match button {
        // A tap that already ended has no amount left, so it counts as a full press.
        Button::Gamepad(pad) if amounts[pad.index()] > 0.0 => amounts[pad.index()],
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{MouseButton, MouseWheel};
    use crate::map::MouseMode;

    struct Rig {
        input: Input,
        jump: ActionId,
        fire: ActionId,
        next: ActionId,
        lean: ActionId,
        movement: ActionId,
        look: ActionId,
    }

    fn rig() -> Rig {
        let mut map = ActionMap::new();
        let jump = map.button("jump", ["Space", "GamepadSouth"]).unwrap();
        let fire = map.button("fire", ["MouseLeft", "RightTrigger"]).unwrap();
        let next = map.button("next", ["MouseWheelDown"]).unwrap();
        let lean = map.axis1d("lean", ["Q/E", "RightStickX"]).unwrap();
        let movement = map.axis2d("move", ["WASD", "LeftStick"]).unwrap();
        let look = map.axis2d("look", ["MouseMotion", "RightStick"]).unwrap();
        Rig {
            input: Input::new(map),
            jump,
            fire,
            next,
            lean,
            movement,
            look,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn press_hold_release() {
        let mut r = rig();
        r.input.press(Key::Space);
        let frame = r.input.tick();
        assert!(frame.pressed(r.jump) && frame.just_pressed(r.jump));
        assert_eq!(frame.value(r.jump), 1.0);

        let frame = r.input.tick();
        assert!(frame.pressed(r.jump) && !frame.just_pressed(r.jump));

        r.input.release(Key::Space);
        let frame = r.input.tick();
        assert!(!frame.pressed(r.jump) && frame.just_released(r.jump));

        let frame = r.input.tick();
        assert_eq!(frame.get(r.jump), ActionState::default());
    }

    #[test]
    fn tap_shorter_than_a_tick_is_not_lost() {
        let mut r = rig();
        r.input.press(Key::Space);
        r.input.release(Key::Space);
        let frame = r.input.tick();
        assert!(frame.pressed(r.jump) && frame.just_pressed(r.jump));
        let frame = r.input.tick();
        assert!(!frame.pressed(r.jump) && frame.just_released(r.jump));
    }

    #[test]
    fn several_ticks_in_one_frame_see_a_press_once() {
        let mut r = rig();
        r.input.press(Key::Space);
        r.input.move_mouse(4.0, 2.0);
        let presses: Vec<bool> = (0..3)
            .map(|_| r.input.tick().just_pressed(r.jump))
            .collect();
        assert_eq!(presses, [true, false, false]);

        r.input.move_mouse(4.0, 2.0);
        assert_eq!(r.input.tick().delta(r.look), [4.0, 2.0]);
        assert_eq!(r.input.tick().delta(r.look), [0.0, 0.0]);
    }

    #[test]
    fn key_repeat_does_not_press_again() {
        let mut r = rig();
        r.input.press(Key::Space);
        r.input.tick();
        r.input.press(Key::Space);
        assert!(!r.input.tick().just_pressed(r.jump));
    }

    #[test]
    fn release_and_press_again_within_a_tick_is_a_new_press() {
        let mut r = rig();
        r.input.press(Key::Space);
        r.input.tick();
        r.input.release(Key::Space);
        r.input.press(Key::Space);
        let frame = r.input.tick();
        assert!(frame.pressed(r.jump) && frame.just_pressed(r.jump));
        assert!(!frame.just_released(r.jump));
    }

    #[test]
    fn second_binding_does_not_press_a_held_action_again() {
        let mut r = rig();
        r.input.press(Key::Space);
        r.input.tick();
        r.input.press(GamepadButton::GamepadSouth);
        assert!(!r.input.tick().just_pressed(r.jump));
        r.input.release(Key::Space);
        let frame = r.input.tick();
        assert!(frame.pressed(r.jump) && !frame.just_released(r.jump));
    }

    #[test]
    fn every_wheel_notch_tick_is_a_press() {
        let mut r = rig();
        r.input.press(MouseWheel::MouseWheelDown);
        assert!(r.input.tick().just_pressed(r.next));
        r.input.press(MouseWheel::MouseWheelDown);
        assert!(r.input.tick().just_pressed(r.next));
        let frame = r.input.tick();
        assert!(frame.just_released(r.next) && !frame.pressed(r.next));
    }

    #[test]
    fn trigger_presses_half_way_and_does_not_flicker() {
        let mut r = rig();
        r.input.set_button_amount(GamepadButton::RightTrigger, 0.45);
        assert!(!r.input.tick().pressed(r.fire));

        r.input.set_button_amount(GamepadButton::RightTrigger, 0.6);
        let frame = r.input.tick();
        assert!(frame.just_pressed(r.fire));
        assert!(close(frame.value(r.fire), 0.6));

        r.input.set_button_amount(GamepadButton::RightTrigger, 0.45);
        let frame = r.input.tick();
        assert!(frame.pressed(r.fire) && !frame.just_pressed(r.fire));

        r.input.set_button_amount(GamepadButton::RightTrigger, 0.3);
        assert!(r.input.tick().just_released(r.fire));
    }

    #[test]
    fn trigger_tap_between_ticks_counts_as_a_full_press() {
        let mut r = rig();
        r.input.set_button_amount(GamepadButton::RightTrigger, 1.0);
        r.input.set_button_amount(GamepadButton::RightTrigger, 0.0);
        let frame = r.input.tick();
        assert!(frame.just_pressed(r.fire));
        assert_eq!(frame.value(r.fire), 1.0);
    }

    #[test]
    fn keys_form_a_direction_and_diagonals_are_not_faster() {
        let mut r = rig();
        r.input.press(Key::W);
        assert_eq!(r.input.tick().axis2d(r.movement), [0.0, 1.0]);
        r.input.press(Key::D);
        let diagonal = r.input.tick().axis2d(r.movement);
        assert!(close(length(diagonal), 1.0));
        assert!(close(diagonal[0], diagonal[1]));
        r.input.press(Key::S);
        assert_eq!(r.input.tick().axis2d(r.movement), [1.0, 0.0]);
    }

    #[test]
    fn stick_and_keys_do_not_add_up() {
        let mut r = rig();
        r.input.set_stick(Stick::LeftStick, StickAxis::Y, 1.0);
        r.input.press(Key::W);
        assert!(close(r.input.tick().axis2d(r.movement)[1], 1.0));

        r.input.release(Key::W);
        r.input.set_stick(Stick::LeftStick, StickAxis::Y, 0.55);
        let walk = r.input.tick().axis2d(r.movement)[1];
        assert!(close(walk, 0.5), "analog walk speed is kept, got {walk}");
    }

    #[test]
    fn stick_dead_zone_applies_to_actions() {
        let mut r = rig();
        r.input.set_stick(Stick::LeftStick, StickAxis::X, 0.1);
        let frame = r.input.tick();
        assert_eq!(frame.axis2d(r.movement), [0.0, 0.0]);
        assert!(!frame.pressed(r.movement));
    }

    #[test]
    fn one_dimensional_axes() {
        let mut r = rig();
        r.input.press(Key::Q);
        assert_eq!(r.input.tick().value(r.lean), -1.0);
        r.input.press(Key::E);
        assert_eq!(r.input.tick().value(r.lean), 0.0);
        r.input.release(Key::Q);
        r.input.release(Key::E);
        r.input.set_stick(Stick::RightStick, StickAxis::X, 1.0);
        r.input.set_stick(Stick::RightStick, StickAxis::Y, 1.0);
        let frame = r.input.tick();
        assert!(frame.value(r.lean) > 0.5);
        assert_eq!(frame.get(r.lean).axis[1], 0.0);
    }

    #[test]
    fn look_keeps_mouse_counts_and_stick_rate_apart() {
        let mut r = rig();
        r.input.move_mouse(3.0, -1.0);
        r.input.move_mouse(2.0, 0.5);
        r.input.set_stick(Stick::RightStick, StickAxis::X, -1.0);
        let frame = r.input.tick();
        assert_eq!(frame.delta(r.look), [5.0, -0.5]);
        assert!(close(frame.axis2d(r.look)[0], -1.0));
        assert_eq!(frame.delta(r.movement), [0.0, 0.0]);
    }

    #[test]
    fn a_peek_shows_what_no_tick_has_seen_and_uses_nothing_up() {
        let mut r = rig();
        let mut seen = InputFrame::default();
        r.input.peek(&mut seen);
        assert_eq!(seen.delta(r.look), [0.0, 0.0]);
        assert_eq!(seen.tick, 0);

        r.input.move_mouse(3.0, -1.0);
        r.input.set_stick(Stick::RightStick, StickAxis::X, -1.0);
        r.input.press(Key::Space);
        r.input.peek(&mut seen);
        assert_eq!(seen.delta(r.look), [3.0, -1.0]);
        assert!(close(seen.axis2d(r.look)[0], -1.0));
        assert!(seen.just_pressed(r.jump));

        // More movement adds up, and looking twice changes nothing.
        r.input.move_mouse(2.0, 0.5);
        r.input.peek(&mut seen);
        r.input.peek(&mut seen);
        assert_eq!(seen.delta(r.look), [5.0, -0.5]);

        // The tick reports exactly what the peek showed, and then the
        // movement is spent while the stick is still held.
        let ticked = r.input.tick().clone();
        assert_eq!(ticked.actions, seen.actions);
        assert_eq!(ticked.tick, 1);
        r.input.peek(&mut seen);
        assert_eq!(seen.tick, 1);
        assert_eq!(seen.delta(r.look), [0.0, 0.0]);
        assert!(close(seen.axis2d(r.look)[0], -1.0));
        assert!(seen.pressed(r.jump) && !seen.just_pressed(r.jump));
        let again = r.input.tick().clone();
        assert_eq!(again.actions, seen.actions);
    }

    #[test]
    fn a_peek_hears_only_the_live_set() {
        let mut s = sets();
        // Held when the menu opens: the click was for the game, not the menu.
        s.input.press(MouseButton::MouseLeft);
        s.input.tick();
        s.input.set_active_set(s.menu);
        s.input.press(Key::W);
        let mut seen = InputFrame::default();
        s.input.peek(&mut seen);
        assert!(!seen.pressed(s.fire) && !seen.pressed(s.confirm));
        assert!(!seen.pressed(s.movement) && seen.pressed(s.navigate));
        s.input.peek(&mut seen);
        let ticked = s.input.tick().clone();
        assert_eq!(ticked.actions, seen.actions);
    }

    #[test]
    fn stick_curve_is_per_action() {
        let mut map = ActionMap::new();
        let movement = map.axis2d("move", ["RightStick"]).unwrap();
        let look = map.axis2d("look", ["RightStick"]).unwrap();
        map.set_stick_curve(look, 2.0);
        let mut input = Input::new(map);
        input.set_stick(Stick::RightStick, StickAxis::X, 0.55);
        let frame = input.tick();
        assert!(close(frame.axis2d(movement)[0], 0.5));
        assert!(close(frame.axis2d(look)[0], 0.25));
    }

    #[test]
    fn the_mouse_and_the_stick_can_each_have_a_sensitivity() {
        let mut r = rig();
        r.input.set_sensitivities(r.look, [0.5, 0.5], [2.0, -2.0]);
        r.input.move_mouse(10.0, 4.0);
        r.input.set_stick(Stick::RightStick, StickAxis::X, 1.0);
        let frame = r.input.tick();
        assert_eq!(frame.delta(r.look), [5.0, 2.0]);
        let [across, along] = frame.axis2d(r.look);
        assert!(across > 1.5 && along == 0.0, "{across}, {along}");
        // Nothing that is not a number is taken.
        r.input
            .set_sensitivities(r.look, [1.0, 1.0], [f32::NAN, 1.0]);
        assert_eq!(r.input.map().sensitivity(r.look), [0.5, 0.5]);
        assert_eq!(r.input.map().stick_sensitivity(r.look), [2.0, -2.0]);
    }

    #[test]
    fn sensitivity_scales_mouse_and_stick_and_can_swap_up_and_down() {
        let mut r = rig();
        r.input.press(Key::Space);
        r.input.tick();
        // Changed while a key is held, as a settings menu does.
        r.input.set_sensitivity(r.look, [2.0, -0.5]);
        r.input.move_mouse(10.0, 4.0);
        r.input.set_stick(Stick::RightStick, StickAxis::Y, 1.0);
        let frame = r.input.tick();
        assert_eq!(frame.delta(r.look), [20.0, -2.0]);
        assert!(close(frame.axis2d(r.look)[1], -0.5));
        assert!(frame.pressed(r.jump) && !frame.just_pressed(r.jump));
        // No other action is touched, and nonsense is ignored.
        assert_eq!(r.input.map().sensitivity(r.movement), [1.0, 1.0]);
        r.input.set_sensitivity(r.look, [f32::NAN, 1.0]);
        assert_eq!(r.input.map().sensitivity(r.look), [2.0, -0.5]);
    }

    #[test]
    fn losing_focus_releases_everything_and_drops_unseen_input() {
        let mut r = rig();
        r.input.press(Key::Space);
        r.input.set_stick(Stick::LeftStick, StickAxis::X, 1.0);
        r.input.tick();
        r.input.press(MouseButton::MouseLeft);
        r.input.move_mouse(9.0, 9.0);
        r.input.release_all();
        let frame = r.input.tick();
        assert!(frame.just_released(r.jump));
        assert!(frame.just_released(r.movement));
        assert!(!frame.pressed(r.fire));
        assert_eq!(frame.delta(r.look), [0.0, 0.0]);
    }

    #[test]
    fn gamepad_disconnect_leaves_the_keyboard_alone() {
        let mut r = rig();
        r.input.press(Key::W);
        r.input.press(GamepadButton::GamepadSouth);
        r.input.set_button_amount(GamepadButton::RightTrigger, 1.0);
        r.input.tick();
        r.input.release_gamepad();
        let frame = r.input.tick();
        assert!(frame.just_released(r.jump) && frame.just_released(r.fire));
        assert_eq!(frame.axis2d(r.movement), [0.0, 1.0]);
    }

    #[test]
    fn bad_readings_are_ignored() {
        let mut r = rig();
        r.input.set_stick(Stick::LeftStick, StickAxis::X, f32::NAN);
        r.input
            .set_button_amount(GamepadButton::RightTrigger, f32::INFINITY);
        r.input.move_mouse(f32::NAN, 1.0);
        let frame = r.input.tick();
        assert_eq!(frame.axis2d(r.movement), [0.0, 0.0]);
        assert!(!frame.pressed(r.fire));
        assert_eq!(frame.delta(r.look), [0.0, 0.0]);
    }

    #[test]
    fn same_events_give_the_same_frames() {
        let run = || {
            let mut r = rig();
            let mut frames = Vec::new();
            for step in 0..20_u8 {
                if step % 3 == 0 {
                    r.input.press(Key::Space);
                }
                if step % 4 == 0 {
                    r.input.release(Key::Space);
                }
                r.input.move_mouse(f32::from(step), 1.5);
                r.input
                    .set_stick(Stick::LeftStick, StickAxis::X, f32::from(step) / 20.0);
                frames.push(r.input.tick().clone());
            }
            frames
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn ticks_are_numbered_from_one() {
        let mut r = rig();
        assert_eq!(r.input.frame().tick, 0);
        assert_eq!(r.input.tick().tick, 1);
        assert_eq!(r.input.tick().tick, 2);
    }

    #[test]
    fn replacing_the_map_resets_action_state() {
        let mut r = rig();
        r.input.press(Key::Space);
        r.input.tick();
        let mut map = ActionMap::new();
        let hop = map.button("hop", ["Space"]).unwrap();
        r.input.set_map(map);
        let frame = r.input.tick();
        assert_eq!(frame.tick, 2);
        assert!(frame.pressed(hop));
    }

    #[test]
    fn the_next_key_pressed_replaces_the_old_one_and_does_nothing_else() {
        let mut r = rig();
        r.input.start_rebind(r.jump, Key::Space.into()).unwrap();
        assert!(r.input.is_rebinding());
        assert_eq!(r.input.take_rebound(), None);

        r.input.press(Key::F);
        assert!(!r.input.is_rebinding());
        let rebound = r.input.take_rebound().unwrap();
        assert_eq!(
            (rebound.action, rebound.from, rebound.to),
            (r.jump, Key::Space.into(), Key::F.into())
        );
        assert_eq!(rebound.conflicts, Vec::new());
        assert_eq!(r.input.take_rebound(), None, "handed over once");

        // The press that chose the key does not jump, however long it repeats.
        r.input.press(Key::F);
        assert!(!r.input.tick().pressed(r.jump));
        r.input.release(Key::F);
        r.input.tick();

        r.input.press(Key::F);
        assert!(r.input.tick().just_pressed(r.jump));
        r.input.release(Key::F);
        r.input.press(Key::Space);
        assert!(!r.input.tick().pressed(r.jump), "the old key is gone");
        // The gamepad binding is untouched.
        r.input.press(GamepadButton::GamepadSouth);
        assert!(r.input.tick().pressed(r.jump));
    }

    #[test]
    fn listening_hears_only_new_presses_on_the_same_kind_of_device() {
        let mut r = rig();
        r.input.press(MouseButton::MouseLeft);
        r.input.tick();
        r.input.start_rebind(r.jump, Key::Space.into()).unwrap();

        // The click that opened the listener is still down, and repeats.
        r.input.press(MouseButton::MouseLeft);
        // A gamepad cannot fill a keyboard slot.
        r.input.press(GamepadButton::GamepadEast);
        r.input.set_button_amount(GamepadButton::LeftTrigger, 1.0);
        assert!(r.input.is_rebinding());
        assert!(!r.input.tick().pressed(r.jump), "dropped, not played");

        r.input.press(MouseWheel::MouseWheelUp);
        let rebound = r.input.take_rebound().unwrap();
        assert_eq!(rebound.to, MouseWheel::MouseWheelUp.into());
        r.input.tick();
        r.input.press(MouseWheel::MouseWheelUp);
        assert!(r.input.tick().just_pressed(r.jump));
    }

    #[test]
    fn a_trigger_can_be_chosen_for_a_gamepad_binding() {
        let mut r = rig();
        r.input
            .start_rebind(r.jump, GamepadButton::GamepadSouth.into())
            .unwrap();
        r.input.press(Key::F);
        r.input.set_button_amount(GamepadButton::LeftTrigger, 0.2);
        assert!(r.input.is_rebinding(), "a resting trigger is not a press");

        r.input.set_button_amount(GamepadButton::LeftTrigger, 0.9);
        let rebound = r.input.take_rebound().unwrap();
        assert_eq!(rebound.to, GamepadButton::LeftTrigger.into());
        r.input.set_button_amount(GamepadButton::LeftTrigger, 1.0);
        assert!(!r.input.tick().pressed(r.jump), "still the choosing press");

        r.input.set_button_amount(GamepadButton::LeftTrigger, 0.0);
        r.input.set_button_amount(GamepadButton::LeftTrigger, 0.8);
        let frame = r.input.tick();
        assert!(frame.just_pressed(r.jump));
        assert!(close(frame.value(r.jump), 0.8));
    }

    #[test]
    fn a_rebind_onto_a_used_key_goes_through_and_names_the_clash() {
        let mut r = rig();
        r.input.start_rebind(r.jump, Key::Space.into()).unwrap();
        r.input.press(Key::W);
        let rebound = r.input.take_rebound().unwrap();
        let said: Vec<String> = rebound.conflicts.iter().map(ToString::to_string).collect();
        assert_eq!(
            said,
            [
                "\"W\" is bound to both \"jump\" and \"move\", so it does both at once. \
              Bind one of them to something else, unless that is what you want."
            ]
        );
        assert_eq!(
            r.input.map().bindings(r.jump)[0],
            Binding::Button(Key::W.into())
        );
    }

    #[test]
    fn one_key_of_a_group_can_be_rebound() {
        let mut r = rig();
        r.input.start_rebind(r.movement, Key::W.into()).unwrap();
        r.input.press(Key::ArrowUp);
        r.input.release(Key::ArrowUp);
        r.input.tick();
        r.input.press(Key::ArrowUp);
        assert_eq!(r.input.tick().axis2d(r.movement), [0.0, 1.0]);
    }

    #[test]
    fn escape_start_and_a_call_all_cancel() {
        let mut r = rig();
        let before = r.input.map().clone();

        r.input.start_rebind(r.jump, Key::Space.into()).unwrap();
        r.input.press(Key::Escape);
        assert!(!r.input.is_rebinding());
        assert_eq!(r.input.take_rebound(), None);

        r.input
            .start_rebind(r.jump, GamepadButton::GamepadSouth.into())
            .unwrap();
        r.input.press(GamepadButton::GamepadStart);
        assert!(!r.input.is_rebinding());
        assert_eq!(r.input.take_rebound(), None);

        r.input.start_rebind(r.jump, Key::Space.into()).unwrap();
        r.input.cancel_rebind();
        r.input.press(Key::F);
        assert_eq!(r.input.take_rebound(), None);

        r.input.start_rebind(r.jump, Key::Space.into()).unwrap();
        r.input.release_all();
        assert!(!r.input.is_rebinding(), "the window lost focus");

        assert_eq!(r.input.map(), &before);
    }

    #[test]
    fn a_rebind_that_cannot_start_says_what_the_action_uses() {
        let mut r = rig();
        let error = r.input.start_rebind(r.jump, Key::F.into()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Action \"jump\" does not use \"F\", so there is nothing to replace. \
             It is bound to: Space, GamepadSouth."
        );
        assert!(!r.input.is_rebinding());

        let foreign = Input::new(ActionMap::standard())
            .map()
            .action("pause")
            .unwrap();
        let error = r.input.start_rebind(foreign, Key::F.into()).unwrap_err();
        assert!(error.to_string().starts_with("That action is not in"));
    }

    #[test]
    fn choosing_a_key_the_group_already_uses_swaps_the_two() {
        let mut r = rig();
        r.input.start_rebind(r.movement, Key::W.into()).unwrap();
        r.input.press(Key::A);
        let rebound = r.input.take_rebound().unwrap();
        assert_eq!(rebound.conflicts, Vec::new());
        assert_eq!(
            r.input.map().bindings(r.movement)[0],
            Binding::directions(Key::A, Key::S, Key::W, Key::D)
        );

        r.input.start_rebind(r.lean, Key::Q.into()).unwrap();
        r.input.press(Key::E);
        assert_eq!(
            r.input.map().bindings(r.lean)[0],
            Binding::Axis {
                negative: Key::E.into(),
                positive: Key::Q.into()
            }
        );
    }

    #[test]
    fn nothing_pressed_while_listening_plays_once_listening_ends() {
        let mut r = rig();
        r.input.start_rebind(r.jump, Key::Space.into()).unwrap();
        // The wrong device: both stay down past the end of listening.
        r.input.set_button_amount(GamepadButton::RightTrigger, 1.0);
        r.input.press(Key::F);
        assert!(r.input.take_rebound().is_some());

        // A second rebind ends while F, chosen by the first, is still down.
        r.input
            .start_rebind(r.fire, MouseButton::MouseLeft.into())
            .unwrap();
        r.input.press(Key::G);
        assert!(r.input.take_rebound().is_some());

        // Key repeat and further trigger samples of the same presses.
        r.input.press(Key::F);
        r.input.press(Key::G);
        r.input.set_button_amount(GamepadButton::RightTrigger, 0.9);
        let frame = r.input.tick();
        assert!(!frame.pressed(r.jump) && !frame.pressed(r.fire));

        r.input.set_button_amount(GamepadButton::RightTrigger, 0.0);
        r.input.set_button_amount(GamepadButton::RightTrigger, 1.0);
        assert!(r.input.tick().just_pressed(r.fire), "let go, then pulled");
    }

    /// Gameplay with a menu set whose `back` shares Escape with `pause`.
    struct Sets {
        input: Input,
        pause: ActionId,
        fire: ActionId,
        movement: ActionId,
        menu: ActionSetId,
        back: ActionId,
        confirm: ActionId,
        navigate: ActionId,
    }

    fn sets() -> Sets {
        let mut map = ActionMap::new();
        let pause = map.button("pause", ["Escape", "GamepadStart"]).unwrap();
        let fire = map.button("fire", ["MouseLeft", "RightTrigger"]).unwrap();
        let movement = map.axis2d("move", ["WASD", "LeftStick"]).unwrap();
        let menu = map.add_set("menu", MouseMode::Pointer).unwrap();
        let back = map.button("back", ["Escape", "GamepadStart"]).unwrap();
        let confirm = map
            .button("confirm", ["MouseLeft", "RightTrigger"])
            .unwrap();
        let navigate = map.axis2d("navigate", ["WASD", "LeftStick"]).unwrap();
        Sets {
            input: Input::new(map),
            pause,
            fire,
            movement,
            menu,
            back,
            confirm,
            navigate,
        }
    }

    #[test]
    fn only_the_live_set_hears_a_key() {
        let mut s = sets();
        assert_eq!(s.input.active_set(), ActionMap::GAMEPLAY);
        s.input.press(Key::Escape);
        let frame = s.input.tick();
        assert!(frame.just_pressed(s.pause) && !frame.pressed(s.back));
        s.input.release(Key::Escape);
        s.input.tick();

        s.input.set_active_set(s.menu);
        assert_eq!(s.input.active_set(), s.menu);
        s.input.press(Key::Escape);
        let frame = s.input.tick();
        assert!(frame.just_pressed(s.back) && !frame.pressed(s.pause));
    }

    #[test]
    fn the_press_that_switches_sets_does_nothing_in_the_new_one() {
        let mut s = sets();
        s.input.press(Key::Escape);
        assert!(s.input.tick().just_pressed(s.pause));

        s.input.set_active_set(s.menu);
        let frame = s.input.tick();
        assert!(frame.just_released(s.pause), "the old set lets go");
        assert!(!frame.pressed(s.pause) && !frame.pressed(s.back));
        // Key repeat of the same press changes nothing.
        s.input.press(Key::Escape);
        assert!(!s.input.tick().pressed(s.back));

        s.input.release(Key::Escape);
        s.input.press(Key::Escape);
        assert!(s.input.tick().just_pressed(s.back), "let go, then pressed");

        // And the same on the way back.
        s.input.set_active_set(ActionMap::GAMEPLAY);
        let frame = s.input.tick();
        assert!(frame.just_released(s.back) && !frame.pressed(s.pause));
        s.input.release(Key::Escape);
        s.input.tick();
        s.input.press(Key::Escape);
        assert!(s.input.tick().just_pressed(s.pause));
    }

    #[test]
    fn a_click_or_trigger_held_through_a_switch_does_not_fire() {
        let mut s = sets();
        s.input.set_active_set(s.menu);
        s.input.press(MouseButton::MouseLeft);
        s.input.set_button_amount(GamepadButton::RightTrigger, 1.0);
        assert!(s.input.tick().just_pressed(s.confirm));

        s.input.set_active_set(ActionMap::GAMEPLAY);
        s.input.set_button_amount(GamepadButton::RightTrigger, 0.8);
        assert!(!s.input.tick().pressed(s.fire));

        s.input.release(MouseButton::MouseLeft);
        s.input.set_button_amount(GamepadButton::RightTrigger, 0.0);
        assert!(!s.input.tick().pressed(s.fire));
        s.input.set_button_amount(GamepadButton::RightTrigger, 1.0);
        assert!(s.input.tick().just_pressed(s.fire), "pulled again");
    }

    #[test]
    fn a_tap_no_tick_saw_does_not_cross_a_switch() {
        let mut s = sets();
        s.input.press(Key::Escape);
        s.input.release(Key::Escape);
        s.input.set_active_set(s.menu);
        let frame = s.input.tick();
        assert!(!frame.pressed(s.back) && !frame.pressed(s.pause));
    }

    #[test]
    fn held_directions_and_sticks_carry_over_a_switch() {
        let mut s = sets();
        s.input.set_active_set(s.menu);
        s.input.press(Key::W);
        assert_eq!(s.input.tick().axis2d(s.navigate), [0.0, 1.0]);

        s.input.set_active_set(ActionMap::GAMEPLAY);
        let frame = s.input.tick();
        assert_eq!(frame.axis2d(s.movement), [0.0, 1.0], "still holding W");
        assert_eq!(frame.axis2d(s.navigate), [0.0, 0.0]);

        s.input.release(Key::W);
        s.input.set_stick(Stick::LeftStick, StickAxis::X, 1.0);
        s.input.set_active_set(s.menu);
        assert!(s.input.tick().axis2d(s.navigate)[0] > 0.9);
    }

    #[test]
    fn switching_to_the_live_set_or_an_unknown_one_changes_nothing() {
        let mut s = sets();
        s.input.press(Key::Escape);
        s.input.set_active_set(ActionMap::GAMEPLAY);
        assert!(s.input.tick().just_pressed(s.pause), "the press was kept");

        let mut bigger = ActionMap::new();
        bigger.add_set("a", MouseMode::Look).unwrap();
        let stranger = bigger.add_set("b", MouseMode::Look).unwrap();
        s.input.set_active_set(stranger);
        assert_eq!(s.input.active_set(), ActionMap::GAMEPLAY);
    }

    #[test]
    fn looking_away_forgets_what_was_held_through_a_switch() {
        let mut s = sets();
        s.input.press(Key::Escape);
        s.input.tick();
        s.input.set_active_set(s.menu);
        // The window lost focus, so the release never arrives.
        s.input.release_all();
        s.input.tick();
        s.input.press(Key::Escape);
        assert!(s.input.tick().just_pressed(s.back));
    }

    #[test]
    fn a_new_map_keeps_the_live_set_if_it_still_has_it() {
        let mut s = sets();
        s.input.set_active_set(s.menu);
        s.input.set_map(s.input.map().clone());
        assert_eq!(s.input.active_set(), s.menu);
        // A map with only the one set.
        s.input.set_map(ActionMap::new());
        assert_eq!(s.input.active_set(), ActionMap::GAMEPLAY);
    }

    #[test]
    fn new_bindings_do_not_press_a_held_action_again_and_let_go_of_one_they_unbind() {
        let mut input = Input::new(ActionMap::standard());
        let jump = input.map().action("jump").unwrap();
        let crouch = input.map().action("crouch").unwrap();
        input.press(Key::Space);
        input.press(Key::LeftControl);
        let frame = input.tick();
        assert!(frame.just_pressed(jump) && frame.just_pressed(crouch));

        // Jump keeps its key; crouch moves to another.
        let mut saved = ActionMap::standard();
        saved
            .read_bindings("[bindings]\ncrouch = [\"C\", \"GamepadEast\"]")
            .unwrap();
        input.set_bindings(saved);

        let frame = input.tick();
        assert!(frame.pressed(jump) && !frame.just_pressed(jump));
        assert!(frame.just_released(crouch));
        input.press(Key::C);
        assert!(input.tick().just_pressed(crouch));
    }

    #[test]
    fn the_last_device_is_the_one_used_on_purpose() {
        let mut input = Input::new(ActionMap::standard());
        assert_eq!(input.last_device(), Device::KeyboardAndMouse);

        // A stick that drifts and a trigger that rests off zero are not use.
        input.set_stick(Stick::LeftStick, StickAxis::X, 0.2);
        input.set_button_amount(GamepadButton::RightTrigger, 0.3);
        assert_eq!(input.last_device(), Device::KeyboardAndMouse);

        input.set_stick(Stick::LeftStick, StickAxis::Y, -0.9);
        assert_eq!(input.last_device(), Device::Gamepad);

        // Nor is a mouse on a desk that was bumped.
        input.move_mouse(1.0, -2.0);
        input.pointer_moved(3.0);
        assert_eq!(input.last_device(), Device::Gamepad);
        input.pointer_moved(f32::NAN);
        assert_eq!(input.last_device(), Device::Gamepad);
        // What it moved before the gamepad was used again does not add up.
        input.press(GamepadButton::GamepadSouth);
        input.pointer_moved(5.0);
        assert_eq!(input.last_device(), Device::Gamepad);
        input.pointer_moved(5.0);
        assert_eq!(input.last_device(), Device::KeyboardAndMouse);

        input.set_button_amount(GamepadButton::LeftTrigger, 0.8);
        assert_eq!(input.last_device(), Device::Gamepad);
        input.move_mouse(30.0, 0.0);
        assert_eq!(input.last_device(), Device::KeyboardAndMouse);

        input.press(GamepadButton::DpadDown);
        assert_eq!(input.last_device(), Device::Gamepad);
        input.press(MouseWheel::MouseWheelUp);
        assert_eq!(input.last_device(), Device::KeyboardAndMouse);
        input.press(GamepadButton::GamepadEast);
        input.press(Key::Escape);
        assert_eq!(input.last_device(), Device::KeyboardAndMouse);
        // Letting go is not use.
        input.release(GamepadButton::GamepadEast);
        assert_eq!(input.last_device(), Device::KeyboardAndMouse);

        // A gamepad that is unplugged leaves the keyboard and mouse.
        input.press(GamepadButton::GamepadSouth);
        input.gamepad_disconnected();
        assert_eq!(input.last_device(), Device::KeyboardAndMouse);
        let jump = input.map().action("jump").unwrap();
        assert!(!input.tick().pressed(jump));
    }

    #[test]
    fn an_own_action_is_live_in_every_set_and_the_opening_press_does_not_close() {
        let mut map = ActionMap::new();
        let menu = map.add_set("menu", MouseMode::Pointer).unwrap();
        let toggle = map.own_button("toggle", ["F1"]).unwrap();
        let mut input = Input::new(map);

        input.press(Key::F1);
        let frame = input.tick();
        assert!(frame.just_pressed(toggle));

        // The set changes while F1 is still down: that press was for gameplay.
        input.set_active_set(menu);
        let frame = input.tick();
        assert!(!frame.pressed(toggle) && frame.just_released(toggle));

        input.release(Key::F1);
        input.tick();
        input.press(Key::F1);
        let frame = input.tick();
        assert!(frame.just_pressed(toggle));
    }

    #[test]
    fn a_set_scoped_own_action_is_heard_only_in_its_set() {
        let mut map = ActionMap::new();
        let riding = map.add_set("riding", MouseMode::Look).unwrap();
        let gallop = map.own_button_in("gallop", ["H"], riding).unwrap();
        let mut input = Input::new(map);

        input.press(Key::H);
        assert!(!input.tick().pressed(gallop));
        input.release(Key::H);
        input.tick();
        input.set_active_set(riding);
        input.press(Key::H);
        assert!(input.tick().just_pressed(gallop));
    }

    #[test]
    fn a_new_map_keeps_what_is_held_and_the_set_that_is_live() {
        let mut map = ActionMap::new();
        let jump = map.button("jump", ["Space"]).unwrap();
        let shop = map.add_set("shop", MouseMode::Pointer).unwrap();
        let mut input = Input::new(map);
        input.set_active_set(shop);
        input.press(Key::Space);
        input.tick();

        // A new map with another action first, and the same sets reordered.
        let mut next = ActionMap::new();
        let extra = next.button("extra", ["X"]).unwrap();
        let jump2 = next.button("jump", ["Space"]).unwrap();
        let other = next.add_set("other", MouseMode::Look).unwrap();
        let shop2 = next.add_set("shop", MouseMode::Pointer).unwrap();
        assert_ne!(next.layout(), input.map().layout());
        input.set_map(next);
        assert_eq!(input.map().set_name(shop2), "shop");
        assert_ne!(shop2, shop);
        assert_ne!(other, shop2);
        let layout = input.map().layout();
        let frame = input.tick();
        assert_eq!(frame.layout, layout);
        assert!(!frame.just_pressed(extra));
        let _ = (jump, jump2);

        // Held across the swap: not pressed anew. (jump is in gameplay, which
        // is not live in "shop", so use the live set's reading: gameplay.)
        let mut map = ActionMap::new();
        let run = map.button("run", ["Space"]).unwrap();
        let mut input = Input::new(map);
        input.press(Key::Space);
        assert!(input.tick().just_pressed(run));
        let mut next = ActionMap::new();
        next.button("new", ["Q"]).unwrap();
        let run2 = next.button("run", ["Space"]).unwrap();
        input.set_map(next);
        let frame = input.tick();
        assert!(frame.pressed(run2) && !frame.just_pressed(run2));

        // A set that is gone falls back to gameplay.
        let mut map = ActionMap::new();
        let gone = map.add_set("gone", MouseMode::Pointer).unwrap();
        let mut input = Input::new(map);
        input.set_active_set(gone);
        input.set_map(ActionMap::new());
        assert_eq!(input.active_set(), ActionMap::GAMEPLAY);
    }

    #[test]
    fn bindings_alone_do_not_change_the_layout() {
        let mut map = ActionMap::new();
        let jump = map.button("jump", ["Space"]).unwrap();
        let before = map.layout();
        map.set_bindings(jump, ["J".parse::<Binding>().unwrap()])
            .unwrap();
        assert_eq!(map.layout(), before);
        map.button("fire", ["F"]).unwrap();
        assert_ne!(map.layout(), before);
    }

    #[test]
    fn the_wheel_shows_once_and_the_pointer_stays() {
        let mut input = Input::new(ActionMap::new());
        assert_eq!(input.tick().screen, [1920.0, 1080.0]);
        assert_eq!(input.tick().pointer, None);

        input.set_pointer(Some([100.0, 50.0]), [1280.0, 720.0]);
        input.roll_wheel(1.0);
        input.roll_wheel(0.5);
        input.roll_wheel(f32::NAN);
        let mut peeked = InputFrame::default();
        input.peek(&mut peeked);
        assert_eq!(peeked.wheel, 1.5);
        assert_eq!(peeked.pointer, Some([100.0, 50.0]));

        let frame = input.tick();
        assert_eq!(frame.wheel, 1.5);
        assert_eq!(frame.pointer, Some([100.0, 50.0]));
        assert_eq!(frame.screen, [1280.0, 720.0]);
        let frame = input.tick();
        assert_eq!(frame.wheel, 0.0);
        assert_eq!(frame.pointer, Some([100.0, 50.0]));

        // Bad numbers are ignored; the pointer going away is reported.
        input.set_pointer(Some([f32::NAN, 1.0]), [0.0, 720.0]);
        let frame = input.tick();
        assert_eq!(frame.pointer, None);
        assert_eq!(frame.screen, [1280.0, 720.0]);

        input.roll_wheel(5000.0);
        input.release_all();
        assert_eq!(input.tick().wheel, 0.0);
        input.roll_wheel(5000.0);
        assert_eq!(input.tick().wheel, 1000.0);
    }
}
