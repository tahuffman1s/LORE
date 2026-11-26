//! What the player does to the screens, one tick at a time.
//!
//! The host reads the menu actions of the tick into [`Controls`] and calls
//! [`UiState::step`]. The top screen that has a control takes the input:
//! the pointer, the d-pad, A, B, the bumpers and typed text. Then every
//! showing screen moves on a tick, and what the controls said comes back as
//! [`Fired`], for the host to answer with the script's functions.
//!
//! Everything here comes from the tick's input and the tick count, never
//! from a clock, so the same input gives the same screens.

use lore_input::TypedKey;
use lore_markup::{
    Direction, ElementId, Event, EventKind, EventValue, HandlerKind, InputType, Screen as Page, Tag,
};

use super::{
    DrawEnv, Entry, Inner, Measurer, ScreenId, SettingsSync, UiState, entry_mut, has_control,
    keyboard, tick_entries,
};

/// How far one notch of the wheel scrolls a list, in units of the page.
const WHEEL_STEP: f32 = 60.0;

/// What the player did this tick, from the menu actions.
#[derive(Default)]
pub(crate) struct Controls<'a> {
    /// The free pointer, in the page's units.
    pub pointer: Option<[f32; 2]>,
    /// The size of the page, in its units.
    pub screen: [f32; 2],
    pub wheel: f32,
    /// Whether the player last used a gamepad.
    pub gamepad: bool,
    /// The step `menu_move` takes this tick: right and up are positive.
    pub step: [i8; 2],
    pub confirm_down: bool,
    pub confirm_up: bool,
    pub click_down: bool,
    pub click_up: bool,
    pub back: bool,
    pub previous_tab: bool,
    pub next_tab: bool,
    pub space: bool,
    pub erase: bool,
    pub typed: &'a str,
    pub typed_keys: &'a [TypedKey],
}

/// What happened on a screen that a script may answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FiredKind {
    Click,
    Change,
    Input,
    Focus,
    Back,
    NextTab,
    PreviousTab,
}

impl FiredKind {
    /// The word a script reads in `event.kind`.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Change => "change",
            Self::Input => "input",
            Self::Focus => "focus",
            Self::Back => "back",
            Self::NextTab => "nextTab",
            Self::PreviousTab => "previousTab",
        }
    }

    /// Whether the name is one the engine gives, which a script answers
    /// only with `screen:on`.
    pub(crate) fn is_special(self) -> bool {
        matches!(self, Self::Back | Self::NextTab | Self::PreviousTab)
    }
}

/// One thing a handler is to be called for.
#[derive(Debug)]
pub(crate) struct Fired {
    pub screen: ScreenId,
    pub name: String,
    pub kind: FiredKind,
    /// The element, and the generation of the screen it was in.
    pub element: Option<(ElementId, u32)>,
    pub value: EventValue,
    /// Where the attribute that names the handler is written.
    pub file: String,
    pub line: u32,
}

/// What a tick of the screens left for the host to do.
#[derive(Default)]
pub(crate) struct Stepped {
    pub fired: Vec<Fired>,
    /// The sounds to play: name and volume.
    pub sounds: Vec<(String, f32)>,
    /// Whether text is being typed: a text box has focus and the keyboard is
    /// the player's device.
    pub typing: bool,
    /// Whether a screen that takes input is showing.
    pub open: bool,
}

fn is_text_box(page: &Page, id: ElementId) -> bool {
    page.document().get(id).is_some_and(|node| {
        node.tag() == Some(Tag::Input) && node.form.input_type == InputType::Text && !node.disabled
    })
}

fn fire_of(entry: &Entry, page: &Page, event: &Event) -> Fired {
    Fired {
        screen: entry.id,
        name: event.handler.clone(),
        kind: match event.kind {
            EventKind::Click => FiredKind::Click,
            EventKind::Change => FiredKind::Change,
            EventKind::Input => FiredKind::Input,
            EventKind::Focus => FiredKind::Focus,
        },
        element: Some((event.element, entry.generation)),
        value: event.value.clone(),
        file: entry.path.clone(),
        line: page
            .document()
            .get(event.element)
            .map_or(0, |node| node.line),
    }
}

fn special(entry: &Entry, kind: FiredKind) -> Fired {
    Fired {
        screen: entry.id,
        name: kind.word().to_owned(),
        kind,
        element: None,
        value: EventValue::None,
        file: entry.path.clone(),
        line: 0,
    }
}

