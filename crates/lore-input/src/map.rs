//! The action map: named actions and the inputs bound to them.

use std::fmt;
use std::ops::Range;

use crate::binding::{ActionKind, Binding, Button};
use crate::devices::{Stick, StickAxis};
use crate::names::{UnknownName, unknown};
use crate::shaping::Deadzone;

/// A handle to an action in an [`ActionMap`].
///
/// Look an action up by name once, keep the id, and use it every tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActionId(usize);

impl ActionId {
    /// Position of the action in its map.
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// A handle to an action set in an [`ActionMap`].
///
/// Every map starts with one set, [`ActionMap::GAMEPLAY`];
/// [`ActionMap::add_set`] makes more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActionSetId(usize);

/// What the mouse is for while an action set is live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MouseMode {
    /// Looking around: the cursor is hidden and held in the window, and
    /// moving the mouse drives `MouseMotion` bindings.
    #[default]
    Look,
    /// Pointing at things: the cursor is visible and free to leave the
    /// window, as a menu needs.
    Pointer,
}

/// One named group of actions.
#[derive(Debug, Clone, PartialEq)]
struct Set {
    name: Box<str>,
    mouse: MouseMode,
}

/// One named action and where its bindings live in the map.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Action {
    pub(crate) name: Box<str>,
    pub(crate) kind: ActionKind,
    pub(crate) set: ActionSetId,
    pub(crate) bindings: Range<usize>,
    pub(crate) stick_curve: f32,
    /// What the action's direction and mouse movement are multiplied by:
    /// sideways, then up and down.
    pub(crate) sensitivity: [f32; 2],
    /// What the action's direction (a stick, or keys) is multiplied by on
    /// top of nothing else: mouse movement has `sensitivity`.
    pub(crate) stick_sensitivity: [f32; 2],
    /// Whether a game added the action for itself.
    pub(crate) own: bool,
    /// Whether the action is live in every set: a game's own action that
    /// was not given a set of its own.
    pub(crate) everywhere: bool,
}

/// Named actions and the keys, buttons and sticks that drive them.
///
/// ```
/// use lore_input::ActionMap;
///
/// let mut map = ActionMap::new();
/// map.button("jump", ["Space", "GamepadSouth"])?;
/// map.axis2d("move", ["WASD", "LeftStick"])?;
/// map.axis2d("look", ["MouseMotion", "RightStick"])?;
/// # Ok::<(), lore_input::MapError>(())
/// ```
///
/// # Action sets
///
/// Actions are grouped into sets, and one set is live at a time: while a
/// menu is open, `fire` should not fire. A map starts with the set
/// [`GAMEPLAY`](Self::GAMEPLAY). [`add_set`](Self::add_set) starts another,
/// and [`Input::set_active_set`](crate::Input::set_active_set) switches
/// between them. The same key can do one thing in each set.
///
/// ```
/// use lore_input::{ActionMap, MouseMode};
///
/// let mut map = ActionMap::new();
/// map.button("pause", ["Escape", "GamepadStart"])?;
///
/// // Actions added from here on belong to "menu".
/// let menu = map.add_set("menu", MouseMode::Pointer)?;
/// let resume = map.button("resume", ["Escape", "GamepadStart"])?;
///
/// assert_eq!(map.set_of(resume), Some(menu));
/// assert!(map.conflicts().is_empty());
/// # Ok::<(), lore_input::MapError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ActionMap {
    actions: Vec<Action>,
    bindings: Vec<Binding>,
    deadzone: Deadzone,
    /// Never empty: the first is [`Self::GAMEPLAY`].
    sets: Vec<Set>,
    /// The set new actions join: the newest, except that the standard
    /// controls go back to [`Self::GAMEPLAY`] once their menu set is made.
    adding_to: ActionSetId,
}

impl Default for ActionMap {
    fn default() -> Self {
        Self {
            actions: Vec::new(),
            bindings: Vec::new(),
            deadzone: Deadzone::default(),
            sets: vec![Set {
                name: "gameplay".into(),
                mouse: MouseMode::Look,
            }],
            adding_to: Self::GAMEPLAY,
        }
    }
}

/// The standard actions the engine acts on only in a game that turns the
/// ability on (`setAbility`): blocking, dodging and locking on.
const OPTIONAL: [&str; 3] = ["block", "dodge", "lock_on"];

/// The pairs of standard actions that share a key or button on purpose:
/// see [`ActionMap::standard`].
const SHARED_ON_PURPOSE: [(&str, &str); 2] = [("aim", "block"), ("crouch", "dodge")];

impl ActionMap {
    /// The set every map starts with, named `"gameplay"`. The mouse looks
    /// around while it is live.
    pub const GAMEPLAY: ActionSetId = ActionSetId(0);

    /// A map with no actions and the one set [`GAMEPLAY`](Self::GAMEPLAY).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new action set, such as `"menu"`: every action added after
    /// this call belongs to it.
    ///
    /// `mouse` says what the mouse is for while the set is live:
    /// [`MouseMode::Pointer`] for a menu, [`MouseMode::Look`] for a set the
    /// player still looks around in, such as driving.
    ///
    /// Action names stay unique across the whole map, so `"jump"` means one
    /// thing wherever it is read.
    ///
    /// # Errors
    ///
    /// Fails if the name is empty or a set already has it.
    pub fn add_set(&mut self, name: &str, mouse: MouseMode) -> Result<ActionSetId, MapError> {
        if name.trim().is_empty() {
            return Err(MapError::EmptySetName);
        }
        if self.sets.iter().any(|set| &*set.name == name) {
            return Err(MapError::DuplicateSet {
                set: name.to_owned(),
            });
        }
        self.sets.push(Set {
            name: name.into(),
            mouse,
        });
        self.adding_to = ActionSetId(self.sets.len() - 1);
        Ok(self.adding_to)
    }

    /// Finds an action set by name.
    ///
    /// # Errors
    ///
    /// Fails if no set has that name; the error suggests the closest one.
    pub fn set(&self, name: &str) -> Result<ActionSetId, UnknownName> {
        self.sets
            .iter()
            .position(|set| &*set.name == name)
            .map(ActionSetId)
            .ok_or_else(|| unknown("action set", name, self.sets.iter().map(|s| &*s.name)))
    }

    /// Every action set in the map, [`GAMEPLAY`](Self::GAMEPLAY) first.
    pub fn sets(&self) -> impl ExactSizeIterator<Item = ActionSetId> + use<> {
        (0..self.sets.len()).map(ActionSetId)
    }

    /// The set's name, or `""` for an id from a different map.
    #[must_use]
    pub fn set_name(&self, set: ActionSetId) -> &str {
        self.sets.get(set.0).map_or("", |s| &s.name)
    }

    /// What the mouse is for while the set is live, or `None` for an id from
    /// a different map.
    #[must_use]
    pub fn mouse_mode(&self, set: ActionSetId) -> Option<MouseMode> {
        self.sets.get(set.0).map(|s| s.mouse)
    }

