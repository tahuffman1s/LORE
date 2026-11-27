//! The screens of the engine's own menus: `game.menu` and the pause menu.
//!
//! The menu kit (`luau/menu.luau`) draws every menu through a page written
//! in HTML and CSS, the way a game's own screens are drawn. The pages ship
//! with the engine (`menus/`), and a game changes them in three ways:
//!
//! - a ready-made look, `game.ui.theme("parchment")`, is a stylesheet read
//!   after the engine's;
//! - `ui/menus.css` in the game's folder is read after that, so a rule of
//!   the game's wins;
//! - a file of the same name in `ui` (`ui/pause-menu.html`) takes the place
//!   of the engine's page for that screen. It keeps the ids the kit fills
//!   (`#rows`, and if it likes `#tabs`, `#more`, `#keys`, `#shade`).
//!
//! A mistake in a game's file is reported as any screen's is, and the
//! engine's own page (or the page without the game's sheet) is used, so a
//! menu is never blank. The kit drives these pages itself: they take no
//! input of their own, so they are not screens that are "open" to
//! `game.ui.anyOpen`.

use std::collections::HashMap;

use lore_markup::Screen as Page;
use mlua::{Lua, Table, Value};

use super::element::ElementValue;
use super::{
    Entry, FileWatch, KEEPS_OLD, SAVE_TO_FIX, ScreenId, UiState, add_links, kbd_of, markup_error,
    read_error, watch_of,
};
use crate::api::{Shared, fail};
use crate::error::ScriptError;
use crate::modules::{Files, ReadProblem};

/// The page every menu is drawn on, unless the game has its own.
const MENU_HTML: &str = include_str!("menus/menu.html");
/// The keyboard a gamepad types into a menu's text box with.
const KEYBOARD_HTML: &str = include_str!("menus/menu-keyboard.html");
/// How the engine's menus look: variables first, then the parts.
const MENUS_CSS: &str = include_str!("menus/menus.css");

/// The ready-made looks a game picks with `game.ui.theme`, by name, with the
/// text of each: a stylesheet a game may copy to its `ui/menus.css`.
pub const THEMES: &[(&str, &str)] = &[
    ("parchment", include_str!("menus/themes/parchment.css")),
    ("frost", include_str!("menus/themes/frost.css")),
    ("ember", include_str!("menus/themes/ember.css")),
];

/// The files of the engine's menus a game can put its own in place of.
pub(crate) const SCREENS: &[&str] = &[
    "game-menu.html",
    "pause-menu.html",
    "settings-menu.html",
    "quit-menu.html",
    "menu-keyboard.html",
];

/// The game's own sheet for every menu, read after the engine's.
pub(crate) const GAME_SHEET: &str = "ui/menus.css";

/// The page the engine has for the screen `name`.
fn engine_html(name: &str) -> &'static str {
    if name == "menu-keyboard.html" {
        KEYBOARD_HTML
    } else {
        MENU_HTML
    }
}

/// The text of a theme, by name.
pub(crate) fn theme_css(name: &str) -> Option<&'static str> {
    THEMES
        .iter()
        .find(|(theme, _)| *theme == name)
        .map(|(_, css)| *css)
}

/// One menu page, read: the page, the files to watch, and a mistake in a
/// game's file that was left out to make it.
pub(super) struct BuiltMenu {
    pub page: Option<Page>,
    pub watches: Vec<FileWatch>,
    pub error: Option<ScriptError>,
}

/// Reads the page for the menu screen `name`: the game's own if it has one
/// that works, else the engine's; then the engine's sheet, the theme, and
/// the game's `ui/menus.css` if it has one that works.
pub(super) fn build_menu(files: &Files, name: &str, theme: Option<&str>, note: &str) -> BuiltMenu {
    let own = format!("ui/{name}");
    let mut watches = vec![watch_of(files, &own), watch_of(files, GAME_SHEET)];
    let mut error = None;
    let own_html = match files.read(&own) {
        Ok(text) => Some(text),
        Err(ReadProblem::Missing) => None,
        Err(problem) => {
            error = Some(read_error(files, &own, &problem, None, note));
            None
        }
    };
    let sheet = match files.read(GAME_SHEET) {
        Ok(text) => Some(text),
        Err(ReadProblem::Missing) => None,
        Err(problem) => {
            error.get_or_insert(read_error(files, GAME_SHEET, &problem, None, note));
            None
        }
    };
    let engine_file = format!("engine/{name}");
    let mut tries: Vec<(&str, &str, bool)> = Vec::with_capacity(4);
    // The game's page with its sheet, then each of them alone, then the
    // engine's page alone: the first that works is drawn.
    if let Some(html) = &own_html {
        tries.push((&own, html, sheet.is_some()));
        if sheet.is_some() {
            tries.push((&own, html, false));
        }
    }
    tries.push((&engine_file, engine_html(name), sheet.is_some()));
    if sheet.is_some() {
        tries.push((&engine_file, engine_html(name), false));
    }
    let mut page = None;
    for (file, html, with_sheet) in tries {
        let game_page = file == own;
        let sheet = if with_sheet { sheet.as_deref() } else { None };
        match assemble(
            files,
            file,
            html,
            game_page,
            theme,
            sheet,
            &mut watches,
            note,
        ) {
            Ok(made) => {
                page = Some(made);
                break;
            }
            Err(wrong) => {
                error.get_or_insert(*wrong);
            }
        }
    }
    watches.sort_by(|a, b| a.name.cmp(&b.name));
    watches.dedup_by(|a, b| a.name == b.name);
    BuiltMenu {
        page,
        watches,
        error,
    }
}

