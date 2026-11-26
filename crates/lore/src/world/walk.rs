//! Where a person can walk in a world's level: the level's walkable ground
//! (`lore-nav`), worked out from its shapes and the things in it that never
//! move, with every door a gate that opens and shuts with the door.
//!
//! The ground is built on a thread of its own when the level is loaded or
//! saved, so loading does not wait for it. The first question asked of it
//! waits until it is done. Nothing in the simulation reads anything but the
//! finished ground, and the finished ground is the same however long it
//! took, so when it was finished never changes what happens: a game played
//! twice with the same input plays out the same.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;

use lore_nav::{Body, Gate, GateId, Openings, Shapes, Walkable};
use lore_physics::{BodyShape, BodyState, Level, MoveSettings};

use super::things::{Things, turned};
use crate::map::Map;

/// Three corners, in metres.
type Triangle = [[f32; 3]; 3];

/// The body walkable ground is built for: a person who moves as `rules`
/// say, so the ground is where the player could walk.
pub(crate) fn body_of(rules: &MoveSettings) -> Body {
    Body {
        radius: rules.radius,
        height: rules.standing_height,
        crouch_height: rules.crouched_height,
        climb: rules.step_height,
        steepest_slope: rules.steepest_slope,
    }
}

/// Walkable ground, built or being built. Cheap to copy: every copy shares
/// the one ground.
#[derive(Debug, Clone)]
pub(crate) struct Walk(Arc<Slot>);

#[derive(Debug)]
struct Slot {
    built: OnceLock<Walkable>,
    /// The thread building it, until it is waited for.
    worker: Mutex<Option<JoinHandle<Walkable>>>,
    body: Body,
}

/// What walkable ground is built from: a level, and the triangles and
/// doors of the things in it that never move.
struct Job {
    level: Arc<Level>,
    fixed: Vec<Triangle>,
    gates: Vec<Gate>,
    body: Body,
}

impl Job {
    fn run(&self) -> Walkable {
        let mut triangles = self.level.triangles();
        triangles.extend_from_slice(&self.fixed);
        let walkable = Walkable::build(
            &Shapes {
                triangles: &triangles,
                gates: &self.gates,
            },
            self.body,
        );
        for problem in walkable.problems() {
            log::warn!("{problem}");
        }
        walkable
    }
}

impl Walk {
    /// Ground that is already built.
    pub(crate) fn ready(walkable: Walkable) -> Self {
        let body = *walkable.body();
        let built = OnceLock::new();
        let _ = built.set(walkable);
        Self(Arc::new(Slot {
            built,
            worker: Mutex::new(None),
            body,
        }))
    }

    /// Starts building the walkable ground of `level` with `things` in it,
    /// for `body`, on a thread of its own: built here and now if no thread
    /// can be had.
    pub(crate) fn start(level: &Arc<Level>, things: &Things, body: Body) -> Self {
        let (fixed, gates) = round(things);
        let job = Arc::new(Job {
            level: Arc::clone(level),
            fixed,
            gates,
            body,
        });
        let working = Arc::clone(&job);
        let worker = std::thread::Builder::new()
            .name("walkable ground".to_owned())
            .spawn(move || working.run());
        match worker {
            Ok(worker) => Self(Arc::new(Slot {
                built: OnceLock::new(),
                worker: Mutex::new(Some(worker)),
                body,
            })),
            Err(_) => Self::ready(job.run()),
        }
    }

    /// The ground, waiting for it to be built if it is not yet.
    pub(crate) fn get(&self) -> &Walkable {
        self.0.built.get_or_init(|| {
            let worker = (self.0.worker.lock())
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            match worker.map(JoinHandle::join) {
                Some(Ok(walkable)) => walkable,
                Some(Err(_)) => {
                    log::error!(
                        "The walkable ground of the level could not be worked out, so nobody \
                         finds a way across it. This is a mistake in LORE: please report it."
                    );
                    Walkable::none(self.0.body)
                }
                None => Walkable::none(self.0.body),
            }
        })
    }
}

