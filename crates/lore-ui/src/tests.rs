use crate::input::{REPEAT_DELAY, REPEAT_EVERY, Repeater, held_direction};
use crate::{
    Anchor, Color, Direction, Draw, InputMode, SMALLEST_TEXT, Theme, Ui, UiError, UiEvent, UiInput,
    Value, WidgetId,
};

const HD: [u32; 2] = [1920, 1080];
const NOTHING: UiInput = UiInput {
    direction: [0.0, 0.0],
    confirm: false,
    back: false,
    previous_tab: false,
    next_tab: false,
    pointer: None,
    pointer_down: false,
    wheel: 0.0,
};
const DOWN: UiInput = UiInput {
    direction: [0.0, -1.0],
    ..NOTHING
};
const UP: UiInput = UiInput {
    direction: [0.0, 1.0],
    ..NOTHING
};
const LEFT: UiInput = UiInput {
    direction: [-1.0, 0.0],
    ..NOTHING
};
const RIGHT: UiInput = UiInput {
    direction: [1.0, 0.0],
    ..NOTHING
};
const CONFIRM: UiInput = UiInput {
    confirm: true,
    ..NOTHING
};
const BACK: UiInput = UiInput {
    back: true,
    ..NOTHING
};
const NEXT_TAB: UiInput = UiInput {
    next_tab: true,
    ..NOTHING
};
const PREVIOUS_TAB: UiInput = UiInput {
    previous_tab: true,
    ..NOTHING
};

/// One frame with `input` held, then one with it let go.
fn tap(ui: &mut Ui, input: UiInput) -> Vec<UiEvent> {
    let events = ui.update(&input, 1.0 / 60.0, HD).to_vec();
    ui.update(&NOTHING, 1.0 / 60.0, HD);
    events
}

fn idle(ui: &mut Ui) {
    ui.update(&NOTHING, 1.0 / 60.0, HD);
}