/// One try at a menu page from `html`, read as the file `file`.
#[allow(
    clippy::too_many_arguments,
    reason = "one private step of build_menu, called from one place"
)]
fn assemble(
    files: &Files,
    file: &str,
    html: &str,
    game_page: bool,
    theme: Option<&str>,
    sheet: Option<&str>,
    watches: &mut Vec<FileWatch>,
    note: &str,
) -> Result<Page, Box<ScriptError>> {
    let mut sources: HashMap<String, String> = HashMap::new();
    sources.insert(file.to_owned(), html.to_owned());
    let mut page =
        Page::parse(file, html).map_err(|e| Box::new(markup_error(&e, &sources, note)))?;
    page.add_unlinked_stylesheet("engine/menus.css", MENUS_CSS, true)
        .map_err(|e| Box::new(markup_error(&e, &sources, note)))?;
    if let Some(css) = theme.and_then(theme_css) {
        page.add_unlinked_stylesheet("engine/theme.css", css, true)
            .map_err(|e| Box::new(markup_error(&e, &sources, note)))?;
    }
    if game_page {
        add_links(files, &mut page, file, &mut sources, watches, note)?;
    }
    if game_page {
        let (needs, what) = if file.ends_with("menu-keyboard.html") {
            (
                ".key",
                "a key: <div class=\"key\">a</div>, in a <div class=\"keyrow\">",
            )
        } else {
            ("#rows", "<div id=\"rows\"></div>, where the menu's rows go")
        };
        if !matches!(page.find(needs), Ok(Some(_))) {
            return Err(Box::new(ScriptError {
                file: file.to_owned(),
                line: None,
                message: format!(
                    "this page takes the place of the engine's, but has no {needs} for the \
                     menu to fill, so the engine's own page is used"
                ),
                source_line: None,
                hint: Some(format!("add {what}")),
                note: Some(note.to_owned()),
            }));
        }
    }
    if let Some(css) = sheet {
        sources.insert(GAME_SHEET.to_owned(), css.to_owned());
        page.add_unlinked_stylesheet(GAME_SHEET, css, false)
            .map_err(|e| Box::new(markup_error(&e, &sources, note)))?;
    }
    Ok(page)
}

impl UiState {
    /// A new screen for the menu kit, drawn from the menu page `name`.
    pub(crate) fn load_menu(&self, name: &str) -> ScreenId {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id = inner.next_id.wrapping_add(1);
        let built = build_menu(&self.files, name, inner.theme.as_deref(), SAVE_TO_FIX);
        inner.error_news |= built.error.is_some();
        let mut entry = Entry::blank(id, &format!("ui/{name}"));
        entry.menu = Some(name.to_owned());
        entry.claimed = true;
        entry.fresh = true;
        entry.watches = built.watches;
        if let Some(error) = built.error {
            log::error!("{error}");
            entry.error = Some(error);
        }
        if let Some(mut page) = built.page {
            page.set_reduce_motion(inner.reduce_motion);
            entry.kbd = kbd_of(&mut page);
            entry.page = Some(page);
        }
        inner.entries.push(entry);
        id
    }

    /// Reads a menu screen again from its files, keeping the old page if
    /// even the engine's could not be made.
    pub(super) fn reload_menu(&self, entry: &mut Entry, theme: Option<&str>, reduce: bool) {
        let Some(name) = entry.menu.clone() else {
            return;
        };
        let built = build_menu(&self.files, &name, theme, KEEPS_OLD);
        entry.watches = built.watches;
        if let Some(error) = &built.error
            && entry.error.as_ref() != Some(error)
        {
            log::error!("{error}");
        }
        entry.error = built.error;
        if let Some(mut page) = built.page {
            page.set_reduce_motion(reduce);
            entry.kbd = kbd_of(&mut page);
            entry.page = Some(page);
            entry.generation = entry.generation.wrapping_add(1);
            entry.reloaded = true;
            entry.warned = 0;
        }
    }

    /// Whether a menu page was made or read again since the last time this
    /// was asked, so its mistakes may have changed.
    pub(crate) fn take_error_news(&self) -> bool {
        std::mem::take(&mut self.inner.borrow_mut().error_news)
    }