    /// A number that says what the map is made of: its sets (names and mouse
    /// use) and its actions (name, kind and set, in order). Two maps with the
    /// same number give an action the same id, so a frame made from one can be
    /// read with the other. Bindings do not count.
    ///
    /// It walks the whole map: keep the answer rather than asking every tick.
    #[must_use]
    pub fn layout(&self) -> u64 {
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for byte in bytes.iter().chain(&[0xff]) {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(PRIME);
            }
        };
        for set in &self.sets {
            eat(set.name.as_bytes());
            eat(&[set.mouse as u8]);
        }
        eat(b"actions");
        for action in &self.actions {
            eat(action.name.as_bytes());
            eat(&[action.kind as u8]);
            eat(&(action.set.0 as u64).to_le_bytes());
        }
        hash
    }

    /// The set the action belongs to, or `None` for an id from a different map.
    #[must_use]
    pub fn set_of(&self, action: ActionId) -> Option<ActionSetId> {
        self.actions.get(action.0).map(|a| a.set)
    }

    /// The standard controls of an adventure, in first or third person, for
    /// keyboard and mouse and for gamepad.
    ///
    /// | Action | Keyboard and mouse | Gamepad |
    /// | --- | --- | --- |
    /// | `move` | WASD | left stick |
    /// | `look` | mouse | right stick |
    /// | `jump` | Space | south button |
    /// | `crouch` | Left Control | east button |
    /// | `sprint` | Left Shift | left stick press |
    /// | `fire` | left mouse button | right trigger |
    /// | `aim` | right mouse button | left trigger |
    /// | `reload` | R | west button |
    /// | `interact` | E | north button |
    /// | `melee` | V | right stick press |
    /// | `use_item` | G | right bumper |
    /// | `next_item` | wheel down | d-pad right |
    /// | `previous_item` | wheel up | d-pad left |
    /// | `map` | M | d-pad up |
    /// | `inventory` | Tab | d-pad down |
    /// | `switch_view` | F | Select (Xbox View) |
    /// | `block` | right mouse button | left trigger |
    /// | `dodge` | Left Alt | east button |
    /// | `lock_on` | Q | left bumper |
    /// | `pause` | Escape | start |
    ///
    /// A gamepad has no buttons to spare, so two of the fighting actions
    /// share one with the action a player does in its place: `block` with
    /// `aim` (a guard is raised, or a bow is aimed) and, on a gamepad,
    /// `dodge` with `crouch`. The engine acts on `block`, `dodge` and
    /// `lock_on` only in a game that turns those abilities on, and
    /// [`conflicts`](Self::conflicts) does not report the two pairs.
    ///
    /// The engine itself acts on `move`, `look`, `jump`, `crouch`, `sprint`,
    /// `fire`, `aim`, `reload` and `pause`, and on `switch_view` when the
    /// game lets the player choose between first and third person.
    /// `interact`, `melee`, `use_item`, `next_item`, `previous_item`,
    /// `map` and `inventory` are there for a game's script to read, so that
    /// every game calls them the same and a player binds them once. The
    /// left bumper is left for a game's own action.
    ///
    /// A second set, `"menu"`, holds the actions menus are worked with. The
    /// mouse points in it. `menu_click` is a click on a script's menu, and a
    /// button held down to drag a slider; the right trigger is its gamepad
    /// twin, though a gamepad has no pointer, so it only matters where a
    /// script treats it so. `pause` is for a game's pause menu, which the
    /// game makes itself: a game switches to this set with
    /// `Frame::use_action_set`, and a script with `game.input.useSet`.
    ///
    /// | Action | Keyboard | Gamepad |
    /// | --- | --- | --- |
    /// | `menu_move` | arrow keys or WASD | d-pad or left stick |
    /// | `menu_confirm` | Enter or Space | south button |
    /// | `menu_click` | left mouse button | right trigger |
    /// | `menu_back` | Escape | east button |
    /// | `menu_previous_tab` | Q | left bumper |
    /// | `menu_next_tab` | E | right bumper |
    /// | `menu_close` | P | start |
    ///
    /// An action a game adds to these joins `"gameplay"`, with `jump` and
    /// the rest, not the menu's.
    ///
    /// ```
    /// use lore_input::{ActionMap, MouseMode};
    ///
    /// let mut map = ActionMap::standard();
    /// let menu = map.set("menu")?;
    /// assert_eq!(map.mouse_mode(menu), Some(MouseMode::Pointer));
    /// assert_eq!(map.set_of(map.action("menu_confirm")?), Some(menu));
    /// let dash = map.button("dash", ["LeftAlt", "LeftBumper"])?;
    /// assert_eq!(map.set_of(dash), Some(ActionMap::GAMEPLAY));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn standard() -> Self {
        const BUTTONS: &[(&str, [&str; 2])] = &[
            ("jump", ["Space", "GamepadSouth"]),
            ("crouch", ["LeftControl", "GamepadEast"]),
            ("sprint", ["LeftShift", "LeftStickPress"]),
            ("fire", ["MouseLeft", "RightTrigger"]),
            ("aim", ["MouseRight", "LeftTrigger"]),
            ("reload", ["R", "GamepadWest"]),
            ("interact", ["E", "GamepadNorth"]),
            ("melee", ["V", "RightStickPress"]),
            ("use_item", ["G", "RightBumper"]),
            ("next_item", ["MouseWheelDown", "DpadRight"]),
            ("previous_item", ["MouseWheelUp", "DpadLeft"]),
            ("map", ["M", "DpadUp"]),
            ("inventory", ["Tab", "DpadDown"]),
            ("switch_view", ["F", "GamepadSelect"]),
            ("block", ["MouseRight", "LeftTrigger"]),
            ("dodge", ["LeftAlt", "GamepadEast"]),
            ("lock_on", ["Q", "LeftBumper"]),
            ("pause", ["Escape", "GamepadStart"]),
        ];
        const MENU_BUTTONS: &[(&str, [&str; 2])] = &[
            ("menu_back", ["Escape", "GamepadEast"]),
            ("menu_previous_tab", ["Q", "LeftBumper"]),
            ("menu_next_tab", ["E", "RightBumper"]),
            ("menu_close", ["P", "GamepadStart"]),
            ("menu_erase", ["Backspace", "GamepadWest"]),
            ("menu_space", ["Tab", "GamepadNorth"]),
        ];
        let mut map = Self::new();
        // The names and bindings above are fixed and covered by a test, so none
        // of these calls can fail; ignoring the results keeps this infallible.
        let _ = map.axis2d("move", ["WASD", "LeftStick"]);
        if let Ok(look) = map.axis2d("look", ["MouseMotion", "RightStick"]) {
            map.set_stick_curve(look, 2.0);
        }
        for (name, bindings) in BUTTONS {
            let _ = map.button(name, bindings);
        }
        let _ = map.add_set("menu", MouseMode::Pointer);
        let _ = map.axis2d("menu_move", ["ArrowKeys", "WASD", "Dpad", "LeftStick"]);
        let _ = map.button("menu_confirm", ["Enter", "Space", "GamepadSouth"]);
        let _ = map.button("menu_click", ["MouseLeft", "RightTrigger"]);
        for (name, bindings) in MENU_BUTTONS {
            let _ = map.button(name, bindings);
        }
        // What a game adds to these is something the player does in the
        // game, not in a menu.
        map.adding_to = Self::GAMEPLAY;
        map
    }

    /// Adds a button action, such as jump or fire.
    ///
    /// Bind keys, mouse buttons, wheel notches and gamepad buttons by name:
    /// `map.button("jump", ["Space", "GamepadSouth"])`.
    ///
    /// # Errors
    ///
    /// Fails if the name is taken or empty, or if a binding is misspelled or is
    /// not a button.
    pub fn button<I>(&mut self, name: &str, bindings: I) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item: AsRef<str>>,
    {
        self.add_named(name, ActionKind::Button, bindings, false)
    }

    /// Adds a button action a game makes for itself, such as a key that
    /// opens its own menu.
    ///
    /// It is checked exactly like [`button`](Self::button). What differs is
    /// that it is live whichever action set is live, so the key that opens a
    /// menu also closes it. It belongs to [`GAMEPLAY`](Self::GAMEPLAY) for
    /// [`set_of`](Self::set_of), and the bindings file keeps it in a table of
    /// its own, `[game_bindings]`, so that a player's changes to it stay with
    /// the game that made it.
    ///
    /// # Errors
    ///
    /// Fails if the name is taken or empty, or if a binding is misspelled or is
    /// not a button.
    pub fn own_button<I>(&mut self, name: &str, bindings: I) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item: AsRef<str>>,
    {
        self.own_action(name, ActionKind::Button, bindings)
    }

    /// Adds an action a game makes for itself, of any kind: a button, a
    /// one-direction axis such as `"A/D"`, or a two-direction one such as
    /// `"WASD"` and `"LeftStick"`.
    ///
    /// It is kept like [`own_button`](Self::own_button): live in every set,
    /// and written under `[game_bindings]`. A player's file may rebind it only
    /// with bindings of the same kind.
    ///
    /// # Errors
    ///
    /// Fails if the name is taken or empty, or if a binding is misspelled or
    /// is not of `kind`.
    pub fn own_action<I>(
        &mut self,
        name: &str,
        kind: ActionKind,
        bindings: I,
    ) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item: AsRef<str>>,
    {
        let before = self.adding_to;
        self.adding_to = Self::GAMEPLAY;
        let made = self.add_named(name, kind, bindings, true);
        self.adding_to = before;
        made
    }

    /// Adds a button action a game makes for itself that is heard only
    /// while `set` is live, such as a key that only works on horseback.
    ///
    /// It is checked exactly like [`own_button`](Self::own_button), and kept
    /// in the bindings file in the same table. What differs is the set:
    /// [`set_of`](Self::set_of) says `set`, and the action does nothing while
    /// another set is live. A `set` from a different map is taken as
    /// [`GAMEPLAY`](Self::GAMEPLAY).
    ///
    /// # Errors
    ///
    /// Fails if the name is taken or empty, or if a binding is misspelled or is
    /// not a button.
    pub fn own_button_in<I>(
        &mut self,
        name: &str,
        bindings: I,
        set: ActionSetId,
    ) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item: AsRef<str>>,
    {
        self.own_action_in(name, ActionKind::Button, bindings, set)
    }

    /// [`own_action`](Self::own_action) that is heard only while `set` is live.
    ///
    /// # Errors
    ///
    /// Fails if the name is taken or empty, or if a binding is misspelled or
    /// is not of `kind`.
    pub fn own_action_in<I>(
        &mut self,
        name: &str,
        kind: ActionKind,
        bindings: I,
        set: ActionSetId,
    ) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item: AsRef<str>>,
    {
        let before = self.adding_to;
        self.adding_to = if self.mouse_mode(set).is_some() {
            set
        } else {
            Self::GAMEPLAY
        };
        let made = self.add_named(name, kind, bindings, true);
        self.adding_to = before;
        if let Ok(id) = made {
            self.actions[id.0].everywhere = false;
        }
        made
    }

    /// Whether `action` is heard while `set` is live: it belongs to the set,
    /// or it is a game's own action with no set of its own. False for an id
    /// from a different map.
    #[must_use]
    pub fn is_heard_in(&self, action: ActionId, set: ActionSetId) -> bool {
        self.actions
            .get(action.0)
            .is_some_and(|a| a.everywhere || a.set == set)
    }

    /// Whether a game added the action for itself with
    /// [`own_button`](Self::own_button). False for an id from a different map.
    #[must_use]
    pub fn is_own(&self, action: ActionId) -> bool {
        self.actions.get(action.0).is_some_and(|a| a.own)
    }

    /// Adds a 1D axis action, a number from -1 to 1, such as lean.
    ///
    /// Bind pairs of buttons or one axis of a stick. In a pair the button that
    /// pushes toward -1 comes first, so `"Q/E"` leans left with Q:
    /// `map.axis1d("lean", ["Q/E", "LeftStickX"])`.
    ///
    /// # Errors
    ///
    /// Fails if the name is taken or empty, or if a binding is misspelled or is
    /// not a 1D axis.
    pub fn axis1d<I>(&mut self, name: &str, bindings: I) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item: AsRef<str>>,
    {
        self.add_named(name, ActionKind::Axis1D, bindings, false)
    }

    /// Adds a 2D axis action, a direction, such as move or look.
    ///
    /// Bind four-button groups, sticks or the mouse. A custom group lists its
    /// buttons in up/down/left/right order, such as `"I/K/J/L"`:
    /// `map.axis2d("move", ["WASD", "LeftStick"])`.
    ///
    /// # Errors
    ///
    /// Fails if the name is taken or empty, or if a binding is misspelled or is
    /// not a 2D axis.
    pub fn axis2d<I>(&mut self, name: &str, bindings: I) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item: AsRef<str>>,
    {
        self.add_named(name, ActionKind::Axis2D, bindings, false)
    }

    /// Parses binding names, then adds the action.
    fn add_named<I>(
        &mut self,
        name: &str,
        kind: ActionKind,
        bindings: I,
        own: bool,
    ) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item: AsRef<str>>,
    {
        let start = self.bindings.len();
        for text in bindings {
            match text.as_ref().parse() {
                Ok(binding) => self.bindings.push(binding),
                Err(source) => {
                    self.bindings.truncate(start);
                    return Err(MapError::UnknownBinding {
                        action: name.to_owned(),
                        source,
                    });
                }
            }
        }
        self.finish_action(name, kind, start, own)
    }

    /// Adds an action from already-built [`Binding`] values.
    ///
    /// This is for engine code that builds bindings itself. Games normally use
    /// [`button`](Self::button), [`axis1d`](Self::axis1d) and [`axis2d`](Self::axis2d).
    ///
    /// # Errors
    ///
    /// Fails if the name is taken or empty, or if a binding does not match `kind`.
    pub fn add<I>(
        &mut self,
        name: &str,
        kind: ActionKind,
        bindings: I,
    ) -> Result<ActionId, MapError>
    where
        I: IntoIterator<Item = Binding>,
    {
        let start = self.bindings.len();
        self.bindings.extend(bindings);
        self.finish_action(name, kind, start, false)
    }

    /// Validates the action whose bindings start at `start`, and records it.
    ///
    /// On failure the map is left exactly as it was before the bindings were pushed.
    fn finish_action(
        &mut self,
        name: &str,
        kind: ActionKind,
        start: usize,
        own: bool,
    ) -> Result<ActionId, MapError> {
        let problem = if name.trim().is_empty() {
            Some(MapError::EmptyName)
        } else if self.actions.iter().any(|action| &*action.name == name) {
            Some(MapError::DuplicateAction {
                action: name.to_owned(),
            })
        } else {
            self.bindings[start..]
                .iter()
                .find(|binding| binding.kind() != kind)
                .map(|binding| MapError::WrongKind {
                    action: name.to_owned(),
                    kind,
                    binding: *binding,
                })
        };
        if let Some(problem) = problem {
            self.bindings.truncate(start);
            return Err(problem);
        }
        self.actions.push(Action {
            name: name.into(),
            kind,
            set: self.adding_to,
            bindings: start..self.bindings.len(),
            stick_curve: 1.0,
            sensitivity: [1.0, 1.0],
            stick_sensitivity: [1.0, 1.0],
            own,
            everywhere: own,
        });
        Ok(ActionId(self.actions.len() - 1))
    }

    /// Finds an action by name.
    ///
    /// # Errors
    ///
    /// Fails if no action has that name; the error suggests the closest one.
    pub fn action(&self, name: &str) -> Result<ActionId, UnknownName> {
        self.actions
            .iter()
            .position(|action| &*action.name == name)
            .map(ActionId)
            .ok_or_else(|| unknown("action", name, self.actions.iter().map(|a| &*a.name)))
    }

    /// Every action in the map, in the order they were added.
    pub fn actions(&self) -> impl ExactSizeIterator<Item = ActionId> + use<> {
        (0..self.actions.len()).map(ActionId)
    }

    /// How many actions the map has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Whether the map has no actions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// The action's name, or `""` for an id from a different map.
    #[must_use]
    pub fn name(&self, action: ActionId) -> &str {
        self.actions.get(action.0).map_or("", |a| &a.name)
    }

    /// The action's kind, or `None` for an id from a different map.
    #[must_use]
    pub fn kind(&self, action: ActionId) -> Option<ActionKind> {
        self.actions.get(action.0).map(|a| a.kind)
    }

    /// The inputs bound to the action.
    #[must_use]
    pub fn bindings(&self, action: ActionId) -> &[Binding] {
        self.actions
            .get(action.0)
            .and_then(|a| self.bindings.get(a.bindings.clone()))
            .unwrap_or_default()
    }

    /// The actions that `binding` presses, in the order they were added. A
    /// button counts wherever an action uses it, alone or as one of the keys
    /// of a direction such as `"WASD"`; any other binding (a stick, mouse
    /// motion) counts only where an action has exactly it.
    #[must_use]
    pub fn actions_bound_to(&self, binding: Binding) -> Vec<ActionId> {
        self.actions()
            .filter(|action| {
                let bound = self.bindings(*action);
                match binding {
                    Binding::Button(button) => Binding::buttons_of(bound).any(|b| b == button),
                    other => bound.contains(&other),
                }
            })
            .collect()
    }

    /// Replaces everything bound to the action.
    ///
    /// An empty list leaves the action with nothing bound, which is how a
    /// player turns one off. Ids stay valid. An id from a different map is
    /// ignored.
    ///
    /// # Errors
    ///
    /// Fails, changing nothing, if a binding does not match the action's kind.
    pub fn set_bindings<I>(&mut self, action: ActionId, bindings: I) -> Result<(), MapError>
    where
        I: IntoIterator<Item = Binding>,
    {
        let Some(target) = self.actions.get(action.0) else {
            return Ok(());
        };
        let new: Vec<Binding> = bindings.into_iter().collect();
        if let Some(binding) = new.iter().find(|binding| binding.kind() != target.kind) {
            return Err(MapError::WrongKind {
                action: target.name.to_string(),
                kind: target.kind,
                binding: *binding,
            });
        }
        let old = target.bindings.clone();
        let end = old.start + new.len();
        self.bindings.splice(old.clone(), new);
        // Every action stored after this one moves by the change in length.
        for (index, other) in self.actions.iter_mut().enumerate() {
            if index == action.0 {
                other.bindings = old.start..end;
            } else if other.bindings.start >= old.end {
                let length = other.bindings.len();
                let start = other.bindings.start - old.end + end;
                other.bindings = start..start + length;
            }
        }
        Ok(())
    }

    /// Swaps one key or button of the action for another, wherever the action
    /// uses it: alone, or as one of the keys in a group such as `"WASD"`.
    ///
    /// If the action already uses `to` somewhere, the two trade places, so a
    /// group never ends up with one key pulling two ways.
    ///
    /// Returns whether anything changed. This is what a controls screen calls
    /// once the player has pressed the new key; [`Input::start_rebind`]
    /// does the listening.
    ///
    /// [`Input::start_rebind`]: crate::Input::start_rebind
    pub fn replace_button(&mut self, action: ActionId, from: Button, to: Button) -> bool {
        let Some(bindings) = self
            .actions
            .get(action.0)
            .and_then(|a| self.bindings.get_mut(a.bindings.clone()))
        else {
            return false;
        };
        let mut changed = false;
        let mut swap = |button: &mut Button| {
            if from == to {
                return;
            }
            if *button == from {
                *button = to;
                changed = true;
            } else if *button == to {
                *button = from;
            }
        };
        for binding in bindings {
            match binding {
                Binding::Button(button) => swap(button),
                Binding::Axis { negative, positive } => {
                    swap(negative);
                    swap(positive);
                }
                Binding::Directions {
                    up,
                    down,
                    left,
                    right,
                } => {
                    swap(up);
                    swap(down);
                    swap(left);
                    swap(right);
                }
                Binding::StickAxis(..) | Binding::Stick(_) | Binding::MouseMotion => {}
            }
        }
        changed
    }

    /// Lists every key, button or stick that two different actions in the
    /// same set both use. Actions in different sets are never live together,
    /// so they share inputs freely. The exception is an action a game made
    /// for itself with [`own_button`](Self::own_button): it is live in every
    /// set, so it is checked against all of them.
    ///
    /// Sharing is allowed, and sometimes wanted, so this never stops anything:
    /// each [`Conflict`] prints as a sentence to show the player. An empty
    /// list means no input does two things.
    #[must_use]
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut found: Vec<Conflict> = Vec::new();
        for (index, first) in self.actions.iter().enumerate() {
            for second in &self.actions[index + 1..] {
                if second.set != first.set && !first.everywhere && !second.everywhere {
                    continue;
                }
                // An action a game made for itself may take the button of
                // one the engine only acts on when the game asks for it:
                // most games never turn those on.
                let optional = |name: &str| OPTIONAL.contains(&name);
                if (first.own && optional(&second.name)) || (second.own && optional(&first.name)) {
                    continue;
                }
                // The standard controls share these on purpose.
                let pair = (&*first.name, &*second.name);
                if SHARED_ON_PURPOSE
                    .iter()
                    .any(|(a, b)| pair == (*a, *b) || pair == (*b, *a))
                {
                    continue;
                }
                for ours in &self.bindings[first.bindings.clone()] {
                    for theirs in &self.bindings[second.bindings.clone()] {
                        for input in shared(*ours, *theirs) {
                            let conflict = Conflict {
                                input,
                                first: first.name.to_string(),
                                second: second.name.to_string(),
                            };
                            if !found.contains(&conflict) {
                                found.push(conflict);
                            }
                        }
                    }
                }
            }
        }
        found
    }

    /// Sets how sticks bound to this action respond to tilt.
    ///
    /// 1 is linear, the default. Higher values give finer control near the
    /// centre: 2 is a good choice for aiming. Values that are not numbers and
    /// ids from a different map are ignored.
    pub fn set_stick_curve(&mut self, action: ActionId, curve: f32) {
        if let Some(action) = self.actions.get_mut(action.0)
            && curve.is_finite()
        {
            action.stick_curve = curve.max(0.01);
        }
    }

    /// Multiplies what this action reports, sideways and then up and down:
    /// its direction and its mouse movement alike. This is how a player's
    /// sensitivity setting reaches every game the same way.
    ///
    /// `[1.0, 1.0]` is the default. `[2.0, 2.0]` turns twice as fast, and a
    /// negative second number swaps up and down, for players who invert
    /// their view. A direction can then be longer than 1. Numbers that are
    /// not finite and ids from a different map are ignored.
    ///
    /// ```
    /// use lore_input::ActionMap;
    ///
    /// let mut map = ActionMap::standard();
    /// let look = map.action("look")?;
    /// // Half as fast, with up and down swapped.
    /// map.set_sensitivity(look, [0.5, -0.5]);
    /// assert_eq!(map.sensitivity(look), [0.5, -0.5]);
    /// # Ok::<(), lore_input::UnknownName>(())
    /// ```
    pub fn set_sensitivity(&mut self, action: ActionId, sensitivity: [f32; 2]) {
        if let Some(action) = self.actions.get_mut(action.0)
            && sensitivity.iter().all(|axis| axis.is_finite())
        {
            action.sensitivity = sensitivity;
            action.stick_sensitivity = sensitivity;
        }
    }

    /// Multiplies the action's mouse movement by `mouse` and its direction
    /// (a stick, or keys) by `stick`, each sideways and then up and down:
    /// for a player who wants a fast mouse and a slow stick, or the other
    /// way round. [`set_sensitivity`](Self::set_sensitivity) sets both to
    /// the same. Numbers that are not finite and ids from a different map
    /// are ignored.
    ///
    /// ```
    /// use lore_input::ActionMap;
    ///
    /// let mut map = ActionMap::standard();
    /// let look = map.action("look")?;
    /// map.set_sensitivities(look, [0.4, 0.4], [1.5, -1.5]);
    /// assert_eq!(map.sensitivity(look), [0.4, 0.4]);
    /// assert_eq!(map.stick_sensitivity(look), [1.5, -1.5]);
    /// # Ok::<(), lore_input::UnknownName>(())
    /// ```
    pub fn set_sensitivities(&mut self, action: ActionId, mouse: [f32; 2], stick: [f32; 2]) {
        if let Some(action) = self.actions.get_mut(action.0)
            && mouse.iter().chain(&stick).all(|axis| axis.is_finite())
        {
            action.sensitivity = mouse;
            action.stick_sensitivity = stick;
        }
    }

    /// What the action's direction is multiplied by; see
    /// [`set_sensitivities`](Self::set_sensitivities). `[1.0, 1.0]` for an
    /// id from a different map.
    #[must_use]
    pub fn stick_sensitivity(&self, action: ActionId) -> [f32; 2] {
        self.actions
            .get(action.0)
            .map_or([1.0, 1.0], |action| action.stick_sensitivity)
    }

    /// What the action's mouse movement is multiplied by (and its
    /// direction too, unless [`set_sensitivities`](Self::set_sensitivities)
    /// said otherwise); see [`set_sensitivity`](Self::set_sensitivity). `[1.0, 1.0]` for an
    /// id from a different map.
    #[must_use]
    pub fn sensitivity(&self, action: ActionId) -> [f32; 2] {
        self.actions
            .get(action.0)
            .map_or([1.0, 1.0], |action| action.sensitivity)
    }

    /// The dead zone applied to both thumbsticks.
    #[must_use]
    pub fn deadzone(&self) -> Deadzone {
        self.deadzone
    }

    /// Changes the dead zone applied to both thumbsticks.
    pub fn set_deadzone(&mut self, deadzone: Deadzone) {
        if deadzone.inner.is_finite() && deadzone.outer.is_finite() {
            self.deadzone = deadzone;
        }
    }

    /// Lists every action a player on one kind of device could not use.
    ///
    /// LORE games are playable on a gamepad and on keyboard and mouse, so
    /// every action needs a binding on each. An empty list means the map is
    /// complete. Each [`ParityGap`] prints as a sentence saying what to add.
    #[must_use]
    pub fn parity_gaps(&self) -> Vec<ParityGap> {
        let mut gaps = Vec::new();
        for action in &self.actions {
            let bindings = &self.bindings[action.bindings.clone()];
            for (missing, has) in [
                (Device::Gamepad, bindings.iter().any(|b| b.is_gamepad())),
                (
                    Device::KeyboardAndMouse,
                    bindings.iter().any(|b| !b.is_gamepad()),
                ),
            ] {
                if !has {
                    gaps.push(ParityGap {
                        action: action.name.to_string(),
                        kind: action.kind,
                        missing,
                    });
                }
            }
        }
        gaps
    }

    /// The actions and flat binding list, for the tick loop.
    pub(crate) fn parts(&self) -> (&[Action], &[Binding]) {
        (&self.actions, &self.bindings)
    }
}