impl lore_script::WalkingGround for Walk {
    fn walkable(&self) -> &Walkable {
        self.get()
    }
}

/// What each door is called, in the order of their gates: its name in
/// `map`, or its id if it has none.
pub(crate) fn door_names(things: &Things, map: &Map) -> Vec<String> {
    things
        .door_ids()
        .map(|id| {
            map.entities()
                .iter()
                .find(|entity| entity.id == id && !entity.name.is_empty())
                .map_or_else(|| id.to_owned(), |entity| entity.name.clone())
        })
        .collect()
}

/// The triangles of the things that never move and the doors among them,
/// in the order the map lists them: each door is the gate of the same
/// number.
fn round(things: &Things) -> (Vec<Triangle>, Vec<Gate>) {
    let mut fixed = Vec::new();
    let mut gates = Vec::new();
    things.each_fixed(|shape, now, door| {
        if door.is_some() {
            gates.push(gate_of(shape, now));
        } else {
            fixed.extend(triangles_of(shape, now));
        }
    });
    (fixed, gates)
}

/// Half the size of the box round `shape`, along its own x, y and z.
fn half_of(shape: BodyShape) -> [f32; 3] {
    match shape {
        BodyShape::Box { half } => half,
        BodyShape::Barrel {
            radius,
            half_height,
        } => [radius, half_height, radius],
        BodyShape::Ball { radius } => [radius; 3],
    }
}

/// The gate of a door of `shape` standing as `now`: where it is when shut.
fn gate_of(shape: BodyShape, now: &BodyState) -> Gate {
    let [x, y, z, w] = now.rotation;
    // The turn about the upright, for something that has not tipped.
    let yaw = libm::atan2f(2.0 * (w * y + x * z), 1.0 - 2.0 * (y * y + x * x));
    Gate::of_box(now.position, half_of(shape), yaw)
}

/// The triangles of the box round `shape`, standing as `now`.
fn triangles_of(shape: BodyShape, now: &BodyState) -> Vec<Triangle> {
    let [hx, hy, hz] = half_of(shape);
    let corner = |x: f32, y: f32, z: f32| {
        let off = turned(now.rotation, [x * hx, y * hy, z * hz]);
        std::array::from_fn(|i| now.position[i] + off[i])
    };
    let c: [[f32; 3]; 8] = std::array::from_fn(|i| {
        let sign = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
        corner(sign(1), sign(2), sign(4))
    });
    // Each face as four corners round it; which way round does not matter.
    [
        [0, 1, 3, 2],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 3, 7, 5],
    ]
    .iter()
    .flat_map(|&[a, b, d, e]| [[c[a], c[b], c[d]], [c[a], c[d], c[e]]])
    .collect()
}

/// Says which gates are open: each door's.
pub(crate) fn follow_doors(openings: &mut Openings, things: &Things) {
    for (index, open) in things.doors_open().enumerate() {
        // Fewer doors than fit in a u32.
        #[allow(clippy::cast_possible_truncation)]
        openings.set_gate(GateId(index as u32), open);
    }
}

/// The kinds of entity that are places someone means to walk to, and are
/// said to be in the wrong place when there is no ground under them.
const PLACE_KINDS: [&str; 2] = ["place", "npc"];

/// How far from a named entity of another kind walkable ground is looked
/// for, across and up or down, in metres: further off, it is something
/// in the air, such as a lamp, and not a place to walk to.
const NEAR_GROUND: [f32; 3] = [1.0, 2.5, 1.0];

/// How far below a spawn point the ground it lands on is looked for.
const FALL: f32 = 40.0;

