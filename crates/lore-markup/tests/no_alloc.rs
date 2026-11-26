//! A settled screen must not touch the heap when time passes and nothing
//! moves, nor may a running opacity and transform transition.

#![allow(clippy::unwrap_used, reason = "tests")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use lore_markup::{FontRef, Screen, TextMeasure};

/// Counts the allocations each thread makes.
struct Counting;

thread_local! {
    /// How many times this thread has asked for memory. Each thread has its
    /// own count: the test harness thread prints and keeps time meanwhile.
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
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn counted() -> usize {
    let before = ALLOCATIONS.get();
    drop(std::hint::black_box(Box::new(0_u64)));
    assert!(
        ALLOCATIONS.get() > before,
        "this thread's allocations are not being counted"
    );
    ALLOCATIONS.get()
}

struct Measure;

impl TextMeasure for Measure {
    fn width(&mut self, font: &FontRef<'_>, text: &str) -> f32 {
        text.chars().count() as f32 * (font.size * 0.5 + font.letter_spacing)
    }
    fn metrics(&mut self, font: &FontRef<'_>) -> (f32, f32) {
        (font.size * 0.8, font.size * 0.2)
    }
    fn image_size(&mut self, _: &str) -> Option<[f32; 2]> {
        None
    }
}

const MENU: &str = r#"<html><head><style>
body { display: flex; flex-direction: column; align-items: center; padding: 40px; gap: 12px; background-color: #101820; }
h1 { font-size: 64px; color: #fc0; }
.row { display: flex; gap: 12px; }
button { width: 320px; height: 56px; font-size: 28px; color: white; background-color: #234;
  border-radius: 8px; opacity: 1; transition: opacity 0.5s linear, transform 0.5s ease; }
button:hover { opacity: 0.5; transform: scale(1.1); }
p { color: #aab; font-size: 20px; }
</style></head><body>
<h1>The Long Road</h1>
<button id="play">Play</button>
<button id="load">Load</button>
<button id="options">Options</button>
<div class="row"><button id="a">A</button><button id="b">B</button></div>
<p>Version 1.0 - press up and down to choose.</p>
</body></html>"#;

fn menu() -> Screen {
    let mut screen = Screen::parse("ui/main.html", MENU).unwrap();
    screen.set_size([1920.0, 1080.0]);
    for _ in 0..4 {
        screen.frame(&mut Measure);
    }
    screen
}

#[test]
fn an_idle_screen_never_allocates() {
    let mut screen = menu();
    assert!(!screen.is_animating());
    let before = counted();
    for _ in 0..100 {
        screen.tick();
        std::hint::black_box(screen.frame(&mut Measure));
    }
    let after = counted();
    assert_eq!(after - before - 1, 0, "idle ticks allocated");
}

#[test]
fn a_running_transition_does_not_allocate() {
    let mut screen = menu();
    let rect = screen
        .element_rect(screen.document().find_by_id_str("play").unwrap())
        .unwrap();
    screen.pointer_move(rect.x + 5.0, rect.y + 5.0);
    screen.frame(&mut Measure);
    assert!(screen.is_animating());
    // The first ticks may grow buffers; measure after a warm-up.
    for _ in 0..3 {
        screen.tick();
        screen.frame(&mut Measure);
    }
    let before = counted();
    let n = 20;
    for _ in 0..n {
        screen.tick();
        std::hint::black_box(screen.frame(&mut Measure));
    }
    let after = counted();
    assert_eq!(after - before - 1, 0, "running transition allocated");
    assert!(screen.is_animating());
}