/// A screen of buttons with these names, opened and laid out.
fn buttons(names: &[&str]) -> (Ui, Vec<WidgetId>) {
    let mut ui = Ui::default();
    let mut ids = Vec::new();
    let screen = ui
        .add_screen("Menu", |screen| {
            ids.extend(names.iter().map(|name| screen.button(name)))
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    (ui, ids)
}

fn rects(ui: &Ui) -> Vec<([f32; 2], [f32; 2], Color)> {
    ui.draw()
        .iter()
        .filter_map(|draw| match draw {
            Draw::Rect {
                position,
                size,
                color,
                ..
            } => Some((position, size, color)),
            Draw::Text { .. } => None,
        })
        .collect()
}

fn texts(ui: &Ui) -> Vec<String> {
    ui.draw()
        .iter()
        .filter_map(|draw| match draw {
            Draw::Text { text, .. } => Some(text.to_owned()),
            Draw::Rect { .. } => None,
        })
        .collect()
}

/// Where the text `wanted` is drawn.
fn find(ui: &Ui, wanted: &str) -> [f32; 2] {
    ui.draw()
        .iter()
        .find_map(|draw| match draw {
            Draw::Text { position, text, .. } if text == wanted => Some(position),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text \"{wanted}\" is drawn"))
}

fn point_at(ui: &Ui, text: &str) -> UiInput {
    let [x, y] = find(ui, text);
    UiInput {
        pointer: Some([x + 2.0, y + 2.0]),
        ..NOTHING
    }
}

fn has_outline(ui: &Ui) -> bool {
    let accent = Theme::default().accent;
    rects(ui).iter().any(|&(_, _, color)| color == accent)
}

// ---- HUD -------------------------------------------------------------------

#[test]
fn hud_pieces_are_pinned_inside_the_safe_area() {
    let anchors = [
        (Anchor::TopLeft, [96.0, 54.0]),
        (Anchor::Top, [910.0, 54.0]),
        (Anchor::TopRight, [1724.0, 54.0]),
        (Anchor::Left, [96.0, 515.0]),
        (Anchor::Center, [910.0, 515.0]),
        (Anchor::Right, [1724.0, 515.0]),
        (Anchor::BottomLeft, [96.0, 976.0]),
        (Anchor::Bottom, [910.0, 976.0]),
        (Anchor::BottomRight, [1724.0, 976.0]),
    ];
    for (anchor, expected) in anchors {
        let mut ui = Ui::default();
        ui.hud_rect(anchor, [0.0, 0.0], [100.0, 50.0], Color::WHITE);
        idle(&mut ui);
        let (position, size, _) = rects(&ui)[0];
        assert_eq!(size, [100.0, 50.0]);
        assert!(
            (position[0] - expected[0]).abs() < 0.01,
            "{anchor:?} x {position:?}"
        );
        assert!(
            (position[1] - expected[1]).abs() < 0.01,
            "{anchor:?} y {position:?}"
        );
    }
}

#[test]
fn offsets_move_right_and_down() {
    let mut ui = Ui::default();
    ui.hud_rect(Anchor::TopLeft, [10.0, 20.0], [4.0, 4.0], Color::WHITE);
    idle(&mut ui);
    assert_eq!(rects(&ui)[0].0, [106.0, 74.0]);
}

#[test]
fn everything_scales_with_screen_height_and_the_player_setting() {
    let mut ui = Ui::default();
    ui.hud_rect(Anchor::TopLeft, [0.0, 0.0], [100.0, 50.0], Color::WHITE);
    ui.update(&NOTHING, 0.016, [3840, 2160]);
    assert_eq!(rects(&ui)[0].1, [200.0, 100.0]);

    ui.set_ui_scale(9.0);
    ui.update(&NOTHING, 0.016, HD);
    assert_eq!(rects(&ui)[0].1, [150.0, 75.0]);

    ui.set_ui_scale(0.1);
    ui.update(&NOTHING, 0.016, HD);
    assert_eq!(rects(&ui)[0].1, [75.0, 37.5]);

    // A minimised window has no height; nothing divides by zero.
    ui.update(&NOTHING, 0.016, [0, 0]);
    assert!(
        rects(&ui)
            .iter()
            .all(|(position, ..)| position[0].is_finite())
    );
}

#[test]
fn text_is_never_smaller_than_the_smallest_size() {
    let mut ui = Ui::default();
    ui.hud_text(Anchor::Center, [0.0, 0.0], 8.0, "tiny");
    idle(&mut ui);
    let Some(Draw::Text { size, .. }) = ui.draw().iter().next() else {
        panic!("no text")
    };
    assert_eq!(size, SMALLEST_TEXT);
}

#[test]
fn hud_values_show_up_in_the_draw_list() {
    let mut ui = Ui::default();
    let ammo = ui.hud_text(Anchor::BottomRight, [0.0, 0.0], 32.0, "30 / 90");
    let health = ui.hud_bar(
        Anchor::BottomLeft,
        [0.0, 0.0],
        [300.0, 20.0],
        Color::WHITE,
        1.0,
    );
    ui.set_text(ammo, "7 / 90").unwrap();
    ui.set_bar(health, 0.25).unwrap();
    idle(&mut ui);
    assert_eq!(texts(&ui), ["7 / 90"]);
    assert_eq!(rects(&ui)[1].1, [75.0, 20.0]);

    ui.set_bar(health, f32::NAN).unwrap();
    ui.set_color(health, Color::BLACK).unwrap();
    idle(&mut ui);
    assert_eq!(rects(&ui).len(), 1, "an empty bar draws only its track");
}

#[test]
fn a_hidden_hud_piece_is_not_drawn_and_comes_back_as_it_was() {
    let mut ui = Ui::default();
    let ammo = ui.hud_text(Anchor::BottomRight, [0.0, 0.0], 32.0, "30 / 90");
    let health = ui.hud_bar(
        Anchor::BottomLeft,
        [0.0, 0.0],
        [300.0, 20.0],
        Color::rgb(1.0, 0.0, 0.0),
        0.5,
    );
    idle(&mut ui);
    let (boxes, words) = (rects(&ui), texts(&ui));
    assert_eq!((boxes.len(), words.len()), (2, 1));

    ui.set_shown(ammo, false).unwrap();
    ui.set_shown(health, false).unwrap();
    // A hidden piece still takes new values.
    ui.set_text(ammo, "29 / 90").unwrap();
    idle(&mut ui);
    assert_eq!((rects(&ui).len(), texts(&ui).len()), (0, 0));

    ui.set_shown(ammo, true).unwrap();
    ui.set_shown(health, true).unwrap();
    idle(&mut ui);
    assert_eq!(rects(&ui), boxes);
    assert_eq!(texts(&ui), ["29 / 90"]);

    // Only HUD pieces are hidden this way.
    let (mut menu, ids) = buttons(&["Resume"]);
    assert!(menu.set_shown(ids[0], false).is_err());
    assert!(menu.set_offset(ids[0], [1.0, 1.0]).is_err());
}

#[test]
fn a_hud_piece_can_be_moved_after_it_is_made() {
    let mut ui = Ui::default();
    let dot = ui.hud_rect(Anchor::TopLeft, [0.0, 0.0], [10.0, 10.0], Color::WHITE);
    idle(&mut ui);
    let before = rects(&ui)[0].0;
    ui.set_offset(dot, [30.0, -5.0]).unwrap();
    idle(&mut ui);
    assert_eq!(rects(&ui)[0].0, [before[0] + 30.0, before[1] - 5.0]);
}

// ---- Moving between widgets ------------------------------------------------

#[test]
fn a_screen_opens_on_its_first_widget_and_down_moves_on() {
    let (mut ui, ids) = buttons(&["Resume", "Settings", "Quit"]);
    assert_eq!(ui.focused(), Some(ids[0]));
    tap(&mut ui, DOWN);
    assert_eq!(ui.focused(), Some(ids[1]));
    tap(&mut ui, UP);
    assert_eq!(ui.focused(), Some(ids[0]));
}

#[test]
fn moving_past_the_end_wraps_round() {
    let (mut ui, ids) = buttons(&["Resume", "Settings", "Quit"]);
    tap(&mut ui, UP);
    assert_eq!(ui.focused(), Some(ids[2]));
    tap(&mut ui, DOWN);
    assert_eq!(ui.focused(), Some(ids[0]));
    // Nothing is beside a column, so sideways stays put.
    tap(&mut ui, LEFT);
    assert_eq!(ui.focused(), Some(ids[0]));
}

#[test]
fn wrapping_can_be_turned_off() {
    let mut ui = Ui::default();
    let mut first = None;
    let screen = ui
        .add_screen("Menu", |screen| {
            first = Some(screen.button("One"));
            screen.button("Two");
            screen.no_wrap();
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    tap(&mut ui, UP);
    assert_eq!(ui.focused(), first);
}

#[test]
fn a_grid_moves_along_rows_and_columns() {
    let mut ui = Ui::default();
    let mut ids = Vec::new();
    let screen = ui
        .add_screen("Loadout", |screen| {
            screen.grid(3, |screen| {
                for name in ["A", "B", "C", "D", "E", "F"] {
                    ids.push(screen.button(name));
                }
            });
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    tap(&mut ui, RIGHT);
    assert_eq!(ui.focused(), Some(ids[1]));
    tap(&mut ui, DOWN);
    assert_eq!(ui.focused(), Some(ids[4]));
    tap(&mut ui, RIGHT);
    assert_eq!(ui.focused(), Some(ids[5]));
    tap(&mut ui, RIGHT);
    assert_eq!(
        ui.focused(),
        Some(ids[3]),
        "right from the end of a row wraps along the row"
    );
    tap(&mut ui, UP);
    assert_eq!(ui.focused(), Some(ids[0]));
}

#[test]
fn equally_good_widgets_go_to_the_one_made_first() {
    let mut ui = Ui::default();
    let (mut left, mut wide) = (None, None);
    let screen = ui
        .add_screen("", |screen| {
            screen.row(|screen| {
                left = Some(screen.button("Left"));
                screen.button("Right");
            });
            wide = Some(screen.button("Wide"));
            screen.start_on(wide.unwrap());
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    assert_eq!(ui.focused(), wide);
    tap(&mut ui, UP);
    assert_eq!(ui.focused(), left);
}

#[test]
fn a_chosen_neighbor_beats_the_automatic_one() {
    let mut ui = Ui::default();
    let mut ids = Vec::new();
    let screen = ui
        .add_screen("", |screen| {
            ids = vec![
                screen.button("One"),
                screen.button("Two"),
                screen.button("Three"),
            ];
            screen.neighbor(ids[0], Direction::Down, ids[2]);
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    tap(&mut ui, DOWN);
    assert_eq!(ui.focused(), Some(ids[2]));
}

#[test]
fn labels_are_skipped() {
    let mut ui = Ui::default();
    let mut ids = Vec::new();
    let screen = ui
        .add_screen("Title", |screen| {
            ids.push(screen.button("One"));
            screen.label("Just words");
            ids.push(screen.button("Two"));
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    tap(&mut ui, DOWN);
    assert_eq!(ui.focused(), Some(ids[1]));
}

// ---- Holding a direction ---------------------------------------------------

#[test]
fn a_held_direction_steps_once_then_repeats_after_a_delay() {
    let mut repeater = Repeater::default();
    let held = Some(Direction::Down);
    let dt = 0.01;
    let steps_by = |seconds: f32, repeater: &mut Repeater, elapsed: &mut f32| {
        let mut steps = 0;
        while *elapsed < seconds {
            steps += u32::from(repeater.step(held, dt).is_some());
            *elapsed += dt;
        }
        steps
    };
    let mut elapsed = 0.0;
    assert_eq!(
        steps_by(REPEAT_DELAY - 0.05, &mut repeater, &mut elapsed),
        1
    );
    assert_eq!(
        steps_by(REPEAT_DELAY + 0.05, &mut repeater, &mut elapsed),
        1
    );
    assert_eq!(
        steps_by(
            REPEAT_DELAY + 0.05 + 5.0 * REPEAT_EVERY,
            &mut repeater,
            &mut elapsed
        ),
        5
    );

    // Letting go and pressing again steps at once.
    assert_eq!(repeater.step(None, dt), None);
    assert_eq!(repeater.step(held, dt), held);
    // A frame that took a whole second still moves one widget, not ten.
    assert_eq!(repeater.step(held, 1.0), held);
    assert_eq!(repeater.step(held, 0.01), None);
}

#[test]
fn a_stick_counts_past_halfway_and_the_stronger_axis_wins() {
    assert_eq!(held_direction([0.0, -0.49]), None);
    assert_eq!(held_direction([0.0, -0.51]), Some(Direction::Down));
    assert_eq!(held_direction([0.0, 0.51]), Some(Direction::Up));
    assert_eq!(held_direction([-0.9, 0.6]), Some(Direction::Left));
    assert_eq!(held_direction([0.9, 0.6]), Some(Direction::Right));
    assert_eq!(held_direction([f32::NAN, f32::NAN]), None);
}

#[test]
fn holding_down_walks_the_list() {
    let (mut ui, ids) = buttons(&["One", "Two", "Three", "Four"]);
    for _ in 0..33 {
        // 0.55 seconds: the first step, then repeats at 0.4 and 0.5.
        ui.update(&DOWN, 1.0 / 60.0, HD);
    }
    assert_eq!(ui.focused(), Some(ids[3]));
}

// ---- Widgets ---------------------------------------------------------------

struct Settings {
    ui: Ui,
    toggle: WidgetId,
    slider: WidgetId,
    choice: WidgetId,
}

fn settings() -> Settings {
    let mut ui = Ui::default();
    let mut ids = Vec::new();
    let screen = ui
        .add_screen("Settings", |screen| {
            ids.push(screen.toggle("Invert look", false));
            ids.push(screen.slider("Volume", 0.0, 100.0, 10.0, 90.0));
            ids.push(screen.choice("Quality", &["Low", "Medium", "High"], 1));
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    Settings {
        ui,
        toggle: ids[0],
        slider: ids[1],
        choice: ids[2],
    }
}

#[test]
fn confirm_presses_buttons_and_flips_toggles() {
    let (mut ui, ids) = buttons(&["Play"]);
    assert_eq!(tap(&mut ui, CONFIRM), [UiEvent::Pressed(ids[0])]);

    let Settings { mut ui, toggle, .. } = settings();
    assert_eq!(
        tap(&mut ui, CONFIRM),
        [UiEvent::Changed(toggle, Value::Toggle(true))]
    );
    assert_eq!(ui.value(toggle), Some(Value::Toggle(true)));
    assert!(texts(&ui).contains(&"On".to_owned()));
}

#[test]
fn holding_confirm_presses_once() {
    let (mut ui, _) = buttons(&["Play"]);
    assert_eq!(ui.update(&CONFIRM, 0.016, HD).len(), 1);
    assert_eq!(ui.update(&CONFIRM, 0.016, HD).len(), 0);
}

#[test]
fn left_and_right_change_a_slider_and_stop_at_its_ends() {
    let Settings { mut ui, slider, .. } = settings();
    tap(&mut ui, DOWN);
    assert_eq!(
        tap(&mut ui, RIGHT),
        [UiEvent::Changed(slider, Value::Slider(100.0))]
    );
    assert_eq!(tap(&mut ui, RIGHT), []);
    assert_eq!(
        tap(&mut ui, LEFT),
        [UiEvent::Changed(slider, Value::Slider(90.0))]
    );
    assert_eq!(
        ui.focused(),
        Some(slider),
        "left and right do not leave a slider"
    );
    assert!(texts(&ui).contains(&"90".to_owned()));
}

#[test]
fn left_and_right_cycle_a_choice() {
    let Settings { mut ui, choice, .. } = settings();
    tap(&mut ui, UP);
    assert_eq!(ui.focused(), Some(choice));
    assert_eq!(
        tap(&mut ui, RIGHT),
        [UiEvent::Changed(choice, Value::Choice(2))]
    );
    assert_eq!(
        tap(&mut ui, RIGHT),
        [UiEvent::Changed(choice, Value::Choice(0))]
    );
    assert_eq!(
        tap(&mut ui, LEFT),
        [UiEvent::Changed(choice, Value::Choice(2))]
    );
    assert_eq!(
        tap(&mut ui, CONFIRM),
        [UiEvent::Changed(choice, Value::Choice(0))]
    );
    assert!(texts(&ui).contains(&"Low".to_owned()));
}

#[test]
fn setters_change_values_without_events() {
    let Settings {
        mut ui,
        toggle,
        slider,
        choice,
    } = settings();
    ui.set_toggle(toggle, true).unwrap();
    ui.set_slider(slider, 33.0).unwrap();
    ui.set_choice(choice, 99).unwrap();
    assert_eq!(ui.update(&NOTHING, 0.016, HD), []);
    assert_eq!(ui.value(toggle), Some(Value::Toggle(true)));
    assert_eq!(ui.value(slider), Some(Value::Slider(30.0)));
    assert_eq!(ui.value(choice), Some(Value::Choice(2)));
}

fn tabbed() -> (Ui, WidgetId, [WidgetId; 2]) {
    let mut ui = Ui::default();
    let mut pages = Vec::new();
    let mut tabs = None;
    let screen = ui
        .add_screen("Settings", |screen| {
            tabs = Some(screen.tabs(&["Game", "Sound"], |screen, tab| {
                pages.push(if tab == 0 {
                    screen.toggle("Hints", true)
                } else {
                    screen.slider("Volume", 0.0, 1.0, 0.05, 0.5)
                });
            }));
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    (ui, tabs.unwrap(), [pages[0], pages[1]])
}

#[test]
fn shoulder_buttons_switch_tabs_and_only_the_open_page_shows() {
    let (mut ui, tabs, [hints, volume]) = tabbed();
    assert_eq!(ui.focused(), Some(hints));
    assert!(!texts(&ui).contains(&"Volume".to_owned()));

    assert_eq!(
        tap(&mut ui, NEXT_TAB),
        [UiEvent::Changed(tabs, Value::Tab(1))]
    );
    assert_eq!(ui.focused(), Some(volume));
    let words = texts(&ui);
    assert!(words.contains(&"Volume".to_owned()) && !words.contains(&"Hints".to_owned()));
    assert!(words.contains(&"0.50".to_owned()), "{words:?}");

    assert_eq!(
        tap(&mut ui, NEXT_TAB),
        [UiEvent::Changed(tabs, Value::Tab(0))]
    );
    assert_eq!(
        tap(&mut ui, PREVIOUS_TAB),
        [UiEvent::Changed(tabs, Value::Tab(1))]
    );
    // Up and down never reach a widget on a hidden page.
    tap(&mut ui, DOWN);
    assert_eq!(ui.focused(), Some(volume));
}

#[test]
fn clicking_a_tab_opens_it() {
    let (mut ui, tabs, [_, volume]) = tabbed();
    let over = point_at(&ui, "Sound");
    ui.update(&over, 0.016, HD);
    let events = ui
        .update(
            &UiInput {
                pointer_down: true,
                ..over
            },
            0.016,
            HD,
        )
        .to_vec();
    assert_eq!(events, [UiEvent::Changed(tabs, Value::Tab(1))]);
    assert_eq!(ui.focused(), Some(volume));
}

// ---- Screens on top of screens --------------------------------------------

#[test]
fn back_closes_the_top_screen_and_focus_is_remembered() {
    let mut ui = Ui::default();
    let mut settings_button = None;
    let pause = ui
        .add_screen("Paused", |screen| {
            screen.button("Resume");
            settings_button = Some(screen.button("Settings"));
        })
        .unwrap();
    let settings = ui
        .add_screen("Settings", |screen| _ = screen.toggle("Hints", true))
        .unwrap();

    ui.open(pause).unwrap();
    idle(&mut ui);
    tap(&mut ui, DOWN);
    ui.open(settings).unwrap();
    idle(&mut ui);
    assert_eq!(ui.open_screen(), Some(settings));

    assert_eq!(tap(&mut ui, BACK), [UiEvent::Back(settings)]);
    assert_eq!(ui.open_screen(), Some(pause));
    assert_eq!(ui.focused(), settings_button);

    assert_eq!(tap(&mut ui, BACK), [UiEvent::Back(pause)]);
    assert!(!ui.is_menu_open());
    assert_eq!(tap(&mut ui, BACK), []);

    // Closed and opened again, the screen is where the player left it.
    ui.open(pause).unwrap();
    idle(&mut ui);
    assert_eq!(ui.focused(), settings_button);
}

#[test]
fn a_screen_that_stays_open_reports_back_but_does_not_close() {
    let mut ui = Ui::default();
    let main = ui
        .add_screen("Main menu", |screen| {
            screen.button("Play");
            screen.stays_open();
        })
        .unwrap();
    ui.open(main).unwrap();
    assert_eq!(tap(&mut ui, BACK), [UiEvent::Back(main)]);
    assert_eq!(ui.open_screen(), Some(main));
}

#[test]
fn a_button_held_since_before_the_menu_opened_does_nothing() {
    let mut ui = Ui::default();
    let pause = ui
        .add_screen("Paused", |screen| _ = screen.button("Resume"))
        .unwrap();
    // Escape both pauses the game and means "back".
    let held = UiInput {
        back: true,
        confirm: true,
        ..NOTHING
    };
    ui.update(&held, 0.016, HD);
    ui.open(pause).unwrap();
    assert_eq!(ui.update(&held, 0.016, HD), []);
    assert!(ui.is_menu_open());
}

#[test]
fn opening_a_screen_that_is_underneath_goes_back_to_it() {
    let mut ui = Ui::default();
    let one = ui
        .add_screen("One", |screen| _ = screen.button("A"))
        .unwrap();
    let two = ui
        .add_screen("Two", |screen| _ = screen.button("B"))
        .unwrap();
    ui.open(one).unwrap();
    ui.open(two).unwrap();
    ui.open(one).unwrap();
    assert_eq!(ui.open_screen(), Some(one));
    assert!(ui.close());
    assert!(!ui.close());
}

#[test]
fn an_open_menu_dims_the_game_and_only_the_top_screen_is_drawn() {
    let mut ui = Ui::default();
    let one = ui
        .add_screen("One", |screen| _ = screen.button("A"))
        .unwrap();
    let two = ui
        .add_screen("Two", |screen| _ = screen.button("B"))
        .unwrap();
    ui.open(one).unwrap();
    ui.open(two).unwrap();
    idle(&mut ui);
    assert_eq!(texts(&ui), ["Two", "B"]);
    let (position, size, color) = rects(&ui)[0];
    assert_eq!(
        (position, size, color),
        ([0.0, 0.0], [1920.0, 1080.0], Theme::default().backdrop)
    );
}

// ---- The mouse -------------------------------------------------------------

#[test]
fn the_mouse_hides_the_outline_and_a_click_presses() {
    let (mut ui, ids) = buttons(&["Resume", "Quit"]);
    assert!(has_outline(&ui));

    let over = point_at(&ui, "Quit");
    // The pointer merely being somewhere is not the player using it.
    ui.update(
        &UiInput {
            pointer: Some([5.0, 5.0]),
            ..NOTHING
        },
        0.016,
        HD,
    );
    assert_eq!(ui.mode(), InputMode::Focus);

    ui.update(&over, 0.016, HD);
    assert_eq!(ui.mode(), InputMode::Pointer);
    assert_eq!(ui.focused(), Some(ids[1]));
    assert!(!has_outline(&ui));

    assert_eq!(
        ui.update(
            &UiInput {
                pointer_down: true,
                ..over
            },
            0.016,
            HD
        ),
        []
    );
    assert_eq!(ui.update(&over, 0.016, HD), [UiEvent::Pressed(ids[1])]);
}

#[test]
fn letting_go_somewhere_else_does_not_press() {
    let (mut ui, _) = buttons(&["Resume", "Quit"]);
    let over = point_at(&ui, "Quit");
    let away = UiInput {
        pointer: Some([5.0, 5.0]),
        ..NOTHING
    };
    ui.update(&over, 0.016, HD);
    ui.update(
        &UiInput {
            pointer_down: true,
            ..over
        },
        0.016,
        HD,
    );
    ui.update(
        &UiInput {
            pointer_down: true,
            ..away
        },
        0.016,
        HD,
    );
    assert_eq!(ui.update(&away, 0.016, HD), []);
}

#[test]
fn the_first_gamepad_press_after_the_mouse_only_shows_the_outline() {
    let (mut ui, ids) = buttons(&["Resume", "Quit"]);
    let over = point_at(&ui, "Quit");
    ui.update(
        &UiInput {
            pointer: Some([5.0, 5.0]),
            ..NOTHING
        },
        0.016,
        HD,
    );
    ui.update(&over, 0.016, HD);
    assert_eq!(ui.mode(), InputMode::Pointer);

    let still = |input: UiInput| UiInput {
        pointer: over.pointer,
        ..input
    };
    assert_eq!(ui.update(&still(CONFIRM), 0.016, HD), []);
    assert_eq!(ui.mode(), InputMode::Focus);
    assert_eq!(ui.focused(), Some(ids[1]));
    assert!(has_outline(&ui));
    ui.update(&still(NOTHING), 0.016, HD);
    assert_eq!(
        ui.update(&still(CONFIRM), 0.016, HD),
        [UiEvent::Pressed(ids[1])]
    );
}

#[test]
fn dragging_sets_a_slider() {
    let Settings { mut ui, slider, .. } = settings();
    let track = track_of(&ui);
    let start = UiInput {
        pointer: Some([track[0] + 1.0, track[1] - 10.0]),
        ..NOTHING
    };
    ui.update(
        &UiInput {
            pointer: Some([5.0, 5.0]),
            ..NOTHING
        },
        0.016,
        HD,
    );
    ui.update(&start, 0.016, HD);
    let events = ui
        .update(
            &UiInput {
                pointer_down: true,
                ..start
            },
            0.016,
            HD,
        )
        .to_vec();
    assert_eq!(events, [UiEvent::Changed(slider, Value::Slider(0.0))]);
    // The drag carries on even when the pointer leaves the slider.
    let far_right = UiInput {
        pointer: Some([1900.0, 20.0]),
        pointer_down: true,
        ..NOTHING
    };
    assert_eq!(
        ui.update(&far_right, 0.016, HD),
        [UiEvent::Changed(slider, Value::Slider(100.0))]
    );
}

// ---- Mistakes --------------------------------------------------------------

#[test]
fn a_bad_screen_is_refused_whole_and_says_what_to_fix() {
    let mut ui = Ui::default();
    let error = ui
        .add_screen("Broken", |screen| {
            screen.button("Fine");
            screen.slider("Volume", 10.0, 0.0, 1.0, 5.0);
            screen.choice("Quality", &[], 0);
        })
        .unwrap_err();
    assert_eq!(
        error,
        UiError::BadSlider {
            label: "Volume".to_owned()
        }
    );
    assert!(error.to_string().contains("lowest value below its highest"));

    // The next screen works as normal.
    let (mut button, mut label) = (None, None);
    let screen = ui
        .add_screen("Good", |screen| {
            button = Some(screen.button("Play"));
            label = Some(screen.label("Words"));
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    assert_eq!(ui.focused(), button);
    assert_eq!(texts(&ui), ["Good", "Play", "Words"]);
}

#[test]
fn each_building_mistake_has_its_own_error() {
    let mut ui = Ui::default();
    let no_options = ui.add_screen("", |screen| _ = screen.choice("Quality", &[], 0));
    assert_eq!(
        no_options,
        Err(UiError::NoOptions {
            label: "Quality".to_owned()
        })
    );
    let no_tabs = ui.add_screen("", |screen| _ = screen.tabs(&[], |_, _| {}));
    assert_eq!(
        no_tabs,
        Err(UiError::NoOptions {
            label: "tabs".to_owned()
        })
    );
    let no_columns = ui.add_screen("", |screen| screen.grid(0, |_| {}));
    assert_eq!(no_columns, Err(UiError::NoColumns));
    let bad_step = ui.add_screen("", |screen| _ = screen.slider("Zoom", 0.0, 1.0, 0.0, 0.0));
    assert!(matches!(bad_step, Err(UiError::BadSlider { .. })));

    let (mut other, mut label) = (None, None);
    ui.add_screen("", |screen| {
        other = Some(screen.button("Other screen"));
        label = Some(screen.label("Words"));
    })
    .unwrap();
    let across = ui.add_screen("", |screen| {
        let here = screen.button("Here");
        screen.neighbor(here, Direction::Up, other.unwrap());
    });
    assert_eq!(across, Err(UiError::NeighborNotOnScreen));
}

#[test]
fn setters_refuse_the_wrong_widget() {
    let Settings {
        mut ui,
        toggle,
        slider,
        ..
    } = settings();
    let error = ui.set_slider(toggle, 1.0).unwrap_err();
    assert_eq!(
        error,
        UiError::WrongKind {
            expected: "slider",
            found: "toggle"
        }
    );
    assert_eq!(
        error.to_string(),
        "This widget is a toggle, not a slider. Use the setter that matches the builder that \
         made it."
    );
    assert!(ui.set_toggle(slider, true).is_err());
    assert!(ui.set_choice(slider, 0).is_err());
    assert!(ui.set_tab(slider, 0).is_err());
    assert!(ui.set_bar(slider, 0.5).is_err());
    assert!(ui.set_color(slider, Color::WHITE).is_err());

    let stranger = WidgetId(9_999);
    assert_eq!(ui.set_text(stranger, "hi"), Err(UiError::UnknownWidget));
    assert_eq!(ui.value(stranger), None);
    assert_eq!(
        Ui::default().open(ui.open_screen().unwrap()),
        Err(UiError::UnknownScreen)
    );
}

#[test]
fn a_screen_with_nothing_to_land_on_still_closes_with_back() {
    let mut ui = Ui::default();
    let loading = ui
        .add_screen("Loading", |screen| _ = screen.label("Please wait"))
        .unwrap();
    ui.open(loading).unwrap();
    idle(&mut ui);
    assert_eq!(ui.focused(), None);
    assert_eq!(tap(&mut ui, CONFIRM), []);
    assert_eq!(tap(&mut ui, DOWN), []);
    assert_eq!(tap(&mut ui, BACK), [UiEvent::Back(loading)]);
}

// ---- The same in, the same out --------------------------------------------

#[test]
fn the_same_inputs_draw_the_same_picture() {
    let play = || {
        let Settings { mut ui, .. } = settings();
        ui.hud_text(Anchor::Top, [0.0, 0.0], 30.0, "1:30");
        for input in [DOWN, NOTHING, RIGHT, NOTHING, DOWN, CONFIRM, NOTHING, BACK] {
            ui.update(&input, 1.0 / 144.0, [2560, 1440]);
        }
        idle(&mut ui);
        let draws: Vec<String> = ui.draw().iter().map(|draw| format!("{draw:?}")).collect();
        draws
    };
    let first = play();
    assert!(!first.is_empty());
    assert_eq!(first, play());
}

// ---- Found in review -------------------------------------------------------

/// Where the slider on show has its track.
fn track_of(ui: &Ui) -> [f32; 2] {
    rects(ui)
        .iter()
        .find(|(_, size, _)| *size == [200.0, 8.0])
        .unwrap()
        .0
}

#[test]
fn clicking_a_slider_beside_its_track_changes_nothing() {
    let Settings { mut ui, slider, .. } = settings();
    let over = point_at(&ui, "Volume");
    ui.update(
        &UiInput {
            pointer: Some([5.0, 5.0]),
            ..NOTHING
        },
        0.016,
        HD,
    );
    ui.update(&over, 0.016, HD);
    assert_eq!(
        ui.update(
            &UiInput {
                pointer_down: true,
                ..over
            },
            0.016,
            HD
        ),
        []
    );
    assert_eq!(ui.update(&over, 0.016, HD), []);
    assert_eq!(ui.focused(), Some(slider));
    assert_eq!(ui.value(slider), Some(Value::Slider(90.0)));
}

#[test]
fn a_press_is_dropped_when_the_pointer_leaves_the_window() {
    let (mut ui, _) = buttons(&["Resume", "Quit"]);
    let over = point_at(&ui, "Quit");
    ui.update(
        &UiInput {
            pointer: Some([5.0, 5.0]),
            ..NOTHING
        },
        0.016,
        HD,
    );
    ui.update(&over, 0.016, HD);
    ui.update(
        &UiInput {
            pointer_down: true,
            ..over
        },
        0.016,
        HD,
    );
    ui.update(
        &UiInput {
            pointer_down: true,
            ..NOTHING
        },
        0.016,
        HD,
    );
    assert_eq!(ui.update(&NOTHING, 0.016, HD), []);
}

#[test]
fn switching_tabs_drops_a_slider_being_dragged() {
    let (mut ui, _, [_, volume]) = tabbed();
    tap(&mut ui, NEXT_TAB);
    let track = track_of(&ui);
    let at = |x: f32, down: bool| UiInput {
        pointer: Some([track[0] + x, track[1]]),
        pointer_down: down,
        ..NOTHING
    };
    ui.update(&at(90.0, false), 0.016, HD);
    ui.update(&at(100.0, false), 0.016, HD);
    ui.update(&at(100.0, true), 0.016, HD);
    ui.update(
        &UiInput {
            next_tab: true,
            ..at(100.0, true)
        },
        0.016,
        HD,
    );
    assert_eq!(ui.update(&at(190.0, true), 0.016, HD), []);
    assert_eq!(ui.value(volume), Some(Value::Slider(0.5)));
}

#[test]
fn down_wraps_to_the_top_from_a_row_under_a_wider_widget() {
    let mut ui = Ui::default();
    let (mut top, mut right) = (None, None);
    let screen = ui
        .add_screen("", |screen| {
            top = Some(screen.slider("Volume", 0.0, 1.0, 0.1, 0.5));
            screen.row(|screen| {
                screen.button("Apply");
                right = Some(screen.button("Cancel"));
            });
            screen.start_on(right.unwrap());
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    tap(&mut ui, DOWN);
    assert_eq!(ui.focused(), top);
}

#[test]
fn a_direction_held_since_before_the_menu_opened_does_nothing() {
    let mut ui = Ui::default();
    let mut first = None;
    let pause = ui
        .add_screen("Paused", |screen| {
            first = Some(screen.button("Resume"));
            screen.button("Quit");
        })
        .unwrap();
    // The player is walking backwards when the game pauses.
    for _ in 0..120 {
        ui.update(&DOWN, 1.0 / 60.0, HD);
    }
    ui.open(pause).unwrap();
    for _ in 0..120 {
        ui.update(&DOWN, 1.0 / 60.0, HD);
    }
    assert_eq!(ui.focused(), first);
    idle(&mut ui);
    ui.update(&DOWN, 1.0 / 60.0, HD);
    assert_ne!(ui.focused(), first);
}

#[test]
fn a_small_step_on_a_huge_slider_still_moves() {
    let mut ui = Ui::default();
    let mut slider = None;
    let screen = ui
        .add_screen("", |screen| {
            slider = Some(screen.slider("Big", 0.0, 3.0e7, 1.0, 2.0e7))
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    tap(&mut ui, LEFT);
    assert_eq!(ui.value(slider.unwrap()), Some(Value::Slider(2.0e7 - 1.0)));
}

#[test]
fn ids_from_a_refused_screen_never_name_a_later_widget() {
    let mut ui = Ui::default();
    let mut stale = None;
    let refused = ui.add_screen("", |screen| {
        stale = Some(screen.toggle("Old", false));
        screen.choice("Empty", &[], 0);
    });
    assert!(refused.is_err());
    let mut fresh = None;
    ui.add_screen("", |screen| fresh = Some(screen.toggle("New", false)))
        .unwrap();
    ui.set_toggle(stale.unwrap(), true).unwrap();
    assert_eq!(ui.value(fresh.unwrap()), Some(Value::Toggle(false)));
}

#[test]
fn a_buffer_holds_several_lists_in_order_with_their_own_words() {
    let mut hud = Ui::default();
    hud.hud_text(Anchor::BottomRight, [0.0, 0.0], 32.0, "30 / 90");
    idle(&mut hud);
    let (mut menu, _) = buttons(&["Resume", "Quit"]);
    idle(&mut menu);

    let mut buffer = crate::DrawBuffer::default();
    buffer.add(hud.draw());
    buffer.add(menu.draw());
    let expected: Vec<Draw<'_>> = hud.draw().iter().chain(menu.draw().iter()).collect();
    assert_eq!(buffer.draw().iter().collect::<Vec<_>>(), expected);
    assert!(
        expected
            .iter()
            .any(|draw| matches!(draw, Draw::Text { text: "Quit", .. }))
    );

    buffer.clear();
    assert!(buffer.draw().is_empty());
    buffer.add(menu.draw());
    assert_eq!(buffer.draw().len(), menu.draw().len());
}

// ---- Screens longer than the window ------------------------------------------

/// The top and bottom of the room a long screen is seen through on an HD
/// window with no hints: the safe area less the panel's edge.
const ROOM: [f32; 2] = [94.0, 986.0];

/// A screen of twenty buttons, "Item 1" to "Item 20", which is taller
/// than an HD window.
fn long() -> (Ui, Vec<WidgetId>) {
    let names: Vec<String> = (1..=20).map(|number| format!("Item {number}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    buttons(&names)
}

fn is_drawn(ui: &Ui, wanted: &str) -> bool {
    texts(ui).iter().any(|text| text == wanted)
}

/// A pointer beside the menu, over no widget, rolling the wheel.
fn roll(notches: f32) -> UiInput {
    UiInput {
        pointer: Some([200.0, 540.0]),
        wheel: notches,
        ..NOTHING
    }
}

/// Checks that no widget is drawn outside the room.
fn assert_inside_the_room(ui: &Ui) {
    let theme = Theme::default();
    for (position, size, color) in rects(ui) {
        if color == theme.backdrop || color == theme.panel {
            continue;
        }
        let allowed = theme.outline + 0.01;
        assert!(
            position[1] >= ROOM[0] - allowed && position[1] + size[1] <= ROOM[1] + allowed,
            "a box from {} to {} is outside the room",
            position[1],
            position[1] + size[1]
        );
    }
    for draw in ui.draw() {
        if let Draw::Text {
            position,
            size,
            text,
            ..
        } = draw
        {
            assert!(
                position[1] >= ROOM[0] - 4.51 && position[1] + size <= ROOM[1] + 4.51,
                "\"{text}\" is drawn outside the room"
            );
        }
    }
}

#[test]
fn a_screen_longer_than_the_window_stays_inside_the_safe_area() {
    let (ui, ids) = long();
    assert_eq!(ui.focused(), Some(ids[0]));
    // It starts at its top: the title and the first buttons, not the last.
    assert!(is_drawn(&ui, "Menu") && is_drawn(&ui, "Item 1"));
    assert!(!is_drawn(&ui, "Item 20"));
    assert_inside_the_room(&ui);

    // The panel is as tall as the room and no taller.
    let theme = Theme::default();
    let (position, size, _) = rects(&ui)
        .into_iter()
        .find(|&(_, _, color)| color == theme.panel)
        .unwrap();
    assert_eq!(
        [position[1], position[1] + size[1]],
        [ROOM[0] - 40.0, ROOM[1] + 40.0]
    );
}

#[test]
fn a_long_screen_follows_the_widget_the_player_is_on() {
    let (mut ui, ids) = long();
    for (place, id) in ids.iter().enumerate().skip(1) {
        tap(&mut ui, DOWN);
        assert_eq!(ui.focused(), Some(*id));
        let name = format!("Item {}", place + 1);
        assert!(is_drawn(&ui, &name), "{name} is not on show");
        assert!(has_outline(&ui));
        // The next one shows too, so the player sees where down leads.
        if place + 1 < ids.len() {
            let next = format!("Item {}", place + 2);
            assert!(is_drawn(&ui, &next), "{next} is not on show");
        }
        assert_inside_the_room(&ui);
    }
    // At the last widget the screen is at its end, and no further.
    let [_, y] = find(&ui, "Item 20");
    assert!((y + 14.0 + 28.0 - ROOM[1]).abs() < 0.01, "{y}");
    assert!(!is_drawn(&ui, "Menu"));

    // Down wraps to the first, and the title is back with it.
    tap(&mut ui, DOWN);
    assert_eq!(ui.focused(), Some(ids[0]));
    assert!(is_drawn(&ui, "Menu") && is_drawn(&ui, "Item 1"));

    // Going up shows the widget before the one the player is on.
    tap(&mut ui, UP);
    assert_eq!(ui.focused(), Some(ids[19]));
    for place in (10..19).rev() {
        tap(&mut ui, UP);
        assert!(is_drawn(&ui, &format!("Item {place}")));
    }
    assert_inside_the_room(&ui);
}

#[test]
fn a_long_screen_moves_no_further_than_it_must() {
    let (mut ui, _) = long();
    let before = find(&ui, "Item 2");
    // The first few widgets are already on show: nothing moves.
    tap(&mut ui, DOWN);
    tap(&mut ui, DOWN);
    assert_eq!(find(&ui, "Item 2"), before);
}

#[test]
fn the_wheel_scrolls_a_long_screen_and_stops_at_its_ends() {
    let (mut ui, _) = long();
    let start = find(&ui, "Item 5")[1];
    // Towards the player shows what is further down: a row and a half.
    ui.update(&roll(-1.0), 1.0 / 60.0, HD);
    assert_eq!(find(&ui, "Item 5")[1], start - 102.0);
    assert_eq!(ui.mode(), InputMode::Pointer);
    ui.update(&roll(1.0), 1.0 / 60.0, HD);
    assert_eq!(find(&ui, "Item 5")[1], start);
    // It cannot be rolled past the top or the end.
    ui.update(&roll(50.0), 1.0 / 60.0, HD);
    assert_eq!(find(&ui, "Item 5")[1], start);
    ui.update(&roll(-500.0), 1.0 / 60.0, HD);
    assert!(is_drawn(&ui, "Item 20") && !is_drawn(&ui, "Item 1"));
    let end = find(&ui, "Item 20");
    ui.update(&roll(-1.0), 1.0 / 60.0, HD);
    assert_eq!(find(&ui, "Item 20"), end);
    ui.update(&roll(f32::NAN), 1.0 / 60.0, HD);
    assert_eq!(find(&ui, "Item 20"), end);
    assert_inside_the_room(&ui);
}

#[test]
fn the_wheel_does_nothing_to_a_screen_that_fits() {
    let (mut ui, _) = buttons(&["Resume", "Quit"]);
    let before: Vec<_> = ui.draw().iter().collect();
    let before = format!("{before:?}");
    ui.update(&roll(-3.0), 1.0 / 60.0, HD);
    let after: Vec<_> = ui.draw().iter().collect();
    assert_eq!(format!("{after:?}"), before);
    assert_eq!(ui.mode(), InputMode::Focus);
    // And it has no scroll bar: nothing is drawn in the track's colour.
    let track = Theme::default().track;
    assert!(rects(&ui).iter().all(|&(_, _, color)| color != track));
}

#[test]
fn a_long_screen_shows_how_far_down_it_is() {
    let (mut ui, _) = long();
    let theme = Theme::default();
    let bar = |ui: &Ui| {
        let all = rects(ui);
        let track = *all.iter().find(|draw| draw.2 == theme.track).unwrap();
        // The thumb is the box drawn straight after its track.
        let place = all.iter().position(|draw| *draw == track).unwrap();
        (track, all[place + 1])
    };
    let (track, thumb) = bar(&ui);
    assert_eq!([track.0[1], track.0[1] + track.1[1]], ROOM);
    assert_eq!(thumb.0[1], ROOM[0], "at the top to begin with");
    assert!(thumb.1[1] < track.1[1] && thumb.1[1] > 100.0);

    ui.update(&roll(-500.0), 1.0 / 60.0, HD);
    let (_, thumb) = bar(&ui);
    assert!((thumb.0[1] + thumb.1[1] - ROOM[1]).abs() < 0.01);
}

#[test]
fn dragging_a_long_screen_moves_it_and_presses_nothing() {
    let (mut ui, _) = long();
    let start = find(&ui, "Item 6")[1];
    let mut at = point_at(&ui, "Item 6");
    ui.update(&at, 1.0 / 60.0, HD);
    at.pointer_down = true;
    ui.update(&at, 1.0 / 60.0, HD);
    // The screen follows the pointer.
    let from = at.pointer.unwrap();
    at.pointer = Some([from[0], from[1] - 200.0]);
    assert!(ui.update(&at, 1.0 / 60.0, HD).is_empty());
    assert_eq!(find(&ui, "Item 6")[1], start - 200.0);
    at.pointer = Some([from[0], from[1] - 150.0]);
    ui.update(&at, 1.0 / 60.0, HD);
    assert_eq!(find(&ui, "Item 6")[1], start - 150.0);
    // Letting go over a button presses nothing: it was a drag.
    at.pointer_down = false;
    assert!(ui.update(&at, 1.0 / 60.0, HD).is_empty());
    assert_eq!(find(&ui, "Item 6")[1], start - 150.0);
    assert_inside_the_room(&ui);
}

#[test]
fn an_unsteady_click_on_a_long_screen_is_still_a_click() {
    let (mut ui, ids) = long();
    let start = find(&ui, "Item 6");
    let mut at = point_at(&ui, "Item 6");
    ui.update(&at, 1.0 / 60.0, HD);
    at.pointer_down = true;
    ui.update(&at, 1.0 / 60.0, HD);
    let from = at.pointer.unwrap();
    at.pointer = Some([from[0] + 3.0, from[1] + 5.0]);
    ui.update(&at, 1.0 / 60.0, HD);
    assert_eq!(find(&ui, "Item 6"), start, "five pixels is not a drag");
    at.pointer_down = false;
    assert_eq!(ui.update(&at, 1.0 / 60.0, HD), [UiEvent::Pressed(ids[5])]);
}

#[test]
fn a_widget_scrolled_out_of_sight_cannot_be_clicked() {
    let (mut ui, _) = long();
    ui.update(&roll(-500.0), 1.0 / 60.0, HD);
    // In the panel's edge above the room, where a button that was
    // rolled past the top would be.
    let mut at = UiInput {
        pointer: Some([960.0, ROOM[0] - 20.0]),
        ..NOTHING
    };
    ui.update(&at, 1.0 / 60.0, HD);
    at.pointer_down = true;
    ui.update(&at, 1.0 / 60.0, HD);
    at.pointer_down = false;
    assert!(ui.update(&at, 1.0 / 60.0, HD).is_empty());
    // Nor is anything highlighted there.
    let selected = Theme::default().widget_selected;
    assert!(rects(&ui).iter().all(|&(_, _, color)| color != selected));
}

#[test]
fn confirm_does_not_choose_a_widget_that_was_rolled_out_of_sight() {
    let (mut ui, ids) = long();
    ui.update(&roll(-500.0), 1.0 / 60.0, HD);
    assert!(!is_drawn(&ui, "Item 1"));
    // The first press brings the outline back, and the widget with it.
    assert!(tap(&mut ui, CONFIRM).is_empty());
    assert_eq!(ui.mode(), InputMode::Focus);
    assert!(is_drawn(&ui, "Item 1") && has_outline(&ui));
    assert_eq!(tap(&mut ui, CONFIRM), [UiEvent::Pressed(ids[0])]);
}

#[test]
fn each_screen_remembers_how_far_it_was_scrolled() {
    let (mut ui, ids) = long();
    let other = ui.add_screen("Other", |screen| {
        screen.button("One");
    });
    for _ in 0..12 {
        tap(&mut ui, DOWN);
    }
    let before = find(&ui, "Item 13");
    ui.open(other.unwrap()).unwrap();
    idle(&mut ui);
    assert!(is_drawn(&ui, "One") && !is_drawn(&ui, "Item 13"));
    tap(&mut ui, BACK);
    assert_eq!(ui.focused(), Some(ids[12]));
    assert_eq!(find(&ui, "Item 13"), before);
}

#[test]
fn a_window_made_taller_or_shorter_keeps_the_player_in_view() {
    let (mut ui, ids) = long();
    for _ in 0..19 {
        tap(&mut ui, DOWN);
    }
    // Twice as tall in real pixels is the same picture, twice the size.
    ui.update(&NOTHING, 1.0 / 60.0, [3840, 2160]);
    assert!(is_drawn(&ui, "Item 20"));
    // A window so wide and short that everything is scaled down still
    // shows the widget the player is on.
    ui.set_ui_scale(1.5);
    ui.update(&NOTHING, 1.0 / 60.0, [1280, 360]);
    assert_eq!(ui.focused(), Some(ids[19]));
    assert!(is_drawn(&ui, "Item 20"));
    ui.set_ui_scale(0.75);
    ui.update(&NOTHING, 1.0 / 60.0, HD);
    assert!(is_drawn(&ui, "Item 20"));
}

#[test]
fn a_long_page_of_tabs_goes_back_to_the_top_on_a_shorter_one() {
    let mut ui = Ui::default();
    let screen = ui
        .add_screen("Settings", |screen| {
            screen.tabs(&["Long", "Short"], |screen, tab| {
                let count = if tab == 0 { 20 } else { 2 };
                for number in 1..=count {
                    screen.toggle(&format!("{} {number}", ["Long", "Short"][tab]), false);
                }
            });
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    for _ in 0..19 {
        tap(&mut ui, DOWN);
    }
    assert!(is_drawn(&ui, "Long 20") && !is_drawn(&ui, "Settings"));
    // The other page is short, but the screen is as tall as its longest.
    tap(&mut ui, NEXT_TAB);
    assert!(is_drawn(&ui, "Short 1") && is_drawn(&ui, "Settings"));
    assert_inside_the_room(&ui);
}

// ---- Button hints --------------------------------------------------------------

const XBOX: crate::ButtonNames<'static> = crate::ButtonNames {
    confirm: "A",
    back: "B",
    previous_tab: "LB",
    next_tab: "RB",
    round: true,
};
const KEYBOARD: crate::ButtonNames<'static> = crate::ButtonNames {
    confirm: "Enter",
    back: "Esc",
    previous_tab: "Q",
    next_tab: "E",
    round: false,
};

/// The words of the hints, which are drawn last: everything after
/// `last`, the last words of the last widget.
fn hints(ui: &Ui, last: &str) -> String {
    let all = texts(ui);
    let from = all.iter().rposition(|text| text == last).unwrap();
    all[from + 1..].join(" ")
}

#[test]
fn a_menu_says_what_the_buttons_do_once_it_knows_their_names() {
    let (mut ui, _) = buttons(&["Resume", "Quit"]);
    assert!(!is_drawn(&ui, "Back"), "no names, no hints");
    ui.show_button_hints(&XBOX);
    idle(&mut ui);
    assert_eq!(hints(&ui, "Quit"), "A Select B Back");

    // Another device, other names, at once.
    ui.show_button_hints(&KEYBOARD);
    idle(&mut ui);
    assert_eq!(hints(&ui, "Quit"), "Enter Select Esc Back");

    ui.hide_button_hints();
    idle(&mut ui);
    assert!(!is_drawn(&ui, "Back") && !is_drawn(&ui, "Select"));
}

#[test]
fn hints_follow_the_widget_the_player_is_on() {
    let mut ui = settings().ui;
    ui.show_button_hints(&XBOX);
    idle(&mut ui);
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(hints(&ui, "Medium"));
        tap(&mut ui, DOWN);
    }
    assert_eq!(
        seen,
        ["A Change B Back", "< > Change B Back", "< > Change B Back"],
        "a toggle, a slider and a choice"
    );
}

#[test]
fn a_screen_with_tabs_says_which_buttons_switch_them() {
    let (mut ui, _, _) = tabbed();
    ui.show_button_hints(&XBOX);
    idle(&mut ui);
    assert_eq!(hints(&ui, "On"), "A Change B Back LB RB Switch tab");
    // The hint is for the device in the hand, whatever opened the tab.
    ui.show_button_hints(&KEYBOARD);
    tap(&mut ui, NEXT_TAB);
    assert_eq!(hints(&ui, "0.50"), "< > Change Esc Back Q E Switch tab");
}

#[test]
fn a_button_with_no_name_has_no_hint() {
    let (mut ui, _, _) = tabbed();
    ui.show_button_hints(&crate::ButtonNames {
        confirm: "",
        previous_tab: "",
        ..XBOX
    });
    idle(&mut ui);
    assert!(!is_drawn(&ui, "Select") && !is_drawn(&ui, "Change"));
    assert_eq!(hints(&ui, "On"), "B Back RB Switch tab");
    ui.show_button_hints(&crate::ButtonNames::default());
    idle(&mut ui);
    assert_eq!(hints(&ui, "On"), "");
}

#[test]
fn the_mouse_needs_no_hint_to_choose() {
    let (mut ui, _) = buttons(&["Resume", "Quit"]);
    ui.show_button_hints(&KEYBOARD);
    let at = point_at(&ui, "Quit");
    ui.update(&at, 1.0 / 60.0, HD);
    let mut moved = at;
    moved.pointer = Some([at.pointer.unwrap()[0] + 5.0, at.pointer.unwrap()[1]]);
    ui.update(&moved, 1.0 / 60.0, HD);
    assert_eq!(ui.mode(), InputMode::Pointer);
    assert_eq!(hints(&ui, "Quit"), "Esc Back");
    assert!(!is_drawn(&ui, "Select"));
}

/// The boxes the hints' button names are drawn in: everything in the
/// selected colour that is drawn after the last widget.
fn caps(ui: &Ui) -> Vec<([f32; 2], [f32; 2], f32)> {
    let selected = Theme::default().widget_selected;
    let all: Vec<Draw<'_>> = ui.draw().iter().collect();
    let last_widget = all
        .iter()
        .rposition(|draw| matches!(draw, Draw::Text { text: "Quit", .. }))
        .unwrap();
    all[last_widget..]
        .iter()
        .filter_map(|draw| match *draw {
            Draw::Rect {
                position,
                size,
                color,
                corner,
            } if color == selected => Some((position, size, corner)),
            _ => None,
        })
        .collect()
}

#[test]
fn hints_sit_in_a_line_at_the_bottom_of_the_safe_area() {
    let (mut ui, _) = buttons(&["Resume", "Quit"]);
    ui.show_button_hints(&XBOX);
    idle(&mut ui);
    let drawn = caps(&ui);
    assert_eq!(drawn.len(), 2);
    for (position, size, corner) in &drawn {
        // The bottom of the safe area on an HD window is 1026.
        assert_eq!(position[1] + size[1], 1026.0);
        // One letter is in a circle, as on the gamepad.
        assert_eq!(size[0], size[1]);
        assert_eq!(*corner, size[1] / 2.0);
    }
    // The line is centred: as far from the left as from the right.
    let left = drawn[0].0[0];
    let [x, _] = find(&ui, "Back");
    let right = x + 4.0 * 28.0 * 0.55;
    assert!((left - (1920.0 - right)).abs() < 0.01, "{left} {right}");

    // A key is a squarer shape, wide enough for its name.
    ui.show_button_hints(&KEYBOARD);
    idle(&mut ui);
    let drawn = caps(&ui);
    assert!(drawn[0].1[0] > drawn[0].1[1]);
    assert_eq!(drawn[0].2, Theme::default().corner);

    // Text in a hint is never smaller than the smallest text, and grows
    // with the player's size setting.
    ui.set_ui_scale(1.5);
    idle(&mut ui);
    for draw in ui.draw() {
        if let Draw::Text {
            size, text: "Back", ..
        } = draw
        {
            assert_eq!(size, 42.0);
        }
    }
}

#[test]
fn hints_are_under_the_menu_and_never_over_it() {
    // Eleven buttons fit an HD window, but not with a line of hints
    // under them unless the menu moves up.
    let names: Vec<String> = (1..=11).map(|number| format!("Item {number}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (mut ui, _) = buttons(&names);
    let before = find(&ui, "Item 11")[1];
    ui.show_button_hints(&XBOX);
    idle(&mut ui);
    let theme = Theme::default();
    let all = rects(&ui);
    let panel = all.iter().find(|draw| draw.2 == theme.panel).unwrap();
    let hint_top = find(&ui, "Back")[1] - 6.0;
    assert!(
        panel.0[1] + panel.1[1] <= hint_top,
        "the panel covers the hints"
    );
    assert!(find(&ui, "Item 11")[1] < before);
    assert!(is_drawn(&ui, "Menu") && is_drawn(&ui, "Item 1"));
    // A short menu stays in the middle of the window, where it was.
    let (mut short, _) = buttons(&["Resume", "Quit"]);
    let before = find(&short, "Resume");
    short.show_button_hints(&XBOX);
    idle(&mut short);
    assert_eq!(find(&short, "Resume"), before);
}

#[test]
fn a_long_screen_makes_room_for_its_hints() {
    let (mut ui, _) = long();
    ui.show_button_hints(&XBOX);
    for _ in 0..19 {
        tap(&mut ui, DOWN);
    }
    assert!(is_drawn(&ui, "Item 20"));
    let hint_top = find(&ui, "Back")[1] - 6.0;
    let theme = Theme::default();
    for (position, size, color) in rects(&ui) {
        if color == theme.widget || color == theme.panel {
            assert!(position[1] + size[1] <= hint_top);
        }
    }
    assert_eq!(hints(&ui, "Item 20"), "A Select B Back");
}

#[test]
fn a_widget_cut_by_the_edge_of_a_long_screen_is_only_its_box() {
    let mut ui = Ui::default();
    let screen = ui
        .add_screen("Settings", |screen| {
            for number in 1..=20 {
                screen.slider(&format!("Setting {number}"), 0.0, 10.0, 1.0, 5.0);
            }
        })
        .unwrap();
    ui.open(screen).unwrap();
    idle(&mut ui);
    // A third of a notch leaves a row half over each edge.
    ui.update(&roll(-2.3), 1.0 / 60.0, HD);
    let theme = Theme::default();
    let all = rects(&ui);
    let rows = all.iter().filter(|draw| draw.2 == theme.widget).count();
    // Each whole slider has one filled part, in the accent colour, and
    // its number; a cut one has neither.
    let filled = all.iter().filter(|draw| draw.2 == theme.accent).count();
    let numbers = texts(&ui).iter().filter(|text| *text == "5").count();
    assert_eq!(filled, numbers);
    assert_eq!(
        rows,
        filled + 2,
        "one row is cut at the top and one at the bottom"
    );
    assert_inside_the_room(&ui);
}

#[test]
fn the_wheel_and_a_drag_add_up() {
    let (mut ui, _) = long();
    let start = find(&ui, "Item 9")[1];
    let mut at = point_at(&ui, "Item 9");
    ui.update(&at, 1.0 / 60.0, HD);
    at.pointer_down = true;
    ui.update(&at, 1.0 / 60.0, HD);
    let from = at.pointer.unwrap();
    at.pointer = Some([from[0], from[1] - 100.0]);
    ui.update(&at, 1.0 / 60.0, HD);
    // A notch while the button is held is kept on the frames after it.
    at.wheel = -1.0;
    ui.update(&at, 1.0 / 60.0, HD);
    at.wheel = 0.0;
    ui.update(&at, 1.0 / 60.0, HD);
    assert_eq!(find(&ui, "Item 9")[1], start - 100.0 - 102.0);
}
