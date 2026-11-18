//! What a spray, a splash, a puddle and a footprint should do, as a player
//! would see it, through the public interface only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use common::*;
use lore_fluid::{Flow, Fluids, MOST_DROPS, Underfoot};

/// Litres in a ball 2.5 mm across in radius: a spray this small is one drop.
const ONE_DROP: f32 = 6.0e-5;

fn up(d: &lore_fluid::Drop) -> bool {
    d.facing[1] > 0.99
}

// 1. Sprays -----------------------------------------------------------------

#[test]
fn a_spray_leaves_as_separate_drops_in_a_line() {
    // Checks the stream is made of distinct drops spread along its path,
    // not one blob at the nozzle.
    let mut fluids = Fluids::new(1);
    fluids.spray(0, [0.0, 5.0, 0.0], [20.0, 0.0, 0.0], 0.01, 0.0);
    fluids.step(&floor(), DT);
    let drops = fluids.drops();
    assert!(drops.len() >= 6, "only {} drops", drops.len());
    for (i, a) in drops.iter().enumerate() {
        for b in &drops[i + 1..] {
            assert!(dist(a.place, b.place) > 1.0e-3, "two drops in one place");
        }
    }
    let xs = drops.iter().map(|d| d.place[0]);
    let span = xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min);
    assert!(span > 0.01, "the drops span only {span} m");
    assert!(drops.iter().all(is_flying));
}

#[test]
fn drops_fall_under_gravity_and_air_drag_shortens_the_throw() {
    // A drop thrown flat from 2 m must land nearer than the drag-free
    // parabola says, and must fall as it flies.
    let level = floor();
    let mut fluids = Fluids::new(1);
    fluids.spray(0, [0.0, 2.0, 0.0], [15.0, 0.0, 0.0], ONE_DROP, 0.0);
    let mut fell = false;
    let mut landed = None;
    for _ in 0..120 {
        fluids.step(&level, DT);
        if let Some(d) = lying(&fluids).first() {
            landed = Some(d.place[0]);
            break;
        }
        if let Some(d) = fluids.drops().first()
            && d.place[1] < 1.9
        {
            fell = true;
        }
    }
    let range = landed.expect("the drop never landed");
    let free = 15.0 * (2.0f32 * 2.0 / 9.81).sqrt();
    assert!(fell, "the drop did not fall while flying");
    assert!(range > 0.5 * free, "landed at {range}, drag-free {free}");
    assert!(range < 0.9 * free, "landed at {range}, drag-free {free}");
}

#[test]
fn a_fast_stream_never_gets_through_a_thin_wall() {
    // At 40 m/s a drop moves 0.67 m a tick, so a wall one quad thick is
    // only stopped by sweeping; no drop may ever be on the far side.
    let level = thin_wall();
    let mut fluids = Fluids::new(4);
    for _ in 0..90 {
        fluids.spray(0, [0.0, 0.0, 0.0], [0.0, 0.0, -40.0], 0.01, 0.15);
        fluids.step(&level, DT);
        for d in fluids.drops() {
            assert!(
                d.place[2] > -2.0 - 1.0e-3,
                "a drop is behind the wall: {d:?}"
            );
        }
    }
    assert!(!lying(&fluids).is_empty(), "nothing reached the wall");
}

// 2. Splash and lying -------------------------------------------------------

/// Drops one drop at `speed` straight down onto the floor and returns the
/// drops published on the first tick in which something lies.
fn first_landing(flow: Flow, speed: f32) -> Vec<lore_fluid::Drop> {
    let level = floor();
    let mut fluids = Fluids::new(2);
    fluids.set_flow(0, flow);
    fluids.spray(0, [0.0, 0.05, 0.0], [0.0, -speed, 0.0], ONE_DROP, 0.0);
    for _ in 0..200 {
        fluids.step(&level, DT);
        if !lying(&fluids).is_empty() {
            return fluids.drops().to_vec();
        }
    }
    panic!("nothing landed");
}

#[test]
fn water_hitting_a_floor_at_speed_splashes_into_drops_flying_up_and_out() {
    // Water's splash number is far above the threshold at 8 m/s, so drops
    // are thrown off the surface with an upward part to their direction.
    let drops = first_landing(Flow::default(), 8.0);
    let thrown: Vec<_> = drops.iter().filter(|d| is_flying(d)).collect();
    assert!(thrown.len() >= 2, "{} drops were thrown", thrown.len());
    assert!(thrown.iter().all(|d| d.facing[1] > 0.0), "{thrown:?}");
}

