//! Spraying, stepping and asking what is underfoot must not touch the heap:
//! all storage is made in `Fluids::new`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::*;
use lore_fluid::{Flow, Fluids};

struct Counting;

thread_local! {
    /// How many times this thread has asked for memory. Each thread has
    /// its own count, because the test is not alone in its process.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every call is passed straight to the system allocator, which
// upholds the contract; counting does not touch the memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // A plain number with nothing to set up or tear down, so reading
        // it asks for no memory itself.
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        // SAFETY: the caller's layout is passed on unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `alloc` above with this layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

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

#[test]
fn spraying_stepping_and_asking_underfoot_does_not_allocate() {
    // 600 ticks of everything the game does each tick, with sprays of
    // several kinds into a room with a wall and a ledge, and a clear now
    // and then.
    let mut t = floor_triangles(10.0);
    t.extend(wall_triangles(-2.0, 10.0, 0.0, 4.0));
    t.extend(wall_triangles(-4.0, 10.0, 1.0, 3.0));
    let level = lore_physics::Level::from_triangles(&t);
    let mut fluids = Fluids::new(7);
    fluids.set_flow(1, honey());
    fluids.set_flow(
        2,
        Flow {
            dries: 2.0,
            ..oil()
        },
    );

    let tick = |fluids: &mut Fluids, n: usize| {
        let x = n as f32 * 0.07;
        fluids.spray(0, [x.sin(), 3.0, -1.0], [0.0, 0.0, -5.0], 0.02, 0.1);
        fluids.spray(1, [0.5, 1.5, 0.5], [-1.0, -2.0, 0.0], 0.03, 0.05);
        fluids.spray(2, [-1.0, 2.0, 1.0], [2.0, 5.0, -1.0], 0.02, 0.4);
        fluids.spray(0, [0.0, 2.0, 0.0], [0.0, -9.0, 0.0], 0.05, 0.2);
        fluids.step(&level, DT);
        let under = fluids.underfoot([0.0, 0.0, 0.0], 0.3);
        let drops = fluids.drops().len();
        let litres = fluids.litres(None) + fluids.litres(Some(1));
        if n % 150 == 149 {
            fluids.clear(Some(1));
        }
        std::hint::black_box((under, drops, litres));
    };

    // Warm up so that anything that grows lazily has done so.
    for n in 0..300 {
        tick(&mut fluids, n);
    }
    let before = counted();
    for n in 300..900 {
        tick(&mut fluids, n);
    }
    let after = counted();
    // `counted` itself allocates once on each call.
    assert_eq!(
        after - before,
        1,
        "ticks allocated {} times",
        after - before - 1
    );
    assert!(fluids.drops().len() > 100, "the test world is too quiet");
}