/// Which named places in `map` cannot be reached on foot from where the
/// player starts (`starts`, where the first is the one used when the
/// player starts on no ground), with every door open: a sentence each for
/// the game's maker, and how many places were looked at.
pub(crate) fn unreachable(
    walkable: &Walkable,
    map: &Map,
    starts: &[(String, [f32; 3])],
) -> (Vec<String>, usize) {
    let open = Openings::all_open();
    let islands = walkable.islands(&open);
    let island_at = |point: [f32; 3], reach: [f32; 3]| {
        walkable
            .nearest_within(point, reach)
            .and_then(|spot| islands.of(walkable, &spot))
    };
    let fall = [1.0, FALL, 1.0];
    let home: Vec<u32> = starts
        .iter()
        .filter_map(|(_, at)| island_at(*at, fall))
        .collect();
    let mut said = Vec::new();
    let mut looked = 0;
    // Places away from the start, a list of names for each island.
    let mut apart: Vec<(u32, Vec<String>)> = Vec::new();
    for entity in map.entities() {
        if entity.name.is_empty() || entity.properties.contains_key("model") {
            continue;
        }
        let place = PLACE_KINDS.contains(&entity.kind.as_str());
        let reach = match entity.kind.as_str() {
            "spawn" => fall,
            _ if place => [2.0; 3],
            _ => NEAR_GROUND,
        };
        let name = &entity.name;
        let [x, y, z] = entity.position;
        let Some(island) = island_at(entity.position, reach) else {
            if place {
                looked += 1;
                said.push(format!(
                    "\"{name}\" in map.toml ({x:.1}, {y:.1}, {z:.1}) is not on ground a person \
                     can stand on: there is none within 2 m of it. Move it onto a floor that \
                     is not too steep (45° at most) and has room to stand (1.8 m)."
                ));
            }
            continue;
        };
        looked += 1;
        if home.is_empty() || home.contains(&island) {
            continue;
        }
        match apart.iter_mut().find(|(i, _)| *i == island) {
            Some((_, names)) => names.push(format!("\"{name}\"")),
            None => apart.push((island, vec![format!("\"{name}\"")])),
        }
    }
    for (_, names) in apart {
        let (who, them) = match names.as_slice() {
            [one] => (one.clone(), "it"),
            [first @ .., last] => (format!("{} and {last}", first.join(", ")), "them"),
            [] => continue,
        };
        let together = if names.len() > 1 {
            " They can be reached from each other."
        } else {
            ""
        };
        said.push(format!(
            "{who} cannot be reached on foot from where the player starts, even with every \
             door open.{together} Nothing joins the ground there to the rest: a slope may be \
             steeper than 45°, a step higher than 0.4 m, or a gap narrower than 0.7 m. Add a \
             ramp or steps up to {them}, or widen the way."
        ));
    }
    (said, looked)
}

#[cfg(test)]
mod tests {
    use lore_assets::{Instance, Mesh, Scene};
    use lore_input::ActionMap;
    use lore_nav::NoWay;

    use super::*;
    use crate::ScriptGame;
    use crate::game::Game;
    use crate::scaffold;
    use crate::world::World;

    /// A level of blocks, each from its west-north-bottom corner to its
    /// east-south-top one.
    fn blocks(boxes: &[([f32; 3], [f32; 3])]) -> Scene {
        let mut scene = Scene::default();
        scene.meshes.push(Mesh {
            name: None,
            parts: vec![crate::standin::block()],
        });
        for &(low, high) in boxes {
            let size: [f32; 3] = std::array::from_fn(|i| high[i] - low[i]);
            let foot = [(low[0] + high[0]) / 2.0, low[1], (low[2] + high[2]) / 2.0];
            scene.instances.push(Instance {
                mesh: 0,
                transform: crate::standin::at(foot, size),
                mirrored: false,
                name: None,
                extras: None,
            });
        }
        scene
    }

    /// A floor 20 m on a side with a wall across it along z = -2, and a
    /// doorway 1 m wide in the wall from x = 0 to 1.
    fn walled() -> Scene {
        blocks(&[
            ([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0]),
            ([-10.0, 0.0, -2.1], [0.0, 3.0, -1.9]),
            ([1.0, 0.0, -2.1], [10.0, 3.0, -1.9]),
        ])
    }

    fn world_of(map: &str, level: &Scene) -> World {
        let map = Map::parse("map.toml", &format!("version = 1\n{map}")).unwrap();
        let actions = ActionMap::standard();
        let game = ScriptGame::from_source("", &actions).with_map(map);
        World::new(game, 60).with_level(level)
    }