/// One physical thing a binding reads: what two actions can collide on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Button(Button),
    StickAxis(Stick, StickAxis),
    Mouse,
}

impl Source {
    /// Everything `binding` reads; at most four things.
    const fn of(binding: Binding) -> [Option<Self>; 4] {
        match binding {
            Binding::Button(button) => [Some(Self::Button(button)), None, None, None],
            Binding::Axis { negative, positive } => [
                Some(Self::Button(negative)),
                Some(Self::Button(positive)),
                None,
                None,
            ],
            Binding::Directions {
                up,
                down,
                left,
                right,
            } => [
                Some(Self::Button(up)),
                Some(Self::Button(down)),
                Some(Self::Button(left)),
                Some(Self::Button(right)),
            ],
            Binding::StickAxis(stick, axis) => {
                [Some(Self::StickAxis(stick, axis)), None, None, None]
            }
            Binding::Stick(stick) => [
                Some(Self::StickAxis(stick, StickAxis::X)),
                Some(Self::StickAxis(stick, StickAxis::Y)),
                None,
                None,
            ],
            Binding::MouseMotion => [Some(Self::Mouse), None, None, None],
        }
    }

    /// The name a player knows it by. Both axes of a stick go by the stick's name.
    const fn name(self) -> &'static str {
        match self {
            Self::Button(button) => button.name(),
            Self::StickAxis(stick, _) => stick.name(),
            Self::Mouse => "MouseMotion",
        }
    }
}

