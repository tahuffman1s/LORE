//! The engine's own on-screen keyboard.
//!
//! It is a screen like any other, written in HTML and CSS
//! (`keyboard.html`, `keyboard.css`) and kept with the others. It opens over
//! the stack when a text box is pressed with A while the player uses a
//! gamepad and `game.ui.onScreenKeyboard` is true. Its keys type into that
//! text box, so the box's `oninput` runs for each one. Done, or B, closes it,
//! puts focus back on the text box and runs its `onchange`.

use lore_markup::{ElementId, EventKind, Screen as Page};

use super::input::{Controls, Stepped, commit_text};
use super::{Entry, Inner, Keyboard, Measurer, ScreenId, entry_mut};

const HTML: &str = include_str!("keyboard.html");
const CSS: &str = include_str!("keyboard.css");
/// The name the keyboard's file goes by in messages.
const FILE: &str = "engine/keyboard.html";

fn build() -> Option<Page> {
    let mut page = match Page::parse(FILE, HTML) {
        Ok(page) => page,
        Err(error) => {
            log::error!("the on-screen keyboard has a mistake in it: {error}");
            return None;
        }
    };
    if let Err(error) = page.add_stylesheet("keyboard.css", CSS) {
        log::error!("the on-screen keyboard has a mistake in its style: {error}");
        return None;
    }
    Some(page)
}

/// Opens the keyboard over `target`, a text box on the screen `screen`.
pub(super) fn open(inner: &mut Inner, screen: ScreenId, target: ElementId) {
    let id = match inner.entries.iter().find(|entry| entry.engine) {
        Some(entry) => entry.id,
        None => {
            let Some(page) = build() else { return };
            let id = inner.next_id;
            inner.next_id = inner.next_id.wrapping_add(1);
            let mut entry = Entry::blank(id, FILE);
            entry.engine = true;
            entry.claimed = true;
            entry.page = Some(page);
            inner.entries.push(entry);
            id
        }
    };
    let reduce = inner.reduce_motion;
    if let Some(page) = entry_mut(&mut inner.entries, id).and_then(|e| e.page.as_mut()) {
        page.set_reduce_motion(reduce);
        relabel(page, false);
    }
    inner.keyboard = Some(Keyboard {
        screen: id,
        target: (screen, target),
        shift: false,
        // Not what any text box holds, so the first tick shows it.
        shown: "\u{0}".to_owned(),
    });
    inner.set_visible(id, true);
}

/// Shows the letters as capitals (`shift`) or small.
fn relabel(page: &mut Page, shift: bool) {
    let mut keys = Vec::new();
    if page.find_all(".key", &mut keys).is_err() {
        return;
    }
    for key in keys {
        let text = page.text(key);
        if text.chars().count() != 1 {
            continue;
        }
        let new = if shift {
            text.to_uppercase()
        } else {
            text.to_lowercase()
        };
        if new != text {
            page.set_text(key, &new);
        }
    }
}

/// Closes the keyboard if what it types into has gone, and shows the row of
/// typed text as the text box holds it.
pub(super) fn keep_up(inner: &mut Inner) {
    let Some(keys) = &inner.keyboard else { return };
    let (screen, element) = keys.target;
    let shown = inner.stack.contains(&screen)
        && inner
            .entries
            .iter()
            .find(|e| e.id == screen)
            .and_then(|e| e.page.as_ref())
            .is_some_and(|page| page.focused() == Some(element));
    if !shown {
        super::input::close_keyboard(inner);
        return;
    }
    let text = {
        let Some(page) = inner
            .entries
            .iter()
            .find(|e| e.id == screen)
            .and_then(|e| e.page.as_ref())
        else {
            return;
        };
        let value = page.value(element).unwrap_or("");
        (value != keys.shown).then(|| value.to_owned())
    };
    let Some(text) = text else { return };
    let keyboard = keys.screen;
    if let Some(page) = entry_mut(&mut inner.entries, keyboard).and_then(|e| e.page.as_mut())
        && let Ok(Some(row)) = page.find("#kb-typed")
    {
        page.set_text(row, &format!("{text}|"));
    }
    if let Some(keys) = &mut inner.keyboard {
        keys.shown = text;
    }
}

fn type_into(inner: &mut Inner, text: &str) {
    let Some(keys) = &inner.keyboard else { return };
    let (screen, _) = keys.target;
    if let Some(page) = entry_mut(&mut inner.entries, screen).and_then(|e| e.page.as_mut()) {
        page.type_text(text);
    }
}