#[test]
fn honey_hitting_a_floor_at_the_same_speed_does_not_splash() {
    // Honey is too viscous for the splash number to pass the threshold:
    // everything stays as lying drops. The speed is 4 m/s, not 8: with the
    // design's splash number (Mundo et al.) a 5 mm honey drop at 8 m/s has
    // K of about 118 against a threshold of 57.7 and does splash, while at
    // 4 m/s K is about 51; water splashes at both.
    let water = first_landing(Flow::default(), 4.0);
    assert!(water.iter().any(is_flying), "water did not splash");
    let drops = first_landing(honey(), 4.0);
    assert!(!drops.iter().any(is_flying), "{drops:?}");
}

#[test]
fn what_lies_on_a_floor_faces_up_and_what_lies_on_a_wall_faces_out() {
    // A lying drop's facing is the surface normal it lies on.
    let drops = first_landing(honey(), 3.0);
    assert!(drops.iter().all(|d| !is_flying(d) && up(d)), "{drops:?}");

    let level = floor_and_wall();
    let mut fluids = Fluids::new(2);
    fluids.set_flow(0, honey());
    fluids.spray(0, [0.0, 2.0, -1.0], [0.0, 0.0, -2.0], ONE_DROP, 0.0);
    run(&mut fluids, &level, 60);
    let wall: Vec<_> = lying(&fluids);
    assert!(!wall.is_empty());
    for d in wall {
        let n = d.facing;
        assert!(
            n[0].abs() < 1e-3 && n[1].abs() < 1e-3 && (n[2] - 1.0).abs() < 1e-3,
            "{d:?}"
        );
    }
}

// 3. Walls ------------------------------------------------------------------

/// Puts one gentle drop of `flow` on the wall at z = -2, high up, and
/// returns the fluids with the drop just landed.
fn gentle_drop_on_wall(level: &lore_physics::Level, flow: Flow, litres: f32, y: f32) -> Fluids {
    let mut fluids = Fluids::new(3);
    fluids.set_flow(0, flow);
    fluids.spray(0, [0.0, y, -1.9], [0.0, 0.0, -0.3], litres, 0.0);
    for _ in 0..120 {
        fluids.step(level, DT);
        if !lying(&fluids).is_empty() {
            break;
        }
    }
    assert!(
        !lying(&fluids).is_empty(),
        "the drop never reached the wall"
    );
    fluids
}

/// The height of the lowest drop lying on a wall facing +z.
fn lowest_on_wall(fluids: &Fluids) -> f32 {
    lying(fluids)
        .iter()
        .filter(|d| d.facing[2] > 0.9)
        .map(|d| d.place[1])
        .fold(f32::MAX, f32::min)
}

/// How fast the leading drop runs down the wall, m/s, measured over `ticks`
/// ticks after it has settled for 5 ticks.
fn run_speed(flow: Flow, litres: f32, ticks: usize) -> f32 {
    let level = floor_and_wall();
    let mut fluids = gentle_drop_on_wall(&level, flow, litres, 3.9);
    run(&mut fluids, &level, 5);
    let start = lowest_on_wall(&fluids);
    run(&mut fluids, &level, ticks);
    (start - lowest_on_wall(&fluids)) / (ticks as f32 * DT)
}

#[test]
fn water_runs_down_a_wall_at_no_more_than_the_film_speed_cap() {
    // The film speed is capped at 2 m/s however thin and heavy the fluid.
    let speed = run_speed(Flow::default(), 10.0 * ONE_DROP, 30);
    assert!(speed > 0.3, "water ran at only {speed} m/s");
    assert!(speed <= 2.0 * 1.05, "water ran at {speed} m/s");
}

#[test]
fn water_runs_much_faster_down_a_wall_than_honey() {
    // Viscosity makes honey creep where water races.
    let water = run_speed(Flow::default(), 10.0 * ONE_DROP, 30);
    let honey = run_speed(honey(), 10.0 * ONE_DROP, 300);
    assert!(honey > 0.0, "honey did not move at all");
    assert!(water > 50.0 * honey, "water {water} m/s, honey {honey} m/s");
}

