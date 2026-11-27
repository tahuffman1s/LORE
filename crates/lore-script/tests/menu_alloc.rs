//! A menu that is open and still asks for no memory to be drawn: its page
//! is laid out once, and what it shows is written to it only when it changes.
//! (The script side of a tick reads the player's input through functions
//! that each ask for a little, as every script's tick does; that is counted
//! apart and is not what this file holds to nothing.)

#![allow(clippy::unwrap_used, reason = "tests")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use lore_input::{ActionMap, InputFrame};
use lore_script::ScriptHost;
use lore_ui::DrawBuffer;

/// Counts the allocations each thread makes.
struct Counting;

thread_local! {
    /// How many times this thread has asked for memory.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every method forwards to `System` unchanged, so its contract is upheld.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for `layout`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `System.alloc` with this same `layout`.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        // SAFETY: as for `alloc` and `dealloc`.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

/// Ticks and draws `host` 120 times after 240 to settle, and gives the
/// allocations the ticks and the drawing of its screens asked for.
fn still(host: &mut ScriptHost) -> (usize, usize) {
    let idle = InputFrame::default();
    let mut out = DrawBuffer::default();
    let frame = |host: &mut ScriptHost, out: &mut DrawBuffer| {
        let a = allocations();
        host.tick(&idle, 1.0 / 60.0);
        host.draw(1.0);
        let b = allocations();
        out.clear();
        host.draw_screens(out, [1920.0, 1080.0], 1.0);
        (b - a, allocations() - b)
    };
    for _ in 0..240 {
        frame(host, &mut out);
    }
    let (mut ticking, mut drawing) = (0, 0);
    for _ in 0..120 {
        let (t, d) = frame(host, &mut out);
        ticking += t;
        drawing += d;
    }
    (ticking, drawing)
}

#[test]
fn an_open_menu_that_is_still_is_drawn_without_asking_for_memory() {
    let script = r#"
        local volume = game.menu.slider("Volume", { value = 0.5, format = game.menu.percent })
        local music = game.menu.toggle("Music", { value = true })
        local size = game.menu.choice("Size", { options = { "small", "large" } })
        local name = game.menu.textBox("Name", { placeholder = "Your name" })
        local sound = game.menu.new("Sound", { volume, music, size, name, game.menu.button("Back") })
        local look = game.menu.new("Look", { game.menu.toggle("Fog") })
        game.menu.tabs({ sound, look }):open(sound)
    "#;
    let mut host = ScriptHost::from_source(script, ActionMap::standard());
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let (ticking, drawing) = still(&mut host);
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    assert_eq!(drawing, 0, "drawing the page of a still menu");
    // Reading the input each tick: a few small ones, and no more as time goes.
    assert!(ticking <= 120 * 12, "{ticking} in 120 ticks");
}

#[test]
fn a_still_screen_of_the_games_own_is_drawn_without_asking_for_memory() {
    let html = "<div style=\"width: 400px; background-color: #222\"><p>Hello there</p>\
                <button>Play</button><span>Quit</span></div>";
    let mut host = ScriptHost::from_sources(
        [
            (
                "main.luau".to_owned(),
                "game.ui.load(\"ui/x.html\"):show()".to_owned(),
            ),
            ("ui/x.html".to_owned(), html.to_owned()),
        ],
        ActionMap::standard(),
    );
    assert!(host.last_error().is_none(), "{:?}", host.last_error());
    let (_, drawing) = still(&mut host);
    assert_eq!(drawing, 0);
}
