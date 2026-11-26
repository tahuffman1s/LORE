//! Presses every action with no player there, for `lore check`.
//!
//! A script's `if game.input.justPressed("jump")` is only run when jump is
//! pressed, so a mistake inside it stays hidden while nothing is. This
//! presses, holds and lets go of everything each action is bound to, one
//! action at a time, through the same [`Input`] a window feeds. Then it
//! holds each action while pressing each other one, so that code which
//! needs two at once (`pressed("aim") and justPressed("fire")`, sprinting
//! while moving) is run too.

use lore_input::{
    ActionId, ActionMap, Binding, Button, Input, InputFrame, Stick, StickAxis as Axis,
};

/// Every way a stick is pushed, all the way round: up first, then clockwise.
/// Each is `[right, up]`.
const ROUND: [[f32; 2]; 8] = [
    [0.0, 1.0],
    [1.0, 1.0],
    [1.0, 0.0],
    [1.0, -1.0],
    [0.0, -1.0],
    [-1.0, -1.0],
    [-1.0, 0.0],
    [-1.0, 1.0],
];

/// The mouse movements tried, in counts: a nudge, then a fast flick the
/// other way.
const MOUSE: [[f32; 2]; 2] = [[3.0, 1.0], [-240.0, -90.0]];

/// Puts the pointer in the middle of a 1920 by 1080 screen, where a game
/// run with no window has none: so that code which only runs while
/// something is hovered runs in `lore check` too.
pub fn point_in_middle(input: &mut Input) {
    input.set_pointer(Some([960.0, 540.0]), [1920.0, 1080.0]);
}

/// What [`every_action`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pressed {
    /// How many actions were pressed.
    pub actions: usize,
    /// The actions with nothing bound, which could not be pressed.
    pub unbound: Vec<String>,
}

/// Presses, holds and lets go of every action in `input`'s map, one at a
/// time and in the map's order, and calls `tick` with the action and the
/// input of each tick that passes meanwhile.
///
/// Everything an action is bound to is used in turn, so it is pressed once
/// from the keyboard and mouse and once from the gamepad. A button is seen
/// going down, then held, then let go. A stick is pushed all the way round
/// through its eight directions, direction keys likewise, and the mouse is
/// moved a little and then a lot. Between two actions one tick passes with
/// nothing pressed. An action in another action set is pressed with its set
/// live; the set that was live before is live again afterwards.
///
/// Call it with every device at rest.
pub fn every_action(input: &mut Input, mut tick: impl FnMut(ActionId, &InputFrame)) -> Pressed {
    // Its own copy, so the map can be read while the input is driven.
    let map: ActionMap = input.map().clone();
    let live = input.active_set();
    let mut pressed = Pressed::default();
    for action in map.actions() {
        let bindings = map.bindings(action);
        if bindings.is_empty() {
            pressed.unbound.push(map.name(action).to_owned());
            continue;
        }
        if let Some(set) = map.set_of(action) {
            input.set_active_set(set);
        }
        let mut pass = |input: &mut Input| tick(action, input.tick());
        for binding in bindings {
            use_binding(input, *binding, &mut pass);
        }
        // Nothing is down: the action is seen at rest before the next one.
        pass(input);
        pressed.actions += 1;
    }
    input.set_active_set(live);
    pressed
}

/// One way to turn an action all the way on.
#[derive(Debug, Clone, Copy)]
enum On {
    Button(Button),
    Stick(Stick, [f32; 2]),
    /// The mouse, which is on only while it moves.
    Mouse,
}

impl On {
    /// How to turn on what is bound by `binding`.
    fn of(binding: Binding) -> Self {
        match binding {
            Binding::Button(button)
            | Binding::Axis {
                positive: button, ..
            }
            | Binding::Directions { up: button, .. } => Self::Button(button),
            Binding::StickAxis(stick, Axis::X) => Self::Stick(stick, [1.0, 0.0]),
            Binding::StickAxis(stick, Axis::Y) | Binding::Stick(stick) => {
                Self::Stick(stick, [0.0, 1.0])
            }
            Binding::MouseMotion => Self::Mouse,
        }
    }

    /// Whether this and `other` need the same key, button, stick or mouse,
    /// so that the two cannot be told apart when both are on.
    fn clashes(self, other: Self) -> bool {
        match (self, other) {
            (Self::Button(a), Self::Button(b)) => a == b,
            (Self::Stick(a, _), Self::Stick(b, _)) => a == b,
            (Self::Mouse, Self::Mouse) => true,
            _ => false,
        }
    }