#[test]
fn honey_runs_down_a_wall_at_about_the_nusselt_film_speed_for_its_thickness() {
    // Nusselt: u = rho g h^2 / (3 mu) for a film of thickness h; the drop
    // may be slower (it is held by pinning) but by a factor of a few only.
    let level = floor_and_wall();
    let mut fluids = gentle_drop_on_wall(&level, honey(), ONE_DROP, 3.9);
    run(&mut fluids, &level, 5);
    let drop = *lying(&fluids).first().expect("the drop lies");
    let start = lowest_on_wall(&fluids);
    run(&mut fluids, &level, 300);
    let measured = (start - lowest_on_wall(&fluids)) / (300.0 * DT);
    let (rho, mu) = (1400.0f32, 10.0f32);
    let nusselt = rho * 9.81 * drop.thick * drop.thick / (3.0 * mu);
    assert!(
        measured > nusselt / 5.0 && measured < nusselt * 5.0,
        "measured {measured} m/s, Nusselt {nusselt} m/s (h = {} m)",
        drop.thick
    );
}

#[test]
fn mercury_and_a_tiny_drop_stay_put_on_a_wall() {
    // High surface tension beads and holds a drop; so does smallness. (By
    // the design's pinning formula a mercury drop of 65 microlitres still
    // slides, as water of that size does; it holds from a few microlitres.)
    let level = floor_and_wall();
    for (name, flow, litres) in [
        ("mercury", mercury(), 5.0e-6),
        ("tiny water drop", Flow::default(), 2.0e-6),
    ] {
        let mut fluids = gentle_drop_on_wall(&level, flow, litres, 3.0);
        run(&mut fluids, &level, 5);
        let start = lowest_on_wall(&fluids);
        run(&mut fluids, &level, 240);
        let moved = start - lowest_on_wall(&fluids);
        assert!(moved < 0.01, "{name} moved {moved} m down the wall");
    }
}

#[test]
fn a_drop_running_off_the_end_of_a_wall_lets_go_and_falls() {
    // The wall stops at 1 m: the running drop finds no surface under it,
    // flies off, and lands on the floor.
    let level = floor_and_ledge_wall();
    let mut fluids = gentle_drop_on_wall(&level, Flow::default(), 3.0e-3, 3.0);
    let mut dripped = false;
    for _ in 0..240 {
        fluids.step(&level, DT);
        dripped |= flying(&fluids)
            .iter()
            .any(|d| d.place[1] < 1.0 && d.facing[1] < -0.5);
    }
    assert!(dripped, "no drop was ever seen falling from the ledge");
    let on_floor = lying(&fluids)
        .iter()
        .filter(|d| d.place[1] < 0.01 && up(d))
        .count();
    assert!(on_floor > 0, "nothing reached the floor");
}

#[test]
fn a_running_drop_leaves_a_trail_of_lying_drops() {
    // One drop was landed; running down a metre of wall leaves a film
    // behind it as more lying drops along the path.
    let level = floor_and_wall();
    let mut fluids = gentle_drop_on_wall(&level, Flow::default(), 10.0 * ONE_DROP, 3.0);
    let landed = lying(&fluids).len();
    run(&mut fluids, &level, 120);
    let on_wall: Vec<_> = lying(&fluids)
        .into_iter()
        .filter(|d| d.facing[2] > 0.9 && d.place[1] > 0.05)
        .collect();
    assert!(
        on_wall.len() > landed + 2,
        "{} on the wall from {landed}",
        on_wall.len()
    );
}

// 4. Level ground -----------------------------------------------------------

/// Lands `litres` of `flow` as kind 0 gently on the floor at the origin and
/// settles it for `ticks` ticks.
fn puddle(flow: Flow, litres: f32, ticks: usize) -> Fluids {
    let level = floor();
    let mut fluids = Fluids::new(5);
    fluids.set_flow(0, flow);
    fluids.spray(0, [0.0, 0.4, 0.0], [0.0, -0.5, 0.0], litres, 0.0);
    run(&mut fluids, &level, ticks);
    fluids
}

#[test]
fn water_spreads_to_about_its_puddle_thickness_within_a_second() {
    // The gravity puddle thickness of water is about 3 mm.
    let fluids = puddle(Flow::default(), 0.01, 90);
    let h = mean_thickness(&fluids, 0);
    assert!((0.0022..0.0042).contains(&h), "thickness {h} m");
}

#[test]
fn honey_is_still_thick_after_a_second_and_thinner_later() {
    // Honey mounds and creeps: well above the puddle thickness at 1 s, and
    // spreading towards it with time.
    let water = mean_thickness(&puddle(Flow::default(), 0.01, 90), 0);
    let early = mean_thickness(&puddle(honey(), 0.01, 90), 0);
    let late = mean_thickness(&puddle(honey(), 0.01, 90 + 600), 0);
    assert!(early > 1.3 * water, "honey {early} m, water {water} m");
    assert!(late < 0.9 * early, "honey {early} m then {late} m");
}