/// The names of what two bindings both read.
///
/// When one binding is wholly inside the other, that is one clash under the
/// smaller binding's own name (`"WASD"`, not four keys); otherwise each
/// shared key is named.
fn shared(ours: Binding, theirs: Binding) -> Vec<String> {
    let (ours_reads, theirs_reads) = (Source::of(ours), Source::of(theirs));
    let both: Vec<Source> = (ours_reads.into_iter().flatten())
        .filter(|input| theirs_reads.contains(&Some(*input)))
        .collect();
    let count = |reads: [Option<Source>; 4]| reads.into_iter().flatten().count();
    if both.is_empty() {
        Vec::new()
    } else if both.len() == count(ours_reads) {
        vec![ours.to_string()]
    } else if both.len() == count(theirs_reads) {
        vec![theirs.to_string()]
    } else {
        both.into_iter()
            .map(|input| input.name().to_owned())
            .collect()
    }
}

/// A key, button or stick that two actions both use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The shared input's name, for example `"F"`, `"WASD"` or `"LeftStick"`.
    pub input: String,
    /// The action that comes first in the map.
    pub first: String,
    /// The other action.
    pub second: String,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\"{}\" is bound to both \"{}\" and \"{}\", so it does both at once. \
             Bind one of them to something else, unless that is what you want.",
            self.input, self.first, self.second
        )
    }
}

