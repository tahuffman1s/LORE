//! Fluid that stands: pools that hold an amount, take drops in, mix, are
//! pushed up by what is in them, and run out over their rims.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{DT, run};
use lore_assets::{Instance, Mesh, MeshPart, Scene};
use lore_fluid::{Flow, Fluids};
use lore_physics::Level;

type Triangle = [[f32; 3]; 3];

/// A closed box from `min` to `max`.
fn block(min: [f32; 3], max: [f32; 3]) -> Vec<Triangle> {
    let corner = |x: usize, y: usize, z: usize| {
        let pick = |axis: usize, high: usize| if high == 1 { max[axis] } else { min[axis] };
        [pick(0, x), pick(1, y), pick(2, z)]
    };
    let face = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| [[a, b, c], [a, c, d]];
    [
        face(
            corner(0, 0, 0),
            corner(0, 0, 1),
            corner(0, 1, 1),
            corner(0, 1, 0),
        ),
        face(
            corner(1, 0, 1),
            corner(1, 0, 0),
            corner(1, 1, 0),
            corner(1, 1, 1),
        ),
        face(
            corner(0, 0, 0),
            corner(1, 0, 0),
            corner(1, 0, 1),
            corner(0, 0, 1),
        ),
        face(
            corner(0, 1, 1),
            corner(1, 1, 1),
            corner(1, 1, 0),
            corner(0, 1, 0),
        ),
        face(
            corner(1, 0, 0),
            corner(0, 0, 0),
            corner(0, 1, 0),
            corner(1, 1, 0),
        ),
        face(
            corner(0, 0, 1),
            corner(1, 0, 1),
            corner(1, 1, 1),
            corner(0, 1, 1),
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A tub: a hollow 2 m square inside walls 0.2 m thick, standing on
/// `floor` with its middle at `x`. Three walls stand 1 m over its floor
/// and the one towards +x only 0.8 m: that is where it runs over.
fn tub(x: f32, floor: f32) -> Vec<Triangle> {
    let (low, high) = (floor + 0.8, floor + 1.0);
    [
        block([x - 1.2, floor - 0.2, -1.2], [x + 1.2, floor, 1.2]),
        block([x - 1.2, floor, -1.2], [x - 1.0, high, 1.2]),
        block([x + 1.0, floor, -1.2], [x + 1.2, low, 1.2]),
        block([x - 1.0, floor, -1.2], [x + 1.0, high, -1.0]),
        block([x - 1.0, floor, 1.0], [x + 1.0, high, 1.2]),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A level of `solid` triangles with a sheet of still fluid for each of
/// `sheets`: where its middle is, how high it lies, and what its custom
/// properties say.
fn level_with(solid: Vec<Triangle>, sheets: &[([f32; 2], f32, &str)]) -> Level {
    let part = |positions: Vec<[f32; 3]>| MeshPart {
        indices: (0..u32::try_from(positions.len()).unwrap()).collect(),
        positions,
        ..MeshPart::default()
    };
    let stood = |mesh: u32, name: &str, extras: Option<String>| Instance {
        mesh,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        mirrored: false,
        name: Some(name.to_owned()),
        extras,
    };
    let mut meshes = vec![Mesh {
        name: None,
        parts: vec![part(solid.into_iter().flatten().collect())],
    }];
    let mut instances = vec![stood(0, "land", None)];
    for (number, ([x, z], height, extras)) in sheets.iter().enumerate() {
        let sheet = [
            [x - 0.5, *height, z + 0.5],
            [x + 0.5, *height, z + 0.5],
            [x + 0.5, *height, z - 0.5],
            [x - 0.5, *height, z + 0.5],
            [x + 0.5, *height, z - 0.5],
            [x - 0.5, *height, z - 0.5],
        ];
        meshes.push(Mesh {
            name: None,
            parts: vec![part(sheet.to_vec())],
        });
        instances.push(stood(
            u32::try_from(number + 1).unwrap(),
            &format!("pool {number}"),
            Some((*extras).to_owned()),
        ));
    }
    Level::from_scene(&Scene {
        meshes,
        instances,
        ..Scene::default()
    })
}

/// A tub on a wide floor, half full of what `extras` says.
fn tub_level(extras: &str) -> Level {
    let mut solid = block([-12.0, -0.4, -12.0], [12.0, -0.2, 12.0]);
    solid.extend(tub(0.0, 0.0));
    level_with(solid, &[([0.0, 0.0], 0.5, extras)])
}

const HONEY: u8 = 1;
const LAVA: u8 = 2;

/// A world with honey as kind 1 and something that does not freeze as kind
/// 2, and the level's pools as water.
fn world(level: &Level) -> Fluids {
    let mut fluids = Fluids::new(7);
    fluids.set_flow(
        HONEY,
        Flow {
            viscosity: 0.5,
            density: 1.4,
            ..Flow::default()
        },
    );
    fluids.set_flow(
        LAVA,
        Flow {
            viscosity: 0.6,
            density: 2.6,
            freezes: false,
            ..Flow::default()
        },
    );
    fluids.set_pools(level, |_| 0);
    fluids
}

fn close(a: f32, b: f32, within: f32) -> bool {
    (a - b).abs() <= within
}

#[test]
fn a_pool_holds_what_its_hollow_does_and_knows_its_way_out() {
    let level = tub_level(r#"{"water": true}"#);
    let fluids = world(&level);
    let pool = &fluids.pools()[0];
    assert_eq!(pool.name(), Some("pool 0"));
    assert_eq!(pool.kind(), 0);
    // Two metres square and half a metre deep; to the brim, three
    // centimetres under the low wall, 0.77 m.
    assert!(close(pool.litres(), 2000.0, 60.0), "{}", pool.litres());
    assert!(close(pool.capacity(), 3080.0, 90.0), "{}", pool.capacity());
    assert!(close(pool.brim(), 0.77, 0.01), "{}", pool.brim());
    assert_eq!((pool.surface(), pool.shift()), (0.5, 0.0));
    assert!(close(pool.depth(), 0.5, 1.0e-3), "{}", pool.depth());
    // It runs out over the low wall, towards +x, from the wall's height.
    let spill = pool.spill().expect("a way out");
    assert!(spill[0] > 1.2 && spill[0] < 1.5, "{spill:?}");
    assert!(close(spill[1], 0.8, 0.01), "{spill:?}");
    assert!(spill[2].abs() < 1.0, "{spill:?}");
}

#[test]
fn a_sheet_says_what_kind_a_pool_is_and_how_full_it_starts() {
    let level = tub_level(r#"{"water": true, "fluid": "Honey", "amount": 0.5}"#);
    assert_eq!(level.waters()[0].fluid.as_deref(), Some("honey"));
    let mut fluids = Fluids::new(7);
    fluids.set_flow(
        HONEY,
        Flow {
            viscosity: 0.5,
            density: 1.4,
            ..Flow::default()
        },
    );
    fluids.set_pools(&level, |water| match water.fluid.as_deref() {
        Some("honey") => HONEY,
        _ => 0,
    });
    let pool = &fluids.pools()[0];
    assert_eq!(pool.kind(), HONEY);
    assert!(close(pool.litres(), 1000.0, 30.0), "{}", pool.litres());
    assert!(close(pool.surface(), 0.25, 0.01), "{}", pool.surface());
    // The level says the same to whoever wades in.
    let here = level.pool_at(0.0, 0.0).expect("honey in the tub");
    assert!(close(here.surface, 0.25, 0.01), "{here:?}");
    assert!(close(here.medium.density, 1.4, 1.0e-3), "{here:?}");
    assert!(close(here.medium.viscosity, 10.0, 0.01), "{here:?}");

    // An empty basin has nothing in it to wade in, and can be filled.
    let level = tub_level(r#"{"water": true, "amount": 0}"#);
    let mut fluids = world(&level);
    assert_eq!(fluids.pools()[0].litres(), 0.0);
    assert_eq!(level.water_at(0.0, 0.0), None);
    fluids.pool_add(0, HONEY, 400.0);
    fluids.step(&level, DT);
    assert_eq!(fluids.pools()[0].kind(), HONEY);
    assert!(close(level.water_at(0.0, 0.0).unwrap(), 0.1, 0.005));
}

#[test]
fn what_is_poured_into_a_pool_raises_it() {
    let level = tub_level(r#"{"water": true}"#);
    let mut fluids = world(&level);
    let before = fluids.pools()[0].litres();
    // A hose from over the middle, for five seconds: 40 litres.
    for _ in 0..300 {
        fluids.spray(0, [0.0, 2.0, 0.0], [0.0, -3.0, 0.0], 8.0 * DT, 0.05);
        fluids.step(&level, DT);
    }
    run(&mut fluids, &level, 120);
    let pool = &fluids.pools()[0];
    let poured = pool.litres() - before;
    assert!(close(poured + fluids.litres(None), 40.0, 0.5), "{poured}");
    assert!(poured > 39.0, "{poured} litres went in");
    // Forty litres over four square metres is a centimetre.
    assert!(close(pool.surface(), 0.51, 0.002), "{}", pool.surface());
    assert_eq!(level.water_at(0.0, 0.0), Some(pool.surface()));
}

#[test]
fn drops_that_come_down_fast_are_seen_going_in_and_throw_some_back_up() {
    let level = tub_level(r#"{"water": true}"#);
    let mut fluids = world(&level);
    let (mut hits, mut thrown) = (0, 0);
    for _ in 0..90 {
        fluids.spray(0, [0.0, 2.5, 0.0], [0.0, -4.0, 0.0], 0.5 * DT, 0.05);
        fluids.step(&level, DT);
        hits += fluids.pool_hits().len();
        for hit in fluids.pool_hits() {
            assert_eq!(hit.pool, 0);
            assert!(close(hit.place[1], fluids.pools()[0].surface(), 0.01));
            assert!(hit.speed >= 0.5 && hit.litres > 0.0, "{hit:?}");
        }
        thrown += fluids
            .drops()
            .iter()
            .filter(|drop| drop.flat == 0.0 && drop.facing[1] > 0.0)
            .count();
    }
    assert!(hits > 20, "{hits} drops seen going in");
    assert!(thrown > 20, "{thrown} sightings of drops thrown up");
    // All of it comes back down into the tub.
    run(&mut fluids, &level, 240);
    assert!(fluids.litres(None) < 0.01, "{}", fluids.litres(None));
}

#[test]
fn a_pool_filled_past_its_brim_runs_out_over_the_lowest_point_of_its_rim() {
    let level = tub_level(r#"{"water": true}"#);
    let mut fluids = world(&level);
    let capacity = fluids.pools()[0].capacity();
    let before = fluids.pools()[0].litres();
    // 200 litres more than it holds.
    let over = 200.0;
    fluids.pool_add(0, 0, capacity - before + over);
    fluids.step(&level, DT);
    let pool = &fluids.pools()[0];
    assert!(pool.spilled() > 0.0, "it runs out at once");
    assert!(
        close(pool.surface(), pool.brim(), 1.0e-4),
        "it stands at its brim"
    );
    let mut most_spilled = 0.0_f32;
    for _ in 0..8 * 60 {
        fluids.step(&level, DT);
        most_spilled = most_spilled.max(fluids.pools()[0].spilled());
    }
    // No faster than a rim lets it, and then no more.
    assert!(most_spilled <= 40.0 * DT + 1.0e-3, "{most_spilled}");
    let pool = &fluids.pools()[0];
    assert_eq!(pool.spilled(), 0.0);
    assert!(close(pool.litres(), capacity, 1.0), "{}", pool.litres());
    // What ran out lies on the ground outside the low wall, all of it.
    let out = fluids.litres(Some(0));
    assert!(close(out, over, 2.0), "{out} litres outside");
    let drops = fluids.drops();
    assert!(!drops.is_empty());
    // It falls from the low wall and nowhere else, and lies on the ground
    // round the tub: none of it on the walls or back inside.
    for drop in drops {
        let falling = drop.flat == 0.0;
        assert!(
            if falling {
                drop.place[0] > 1.19
            } else {
                drop.place[1] < -0.19
            },
            "{drop:?}"
        );
    }
}

#[test]
fn a_tub_runs_over_into_the_one_below_it() {
    // Two tubs side by side, the first a metre higher: its low wall is
    // over the second's hollow.
    let mut solid = block([-12.0, -0.4, -12.0], [12.0, -0.2, 12.0]);
    solid.extend(tub(0.0, 1.0));
    solid.extend(block([-1.2, -0.2, -1.2], [1.2, 0.8, 1.2]));
    solid.extend(tub(2.2, 0.0));
    let level = level_with(
        solid,
        &[
            ([0.0, 0.0], 1.5, r#"{"water": true}"#),
            ([2.2, 0.0], 0.5, r#"{"water": true, "amount": 0}"#),
        ],
    );
    let mut fluids = world(&level);
    assert_eq!(fluids.pools().len(), 2);
    assert_eq!(fluids.pools()[1].litres(), 0.0);
    let all = |fluids: &Fluids| {
        fluids
            .pools()
            .iter()
            .map(lore_fluid::Pool::litres)
            .sum::<f32>()
            + fluids.litres(None)
    };
    let before = all(&fluids);
    // A tap over the top one, 30 litres a second for a minute: far more
    // than it has room for.
    let mut poured = 0.0;
    for _ in 0..60 * 60 {
        fluids.spray(0, [0.0, 3.0, 0.0], [0.0, -2.0, 0.0], 30.0 * DT, 0.05);
        poured += 30.0 * DT;
        fluids.step(&level, DT);
    }
    run(&mut fluids, &level, 180);
    let (top, below) = (&fluids.pools()[0], &fluids.pools()[1]);
    assert!(close(top.litres(), top.capacity(), 2.0), "{}", top.litres());
    // 1800 litres went in, about 700 more than the top one had room for.
    assert!(
        (600.0..800.0).contains(&below.litres()),
        "{} litres below",
        below.litres()
    );
    assert!(below.surface() > 0.15, "{}", below.surface());
    // And nothing was made or lost on the way.
    assert!(
        close(all(&fluids), before + poured, 5.0),
        "{} against {}",
        all(&fluids),
        before + poured
    );
}

#[test]
fn kinds_mix_in_a_pool_by_their_shares() {
    let level = tub_level(r#"{"water": true, "amount": 0.5}"#);
    let mut fluids = world(&level);
    let water = fluids.pools()[0].litres();
    fluids.pool_add(0, HONEY, water / 3.0);
    fluids.step(&level, DT);
    let pool = &fluids.pools()[0];
    assert_eq!(pool.kind(), 0, "still mostly water");
    assert!(
        close(pool.share(HONEY), 0.25, 1.0e-3),
        "{}",
        pool.share(HONEY)
    );
    assert!(close(pool.shares().iter().sum::<f32>(), 1.0, 1.0e-4));
    // A quarter honey: a tenth heavier, and ten times as thick (honey is
    // ten thousand times as thick, and thicknesses mix by their powers).
    let medium = pool.medium();
    assert!(close(medium.density, 1.1, 1.0e-3), "{medium:?}");
    assert!(close(medium.viscosity, 0.01, 1.0e-3), "{medium:?}");
    assert_eq!(level.pool_at(0.0, 0.0).unwrap().medium, medium);
    // More honey than water, and it is a pool of honey.
    fluids.pool_add(0, HONEY, water);
    fluids.step(&level, DT);
    assert_eq!(fluids.pools()[0].kind(), HONEY);
    // Taking some out takes of each by its share.
    let share = fluids.pools()[0].share(HONEY);
    let taken = fluids.pool_take(0, 500.0);
    assert_eq!(taken, 500.0);
    assert!(close(fluids.pools()[0].share(HONEY), share, 1.0e-4));
    let left = fluids.pools()[0].litres();
    assert_eq!(fluids.pool_take(0, 1.0e9), left);
    assert_eq!(fluids.pools()[0].litres(), 0.0);
    // Said to be one kind, all of it is.
    fluids.pool_add(0, 0, 300.0);
    fluids.pool_add(0, HONEY, 100.0);
    fluids.pool_set_kind(0, LAVA);
    let pool = &fluids.pools()[0];
    assert_eq!((pool.kind(), pool.share(LAVA)), (LAVA, 1.0));
    assert!(close(pool.litres(), 400.0, 0.01), "{}", pool.litres());
}

#[test]
fn a_body_in_a_pool_pushes_its_surface_up_and_a_full_one_over() {
    let level = tub_level(r#"{"water": true}"#);
    let mut fluids = world(&level);
    fluids.pool_displace(0, 80.0);
    fluids.step(&level, DT);
    // Eighty litres over four square metres: two centimetres.
    assert!(close(fluids.pools()[0].surface(), 0.52, 1.0e-3));
    assert!(
        close(fluids.pools()[0].litres(), 2000.0, 60.0),
        "nothing was added"
    );
    fluids.pool_displace(0, 0.0);
    fluids.step(&level, DT);
    assert_eq!(fluids.pools()[0].surface(), 0.5);
    // Into a tub that is full to its brim: what they push aside runs out,
    // and when they are out again it stands that much lower.
    let room = fluids.pools()[0].capacity() - fluids.pools()[0].litres();
    fluids.pool_add(0, 0, room);
    fluids.pool_displace(0, 80.0);
    run(&mut fluids, &level, 4 * 60);
    assert!(
        close(fluids.litres(None), 80.0, 1.0),
        "{}",
        fluids.litres(None)
    );
    fluids.pool_displace(0, 0.0);
    fluids.step(&level, DT);
    let pool = &fluids.pools()[0];
    assert!(
        close(pool.brim() - pool.surface(), 0.02, 2.0e-3),
        "{}",
        pool.surface()
    );
}

#[test]
fn a_pool_goes_to_the_height_asked_over_the_seconds_given() {
    let level = tub_level(r#"{"water": true}"#);
    let mut fluids = world(&level);
    fluids.pool_level_to(0, 0.1, 2.0);
    run(&mut fluids, &level, 60);
    assert!(close(fluids.pools()[0].surface(), 0.3, 0.005), "half way");
    run(&mut fluids, &level, 61);
    assert!(close(fluids.pools()[0].surface(), 0.1, 1.0e-4));
    assert!(close(fluids.pools()[0].litres(), 400.0, 15.0));
    run(&mut fluids, &level, 60);
    assert!(
        close(fluids.pools()[0].surface(), 0.1, 1.0e-4),
        "and there it stays"
    );
    // At once, and no higher than the brim or lower than empty.
    fluids.pool_level_to(0, 9.0, 0.0);
    fluids.step(&level, DT);
    let pool = &fluids.pools()[0];
    assert!(
        close(pool.surface(), pool.brim(), 1.0e-4),
        "{}",
        pool.surface()
    );
    assert_eq!(pool.spilled(), 0.0, "full is not over");
    fluids.pool_level_to(0, -9.0, 0.0);
    fluids.step(&level, DT);
    assert_eq!(fluids.pools()[0].litres(), 0.0);
    assert_eq!(level.water_at(0.0, 0.0), None);
    // What is no pool or no height is nothing.
    fluids.pool_level_to(0, f32::NAN, 1.0);
    fluids.pool_level_to(7, 1.0, 1.0);
    fluids.pool_add(7, 0, 1.0);
    assert_eq!(fluids.pool_take(7, 1.0), 0.0);
}

#[test]
fn rain_raises_water_and_ice_covers_what_freezes() {
    let level = tub_level(r#"{"water": true}"#);
    let mut fluids = world(&level);
    fluids.set_weather(0.1, 0.0);
    fluids.step(&level, DT);
    assert!(close(fluids.pools()[0].surface(), 0.6, 1.0e-4));
    assert!(
        close(fluids.pools()[0].litres(), 2000.0, 60.0),
        "rain is not poured in"
    );
    // No higher than the brim: there it runs out for as long as it rains.
    fluids.set_weather(0.6, 0.0);
    fluids.step(&level, DT);
    let pool = &fluids.pools()[0];
    assert!(
        close(pool.surface(), pool.brim(), 1.0e-4),
        "{}",
        pool.surface()
    );
    assert!(pool.spilled() > 0.0, "the rain that fell on it runs over");
    fluids.step(&level, DT);
    assert_eq!(fluids.pools()[0].spilled(), 0.0, "it has stopped raining");
    // Ice bears on water.
    fluids.set_weather(0.0, 0.2);
    fluids.step(&level, DT);
    assert!(fluids.pools()[0].frozen());
    assert_eq!(level.water_at(0.0, 0.0), None);
    assert_eq!(level.ice_around([0.0, 0.5, 0.0]), Some(0.5));
    // Something that does not freeze stays open, and rain does not fill it.
    fluids.pool_set_kind(0, LAVA);
    fluids.set_weather(0.2, 0.2);
    fluids.step(&level, DT);
    assert!(!fluids.pools()[0].frozen());
    assert_eq!(level.water_at(0.0, 0.0), Some(0.5));
}

#[test]
fn a_splash_throws_drops_of_the_pool_out_of_it() {
    let level = tub_level(r#"{"water": true, "fluid": "honey"}"#);
    let mut fluids = world(&level);
    fluids.pool_set_kind(0, HONEY);
    let before = fluids.pools()[0].litres();
    fluids.pool_splash(0, [0.2, 0.0, 0.1], 3.0, 5.0);
    let thrown = fluids.litres(Some(HONEY));
    assert!(close(thrown, 3.0, 1.0e-3), "{thrown}");
    assert!(close(fluids.pools()[0].litres(), before - 3.0, 0.01));
    fluids.step(&level, DT);
    let drops = fluids.drops();
    assert!(drops.len() >= 3, "{}", drops.len());
    for drop in drops {
        assert_eq!(drop.kind, HONEY);
        assert!(drop.place[1] > 0.5, "thrown up from the surface: {drop:?}");
    }
    // Some lands back in the tub and some over its walls: none is lost.
    run(&mut fluids, &level, 300);
    let all = fluids.pools()[0].litres() + fluids.litres(None);
    assert!(close(all, before, 0.05), "{all} against {before}");
    // Nothing is thrown out of nothing, or of a frozen pool.
    fluids.pool_splash(0, [f32::NAN, 0.0, 0.0], 3.0, 5.0);
    fluids.pool_splash(3, [0.0; 3], 3.0, 5.0);
    fluids.pool_splash(0, [0.0; 3], -1.0, 5.0);
    let all_again = fluids.pools()[0].litres() + fluids.litres(None);
    assert_eq!(all, all_again);
}

#[test]
fn pools_play_out_the_same_every_time() {
    let played = || {
        let level = tub_level(r#"{"water": true}"#);
        let mut fluids = world(&level);
        fluids.pool_add(0, HONEY, 1300.0);
        for tick in 0..400 {
            fluids.spray(0, [0.3, 2.0, 0.0], [0.0, -3.0, 0.2], 2.0 * DT, 0.2);
            if tick == 100 {
                fluids.pool_splash(0, [0.0, 0.0, 0.0], 4.0, 6.0);
            }
            fluids.step(&level, DT);
        }
        let pool = &fluids.pools()[0];
        (
            pool.litres().to_bits(),
            pool.surface().to_bits(),
            pool.shares().map(f32::to_bits),
            fluids.drops().to_vec(),
        )
    };
    let (first, second) = (played(), played());
    assert!(!first.3.is_empty());
    assert_eq!(first, second);
}