    #[test]
    fn a_door_shuts_the_way_through_it_until_it_is_opened() {
        // A door hinged at the doorway's west side, shut across it.
        let world_map = "[[entity]]\nid = \"door\"\ntype = \"prop\"\nname = \"Cellar door\"\n\
                         position = [0.0, 0.0, -2.0]\n[entity.properties]\nmodel = \"door\"\n\
                         opens = true\n";
        let mut world = world_of(world_map, &walled());
        let (here, there) = ([0.5, 0.0, 3.0], [0.5, 0.0, -6.0]);
        let still = lore_input::InputFrame::default();
        world.tick(&still, 1.0 / 60.0);
        match world.walkable().way(here, there, world.openings()) {
            Err(NoWay::Shut { gates, .. }) => assert_eq!(gates, [GateId(0)]),
            other => panic!("{other:?}"),
        }
        // With every door open, the way is through it.
        assert!(
            world
                .walkable()
                .can_reach(here, there, &Openings::all_open())
        );

        assert!(world.things.open("door", true));
        world.tick(&still, 1.0 / 60.0);
        assert!(world.openings().gate_open(GateId(0)));
        let way = world.walkable().way(here, there, world.openings()).unwrap();
        assert!(
            way.points
                .iter()
                .any(|p| p.leg == lore_nav::Leg::Gate(GateId(0)))
        );

        assert!(world.things.open("door", false));
        world.tick(&still, 1.0 / 60.0);
        assert!(!world.walkable().can_reach(here, there, world.openings()));
    }