    /// Turns it on or off.
    fn turn(self, input: &mut Input, on: bool) {
        match self {
            Self::Button(button) if on => input.press(button),
            Self::Button(button) => input.release(button),
            Self::Stick(stick, way) => push(input, stick, if on { way } else { [0.0, 0.0] }),
            Self::Mouse => {}
        }
    }

    /// What has to be done again before every tick it is on for.
    fn keep(self, input: &mut Input) {
        if matches!(self, Self::Mouse) {
            input.move_mouse(MOUSE[0][0], MOUSE[0][1]);
        }
    }
}

/// Holds every action in `input`'s map while pressing every other one, and
/// calls `tick` with the action held, the action pressed and the input of
/// each tick that passes meanwhile. Says how many pairs of actions were
/// tried.
///
/// Each pair is tried both ways round, since `pressed("aim") and
/// justPressed("fire")` is not `pressed("fire") and justPressed("aim")`:
/// the first goes down, the second goes down while it is held, both are
/// held, the second is let go and then the first. Two actions in different
/// action sets are never live together and are not paired, and neither are
/// two that can only be worked by the same key, button or stick.
///
/// Call it with every device at rest.
pub fn every_pair(
    input: &mut Input,
    mut tick: impl FnMut(ActionId, ActionId, &InputFrame),
) -> usize {
    let map: ActionMap = input.map().clone();
    let live = input.active_set();
    let actions: Vec<ActionId> = map
        .actions()
        .filter(|action| !map.bindings(*action).is_empty())
        .collect();
    let mut pairs = 0;
    for (nth, &held) in actions.iter().enumerate() {
        for (other, &pressed) in actions.iter().enumerate() {
            if nth == other || map.set_of(held) != map.set_of(pressed) {
                continue;
            }
            let first = On::of(map.bindings(held)[0]);
            let Some(second) = (map.bindings(pressed).iter())
                .map(|binding| On::of(*binding))
                .find(|on| !first.clashes(*on))
            else {
                continue;
            };
            if let Some(set) = map.set_of(held) {
                input.set_active_set(set);
            }
            let mut pass = |input: &mut Input, on: &[On]| {
                for one in on {
                    one.keep(input);
                }
                tick(held, pressed, input.tick());
            };
            first.turn(input, true);
            pass(input, &[first]);
            second.turn(input, true);
            pass(input, &[first, second]);
            pass(input, &[first, second]);
            second.turn(input, false);
            pass(input, &[first]);
            first.turn(input, false);
            pass(input, &[]);
            // Counted once, though it is tried both ways round.
            if nth < other {
                pairs += 1;
            }
        }
    }
    input.set_active_set(live);
    pairs
}

/// Works one binding through everything it can report and leaves it at rest.
/// `pass` lets one tick go by.
fn use_binding(input: &mut Input, binding: Binding, pass: &mut impl FnMut(&mut Input)) {
    match binding {
        Binding::Button(button) => {
            hold(input, &[button], pass);
            // The tick that sees it let go.
            pass(input);
        }
        Binding::Axis { negative, positive } => {
            hold(input, &[positive], pass);
            hold(input, &[negative], pass);
            pass(input);
        }
        Binding::StickAxis(stick, axis) => {
            for value in [1.0, -1.0] {
                input.set_stick(stick, axis, value);
                pass(input);
                pass(input);
            }
            input.set_stick(stick, axis, 0.0);
            pass(input);
        }
        Binding::Directions {
            up,
            down,
            left,
            right,
        } => {
            for [x, y] in ROUND {
                let across = if x > 0.0 { Some(right) } else { None };
                let across = if x < 0.0 { Some(left) } else { across };
                let along = if y > 0.0 { Some(up) } else { None };
                let along = if y < 0.0 { Some(down) } else { along };
                for button in [across, along].into_iter().flatten() {
                    input.press(button);
                }
                pass(input);
                for button in [across, along].into_iter().flatten() {
                    input.release(button);
                }
            }
            pass(input);
        }
        Binding::Stick(stick) => {
            for [x, y] in ROUND {
                push(input, stick, [x, y]);
                pass(input);
            }
            push(input, stick, [0.0, 0.0]);
            pass(input);
        }
        Binding::MouseMotion => {
            for [right, up] in MOUSE {
                input.move_mouse(right, up);
                pass(input);
            }
            // The tick that sees the mouse still again.
            pass(input);
        }
    }
}