/// The top screen that takes input, if any.
fn top_of(inner: &Inner) -> Option<ScreenId> {
    inner.stack.iter().rev().copied().find(|id| {
        inner
            .entries
            .iter()
            .any(|e| e.id == *id && e.page.as_ref().is_some_and(has_control))
    })
}

/// Fires the text box's `onchange` if it holds other text than when it took
/// focus, and remembers what it holds now.
pub(super) fn commit_text(entry: &mut Entry, element: ElementId, out: &mut Vec<Fired>) {
    let Some(page) = &entry.page else { return };
    let now = page.value(element).unwrap_or("");
    let Some((el, saved)) = &entry.text_focus else {
        return;
    };
    if *el != element || saved == now {
        return;
    }
    let now = now.to_owned();
    if let Some(name) = page.handler(element, HandlerKind::Change) {
        let event = Event {
            element,
            kind: EventKind::Change,
            handler: name.to_owned(),
            value: EventValue::Text(now.clone()),
        };
        out.push(fire_of(entry, page, &event));
    }
    entry.text_focus = Some((element, now));
}

/// Keeps track of the text box that has focus, and fires its change when
/// focus leaves it with other text in it.
fn track_text(entry: &mut Entry, out: &mut Vec<Fired>) {
    let Some(page) = &entry.page else { return };
    let now = page.focused().filter(|&e| is_text_box(page, e));
    if let Some((el, _)) = &entry.text_focus
        && now != Some(*el)
    {
        let el = *el;
        commit_text(entry, el, out);
        entry.text_focus = None;
    }
    if entry.text_focus.is_none()
        && let (Some(el), Some(page)) = (now, &entry.page)
    {
        entry.text_focus = Some((el, page.value(el).unwrap_or("").to_owned()));
    }
}

/// Closes the on-screen keyboard.
pub(super) fn close_keyboard(inner: &mut Inner) {
    if let Some(keys) = inner.keyboard.take() {
        inner.set_visible(keys.screen, false);
    }
}

impl UiState {
    /// One tick of the screens: the input of the top screen, then every
    /// showing screen's tick, then what happened. `rate` is the ticks to a
    /// second and `label` names the key for an action.
    pub(crate) fn step(
        &self,
        controls: &Controls<'_>,
        env: &DrawEnv<'_>,
        rate: f64,
        label: &dyn Fn(&str) -> String,
    ) -> Stepped {
        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        let mut out = Stepped::default();
        let mut measurer = Measurer {
            tools: env.tools,
            fonts: env.fonts,
            images: env.images,
        };
        if let Some(top) = top_of(inner) {
            drive(inner, top, controls, &mut measurer, &mut out);
        }
        keyboard::keep_up(inner);
        tick_entries(inner, rate, label);
        keyboard::apply_events(inner, controls, &mut measurer, &mut out);
        collect(inner, &mut out);
        out.open = top_of(inner).is_some();
        out.typing = !controls.gamepad
            && top_of(inner).is_some_and(|top| {
                inner
                    .entries
                    .iter()
                    .find(|e| e.id == top)
                    .and_then(|e| e.page.as_ref())
                    .is_some_and(|page| page.focused().is_some_and(|f| is_text_box(page, f)))
            });
        out
    }

    /// Gives each input bound to a setting the setting's value when the
    /// setting changed, and the setting the input's value when the player
    /// changed the input.
    pub(crate) fn sync_binds(&self, settings: &SettingsSync<'_>) {
        let mut inner = self.inner.borrow_mut();
        for entry in &mut inner.entries {
            if entry.binds.is_empty() {
                continue;
            }
            let generation = entry.generation;
            let Some(page) = &mut entry.page else {
                continue;
            };
            for bind in &mut entry.binds {
                super::bind_tick(page, bind, generation, settings);
            }
        }
    }
}