    #[test]
    fn a_script_asks_whether_it_can_walk_somewhere_and_the_way() {
        let map = Map::parse(
            "map.toml",
            "version = 1\n[[entity]]\nid = \"door\"\ntype = \"prop\"\nname = \"Cellar door\"\n\
             position = [0.0, 0.0, -2.0]\n[entity.properties]\nmodel = \"door\"\nopens = true\n",
        )
        .unwrap();
        let script = r#"
            local here, there = vector.create(0.5, 0, 3), vector.create(0.5, 0, -6)
            local ticks = 0
            game.onTick(function()
                ticks += 1
                if ticks == 2 then
                    assert(not game.canWalk(here, there), "the door is shut")
                    local corners, why = game.findWay(here, there)
                    assert(corners == nil and why == 'the door "Cellar door" is shut: open it first', why)
                    assert(game.canWalk(here, vector.create(-5, 0, 5)))
                    local away, no = game.findWay(here, vector.create(0, 50, 0))
                    assert(away == nil and string.find(no, "the place given as `to` is not on ground", 1, true), no)
                    game.thing("Cellar door"):open()
                elseif ticks == 4 then
                    local corners = game.findWay(here, there)
                    assert(#corners >= 2, "a way through the open door")
                    assert(math.abs(corners[#corners].z + 6) < 0.1)
                    print("walked")
                end
            end)"#;
        let actions = ActionMap::standard();
        let game = ScriptGame::from_source(script, &actions).with_map(map);
        let mut world = World::new(game, 60).with_level(&walled());
        let still = lore_input::InputFrame::default();
        for _ in 0..6 {
            world.tick(&still, 1.0 / 60.0);
        }
        assert_eq!(world.game.host().last_error(), None);
    }

    #[test]
    fn a_fixed_crate_is_walked_round_and_a_loose_one_is_not_part_of_the_ground() {
        let crate_at = |id: &str, x: f32, more: &str| {
            format!(
                "[[entity]]\nid = \"{id}\"\ntype = \"prop\"\nposition = [{x}, 0.0, 0.0]\n\
                 [entity.properties]\nmodel = \"crate\"\n{more}"
            )
        };
        let map = [
            crate_at("fixed", -4.0, ""),
            crate_at("loose", 4.0, "moves = true\n"),
        ]
        .concat();
        let world = world_of(&map, &walled());
        let walkable = world.walkable();
        // Nobody stands inside the fixed crate; the loose one is pushed
        // out of the way, and is not part of the ground.
        let in_fixed = walkable.nearest_within([-4.0, 0.0, 0.0], [0.1, 0.5, 0.1]);
        assert!(in_fixed.is_none(), "{in_fixed:?}");
        let in_loose = walkable.nearest_within([4.0, 0.0, 0.0], [0.1, 0.5, 0.1]);
        assert!(in_loose.is_some());
    }

    #[test]
    fn the_starter_level_gives_the_same_ground_every_time() {
        let level = crate::level::from_bytes("level.glb", scaffold::STARTER_LEVEL).unwrap();
        let one = world_of("", &level);
        let two = world_of("", &level);
        assert!(
            one.walkable().polygon_count() > 20,
            "{}",
            one.walkable().polygon_count()
        );
        assert_eq!(one.walkable().problems(), []);
        assert_eq!(one.walkable().fingerprint(), two.walkable().fingerprint());
        // A level saved again (the same one) builds it again, the same.
        let mut saved = two;
        saved.change_level(&level);
        assert_eq!(one.walkable().fingerprint(), saved.walkable().fingerprint());
        // A world with no level walks a flat floor.
        let flat = World::new(ScriptGame::from_source("", &ActionMap::standard()), 60);
        assert_eq!(flat.walkable().polygon_count(), 1);
    }

    #[test]
    fn places_that_cannot_be_walked_to_are_named() {
        // A block 3 m high in the north-east corner: its top is ground no
        // step reaches.
        let level = blocks(&[
            ([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0]),
            ([4.0, 0.0, -9.0], [9.0, 3.0, -4.0]),
        ]);
        let place = |id: &str, kind: &str, name: &str, at: [f32; 3]| {
            format!(
                "[[entity]]\nid = \"{id}\"\ntype = \"{kind}\"\nname = \"{name}\"\n\
                 position = [{}, {}, {}]\n",
                at[0], at[1], at[2]
            )
        };
        let map = [
            place("a", "spawn", "Start", [0.0, 0.0, 5.0]),
            place("b", "place", "Well", [-5.0, 0.0, 0.0]),
            place("c", "place", "Roof", [6.5, 3.0, -6.5]),
            place("d", "zone", "Chimney", [7.0, 3.0, -5.0]),
            place("e", "place", "Cloud", [0.0, 9.0, 0.0]),
            place("f", "lamp", "Lamp", [0.0, 7.0, 0.0]),
        ]
        .concat();
        let map = Map::parse("map.toml", &format!("version = 1\n{map}")).unwrap();
        let check = crate::world::try_walking(&level, &map);
        assert_eq!(check.places, 5, "{:?}", check.unreachable);
        assert_eq!(check.unreachable.len(), 2, "{:?}", check.unreachable);
        let cloud = &check.unreachable[0];
        assert!(
            cloud.starts_with("\"Cloud\" in map.toml (0.0, 9.0, 0.0) is not on ground"),
            "{cloud}"
        );
        let roof = &check.unreachable[1];
        assert!(
            roof.starts_with("\"Roof\" and \"Chimney\" cannot be reached on foot"),
            "{roof}"
        );
        assert!(
            roof.contains("They can be reached from each other."),
            "{roof}"
        );
        let said = crate::cli::walked(&check);
        assert!(said.contains("Warning: \"Roof\" and \"Chimney\""), "{said}");
    }

    #[test]
    fn a_person_is_the_size_the_player_is() {
        let body = body_of(&MoveSettings::default());
        assert_eq!(body, Body::default());
    }

    #[test]
    fn a_box_is_twelve_triangles_round_its_middle() {
        let state = BodyState {
            position: [1.0, 2.0, 3.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            velocity: [0.0; 3],
            resting: true,
        };
        let triangles = triangles_of(
            BodyShape::Box {
                half: [0.5, 1.0, 2.0],
            },
            &state,
        );
        assert_eq!(triangles.len(), 12);
        for corner in triangles.iter().flatten() {
            assert!((corner[0] - 1.0).abs() == 0.5);
            assert!((corner[1] - 2.0).abs() == 1.0);
            assert!((corner[2] - 3.0).abs() == 2.0);
        }
    }
}
