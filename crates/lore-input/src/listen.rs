//! Hearing the next button the player presses, for a script's controls page.

use crate::binding::Button;

/// Which device [`Input::set_listening`](crate::Input::set_listening) hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hears {
    /// Any key, mouse button, wheel notch or gamepad button.
    #[default]
    Any,
    /// The keyboard and mouse only.
    Keyboard,
    /// Gamepad buttons only.
    Gamepad,
}

/// What listening heard: carried by the tick's
/// [`InputFrame::heard`](crate::InputFrame::heard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    /// The player pressed this button. Escape and the gamepad's Start are
    /// never heard as buttons.
    Button(Button),
    /// The player pressed Escape (keyboard) or Start (gamepad) to give up.
    Cancelled,
}