    /// Whether a menu screen was read again since the last time this was
    /// asked: what the kit put on it is gone and must be put back.
    pub(crate) fn take_reloaded(&self, id: ScreenId) -> bool {
        let mut inner = self.inner.borrow_mut();
        inner
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .is_some_and(|entry| std::mem::take(&mut entry.reloaded))
    }

    /// Where an element was at the last picture, in units of the page:
    /// left, top, width and height.
    pub(crate) fn rect_of(&self, element: super::ElementRef) -> Option<[f32; 4]> {
        self.with_element(element, |page, id| {
            page.element_rect(id).map(|r| [r.x, r.y, r.w, r.h])
        })
        .ok()
        .flatten()
    }

    /// Whether the pointer is on an element, and how far across it it is
    /// (0 at the left edge, 1 at the right; less or more off its ends).
    pub(crate) fn pointer_on(&self, element: super::ElementRef) -> (bool, f32) {
        let pointer = self.inner.borrow().pointer;
        let (Some([x, y]), Some([left, top, width, height])) = (pointer, self.rect_of(element))
        else {
            return (false, -1.0);
        };
        let inside = x >= left && x < left + width && y >= top && y < top + height;
        (inside, (x - left) / width.max(1e-3))
    }

    /// Picks the ready-made look of the engine's menus, or none, and reads
    /// every menu screen again with it.
    pub(crate) fn set_theme(&self, theme: Option<String>) {
        let mut inner = self.inner.borrow_mut();
        if inner.theme == theme {
            return;
        }
        inner.theme = theme;
        let reduce = inner.reduce_motion;
        let theme = inner.theme.clone();
        for entry in inner.entries.iter_mut().filter(|e| e.menu.is_some()) {
            self.reload_menu(entry, theme.as_deref(), reduce);
        }
        inner.error_news = true;
    }

    /// The mistakes in the menu files a game has of its own, for `lore check`:
    /// each file it has is read as the menus would read it.
    pub(crate) fn check_menus(&self) -> Vec<String> {
        let theme = self.inner.borrow().theme.clone();
        let has = |name: &str| !matches!(self.files.read(name), Err(ReadProblem::Missing));
        let mut mistakes = Vec::new();
        let names: Vec<&str> = if has(GAME_SHEET) {
            SCREENS.to_vec()
        } else {
            SCREENS
                .iter()
                .copied()
                .filter(|name| has(&format!("ui/{name}")))
                .collect()
        };
        for name in names {
            if let Some(error) = build_menu(&self.files, name, theme.as_deref(), SAVE_TO_FIX).error
            {
                let said = super::format_mistake(&error);
                if !mistakes.contains(&said) {
                    mistakes.push(said);
                }
            }
        }
        mistakes
    }
}

/// The functions only the engine's own scripts get: handed to them when
/// they run, and never put where a game's script can reach them.
pub(crate) fn private_table(lua: &Lua, shared: &std::rc::Rc<Shared>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let state = std::rc::Rc::clone(shared);
    table.set(
        "screen",
        lua.create_function(move |lua, name: String| {
            if !SCREENS.contains(&name.as_str()) {
                return Err(fail(lua, format!("no engine menu page called {name}")));
            }
            let id = state.ui.load_menu(&name);
            super::api::screen_value(lua, id)
        })?,
    )?;
    let state = std::rc::Rc::clone(shared);
    table.set(
        "reloaded",
        lua.create_function(move |lua, screen: Value| {
            let id = super::api::screen_id(lua, &screen)?;
            Ok(state.ui.take_reloaded(id))
        })?,
    )?;
    let state = std::rc::Rc::clone(shared);
    table.set(
        "pointer",
        lua.create_function(move |_, element: mlua::UserDataRef<ElementValue>| {
            Ok(state.ui.pointer_on(element.0))
        })?,
    )?;
    let state = std::rc::Rc::clone(shared);
    table.set(
        "rect",
        lua.create_function(move |_, element: mlua::UserDataRef<ElementValue>| {
            let rect = state.ui.rect_of(element.0);
            Ok((
                rect.map(|r| r[0]),
                rect.map(|r| r[1]),
                rect.map(|r| r[2]),
                rect.map(|r| r[3]),
            ))
        })?,
    )?;
    Ok(table)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn game(files: &[(&str, &str)]) -> Files {
        Files::Memory(
            files
                .iter()
                .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn every_engine_page_and_theme_reads_without_a_mistake() {
        let files = game(&[]);
        for name in SCREENS {
            for theme in [None]
                .into_iter()
                .chain(THEMES.iter().map(|(t, _)| Some(*t)))
            {
                let built = build_menu(&files, name, theme, SAVE_TO_FIX);
                assert!(built.error.is_none(), "{name} {theme:?}: {:?}", built.error);
                let page = built.page.unwrap();
                assert!(page.warnings().is_empty(), "{name}: {:?}", page.warnings());
            }
        }
    }
}
