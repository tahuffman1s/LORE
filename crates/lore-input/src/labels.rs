//! What is printed on a key or button, for hints such as "A Select".
//!
//! The names in a bindings file say where a button is (`GamepadSouth`),
//! so one file works on every gamepad. A hint has to say what the player
//! sees on the button in their hand, which depends on who made it.

use crate::binding::{Binding, Button};
use crate::devices::{GamepadButton, Key, MouseButton, MouseWheel, Stick, StickAxis};
use crate::map::{ActionId, ActionMap, Device};

/// Whose letters are printed on a gamepad's buttons.
///
/// The button at the bottom is A on an Xbox gamepad, Cross on a
/// PlayStation one and B on a Nintendo one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GamepadStyle {
    /// Xbox, and every gamepad that is not known to be one of the others:
    /// most gamepads for computers copy these letters.
    #[default]
    Xbox,
    /// PlayStation: Cross, Circle, Square, Triangle, L1 and R1.
    PlayStation,
    /// Nintendo: A is on the right and B at the bottom.
    Nintendo,
}

impl GamepadButton {
    /// What is printed on this button of a gamepad of `style`, short
    /// enough to draw inside a hint: `"A"`, `"Cross"`, `"LB"`.
    ///
    /// ```
    /// use lore_input::{GamepadButton, GamepadStyle};
    ///
    /// let south = GamepadButton::GamepadSouth;
    /// assert_eq!(south.label(GamepadStyle::Xbox), "A");
    /// assert_eq!(south.label(GamepadStyle::PlayStation), "Cross");
    /// assert_eq!(south.label(GamepadStyle::Nintendo), "B");
    /// ```
    #[must_use]
    pub const fn label(self, style: GamepadStyle) -> &'static str {
        use GamepadStyle::{Nintendo, PlayStation, Xbox};
        match (self, style) {
            (Self::GamepadSouth, Xbox) | (Self::GamepadEast, Nintendo) => "A",
            (Self::GamepadEast, Xbox) | (Self::GamepadSouth, Nintendo) => "B",
            (Self::GamepadWest, Xbox) | (Self::GamepadNorth, Nintendo) => "X",
            (Self::GamepadNorth, Xbox) | (Self::GamepadWest, Nintendo) => "Y",
            (Self::GamepadSouth, PlayStation) => "Cross",
            (Self::GamepadEast, PlayStation) => "Circle",
            (Self::GamepadWest, PlayStation) => "Square",
            (Self::GamepadNorth, PlayStation) => "Triangle",
            (Self::LeftBumper, Xbox) => "LB",
            (Self::RightBumper, Xbox) => "RB",
            (Self::LeftTrigger, Xbox) => "LT",
            (Self::RightTrigger, Xbox) => "RT",
            (Self::LeftBumper, PlayStation) => "L1",
            (Self::RightBumper, PlayStation) => "R1",
            (Self::LeftTrigger, PlayStation) => "L2",
            (Self::RightTrigger, PlayStation) => "R2",
            (Self::LeftBumper, Nintendo) => "L",
            (Self::RightBumper, Nintendo) => "R",
            (Self::LeftTrigger, Nintendo) => "ZL",
            (Self::RightTrigger, Nintendo) => "ZR",
            (Self::LeftStickPress, PlayStation) => "L3",
            (Self::RightStickPress, PlayStation) => "R3",
            (Self::LeftStickPress, Xbox | Nintendo) => "LS",
            (Self::RightStickPress, Xbox | Nintendo) => "RS",
            (Self::DpadUp, _) => "D-pad up",
            (Self::DpadDown, _) => "D-pad down",
            (Self::DpadLeft, _) => "D-pad left",
            (Self::DpadRight, _) => "D-pad right",
            (Self::GamepadStart, Xbox) => "Menu",
            (Self::GamepadSelect, Xbox) => "View",
            (Self::GamepadStart, PlayStation) => "Options",
            (Self::GamepadSelect, PlayStation) => "Share",
            (Self::GamepadStart, Nintendo) => "+",
            (Self::GamepadSelect, Nintendo) => "-",
        }
    }
}

