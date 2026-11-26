//! The per-tick snapshot of every action.

use crate::listen::Heard;
use crate::map::{ActionId, Device};
use crate::typing::TypedKey;

/// What one action is doing during one tick.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ActionState {
    /// The action is active: its button is held, or its axis is off centre.
    pub pressed: bool,
    /// The action became active this tick.
    pub just_pressed: bool,
    /// The action stopped being active this tick.
    pub just_released: bool,
    /// The action's value. Buttons and 1D axes use only the first number.
    pub axis: [f32; 2],
    /// Mouse movement since the previous tick, in raw counts: right and up are positive.
    pub delta: [f32; 2],
}

/// Every action's state for one simulation tick.
///
/// This is plain data. The simulation reads it, and it is also what gets
/// recorded for replays.
#[derive(Debug, PartialEq)]
pub struct InputFrame {
    /// Which tick this is. The first tick is 1; 0 means no tick has run yet.
    pub tick: u64,
    /// Where the pointer is, from the top-left corner of the screen, right and
    /// down, in whatever units the window layer reports (LORE's engine reports
    /// HUD units: pixels on a screen 1080 high). `None` when there is no free
    /// pointer: the mouse looks around, it is outside the window, or there is
    /// no window.
    pub pointer: Option<[f32; 2]>,
    /// The size of the screen in the same units as `pointer`.
    /// `[1920.0, 1080.0]` when nobody said.
    pub screen: [f32; 2],
    /// Notches the wheel rolled over the free pointer since the previous tick.
    /// Above zero is away from the player. A touchpad gives parts of a notch.
    pub wheel: f32,
    /// The characters the player typed since the previous tick, while a
    /// script takes text (see [`Input::set_typing`](crate::Input::set_typing)).
    /// Empty when nothing was typed.
    pub typed: String,
    /// Backspace, Enter and Escape, in the order pressed since the previous
    /// tick, while a script takes text.
    pub typed_keys: Vec<TypedKey>,
    /// The button the player pressed while a script was listening for one
    /// (see [`Input::set_listening`](crate::Input::set_listening)), or that
    /// they gave up. `None` on every other tick. A recording carries it, so
    /// a replay hears the same button on the same tick.
    pub heard: Option<Heard>,
    /// The device the player last used on purpose, as of this tick.
    pub device: Device,
    /// The [`ActionMap::layout`](crate::ActionMap::layout) of the map the frame was made from, so that
    /// a frame is never read with a map whose ids differ. 0 for a frame made
    /// without one.
    pub layout: u64,
    pub(crate) actions: Vec<ActionState>,
}

/// The screen size a frame reports until the window layer says otherwise.
pub(crate) const DEFAULT_SCREEN: [f32; 2] = [1920.0, 1080.0];

impl Default for InputFrame {
    fn default() -> Self {
        Self::idle(0)
    }
}

impl Clone for InputFrame {
    fn clone(&self) -> Self {
        Self {
            tick: self.tick,
            pointer: self.pointer,
            screen: self.screen,
            wheel: self.wheel,
            typed: self.typed.clone(),
            typed_keys: self.typed_keys.clone(),
            heard: self.heard,
            device: self.device,
            layout: self.layout,
            actions: self.actions.clone(),
        }
    }

    /// Copies `source` into this frame, reusing its memory: a frame that is
    /// refreshed every tick this way does not allocate.
    fn clone_from(&mut self, source: &Self) {
        self.tick = source.tick;
        self.pointer = source.pointer;
        self.screen = source.screen;
        self.wheel = source.wheel;
        self.typed.clone_from(&source.typed);
        self.typed_keys.clone_from(&source.typed_keys);
        self.heard = source.heard;
        self.device = source.device;
        self.layout = source.layout;
        self.actions.clone_from(&source.actions);
    }
}

impl InputFrame {
    /// A frame with `actions` idle actions.
    pub(crate) fn idle(actions: usize) -> Self {
        Self {
            tick: 0,
            pointer: None,
            screen: DEFAULT_SCREEN,
            wheel: 0.0,
            typed: String::new(),
            typed_keys: Vec::new(),
            heard: None,
            device: Device::KeyboardAndMouse,
            layout: 0,
            actions: vec![ActionState::default(); actions],
        }
    }

    /// The full state of an action. An id from a different map reads as idle.
    #[must_use]
    pub fn get(&self, action: ActionId) -> ActionState {
        self.actions
            .get(action.index())
            .copied()
            .unwrap_or_default()
    }

    /// Whether the action is active: a held button, or an axis off centre.
    #[must_use]
    pub fn pressed(&self, action: ActionId) -> bool {
        self.get(action).pressed
    }

    /// Whether the action became active this tick. True for exactly one tick per press.
    #[must_use]
    pub fn just_pressed(&self, action: ActionId) -> bool {
        self.get(action).just_pressed
    }

    /// Whether the action stopped being active this tick.
    #[must_use]
    pub fn just_released(&self, action: ActionId) -> bool {
        self.get(action).just_released
    }

    /// The action as one number.
    ///
    /// For a button this is 0 to 1 (how far a trigger is pulled; keys are 0 or 1).
    /// For a 1D axis it is -1 to 1.
    #[must_use]
    pub fn value(&self, action: ActionId) -> f32 {
        self.get(action).axis[0]
    }

    /// The action as a direction: X and Y from -1 to 1, never longer than 1.
    ///
    /// Right and up are positive. Mouse movement is not included; read it with
    /// [`delta`](Self::delta).
    #[must_use]
    pub fn axis2d(&self, action: ActionId) -> [f32; 2] {
        self.get(action).axis
    }

    /// Mouse movement bound to the action since the previous tick, in raw counts.
    ///
    /// Right and up are positive. Multiply by a mouse sensitivity, but not by
    /// the tick length: counts already measure how far the mouse moved. A stick
    /// bound to the same action reads through [`axis2d`](Self::axis2d) as a
    /// rate, which does need multiplying by the tick length.
    #[must_use]
    pub fn delta(&self, action: ActionId) -> [f32; 2] {
        self.get(action).delta
    }
}