/// Presses `buttons`, lets two ticks pass (one sees them go down, one sees
/// them held) and lets them go.
fn hold(input: &mut Input, buttons: &[Button], pass: &mut impl FnMut(&mut Input)) {
    for button in buttons {
        input.press(*button);
    }
    pass(input);
    pass(input);
    for button in buttons {
        input.release(*button);
    }
}

/// Pushes `stick` to `[right, up]`.
fn push(input: &mut Input, stick: Stick, [right, up]: [f32; 2]) {
    input.set_stick(stick, Axis::X, right);
    input.set_stick(stick, Axis::Y, up);
}

#[cfg(test)]
mod tests {
    use lore_input::{ActionState, MouseMode};

    use super::*;

    /// What `name` reported on each tick it was the action being pressed.
    fn seen(map: ActionMap, name: &str) -> Vec<ActionState> {
        let wanted = map.action(name).unwrap();
        let mut input = Input::new(map);
        let mut states = Vec::new();
        every_action(&mut input, |action, frame| {
            if action == wanted {
                states.push(frame.get(wanted));
            }
        });
        states
    }

    #[test]
    fn every_two_actions_are_held_together_both_ways_round() {
        let mut map = ActionMap::new();
        map.button("aim", ["MouseRight"]).unwrap();
        map.button("fire", ["MouseLeft"]).unwrap();
        map.button("jump", ["Space"]).unwrap();
        let (aim, fire) = (map.action("aim").unwrap(), map.action("fire").unwrap());
        let mut input = Input::new(map);
        // What a script's two-button lines would have seen.
        let (mut aimed_then_fired, mut fired_then_aimed, mut both) = (0, 0, 0);
        let pairs = every_pair(&mut input, |_, _, frame| {
            let (aim, fire) = (frame.get(aim), frame.get(fire));
            aimed_then_fired += usize::from(aim.pressed && fire.just_pressed);
            fired_then_aimed += usize::from(fire.pressed && aim.just_pressed);
            both += usize::from(aim.pressed && fire.pressed);
        });
        assert_eq!(pairs, 3);
        assert_eq!((aimed_then_fired, fired_then_aimed), (1, 1));
        assert_eq!(both, 4);
        // Everything is at rest afterwards.
        let frame = input.tick();
        assert_eq!(frame.get(aim), ActionState::default());
        assert_eq!(frame.get(fire), ActionState::default());
    }

    #[test]
    fn two_actions_on_the_same_key_or_in_different_sets_are_not_paired() {
        let mut map = ActionMap::new();
        map.button("use", ["E"]).unwrap();
        map.button("open", ["E"]).unwrap();
        let mut input = Input::new(map);
        assert_eq!(every_pair(&mut input, |_, _, _| {}), 0);

        let mut map = ActionMap::new();
        map.button("jump", ["Space"]).unwrap();
        map.add_set("menu", MouseMode::Pointer).unwrap();
        map.button("accept", ["Enter"]).unwrap();
        let mut input = Input::new(map);
        assert_eq!(every_pair(&mut input, |_, _, _| {}), 0);
    }

    #[test]
    fn a_button_goes_down_is_held_and_comes_up_on_each_device() {
        let mut map = ActionMap::new();
        map.button("jump", ["Space", "GamepadSouth"]).unwrap();
        let states = seen(map, "jump");

        let downs = states.iter().filter(|state| state.just_pressed).count();
        let ups = states.iter().filter(|state| state.just_released).count();
        let held = (states.iter())
            .filter(|state| state.pressed && !state.just_pressed)
            .count();
        assert_eq!((downs, held, ups), (2, 2, 2), "{states:?}");
        assert_eq!(states.last(), Some(&ActionState::default()));
    }

    #[test]
    fn a_wheel_notch_and_a_trigger_are_pressed_too() {
        let mut map = ActionMap::new();
        map.button("next", ["MouseWheelDown"]).unwrap();
        map.button("fire", ["RightTrigger"]).unwrap();
        for name in ["next", "fire"] {
            let states = seen(map.clone(), name);
            assert!(states.iter().any(|state| state.just_pressed), "{name}");
            assert!(states.iter().any(|state| state.just_released), "{name}");
        }
    }

    #[test]
    fn a_one_way_axis_goes_both_ways_from_keys_and_from_a_stick() {
        let mut map = ActionMap::new();
        map.axis1d("lean", ["Q/E", "RightStickX"]).unwrap();
        let states = seen(map, "lean");

        let right = states.iter().filter(|state| state.axis[0] > 0.9).count();
        let left = states.iter().filter(|state| state.axis[0] < -0.9).count();
        assert_eq!((right, left), (4, 4), "{states:?}");
        assert_eq!(states.last(), Some(&ActionState::default()));
    }