#[test]
fn many_sprays_on_one_spot_do_not_stack_and_drops_merge() {
    // The drops push apart instead of piling up, and neighbours merge so
    // the count falls.
    let level = floor();
    let mut fluids = Fluids::new(6);
    let mut peak = 0;
    for _ in 0..40 {
        fluids.spray(0, [0.0, 0.3, 0.0], [0.0, -0.5, 0.0], 0.01, 0.02);
        run(&mut fluids, &level, 3);
        peak = peak.max(fluids.drops().len());
    }
    run(&mut fluids, &level, 600);
    let drops = lying(&fluids);
    assert!(
        drops.len() < peak,
        "{} drops from a peak of {peak}",
        drops.len()
    );
    for (i, a) in drops.iter().enumerate() {
        for b in &drops[i + 1..] {
            let flat =
                ((a.place[0] - b.place[0]).powi(2) + (a.place[2] - b.place[2]).powi(2)).sqrt();
            let overlap = a.radius + b.radius - flat;
            assert!(
                overlap < 0.3 * a.radius.min(b.radius) + 0.002,
                "overlap {overlap} m between {a:?} and {b:?}"
            );
        }
    }
}

#[test]
fn on_a_tilted_floor_water_moves_downhill() {
    // The floor rises towards +x, so a puddle's middle must drift to -x.
    let level = tilted_floor(0.6);
    let mut fluids = Fluids::new(7);
    fluids.spray(0, [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], 0.3, 0.0);
    run(&mut fluids, &level, 60);
    let mean_x = |f: &Fluids| {
        let l = lying(f);
        l.iter().map(|d| d.place[0]).sum::<f32>() / l.len().max(1) as f32
    };
    let before = mean_x(&fluids);
    run(&mut fluids, &level, 300);
    let after = mean_x(&fluids);
    assert!(after < before - 0.02, "x went from {before} to {after}");
}

// 5. Kinds meeting ----------------------------------------------------------

#[test]
fn water_landing_on_honey_washes_the_honey_away() {
    // Water washes: the honey under its patch is removed.
    let level = floor();
    let mut fluids = Fluids::new(8);
    fluids.set_flow(1, honey());
    fluids.spray(1, [0.0, 0.3, 0.0], [0.0, -0.5, 0.0], 0.02, 0.0);
    run(&mut fluids, &level, 120);
    let before = fluids.litres(Some(1));
    assert!(before > 0.01);
    fluids.spray(0, [0.0, 0.3, 0.0], [0.0, -0.5, 0.0], 0.05, 0.0);
    run(&mut fluids, &level, 60);
    let after = fluids.litres(Some(1));
    assert!(
        after < 0.5 * before,
        "honey went from {before} to {after} L"
    );
}

#[test]
fn oil_landing_on_honey_lies_on_top_of_it() {
    // Oil does not wash: the newest fluid is on top, so underfoot reports oil.
    let level = floor();
    let mut fluids = Fluids::new(9);
    fluids.set_flow(1, honey());
    fluids.set_flow(2, oil());
    fluids.spray(1, [0.0, 0.3, 0.0], [0.0, -0.5, 0.0], 0.02, 0.0);
    run(&mut fluids, &level, 120);
    let honey_before = fluids.litres(Some(1));
    assert_eq!(fluids.underfoot([0.0; 3], 0.05).kind, Some(1));
    fluids.spray(2, [0.0, 0.3, 0.0], [0.0, -0.5, 0.0], 0.05, 0.0);
    run(&mut fluids, &level, 60);
    assert!(
        fluids.litres(Some(1)) < honey_before,
        "honey was not covered"
    );
    assert_eq!(fluids.underfoot([0.0; 3], 0.05).kind, Some(2));
}

// 6. Drying and the full store -----------------------------------------------

#[test]
fn a_fluid_that_dries_is_gone_after_about_a_second() {
    // With dries = 1 a settled drop shrinks and has vanished soon after 1 s.
    let level = floor();
    let flow = Flow {
        dries: 1.0,
        ..Flow::default()
    };
    let mut fluids = Fluids::new(10);
    fluids.set_flow(0, flow);
    fluids.spray(0, [0.0, 0.3, 0.0], [0.0, -0.5, 0.0], 0.01, 0.0);
    run(&mut fluids, &level, 40);
    let wet = fluids.litres(Some(0));
    assert!(wet > 0.0 && wet < 0.01, "{wet} L after 40 ticks");
    run(&mut fluids, &level, 110);
    assert_eq!(fluids.litres(Some(0)), 0.0);
    assert!(fluids.drops().is_empty());
}

