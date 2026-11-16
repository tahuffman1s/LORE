//! The per-tick input path must not touch the heap.

#![allow(clippy::unwrap_used, reason = "tests")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use lore_input::{ActionMap, GamepadButton, Input, Key, MouseWheel, Stick, StickAxis};

/// Counts the allocations each thread makes.
struct Counting;

thread_local! {
    /// How many times this thread has asked for memory. Each thread has
    /// its own count, because the test is not alone in its process: the
    /// thread that started it prints and keeps time meanwhile, and what
    /// that sets aside is not the code under test's.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every method forwards to `System` unchanged, so its contract is upheld.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // A plain number with nothing to set up or tear down, so reading
        // it asks for no memory itself. A thread that is ending no longer
        // has one, and is not counted.
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

/// How many allocations this thread has made so far, having first made
/// sure that they are being counted at all.
fn counted() -> usize {
    let before = ALLOCATIONS.get();
    drop(std::hint::black_box(Box::new(0_u64)));
    assert!(
        ALLOCATIONS.get() > before,
        "this thread's allocations are not being counted"
    );
    ALLOCATIONS.get()
}

// Only what this test's own thread sets aside is counted.
#[test]
fn feeding_events_and_ticking_never_allocates() {
    let map = ActionMap::default_fps();
    let fire = map.action("fire").unwrap();
    let menu = map.set("menu").unwrap();
    let mut input = Input::new(map);
    let mut shots = 0_u32;

    let before = counted();
    for step in 0..1_000_u16 {
        let phase = f32::from(step % 100) / 100.0;
        input.press(Key::W);
        input.press(MouseWheel::MouseWheelUp);
        input.move_mouse(phase * 10.0, -phase);
        input.set_stick(Stick::RightStick, StickAxis::X, phase);
        input.set_button_amount(GamepadButton::RightTrigger, phase);
        if step % 7 == 0 {
            input.release(Key::W);
        }
        if step % 50 == 0 {
            input.release_all();
        }
        if step % 90 == 0 {
            input.set_active_set(menu);
        } else if step % 90 == 3 {
            input.set_active_set(ActionMap::GAMEPLAY);
        }
        shots += u32::from(input.tick().just_pressed(fire));
    }
    let after = ALLOCATIONS.get();

    assert!(shots > 0, "the loop should have exercised the fire action");
    assert_eq!(after - before, 0, "the input tick path allocated");
}
