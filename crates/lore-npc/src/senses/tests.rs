use super::*;

type Tri = [[f32; 3]; 3];

fn floor() -> Vec<Tri> {
    vec![
        [[-50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [50.0, 0.0, -50.0]],
        [[-50.0, 0.0, -50.0], [-50.0, 0.0, 50.0], [50.0, 0.0, 50.0]],
    ]
}

/// A wall across the way along -Z, `high` metres tall, at z = `z`.
fn wall(z: f32, high: f32) -> Vec<Tri> {
    vec![
        [[-5.0, 0.0, z], [5.0, 0.0, z], [5.0, high, z]],
        [[-5.0, 0.0, z], [5.0, high, z], [-5.0, high, z]],
    ]
}

fn level(parts: &[Vec<Tri>]) -> Level {
    Level::from_triangles(&parts.concat())
}

/// An NPC at the middle of the world with its eyes at 1.7 m, facing -Z.
fn me() -> Looker<u32> {
    Looker {
        who: 1,
        eyes: [0.0, 1.7, 0.0],
        facing: 0.0,
    }
}

/// Someone standing still at `x`, `z`.
fn person(who: u32, x: f32, z: f32) -> Seeable<u32> {
    Seeable {
        who,
        feet: [x, 0.0, z],
        height: 1.8,
        speed: 4.5,
        crouched: false,
        down: false,
    }
}

fn open(target: &Seeable<u32>) -> f32 {
    seen_openly(&me(), &Keenness::default(), target, 0.0)
}

/// One look by `senses` at `seeable` in full light, and what it told.
fn look(
    senses: &mut Senses<u32>,
    level: &Level,
    tick: u32,
    seeable: &[Seeable<u32>],
) -> Vec<Sensed<u32>> {
    let mut told = Vec::new();
    let sight = Sight {
        level,
        tick,
        seeable,
        haze: 0.0,
    };
    let mut rays = 0;
    senses.look(
        &me(),
        &sight,
        |_| 1.0,
        &mut rays,
        |sensed| told.push(sensed),
    );
    told
}

#[test]
fn the_cone_ends_at_half_its_width_to_each_side() {
    // 120 degrees wide: 60 to each side of straight ahead (-Z).
    let at = |degrees: f32| {
        let turn = degrees.to_radians();
        person(2, -10.0 * turn.sin(), -10.0 * turn.cos())
    };
    assert!(open(&at(0.0)) > 0.5);
    assert!(open(&at(58.0)) > 0.0);
    assert!(open(&at(-58.0)) > 0.0);
    assert_eq!(open(&at(62.0)), 0.0);
    assert_eq!(open(&at(-62.0)), 0.0);
    // Toward the edge is fainter than straight ahead.
    assert!(open(&at(50.0)) < open(&at(0.0)));
}

#[test]
fn someone_behind_is_not_seen_unless_it_sees_all_round() {
    let behind = person(2, 0.0, 5.0);
    assert_eq!(open(&behind), 0.0);
    let all_round = Keenness {
        width: 360.0,
        ..Keenness::default()
    };
    assert!(seen_openly(&me(), &all_round, &behind, 0.0) > 0.0);
}

#[test]
fn sight_ends_at_its_reach_and_fades_toward_it() {
    assert!(open(&person(2, 0.0, -28.0)) >= SEEN_FROM);
    assert_eq!(open(&person(2, 0.0, -31.0)), 0.0);
    assert!(open(&person(2, 0.0, -20.0)) < open(&person(2, 0.0, -5.0)));
    let blind = Keenness {
        sight: 0.0,
        ..Keenness::default()
    };
    assert_eq!(seen_openly(&me(), &blind, &person(2, 0.0, -2.0), 0.0), 0.0);
}

#[test]
fn crouching_keeping_still_and_fog_are_harder_to_see() {
    let running = person(2, 0.0, -10.0);
    let still = Seeable {
        speed: 0.0,
        ..running
    };
    let crouched = Seeable {
        crouched: true,
        height: 1.1,
        ..running
    };
    assert!(open(&still) < open(&running));
    assert!(open(&crouched) < open(&running));
    let foggy = seen_openly(&me(), &Keenness::default(), &running, haze(1.0, 0.0));
    assert!(foggy < open(&running) * 0.5, "{foggy}");
}

#[test]
fn a_wall_hides_all_and_a_low_wall_hides_the_legs() {
    let target = person(2, 0.0, -10.0);
    let mut rays = 0;
    let clear = level(&[floor()]);
    assert_eq!(showing(&clear, me().eyes, &target, &mut rays), 1.0);
    let high = level(&[floor(), wall(-5.0, 3.0)]);
    assert_eq!(showing(&high, me().eyes, &target, &mut rays), 0.0);
    // A wall to the waist: head and chest show, the knees do not.
    let low = level(&[floor(), wall(-5.0, 1.2)]);
    let shows = showing(&low, me().eyes, &target, &mut rays);
    assert!((shows - 0.7).abs() < 1e-5, "{shows}");
    assert_eq!(rays, 9);
}

#[test]
fn coming_into_view_and_going_out_of_it_are_told_once_each() {
    let clear = level(&[floor()]);
    let walled = level(&[floor(), wall(-5.0, 3.0)]);
    let mut senses = Senses::new(Keenness::default());
    let them = [person(2, 0.0, -10.0)];
    assert_eq!(look(&mut senses, &clear, 1, &them), [Sensed::Saw(2)]);
    assert!(senses.sees(2));
    assert_eq!(look(&mut senses, &clear, 13, &them), []);
    assert_eq!(
        look(&mut senses, &walled, 25, &them),
        [Sensed::Lost {
            who: 2,
            at: [0.0, 0.0, -10.0]
        }]
    );
    assert!(!senses.sees(2));
    // What it keeps of them: where and when they were last seen.
    let kept = senses.sighting(2).copied();
    assert_eq!(kept.map(|seen| (seen.seen, seen.since)), Some((13, 1)));
    assert_eq!(kept.map(|seen| seen.strength), Some(0.0));
    // Someone who leaves the world while in view is lost too.
    look(&mut senses, &clear, 37, &them);
    assert_eq!(
        look(&mut senses, &clear, 49, &[]),
        [Sensed::Lost {
            who: 2,
            at: [0.0, 0.0, -10.0]
        }]
    );
}

#[test]
fn it_does_not_see_itself_and_a_body_is_seen_as_one() {
    let clear = level(&[floor()]);
    let mut senses = Senses::new(Keenness::default());
    let myself = Seeable {
        feet: [0.0, 0.0, -1.0],
        ..person(1, 0.0, 0.0)
    };
    let body = Seeable {
        down: true,
        height: 0.4,
        speed: 0.0,
        ..person(3, 0.0, -4.0)
    };
    assert_eq!(
        look(&mut senses, &clear, 1, &[myself, body]),
        [Sensed::Saw(3)]
    );
    assert_eq!(senses.sighting(3).map(|seen| seen.body), Some(true));
}

#[test]
fn more_in_view_than_it_keeps_lets_the_faintest_go() {
    let clear = level(&[floor()]);
    let mut senses = Senses::new(Keenness::default());
    // Near ones first, then one nearer than any: it takes the place of
    // the furthest.
    let crowd: Vec<Seeable<u32>> = (0..MOST_SIGHTED as u32)
        .map(|n| person(10 + n, 0.0, -10.0 - n as f32))
        .collect();
    look(&mut senses, &clear, 1, &crowd);
    assert_eq!(senses.in_view().count(), MOST_SIGHTED);
    let mut more = crowd.clone();
    more.push(person(99, 0.0, -3.0));
    let told = look(&mut senses, &clear, 13, &more);
    assert!(told.contains(&Sensed::Saw(99)), "{told:?}");
    assert!(
        told.iter()
            .any(|sensed| matches!(sensed, Sensed::Lost { who: 17, .. })),
        "{told:?}"
    );
    assert_eq!(senses.in_view().count(), MOST_SIGHTED);
}

#[test]
fn a_crowd_takes_turns_to_look() {
    let senses = Senses::<u32>::new(Keenness::default());
    // Each of twelve NPCs looks once in twelve ticks, each on its own.
    let mut looked = [0; LOOK_EVERY as usize];
    for tick in 100..100 + LOOK_EVERY {
        let lookers: Vec<u32> = (0..LOOK_EVERY)
            .filter(|&nth| senses.looks_on(tick, nth))
            .collect();
        assert_eq!(lookers.len(), 1, "tick {tick}");
        looked[lookers[0] as usize] += 1;
    }
    assert_eq!(looked, [1; LOOK_EVERY as usize]);
    // Looking less often is one number.
    let slow = Senses::<u32> {
        every: 60,
        ..senses
    };
    assert_eq!((0..60).filter(|&tick| slow.looks_on(tick, 7)).count(), 1);
}

#[test]
fn the_dark_hides_all_but_the_near_and_night_eyes_see_through_it() {
    assert!((daylight(Some(12.0), 0.0) - 1.0).abs() < 1e-5);
    assert!((daylight(Some(0.0), 0.0) - NIGHT).abs() < 1e-5);
    assert!(daylight(Some(12.0), 1.0) < daylight(Some(12.0), 0.0));
    assert_eq!(daylight(None, 1.0), 1.0);
    assert!(light_helps(NIGHT, 0.0, 20.0) < 0.2);
    assert!((light_helps(NIGHT, 1.0, 20.0) - 1.0).abs() < 1e-6);
    assert!((light_helps(0.0, 0.0, 0.5) - 1.0 / 6.0 * 5.0).abs() < 1e-5);
    // A lamp lights the night near it, and not past its reach.
    let near_lamp = lit(
        NIGHT,
        lamp_lux([0.0; 3], [0.0, 2.0, 0.0], 400.0, 10.0, None),
    );
    assert!(near_lamp > 0.9, "{near_lamp}");
    assert_eq!(lamp_lux([0.0; 3], [0.0, 20.0, 0.0], 400.0, 10.0, None), 0.0);
    // And not outside its beam.
    let down = Some(([0.0, -1.0, 0.0], 0.9));
    let up = Some(([0.0, 1.0, 0.0], 0.9));
    assert!(lamp_lux([0.0; 3], [0.0, 2.0, 0.0], 400.0, 0.0, down) > 0.0);
    assert_eq!(lamp_lux([0.0; 3], [0.0, 2.0, 0.0], 400.0, 0.0, up), 0.0);
}

fn shot(at: [f32; 3], by: Option<u32>) -> Sound<u32> {
    Sound {
        at,
        loudness: 1.0,
        near: 4.0,
        far: 150.0,
        by,
    }
}

#[test]
fn a_noise_is_heard_as_far_as_its_reach_and_no_further() {
    let ears = me().eyes;
    assert!(arriving(&shot([0.0, 1.7, -140.0], None), ears, 1.0) >= HEARD_FROM);
    assert_eq!(arriving(&shot([0.0, 1.7, -151.0], None), ears, 1.0), 0.0);
    // Twice as good hearing hears twice as far; none hears nothing.
    assert!(arriving(&shot([0.0, 1.7, -250.0], None), ears, 2.0) > 0.0);
    assert_eq!(arriving(&shot([0.0, 1.7, -2.0], None), ears, 0.0), 0.0);
    // Fainter with distance.
    let near = arriving(&shot([0.0, 1.7, -10.0], None), ears, 1.0);
    let far = arriving(&shot([0.0, 1.7, -40.0], None), ears, 1.0);
    assert!(far < near);
}

#[test]
fn it_does_not_hear_itself_and_a_wall_muffles() {
    let clear = level(&[floor()]);
    let walled = level(&[floor(), wall(-5.0, 3.0)]);
    let mut senses = Senses::new(Keenness::default());
    let sounds = [
        shot([0.0, 1.7, -10.0], Some(1)),
        shot([0.0, 1.7, -10.0], Some(0)),
    ];
    let mut told = Vec::new();
    let mut rays = 0;
    senses.listen(&me(), &clear, 5, &sounds, &mut rays, |s| told.push(s));
    assert_eq!(told, [Sensed::Heard(1)]);
    let open = senses.last_heard().map(|heard| heard.strength);
    senses.listen(&me(), &walled, 6, &sounds, &mut rays, |_| {});
    let heard = senses.last_heard().copied();
    assert_eq!(
        heard.map(|heard| (heard.tick, heard.walled)),
        Some((6, true))
    );
    let muffled = heard.map(|heard| heard.strength);
    assert!(
        matches!((open, muffled), (Some(open), Some(muffled)) if (muffled - open * THROUGH_WALL).abs() < 1e-5)
    );
    assert_eq!(senses.last_heard().and_then(|heard| heard.by), Some(0));
}

#[test]
fn it_keeps_the_newest_noises_it_heard() {
    let clear = level(&[floor()]);
    let mut senses = Senses::new(Keenness::default());
    let mut rays = 0;
    for tick in 1..=6 {
        let sound = [shot([0.0, 1.7, -(tick as f32)], None)];
        senses.listen(&me(), &clear, tick, &sound, &mut rays, |_| {});
    }
    let ticks: Vec<u32> = senses.heard().map(|heard| heard.tick).collect();
    assert_eq!(ticks, [6, 5, 4, 3]);
    senses.forget();
    assert_eq!(senses.heard().count(), 0);
}