/// A kind of input device a player might be using.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Device {
    /// Keyboard and mouse.
    KeyboardAndMouse,
    /// A gamepad.
    Gamepad,
}

/// An action that players on one kind of device cannot use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityGap {
    /// The action's name.
    pub action: String,
    /// The action's kind.
    pub kind: ActionKind,
    /// The device with no binding for it.
    pub missing: Device,
}

impl fmt::Display for ParityGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (device, players, example) = match (self.missing, self.kind) {
            (Device::Gamepad, ActionKind::Button) => ("gamepad", "gamepad", "GamepadSouth"),
            (Device::Gamepad, ActionKind::Axis1D) => ("gamepad", "gamepad", "LeftStickX"),
            (Device::Gamepad, ActionKind::Axis2D) => ("gamepad", "gamepad", "LeftStick"),
            (Device::KeyboardAndMouse, ActionKind::Button) => {
                ("keyboard or mouse", "keyboard and mouse", "Space")
            }
            (Device::KeyboardAndMouse, ActionKind::Axis1D) => {
                ("keyboard or mouse", "keyboard and mouse", "Q/E")
            }
            (Device::KeyboardAndMouse, ActionKind::Axis2D) => {
                ("keyboard or mouse", "keyboard and mouse", "WASD")
            }
        };
        write!(
            f,
            "Action \"{}\" has no {device} binding, so {players} players cannot use it. \
             Add one, for example \"{example}\".",
            self.action
        )
    }
}