/// Fills the store with lying drops 8 mm apart on a grid, oldest first, and
/// returns where each was dabbed, in order.
fn fill_store(fluids: &mut Fluids, level: &lore_physics::Level) -> Vec<[f32; 3]> {
    let mut places = Vec::new();
    for i in 0..MOST_DROPS {
        let at = [
            -0.3 + 0.008 * (i % 80) as f32,
            0.0,
            -0.3 + 0.008 * (i / 80) as f32,
        ];
        dab(fluids, 0, at, ONE_DROP);
        places.push(at);
        if i % 40 == 39 {
            fluids.step(level, DT);
        }
    }
    run(fluids, level, 30);
    places
}

#[test]
fn a_full_store_replaces_the_oldest_lying_drops_and_never_the_stream() {
    // At MOST_DROPS, new drops overwrite the oldest lying ones; drops in
    // flight are never the ones to go; the count never passes the limit.
    let level = floor();
    let mut fluids = Fluids::new(11);
    let places = fill_store(&mut fluids, &level);
    assert_eq!(fluids.drops().len(), MOST_DROPS, "the store did not fill");
    assert!(flying(&fluids).is_empty());
    let near = |fluids: &Fluids, at: [f32; 3]| {
        fluids
            .drops()
            .iter()
            .any(|d| (d.place[0] - at[0]).abs() < 1e-4 && (d.place[2] - at[2]).abs() < 1e-4)
    };
    assert!(near(&fluids, places[0]) && near(&fluids, places[100]));

    // A stream in flight, then more sprays than there is room for.
    for k in 0..50 {
        fluids.spray(
            0,
            [1.0 + 0.01 * k as f32, 3.0, 1.0],
            [0.0, -1.0, 0.0],
            ONE_DROP,
            0.0,
        );
        assert!(fluids.drops().len() <= MOST_DROPS);
    }
    assert_eq!(flying(&fluids).len(), 50, "a drop in flight was removed");
    assert!(!near(&fluids, places[0]), "the oldest drop is still there");
    assert!(near(&fluids, places[MOST_DROPS - 1]));
    // And once the stream starts to land, nothing in flight is lost either.
    fluids.step(&level, DT);
    assert!(fluids.drops().len() <= MOST_DROPS);
    assert_eq!(flying(&fluids).len(), 50);
}

// 7. Underfoot ----------------------------------------------------------------

#[test]
fn underfoot_with_no_fluid_is_the_default() {
    // Nothing wet: no kind, no cover, all zeros.
    let fluids = Fluids::new(1);
    assert_eq!(fluids.underfoot([0.0; 3], 0.3), Underfoot::default());
    assert_eq!(Underfoot::default().kind, None);
}

#[test]
fn underfoot_names_the_kind_and_is_covered_in_the_middle_of_a_big_puddle() {
    // Cover is near 1 inside a big puddle, near 0 far from it, and the kind
    // is the one stood in.
    let level = floor();
    let mut fluids = Fluids::new(12);
    fluids.set_flow(3, honey());
    fluids.spray(3, [0.0, 0.4, 0.0], [0.0, -0.5, 0.0], 0.1, 0.0);
    run(&mut fluids, &level, 600);
    let middle = fluids.underfoot([0.0; 3], 0.03);
    assert_eq!(middle.kind, Some(3));
    assert!(middle.cover > 0.9, "{middle:?}");
    assert!(middle.depth > 0.001, "{middle:?}");
    assert!(middle.drag > 0.0 && middle.slippery > 0.0, "{middle:?}");
    let far = fluids.underfoot([5.0, 0.0, 5.0], 0.3);
    assert_eq!(far, Underfoot::default());
    assert!(far.cover < 0.05);
}

#[test]
fn drops_on_a_wall_are_not_underfoot() {
    // A drop lying on a wall faces sideways, so standing there is dry.
    let level = floor_and_wall();
    let fluids = gentle_drop_on_wall(&level, mercury(), 2.0e-6, 3.0);
    let drop = *lying(&fluids).first().expect("the drop lies");
    assert!(drop.facing[2] > 0.9);
    let feet = [drop.place[0], drop.place[1], drop.place[2] + 0.05];
    assert_eq!(fluids.underfoot(feet, 0.3), Underfoot::default());
}