impl Key {
    /// What is printed on this key of a US keyboard: `"Enter"`, `"Esc"`,
    /// `"Q"`.
    ///
    /// A key is named by where it is, so on another layout the letter in
    /// that place may be a different one.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Digit0 => "0",
            Self::Digit1 => "1",
            Self::Digit2 => "2",
            Self::Digit3 => "3",
            Self::Digit4 => "4",
            Self::Digit5 => "5",
            Self::Digit6 => "6",
            Self::Digit7 => "7",
            Self::Digit8 => "8",
            Self::Digit9 => "9",
            Self::Escape => "Esc",
            Self::CapsLock => "Caps Lock",
            Self::LeftShift => "Shift",
            Self::RightShift => "Right Shift",
            Self::LeftControl => "Ctrl",
            Self::RightControl => "Right Ctrl",
            Self::LeftAlt => "Alt",
            Self::RightAlt => "Right Alt",
            Self::LeftSuper => "Super",
            Self::RightSuper => "Right Super",
            Self::ArrowUp => "Up",
            Self::ArrowDown => "Down",
            Self::ArrowLeft => "Left",
            Self::ArrowRight => "Right",
            Self::PageUp => "Page Up",
            Self::PageDown => "Page Down",
            Self::Backquote => "`",
            Self::Minus => "-",
            Self::Equal => "=",
            Self::LeftBracket => "[",
            Self::RightBracket => "]",
            Self::Backslash => "\\",
            Self::Semicolon => ";",
            Self::Quote => "'",
            Self::Comma => ",",
            Self::Period => ".",
            Self::Slash => "/",
            Self::Numpad0 => "Num 0",
            Self::Numpad1 => "Num 1",
            Self::Numpad2 => "Num 2",
            Self::Numpad3 => "Num 3",
            Self::Numpad4 => "Num 4",
            Self::Numpad5 => "Num 5",
            Self::Numpad6 => "Num 6",
            Self::Numpad7 => "Num 7",
            Self::Numpad8 => "Num 8",
            Self::Numpad9 => "Num 9",
            Self::NumpadAdd => "Num +",
            Self::NumpadSubtract => "Num -",
            Self::NumpadMultiply => "Num *",
            Self::NumpadDivide => "Num /",
            Self::NumpadDecimal => "Num .",
            Self::NumpadEnter => "Num Enter",
            // Letters, F1 to F12, Space, Enter, Tab and the rest are
            // printed as they are named.
            _ => self.name(),
        }
    }
}

impl Button {
    /// What is printed on this key or button, short enough to draw
    /// inside a hint. `style` says whose gamepad it is, and makes no
    /// difference to a key or a mouse button.
    ///
    /// ```
    /// use lore_input::{Button, GamepadButton, GamepadStyle, Key};
    ///
    /// let style = GamepadStyle::PlayStation;
    /// assert_eq!(Button::Key(Key::Escape).label(style), "Esc");
    /// assert_eq!(Button::Gamepad(GamepadButton::LeftBumper).label(style), "L1");
    /// ```
    #[must_use]
    pub const fn label(self, style: GamepadStyle) -> &'static str {
        match self {
            Self::Key(key) => key.label(),
            Self::Mouse(MouseButton::MouseLeft) => "Left click",
            Self::Mouse(MouseButton::MouseRight) => "Right click",
            Self::Mouse(MouseButton::MouseMiddle) => "Middle click",
            Self::Mouse(MouseButton::MouseBack) => "Mouse 4",
            Self::Mouse(MouseButton::MouseForward) => "Mouse 5",
            Self::Wheel(MouseWheel::MouseWheelUp) => "Wheel up",
            Self::Wheel(MouseWheel::MouseWheelDown) => "Wheel down",
            Self::Gamepad(button) => button.label(style),
        }
    }
}

impl Binding {
    /// What to show for this binding on a hint: the printed name of its
    /// button, or of each button of a pair or a set of four.
    #[must_use]
    pub fn label(self, style: GamepadStyle) -> String {
        let label = |button: Button| button.label(style);
        match self {
            Self::Button(button) => label(button).to_owned(),
            Self::Axis { negative, positive } => {
                format!("{} / {}", label(negative), label(positive))
            }
            Self::Directions {
                up: Button::Gamepad(GamepadButton::DpadUp),
                down: Button::Gamepad(GamepadButton::DpadDown),
                left: Button::Gamepad(GamepadButton::DpadLeft),
                right: Button::Gamepad(GamepadButton::DpadRight),
            } => "D-pad".to_owned(),
            Self::Directions {
                up,
                down,
                left,
                right,
            } => format!(
                "{} {} {} {}",
                label(up),
                label(left),
                label(down),
                label(right)
            ),
            Self::Stick(stick) => stick_label(stick).to_owned(),
            Self::StickAxis(stick, StickAxis::X) => format!("{} X", stick_label(stick)),
            Self::StickAxis(stick, StickAxis::Y) => format!("{} Y", stick_label(stick)),
            Self::MouseMotion => "Mouse".to_owned(),
        }
    }
}