/// Why an action could not be added to an [`ActionMap`].
#[derive(Debug, Clone, PartialEq)]
pub enum MapError {
    /// The action name was empty.
    EmptyName,
    /// The action set name was empty.
    EmptySetName,
    /// An action set with this name already exists.
    DuplicateSet {
        /// The repeated name.
        set: String,
    },
    /// An action with this name already exists.
    DuplicateAction {
        /// The repeated name.
        action: String,
    },
    /// A binding name was not recognised.
    UnknownBinding {
        /// The action being added.
        action: String,
        /// Which name failed, with a suggestion when one is close.
        source: UnknownName,
    },
    /// A binding produces a different kind of value than the action expects.
    WrongKind {
        /// The action being added.
        action: String,
        /// The action's kind.
        kind: ActionKind,
        /// The binding that does not fit.
        binding: Binding,
    },
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => f.write_str("An action needs a name, for example \"jump\"."),
            Self::EmptySetName => f.write_str("An action set needs a name, for example \"menu\"."),
            Self::DuplicateSet { set } => write!(
                f,
                "There is already an action set called \"{set}\". \
                 Look it up with `map.set(\"{set}\")`, or give the new one a different name."
            ),
            Self::DuplicateAction { action } => write!(
                f,
                "Action \"{action}\" is defined twice. \
                 Put all of its bindings in one list, or give the second action a different name."
            ),
            Self::UnknownBinding { action, source } => {
                write!(f, "Action \"{action}\": {source}")
            }
            Self::WrongKind {
                action,
                kind,
                binding,
            } => {
                let fix = match binding.kind() {
                    ActionKind::Button => "button",
                    ActionKind::Axis1D => "axis1d",
                    ActionKind::Axis2D => "axis2d",
                };
                write!(
                    f,
                    "Action \"{action}\" is {}, but \"{binding}\" is {}. \
                     Pick a different binding, or declare the action with `{fix}`.",
                    kind.describe(),
                    binding.kind().describe(),
                )
            }
        }
    }
}

impl std::error::Error for MapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnknownBinding { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{Key, Stick};

    #[test]
    fn default_map_is_complete_on_both_devices() {
        let map = ActionMap::standard();
        assert_eq!(map.len(), 29);
        // No scores are kept, so nothing shows them; Select (the Xbox View
        // button) switches the view.
        assert!(map.action("scoreboard").is_err());
        assert_eq!(map.parity_gaps(), Vec::new());
        // Nothing in the standard controls shares a key or a button.
        assert_eq!(map.conflicts(), Vec::new());
        for name in [
            "move",
            "look",
            "jump",
            "fire",
            "pause",
            "use_item",
            "next_item",
            "previous_item",
            "map",
            "inventory",
            "switch_view",
            "block",
            "dodge",
            "lock_on",
        ] {
            let action = map
                .action(name)
                .unwrap_or_else(|_| panic!("missing {name}"));
            assert_eq!(map.set_of(action), Some(ActionMap::GAMEPLAY), "{name}");
        }
        // What a shooter called them is gone.
        for name in ["grenade", "next_weapon", "previous_weapon"] {
            assert!(map.action(name).is_err(), "{name} is still an action");
        }
        let look = map.action("look").unwrap();
        assert_eq!(map.kind(look), Some(ActionKind::Axis2D));
    }

    #[test]
    fn actions_keep_their_bindings() {
        let mut map = ActionMap::new();
        let jump = map.button("jump", ["Space", "GamepadSouth"]).unwrap();
        let movement = map.axis2d("move", ["WASD", "LeftStick"]).unwrap();
        assert_eq!(map.name(jump), "jump");
        assert_eq!(map.bindings(jump).len(), 2);
        assert_eq!(
            map.bindings(movement),
            [
                Binding::directions(Key::W, Key::S, Key::A, Key::D),
                Binding::Stick(Stick::LeftStick)
            ]
        );
        assert_eq!(map.actions().collect::<Vec<_>>(), [jump, movement]);
    }