fn erase(inner: &mut Inner) {
    let Some(keys) = &inner.keyboard else { return };
    let (screen, _) = keys.target;
    if let Some(page) = entry_mut(&mut inner.entries, screen).and_then(|e| e.page.as_mut()) {
        page.backspace();
    }
}

/// Closes the keyboard, and gives the text box its `onchange` if the
/// keyboard changed what it holds. Focus never left the text box.
fn finish(inner: &mut Inner, out: &mut Stepped) {
    let Some(keys) = &inner.keyboard else { return };
    let (screen, element) = keys.target;
    super::input::close_keyboard(inner);
    if let Some(entry) = entry_mut(&mut inner.entries, screen) {
        commit_text(entry, element, &mut out.fired);
    }
}

fn press_sound(inner: &Inner, out: &mut Stepped) {
    if let Some(name) = &inner.sounds.press {
        out.sounds.push((name.clone(), inner.sounds.volume));
    }
}

/// What the keyboard's own buttons asked for this tick.
pub(super) fn apply_events(
    inner: &mut Inner,
    _controls: &Controls<'_>,
    _measurer: &mut Measurer<'_>,
    out: &mut Stepped,
) {
    let Some(keys) = &inner.keyboard else { return };
    let id = keys.screen;
    let mut events = std::mem::take(&mut inner.events);
    if let Some(page) = entry_mut(&mut inner.entries, id).and_then(|e| e.page.as_mut()) {
        page.take_events(&mut events);
    }
    let mut done = false;
    for event in &events {
        if event.kind != EventKind::Click {
            continue;
        }
        press_sound(inner, out);
        match event.handler.as_str() {
            "key" => {
                let text = entry_mut(&mut inner.entries, id)
                    .and_then(|e| e.page.as_ref())
                    .map(|page| page.text(event.element))
                    .unwrap_or_default();
                type_into(inner, &text);
            }
            "space" => type_into(inner, " "),
            "erase" => erase(inner),
            "shift" => {
                let shift = inner.keyboard.as_mut().map(|keys| {
                    keys.shift = !keys.shift;
                    keys.shift
                });
                if let Some(page) = entry_mut(&mut inner.entries, id).and_then(|e| e.page.as_mut())
                {
                    relabel(page, shift.unwrap_or(false));
                }
            }
            "done" => done = true,
            _ => {}
        }
    }
    inner.events = events;
    if done {
        finish(inner, out);
    }
}

/// Space, Erase and B on the controller while the keyboard is open:
/// `[space, erase, back]`.
pub(super) fn keys_pressed(inner: &mut Inner, keys: [bool; 3], out: &mut Stepped) {
    let [space, erase_key, back] = keys;
    if space {
        type_into(inner, " ");
    }
    if erase_key {
        erase(inner);
    }
    if back {
        finish(inner, out);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use lore_markup::Direction;

    use super::*;
    use crate::tools_api::Tools;

    fn focused_text(page: &Page) -> String {
        page.text(page.focused().unwrap())
    }

    #[test]
    fn the_keyboard_fits_the_screen_and_the_dpad_reaches_every_key() {
        let mut page = build().expect("the keyboard's own files are valid");
        page.set_size([1920.0, 1080.0]);
        let tools = Tools::default();
        let mut measurer = Measurer {
            tools: &tools,
            fonts: &[],
            images: &[],
        };
        page.frame(&mut measurer);
        assert_eq!(focused_text(&page), "f", "it starts in the middle");
        let mut keys = Vec::new();
        page.find_all("button", &mut keys).unwrap();
        for key in &keys {
            let rect = page.element_rect(*key).unwrap();
            assert!(
                rect.x >= 0.0
                    && rect.y >= 0.0
                    && rect.x + rect.w <= 1920.0
                    && rect.y + rect.h <= 1080.0,
                "{} is off the screen: {rect:?}",
                page.text(*key)
            );
        }
        // Down to the last row (the Space key), then right to Done.
        page.navigate(Direction::Down);
        page.frame(&mut measurer);
        page.navigate(Direction::Down);
        page.frame(&mut measurer);
        assert_eq!(focused_text(&page), "Space");
        page.navigate(Direction::Right);
        page.frame(&mut measurer);
        assert_eq!(focused_text(&page), "Done");
    }
}
