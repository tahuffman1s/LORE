//! Once made, a mixer must not touch the heap: not to start sounds, not to
//! cut them off, and not to mix. Footsteps and sound sets must not either.

#![allow(clippy::unwrap_used, reason = "tests")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use lore_audio::{
    Feet, FootstepSettings, Footsteps, Importance, Listener, Mixer, Play, Range, starter,
};

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
fn playing_and_mixing_never_allocate() {
    let mut mixer = Mixer::with_voices(48_000, 8);
    let mut steps = starter::footsteps();
    let mut gunfire = starter::gunfire();
    let reload = starter::reload();
    let settings = FootstepSettings::default();
    let mut footsteps = Footsteps::default();
    let mut buffer = vec![0.0_f32; 800 * 2];
    let feet = Feet {
        speed: 6.5,
        grounded: true,
        sprinting: true,
        ..Feet::default()
    };

    let before = counted();
    let mut heard = 0.0_f32;
    for tick in 0..2_000_u32 {
        let turn = tick as f32 * 0.01;
        mixer.set_listener(Listener {
            position: [0.0, 1.6, 0.0],
            yaw: turn,
        });
        let enemy = [10.0 * turn.sin(), 0.0, -10.0 * turn.cos()];
        if footsteps.step(&settings, &feet, 1.0 / 60.0).is_some() {
            mixer.play(steps.at(enemy, tick).range(Range::FOOTSTEP));
        }
        // More gunfire than there are voices, so sounds are cut off too.
        if tick % 3 == 0 {
            mixer.play(gunfire.at(enemy, tick).range(Range::GUNFIRE));
            mixer.play(gunfire.everywhere(tick).importance(Importance::High));
        }
        if tick % 200 == 0 {
            let playing = mixer.play(Play::everywhere(reload.clone()).looping());
            mixer.move_to(playing, enemy);
            mixer.stop(playing);
        }
        mixer.mix(&mut buffer);
        heard = heard.max(buffer[0].abs());
    }
    let allocations = ALLOCATIONS.get() - before;

    assert!(heard > 0.0, "the test should have mixed something");
    assert_eq!(allocations, 0);
}

/// A streamed sound is decoded by another thread, so mixing it asks for
/// nothing either: only starting it does, and that is not done by the
/// thread that mixes when the speakers are playing.
#[test]
fn mixing_a_streamed_sound_never_allocates() {
    use std::time::{Duration, Instant};

    let song = lore_audio::Sound::streamed(&include_bytes!("data/song.ogg")[..]).unwrap();
    let mut mixer = Mixer::with_voices(48_000, 8);
    mixer.play(Play::everywhere(song).music().looping());
    let mut buffer = vec![0.0_f32; 480 * 2];
    let give_up = Instant::now() + Duration::from_secs(30);

    let before = counted();
    let mut heard = 0.0_f32;
    let mut mixes = 0;
    // Until it has started and then for two seconds of sound more, which
    // is further than was decoded ahead.
    while mixes < 200 {
        mixer.mix(&mut buffer);
        heard = heard.max(buffer[0].abs());
        if heard > 0.0 {
            mixes += 1;
        }
        assert!(Instant::now() < give_up, "the stream never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    mixer.set_group_volume(lore_audio::Group::Music, 0.5);
    mixer.stop_all();
    mixer.mix(&mut buffer);
    let allocations = ALLOCATIONS.get() - before;

    assert_eq!(allocations, 0);
}