    #[test]
    fn unknown_action_suggests_the_closest() {
        let map = ActionMap::standard();
        let error = map.action("jmup").unwrap_err();
        assert_eq!(
            error.to_string(),
            "\"jmup\" is not a known action. Did you mean \"jump\"?"
        );
    }

    #[test]
    fn misspelled_binding_is_explained_and_leaves_the_map_unchanged() {
        let mut map = ActionMap::new();
        let error = map.button("jump", ["Space", "GamepadSuoth"]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Action \"jump\": \"GamepadSuoth\" is not a known binding. \
             Did you mean \"GamepadSouth\"?"
        );
        assert_eq!(map, ActionMap::new());
    }

    #[test]
    fn wrong_kind_is_explained_and_leaves_the_map_unchanged() {
        let mut map = ActionMap::new();
        let error = map.button("jump", ["LeftStick"]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Action \"jump\" is a button, but \"LeftStick\" is a 2D axis (a direction). \
             Pick a different binding, or declare the action with `axis2d`."
        );
        assert_eq!(map, ActionMap::new());
    }

    #[test]
    fn duplicate_and_empty_names_are_rejected() {
        let mut map = ActionMap::new();
        map.button("jump", ["Space"]).unwrap();
        assert!(matches!(
            map.button("jump", ["Enter"]),
            Err(MapError::DuplicateAction { .. })
        ));
        assert_eq!(map.button(" ", ["Enter"]), Err(MapError::EmptyName));
        assert_eq!(map.len(), 1);
        assert_eq!(map.bindings(map.action("jump").unwrap()).len(), 1);
    }

    #[test]
    fn parity_gaps_say_what_to_add() {
        let mut map = ActionMap::new();
        map.button("jump", ["Space"]).unwrap();
        map.axis2d("look", ["RightStick"]).unwrap();
        let gaps: Vec<String> = map.parity_gaps().iter().map(ToString::to_string).collect();
        assert_eq!(
            gaps,
            [
                "Action \"jump\" has no gamepad binding, so gamepad players cannot use it. \
                 Add one, for example \"GamepadSouth\".",
                "Action \"look\" has no keyboard or mouse binding, so keyboard and mouse players \
                 cannot use it. Add one, for example \"WASD\".",
            ]
        );
    }

    #[test]
    fn foreign_ids_are_harmless() {
        let map = ActionMap::new();
        let foreign = ActionId(7);
        assert_eq!(map.name(foreign), "");
        assert_eq!(map.kind(foreign), None);
        assert!(map.bindings(foreign).is_empty());
    }

    #[test]
    fn bindings_can_be_replaced_without_disturbing_other_actions() {
        let mut map = ActionMap::standard();
        let before = map.clone();
        let jump = map.action("jump").unwrap();
        let wide: Vec<Binding> = ["F", "Enter", "GamepadNorth"]
            .iter()
            .map(|name| name.parse().unwrap())
            .collect();

        map.set_bindings(jump, wide.clone()).unwrap();
        assert_eq!(map.bindings(jump), wide);
        map.set_bindings(jump, []).unwrap();
        assert!(map.bindings(jump).is_empty());
        for action in map.actions().filter(|action| *action != jump) {
            assert_eq!(map.bindings(action), before.bindings(action));
        }

        map.set_bindings(jump, before.bindings(jump).to_vec())
            .unwrap();
        for action in map.actions() {
            assert_eq!(map.bindings(action), before.bindings(action));
        }
    }

    #[test]
    fn bindings_of_the_wrong_kind_are_refused_and_change_nothing() {
        let mut map = ActionMap::standard();
        let before = map.clone();
        let jump = map.action("jump").unwrap();
        let error = map
            .set_bindings(jump, [Binding::Stick(Stick::LeftStick)])
            .unwrap_err();
        assert!(matches!(error, MapError::WrongKind { .. }));
        assert_eq!(map, before);
        assert_eq!(map.set_bindings(ActionId(99), []), Ok(()));
        assert_eq!(map, before);
    }

    #[test]
    fn one_button_can_be_swapped_alone_or_inside_a_group() {
        let mut map = ActionMap::standard();
        let jump = map.action("jump").unwrap();
        let movement = map.action("move").unwrap();

        assert!(map.replace_button(jump, Key::Space.into(), Key::F.into()));
        assert_eq!(map.bindings(jump)[0], Binding::Button(Key::F.into()));
        assert!(!map.replace_button(jump, Key::Space.into(), Key::G.into()));
        assert!(!map.replace_button(jump, Key::F.into(), Key::F.into()));

        assert!(map.replace_button(movement, Key::W.into(), Key::ArrowUp.into()));
        assert_eq!(
            map.bindings(movement)[0],
            Binding::directions(Key::ArrowUp, Key::S, Key::A, Key::D)
        );
        assert!(!map.replace_button(ActionId(99), Key::W.into(), Key::E.into()));
    }

    #[test]
    fn shared_inputs_are_found_and_explained() {
        let mut map = ActionMap::standard();
        assert_eq!(map.conflicts(), Vec::new());

        let jump = map.action("jump").unwrap();
        map.replace_button(jump, Key::Space.into(), Key::W.into());
        let look = map.action("look").unwrap();
        map.set_bindings(
            look,
            [Binding::MouseMotion, Binding::Stick(Stick::LeftStick)],
        )
        .unwrap();

        let said: Vec<String> = map.conflicts().iter().map(ToString::to_string).collect();
        assert_eq!(
            said,
            [
                "\"LeftStick\" is bound to both \"move\" and \"look\", so it does both at \
                 once. Bind one of them to something else, unless that is what you want.",
                "\"W\" is bound to both \"move\" and \"jump\", so it does both at once. \
                 Bind one of them to something else, unless that is what you want.",
            ]
        );
    }

    #[test]
    fn the_two_axes_of_a_stick_do_not_collide() {
        let mut map = ActionMap::new();
        map.axis1d("lean", ["LeftStickX"]).unwrap();
        map.axis1d("zoom", ["LeftStickY"]).unwrap();
        assert_eq!(map.conflicts(), Vec::new());
        map.axis2d("move", ["LeftStick"]).unwrap();
        let inputs: Vec<String> = map.conflicts().into_iter().map(|c| c.input).collect();
        assert_eq!(inputs, ["LeftStickX", "LeftStickY"]);
    }

    #[test]
    fn a_whole_group_shared_is_one_clash_and_part_of_one_names_the_keys() {
        let mut map = ActionMap::new();
        map.axis2d("move", ["WASD"]).unwrap();
        map.axis2d("look", ["WASD"]).unwrap();
        map.axis2d("menu", ["W/S/ArrowLeft/ArrowRight"]).unwrap();
        let found: Vec<(String, String, String)> = (map.conflicts().into_iter())
            .map(|c| (c.input, c.first, c.second))
            .collect();
        let clash = |input: &str, first: &str, second: &str| {
            (input.to_owned(), first.to_owned(), second.to_owned())
        };
        assert_eq!(
            found,
            [
                clash("WASD", "move", "look"),
                clash("W", "move", "menu"),
                clash("S", "move", "menu"),
                clash("W", "look", "menu"),
                clash("S", "look", "menu"),
            ]
        );
    }