    #[test]
    fn a_stick_and_direction_keys_go_all_the_way_round() {
        for binding in ["WASD", "LeftStick"] {
            let mut map = ActionMap::new();
            map.axis2d("move", [binding]).unwrap();
            let states = seen(map, "move");

            // Each of the eight directions, as the signs of x and y.
            #[allow(clippy::cast_possible_truncation)]
            let mut ways: Vec<[i8; 2]> = (states.iter())
                .filter(|state| state.pressed)
                .map(|state| state.axis.map(|part| (part * 1.3).round() as i8))
                .collect();
            assert_eq!(ways.len(), 8, "{binding}: {states:?}");
            ways.sort_unstable();
            ways.dedup();
            assert_eq!(ways.len(), 8, "{binding}: {states:?}");
            assert_eq!(states.last(), Some(&ActionState::default()));
        }
    }

    #[test]
    fn the_mouse_is_moved_and_then_still() {
        let mut map = ActionMap::new();
        map.axis2d("look", ["MouseMotion"]).unwrap();
        let states = seen(map, "look");

        let moves: Vec<[f32; 2]> = states.iter().map(|state| state.delta).collect();
        assert_eq!(moves[..2], MOUSE, "{states:?}");
        assert_eq!(moves.last(), Some(&[0.0, 0.0]));
    }

    #[test]
    fn actions_are_pressed_one_at_a_time_in_the_maps_order() {
        let map = ActionMap::standard();
        let names: Vec<String> = (map.actions())
            .map(|action| map.name(action).to_owned())
            .collect();
        let mut input = Input::new(map.clone());
        let mut order: Vec<String> = Vec::new();
        let pressed = every_action(&mut input, |action, frame| {
            // But for the two pairs the standard controls put on one
            // button on purpose.
            let shares = |a: &str, b: &str| {
                [("aim", "block"), ("crouch", "dodge")]
                    .iter()
                    .any(|pair| *pair == (a, b) || *pair == (b, a))
            };
            for other in map.actions().filter(|other| *other != action) {
                if shares(map.name(other), map.name(action)) {
                    continue;
                }
                assert!(
                    !frame.pressed(other),
                    "{} while pressing {}",
                    map.name(other),
                    map.name(action)
                );
            }
            if order.last().map(String::as_str) != Some(map.name(action)) {
                order.push(map.name(action).to_owned());
            }
        });

        assert_eq!(order, names);
        assert_eq!(pressed.actions, names.len());
        assert_eq!(pressed.unbound, Vec::<String>::new());
        // Everything is at rest afterwards.
        let frame = input.tick();
        assert!(map.actions().all(|action| !frame.pressed(action)));
    }

    #[test]
    fn an_own_action_of_a_set_is_pressed_with_its_set_live() {
        let mut map = ActionMap::standard();
        let riding = map.add_set("riding", MouseMode::Look).unwrap();
        let gallop = map.own_button_in("gallop", ["H"], riding).unwrap();
        let mut input = Input::new(map);
        let mut heard = false;
        every_action(&mut input, |action, frame| {
            heard |= action == gallop && frame.pressed(gallop);
        });
        assert!(heard, "gallop was never heard");
        assert_eq!(input.active_set(), ActionMap::GAMEPLAY);
    }

    #[test]
    fn an_action_in_another_set_is_pressed_and_the_live_set_is_put_back() {
        let mut map = ActionMap::new();
        map.button("pause", ["Escape"]).unwrap();
        map.add_set("menu", MouseMode::Pointer).unwrap();
        map.button("resume", ["Escape"]).unwrap();
        let states = seen(map.clone(), "resume");
        assert!(states.iter().any(|state| state.just_pressed), "{states:?}");

        let mut input = Input::new(map);
        every_action(&mut input, |_, _| {});
        assert_eq!(input.active_set(), ActionMap::GAMEPLAY);
    }

    #[test]
    fn an_action_with_nothing_bound_is_named_and_not_counted() {
        let mut map = ActionMap::new();
        let jump = map.button("jump", ["Space"]).unwrap();
        map.button("fire", ["MouseLeft"]).unwrap();
        map.set_bindings(jump, Vec::<Binding>::new()).unwrap();
        let mut input = Input::new(map);

        let pressed = every_action(&mut input, |_, _| {});
        assert_eq!(pressed.actions, 1);
        assert_eq!(pressed.unbound, ["jump"]);
    }
}