/// The input of one tick on the top screen.
fn drive(
    inner: &mut Inner,
    top: ScreenId,
    c: &Controls<'_>,
    measurer: &mut Measurer<'_>,
    out: &mut Stepped,
) {
    let is_keyboard = inner
        .keyboard
        .as_ref()
        .is_some_and(|keys| keys.screen == top);
    let keyboard_on = inner.on_screen_keyboard;
    let mut open_keyboard = None;
    let mut keyboard_keys = [false; 3];
    {
        let Some(entry) = entry_mut(&mut inner.entries, top) else {
            return;
        };
        let Some(page) = entry.page.as_mut() else {
            return;
        };
        if c.screen.iter().all(|n| n.is_finite() && *n > 0.0) {
            page.set_size(c.screen);
        }
        page.frame(measurer);
        if let Some(asked) = entry.focus_asked.take() {
            page.focus(asked);
            page.frame(measurer);
        }
        let before = page.focused();
        if !c.gamepad {
            if let Some([x, y]) = c.pointer {
                page.pointer_move(x, y);
            }
            if c.click_down {
                page.pointer_down();
            }
            if c.click_up {
                page.pointer_up();
            }
        }
        if c.wheel != 0.0 && c.wheel.is_finite() {
            page.scroll(0.0, -c.wheel * WHEEL_STEP);
        }
        page.frame(measurer);
        let mut moved = false;
        if c.step != [0, 0] {
            let dir = if c.step[0] != 0 {
                if c.step[0] > 0 {
                    Direction::Right
                } else {
                    Direction::Left
                }
            } else if c.step[1] > 0 {
                Direction::Up
            } else {
                Direction::Down
            };
            page.navigate(dir);
            page.frame(measurer);
            moved = true;
        }
        if c.confirm_down {
            let box_focused = page
                .focused()
                .filter(|&f| is_text_box(page, f))
                .filter(|_| c.gamepad && keyboard_on && !is_keyboard);
            if let Some(f) = box_focused {
                open_keyboard = Some(f);
            } else {
                page.activate();
            }
        }
        if c.confirm_up {
            page.activate_release();
        }
        if is_keyboard {
            keyboard_keys = [c.space, c.erase, c.back];
        } else {
            if c.space {
                page.focus_next();
                page.frame(measurer);
                moved = true;
            }
            if c.erase {
                page.backspace();
            }
            if !c.typed.is_empty() {
                page.type_text(c.typed);
            }
            for key in c.typed_keys {
                match key {
                    TypedKey::Backspace => {
                        if let Some(page) = entry.page.as_mut() {
                            page.backspace();
                        }
                    }
                    TypedKey::Enter => {
                        let focused = entry
                            .page
                            .as_ref()
                            .and_then(|page| page.focused().filter(|&f| is_text_box(page, f)));
                        if let Some(f) = focused {
                            commit_text(entry, f, &mut out.fired);
                            if let Some(page) = entry.page.as_mut() {
                                page.focus_next();
                            }
                        }
                    }
                    TypedKey::Escape => out.fired.push(special(entry, FiredKind::Back)),
                }
            }
            if c.back {
                out.fired.push(special(entry, FiredKind::Back));
            }
            if c.previous_tab {
                out.fired.push(special(entry, FiredKind::PreviousTab));
            }
            if c.next_tab {
                out.fired.push(special(entry, FiredKind::NextTab));
            }
            track_text(entry, &mut out.fired);
        }
        let after = entry.page.as_ref().and_then(|p| p.focused());
        if moved && after != before {
            let sounds = &inner.sounds;
            if let Some(name) = &sounds.moved {
                out.sounds.push((name.clone(), sounds.volume));
            }
        }
    }
    if let Some(element) = open_keyboard {
        keyboard::open(inner, top, element);
    }
    if is_keyboard {
        keyboard::keys_pressed(inner, keyboard_keys, out);
    }
}

/// Takes the events of every showing screen but the keyboard.
fn collect(inner: &mut Inner, out: &mut Stepped) {
    let mut events = std::mem::take(&mut inner.events);
    let keyboard_screen = inner.keyboard.as_ref().map(|keys| keys.screen);
    for at in 0..inner.stack.len() {
        let id = inner.stack[at];
        if keyboard_screen == Some(id) {
            continue;
        }
        let Some(entry) = inner.entries.iter_mut().find(|entry| entry.id == id) else {
            continue;
        };
        let Some(page) = entry.page.as_mut() else {
            continue;
        };
        page.take_events(&mut events);
        let Some(page) = entry.page.as_ref() else {
            continue;
        };
        for event in &events {
            let fired = fire_of(entry, page, event);
            let sounds = &inner.sounds;
            let name = if fired.kind == FiredKind::Click {
                &sounds.press
            } else {
                &sounds.moved
            };
            if fired.kind != FiredKind::Focus
                && let Some(name) = name
            {
                out.sounds.push((name.clone(), sounds.volume));
            }
            out.fired.push(fired);
        }
    }
    inner.events = events;
}