    #[test]
    fn a_map_starts_with_the_gameplay_set() {
        let mut map = ActionMap::new();
        let jump = map.button("jump", ["Space", "GamepadSouth"]).unwrap();
        assert_eq!(map.sets().collect::<Vec<_>>(), [ActionMap::GAMEPLAY]);
        assert_eq!(map.set_name(ActionMap::GAMEPLAY), "gameplay");
        assert_eq!(map.set("gameplay"), Ok(ActionMap::GAMEPLAY));
        assert_eq!(map.mouse_mode(ActionMap::GAMEPLAY), Some(MouseMode::Look));
        assert_eq!(map.set_of(jump), Some(ActionMap::GAMEPLAY));
    }

    #[test]
    fn the_standard_controls_have_a_menu_set_where_the_mouse_points() {
        let map = ActionMap::standard();
        let menu = map.set("menu").unwrap();
        assert_eq!(map.sets().collect::<Vec<_>>(), [ActionMap::GAMEPLAY, menu]);
        assert_eq!(map.mouse_mode(menu), Some(MouseMode::Pointer));
        // Escape and Start each do one thing in each set.
        assert_eq!(map.conflicts(), Vec::new());
        for action in map.actions() {
            let in_menu = map.name(action).starts_with("menu_");
            let set = if in_menu { menu } else { ActionMap::GAMEPLAY };
            assert_eq!(map.set_of(action), Some(set), "{}", map.name(action));
        }
        let names: Vec<&str> = (map.actions())
            .filter(|action| map.set_of(*action) == Some(menu))
            .map(|action| map.name(action))
            .collect();
        assert_eq!(
            names,
            [
                "menu_move",
                "menu_confirm",
                "menu_click",
                "menu_back",
                "menu_previous_tab",
                "menu_next_tab",
                "menu_close",
                "menu_erase",
                "menu_space"
            ]
        );
    }

    #[test]
    fn actions_join_the_set_added_last() {
        let mut map = ActionMap::new();
        let pause = map.button("pause", ["Escape", "GamepadStart"]).unwrap();
        let menu = map.add_set("menu", MouseMode::Pointer).unwrap();
        let back = map.button("back", ["Escape", "GamepadEast"]).unwrap();

        assert_eq!(map.set_of(pause), Some(ActionMap::GAMEPLAY));
        assert_eq!(map.set_of(back), Some(menu));
        assert_eq!(map.set("menu"), Ok(menu));
        assert_eq!(map.set_name(menu), "menu");
        assert_eq!(map.mouse_mode(menu), Some(MouseMode::Pointer));
        assert_eq!(map.sets().len(), 2);
        // An action that fails to be added does not move the next one.
        assert!(map.button("back", ["Enter"]).is_err());
        let confirm = map.button("confirm", ["Enter", "GamepadSouth"]).unwrap();
        assert_eq!(map.set_of(confirm), Some(menu));
    }

    #[test]
    fn set_names_are_checked() {
        let mut map = ActionMap::new();
        assert_eq!(
            map.add_set(" ", MouseMode::Pointer),
            Err(MapError::EmptySetName)
        );
        map.add_set("menu", MouseMode::Pointer).unwrap();
        let twice = map.add_set("menu", MouseMode::Pointer).unwrap_err();
        assert!(
            twice.to_string().contains("already an action set"),
            "{twice}"
        );
        let builtin = map.add_set("gameplay", MouseMode::Look).unwrap_err();
        assert!(matches!(builtin, MapError::DuplicateSet { .. }));

        let typo = map.set("menus").unwrap_err();
        assert_eq!(typo.suggestion.as_deref(), Some("menu"));
        assert!(typo.to_string().contains("action set"), "{typo}");
    }

    #[test]
    fn an_action_name_is_taken_in_every_set() {
        let mut map = ActionMap::new();
        map.button("jump", ["Space", "GamepadSouth"]).unwrap();
        map.add_set("menu", MouseMode::Pointer).unwrap();
        let error = map.button("jump", ["Enter"]).unwrap_err();
        assert!(matches!(error, MapError::DuplicateAction { .. }));
    }

    #[test]
    fn sharing_a_key_across_sets_is_not_a_conflict() {
        let mut map = ActionMap::new();
        map.button("pause", ["Escape", "GamepadStart"]).unwrap();
        map.add_set("menu", MouseMode::Pointer).unwrap();
        map.button("back", ["Escape", "GamepadStart"]).unwrap();
        assert_eq!(map.conflicts(), []);

        map.button("close", ["Escape"]).unwrap();
        let conflicts = map.conflicts();
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        assert_eq!(
            (&*conflicts[0].first, &*conflicts[0].second),
            ("back", "close")
        );
    }

    #[test]
    fn an_own_button_is_in_gameplay_and_clashes_with_every_set() {
        let mut map = ActionMap::standard();
        let menu_key = map.own_button("weather menu", ["F1", "DpadUp"]).unwrap();
        assert!(map.is_own(menu_key));
        assert!(!map.is_own(map.action("jump").unwrap()));
        assert_eq!(map.set_of(menu_key), Some(ActionMap::GAMEPLAY));
        assert!(map.own_button("weather menu", ["F2"]).is_err());
        assert!(map.own_button("bad", ["Nonsense"]).is_err());

        // Up on the d-pad is `map` in gameplay and `menu_move` in the menu.
        let said: Vec<String> = map.conflicts().iter().map(ToString::to_string).collect();
        assert_eq!(map.conflicts().len(), 2, "{said:?}");
        assert!(
            said.iter()
                .any(|c| c.contains("weather menu") && c.contains("map"))
        );
        assert!(said.iter().any(|c| c.contains("menu_move")));
        // What the map made for itself is still the standard's own, and
        // later actions still join gameplay.
        assert!(!map.is_own(map.action("menu_click").unwrap()));
    }

    #[test]
    fn an_own_button_in_a_set_is_heard_only_there() {
        let mut map = ActionMap::standard();
        let riding = map.add_set("riding", MouseMode::Look).unwrap();
        let gallop = map
            .own_button_in("gallop", ["H", "LeftBumper"], riding)
            .unwrap();
        let wave = map.own_button("wave", ["F3", "DpadUp"]).unwrap();
        assert!(map.is_own(gallop));
        assert_eq!(map.set_of(gallop), Some(riding));
        assert!(map.is_heard_in(gallop, riding));
        assert!(!map.is_heard_in(gallop, ActionMap::GAMEPLAY));
        assert!(map.is_heard_in(wave, riding));
        assert!(map.is_heard_in(wave, ActionMap::GAMEPLAY));
        // Another set's actions are checked only against its own: H is free.
        assert!(
            map.conflicts()
                .iter()
                .all(|clash| clash.first != "gallop" && clash.second != "gallop")
        );
        assert!(map.own_button_in("gallop", ["J"], riding).is_err());
    }
}