/// How a thumbstick reads in a hint.
const fn stick_label(stick: Stick) -> &'static str {
    match stick {
        Stick::LeftStick => "Left stick",
        Stick::RightStick => "Right stick",
    }
}

impl ActionMap {
    /// What to show for `action` on the device the player uses, then on
    /// the other; empty when it is bound to nothing.
    #[must_use]
    pub fn label(&self, action: ActionId, device: Device, style: GamepadStyle) -> String {
        let wants_gamepad = device == Device::Gamepad;
        let bindings = self.bindings(action);
        for gamepad in [wants_gamepad, !wants_gamepad] {
            if let Some(binding) = bindings.iter().find(|b| b.is_gamepad() == gamepad) {
                return binding.label(style);
            }
        }
        String::new()
    }
}

impl ActionMap {
    /// What to show for `action` on one device only: every binding it has
    /// there, printed and joined with `", "`; empty when it has none there.
    #[must_use]
    pub fn label_on(&self, action: ActionId, gamepad: bool, style: GamepadStyle) -> String {
        let bindings = self.bindings(action);
        let shown: Vec<String> = bindings
            .iter()
            .filter(|binding| binding.is_gamepad() == gamepad)
            .map(|binding| binding.label(style))
            .collect();
        shown.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_is_printed_for_a_player() {
        let style = GamepadStyle::PlayStation;
        let label = |text: &str| text.parse::<Binding>().unwrap().label(style);
        assert_eq!(label("Dpad"), "D-pad");
        assert_eq!(label("LeftStick"), "Left stick");
        assert_eq!(label("RightStickX"), "Right stick X");
        assert_eq!(label("MouseMotion"), "Mouse");
        assert_eq!(label("Q/E"), "Q / E");
    }

    #[test]
    fn a_label_can_ask_for_one_device() {
        let mut map = ActionMap::new();
        let jump = map.button("jump", ["Space", "GamepadSouth"]).unwrap();
        let style = GamepadStyle::PlayStation;
        assert_eq!(map.label_on(jump, false, style), "Space");
        assert_eq!(map.label_on(jump, true, style), "Cross");
    }

    const STYLES: [GamepadStyle; 3] = [
        GamepadStyle::Xbox,
        GamepadStyle::PlayStation,
        GamepadStyle::Nintendo,
    ];

    #[test]
    fn no_two_buttons_of_one_gamepad_are_printed_alike() {
        for style in STYLES {
            let mut seen = std::collections::HashSet::new();
            for button in GamepadButton::ALL {
                let label = button.label(style);
                assert!(!label.is_empty());
                assert!(seen.insert(label), "{label} is on two buttons of {style:?}");
            }
        }
    }

    #[test]
    fn no_two_keys_are_printed_alike_and_all_are_short() {
        let mut seen = std::collections::HashSet::new();
        for key in Key::ALL {
            let label = key.label();
            assert!(!label.is_empty() && label.len() <= 11, "{label}");
            assert!(seen.insert(label), "{label} is on two keys");
        }
        assert_eq!(Key::Q.label(), "Q");
        assert_eq!(Key::Digit7.label(), "7");
        assert_eq!(Key::F5.label(), "F5");
        assert_eq!(Key::Enter.label(), "Enter");
    }

    #[test]
    fn nintendo_letters_are_where_nintendo_prints_them() {
        let style = GamepadStyle::Nintendo;
        assert_eq!(GamepadButton::GamepadEast.label(style), "A");
        assert_eq!(GamepadButton::GamepadSouth.label(style), "B");
        assert_eq!(GamepadButton::GamepadNorth.label(style), "X");
        assert_eq!(GamepadButton::GamepadWest.label(style), "Y");
    }
}
