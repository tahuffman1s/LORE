//! What the player walks on and where they start: the level's solid shapes,
//! its spawn points and the rules of movement.

use std::f32::consts::TAU;
use std::sync::Arc;

use lore_assets::{Marker, Scene};
use lore_physics::{Level, MoveInput, MoveSettings, Player};

use super::spawn;
use crate::map::{MAP_FILE, Map};
use crate::scaffold::LEVEL_FILE;

/// Half the width of the floor the player stands on until a game has a level.
const FLOOR_REACH: f32 = 500.0;

/// What a marker in a level is called to be a place the player starts: `spawn`,
/// or anything that begins with it, such as `spawn.001` or `Spawn Red`.
const SPAWN_NAME: &str = "spawn";

/// How long a player started at a spawn point is given to land when a level
/// is tried out, in ticks: three seconds, a fall of some 40 m.
const LANDING_TICKS: u32 = 180;

/// A player started where the editor's camera is stands at least this far
/// above the floor under it, in metres, so the first step settles them on it.
#[cfg(feature = "editor")]
const STEP_CLEAR: f32 = 0.01;

/// A player who has just been put in the world is this near a spawn point,
/// in metres across the ground, if that is where they started.
const AT_SPAWN: f32 = 0.5;

/// The ground of a world: its solid shapes, where the player starts on it and
/// how they move over it.
///
/// Cheap to copy: the level's shapes are shared, not copied.
#[derive(Debug, Clone)]
pub(crate) struct Ground {
    pub(super) level: Arc<Level>,
    /// Where the player may start. Never empty.
    spawns: Arc<[[f32; 3]]>,
    /// Which way a player starting at each of `spawns` faces.
    facing: Arc<[f32]>,
    pub(super) rules: MoveSettings,
    /// Ticks in one second.
    pub(super) tick_rate: u32,
    /// Seconds in one tick.
    pub(super) step: f32,
}

impl Ground {
    /// A flat floor, 1 km on a side, at height zero, with the player
    /// starting at its middle.
    pub(super) fn flat(tick_rate: u32) -> Self {
        let r = FLOOR_REACH;
        let corners = [[-r, 0.0, -r], [r, 0.0, -r], [r, 0.0, r], [-r, 0.0, r]];
        let [a, b, c, d] = corners;
        Self::new(
            Level::from_triangles(&[[a, b, c], [a, c, d]]),
            &[],
            tick_rate,
        )
    }

    /// The ground of a game's level: every shape in it is solid, and the
    /// player starts at its spawn markers, or at its middle if it has none.
    fn from_level(level: &Scene, tick_rate: u32) -> Self {
        let spawns: Vec<&Marker> = spawn_markers(level).collect();
        Self::new(Level::from_scene(level), &spawns, tick_rate)
    }

    fn new(level: Level, markers: &[&Marker], tick_rate: u32) -> Self {
        let mut spawns: Vec<[f32; 3]> = markers.iter().map(|marker| position_of(marker)).collect();
        let mut facing: Vec<f32> = markers.iter().map(|marker| yaw_of(marker)).collect();
        if spawns.is_empty() {
            spawns.push([0.0; 3]);
            facing.push(0.0);
        }
        Self {
            level: Arc::new(level),
            spawns: spawns.into(),
            facing: facing.into(),
            rules: MoveSettings::default(),
            tick_rate,
            step: 1.0 / tick_rate.max(1) as f32,
        }
    }

    /// The ground of a game with `level`, or the flat floor of a game with
    /// none. What is wrong for walking with the level and the spawn points
    /// in `map` is logged.
    pub(super) fn of(level: Option<&Scene>, map: &Map, tick_rate: u32) -> Self {
        let Some(level) = level else {
            return Self::flat(tick_rate);
        };
        let ground = Self::from_level(level, tick_rate);
        for problem in ground.problems(level, map) {
            log::warn!("{problem}");
        }
        ground
    }

    /// The player, about to be put in the world. Where they start is one
    /// rule everywhere: at the spawn points in `map` if it has any, else
    /// at the level's spawn markers, else in the middle of the level. Of
    /// those the one furthest from `others`, the feet of whoever else
    /// stands in the world, is taken, or one nearly as good; `seed`
    /// settles the choice and should differ from one call to the next.
    pub(super) fn spawn_in(
        &self,
        map: &Map,
        others: impl Iterator<Item = [f32; 3]>,
        seed: u32,
    ) -> Player {
        let standing: Vec<[f32; 3]> = others.collect();
        let placed: Vec<[f32; 3]> = map.spawns().map(|spawn| spawn.position).collect();
        let points: &[[f32; 3]] = if placed.is_empty() {
            &self.spawns
        } else {
            &placed
        };
        let chosen = spawn::choose(points, &standing, seed)
            .and_then(|point| points.get(point))
            .map_or([0.0; 3], |point| *point);
        Player::at(chosen, &self.rules)
    }

    /// The ground the editor plays a game on: `level`'s, or the flat floor of
    /// a game with none. Made once and used for every play, with whatever
    /// map the editor then holds, so nothing is said here about its spawn
    /// points.
    #[cfg(feature = "editor")]
    pub(crate) fn for_editor(level: Option<&Scene>, tick_rate: u32) -> Self {
        level.map_or_else(
            || Self::flat(tick_rate),
            |level| Self::from_level(level, tick_rate),
        )
    }

    /// Where the feet of a player go whose eyes are at `eye`, if there is
    /// anything under them to land on: a standing player's height below
    /// the eyes, and never under the floor.
    #[cfg(feature = "editor")]
    pub(super) fn feet_under(&self, eye: [f32; 3]) -> Option<[f32; 3]> {
        let floor = self.level.raycast(eye, [0.0, -1.0, 0.0], f32::MAX)?.point[1];
        let feet = (eye[1] - self.rules.standing_eye_height).max(floor + STEP_CLEAR);
        Some([eye[0], feet, eye[2]])
    }

    /// Which way a player who has just started at `position` faces, if that
    /// is one of the spawn points: `map`'s if it has any, else the level's.
    pub(super) fn facing_at(&self, map: &Map, position: [f32; 3]) -> Option<f32> {
        // `libm`, not the platform's maths: where the player starts, and
        // the way they face there, must be the same on every machine.
        let near = |point: [f32; 3]| {
            libm::hypotf(point[0] - position[0], point[2] - position[2]) <= AT_SPAWN
        };
        if map.spawns().next().is_some() {
            return map
                .spawns()
                .find(|spawn| near(spawn.position))
                .map(|spawn| spawn.facing.to_radians().rem_euclid(TAU))
                .filter(|yaw| yaw.is_finite());
        }
        self.spawns
            .iter()
            .zip(self.facing.iter())
            .find(|(point, _)| near(**point))
            .map(|(_, yaw)| *yaw)
    }

    /// What is wrong for walking with `level`, which this ground was made
    /// from, and the spawn points in `map`: nothing to stand on, nowhere to
    /// start, or a start that a player falls from. Each is a sentence for
    /// the game's maker.
    fn problems(&self, level: &Scene, map: &Map) -> Vec<String> {
        if self.level.triangle_count() == 0 {
            return vec![format!(
                "{LEVEL_FILE} has no shapes in it, so there is nothing to stand on and the player \
                 falls for ever. Add a floor in your 3D tool and export the level again."
            )];
        }
        let mut problems = Vec::new();
        let starts = starts(level, map);
        if starts.is_empty() {
            problems.push(format!(
                "This game has no spawn points, so the player starts at the level's middle \
                 (0, 0, 0). To choose where they start, run `lore edit` and use \"Place a \
                 spawn here\", which writes {MAP_FILE}; or add an empty object named `spawn` \
                 to the level in your 3D tool, turned the way the player should look, and \
                 export it again."
            ));
        }
        let middle = [("the level's middle".to_owned(), [0.0; 3])];
        let starts = if starts.is_empty() {
            &middle[..]
        } else {
            &starts[..]
        };
        let still = MoveInput::default();
        for (place, position) in starts {
            let mut player = Player::at(*position, &self.rules);
            for _ in 0..LANDING_TICKS {
                player.step(&self.level, &self.rules, &still, self.step);
                if player.grounded {
                    break;
                }
            }
            if player.grounded {
                continue;
            }
            let [x, y, z] = position;
            let seconds = LANDING_TICKS as f32 * self.step;
            problems.push(format!(
                "A player who starts at {place} ({x:.1}, {y:.1}, {z:.1}) falls and has not \
                 landed after {seconds:.0} seconds: there is no floor under it. Move it to \
                 just above a floor, or put a floor under it."
            ));
        }
        problems
    }
}

/// Where the player starts in a game with `level` and `map`, each with what to
/// call it: the spawn points in the map if it has any, else the level's
/// spawn markers. Empty when the game has neither.
fn starts(level: &Scene, map: &Map) -> Vec<(String, [f32; 3])> {
    let placed: Vec<_> = map
        .spawns()
        .map(|spawn| {
            let name = if spawn.name.is_empty() {
                &spawn.id
            } else {
                &spawn.name
            };
            (
                format!("the spawn point \"{name}\" in {MAP_FILE}"),
                spawn.position,
            )
        })
        .collect();
    if !placed.is_empty() {
        return placed;
    }
    spawn_markers(level)
        .map(|marker| {
            (
                format!(
                    "the spawn point \"{}\" in {LEVEL_FILE}",
                    marker.name.as_deref().unwrap_or_default()
                ),
                position_of(marker),
            )
        })
        .collect()
}

/// The markers in `level` that are places for the player to start.
fn spawn_markers(level: &Scene) -> impl Iterator<Item = &Marker> {
    level.markers.iter().filter(|marker| {
        marker.name.as_deref().is_some_and(|name| {
            name.trim_start()
                .get(..SPAWN_NAME.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(SPAWN_NAME))
        })
    })
}

/// Where `marker` stands.
fn position_of(marker: &Marker) -> [f32; 3] {
    let [x, y, z, _] = marker.transform[3];
    [x, y, z]
}

/// Which way `marker` faces across the ground, as a player's yaw: 0 faces
/// -Z and it grows turning left. A marker looks along its own -Z, as a
/// camera in a glTF file does; one pointing straight up or down faces -Z.
fn yaw_of(marker: &Marker) -> f32 {
    let [x, _, z, _] = marker.transform[2];
    // The marker's -Z is (-x, -z); yaw is the turn left from -Z to it.
    if x.is_finite() && z.is_finite() && libm::hypotf(x, z) > 1.0e-6 {
        libm::atan2f(x, z).rem_euclid(TAU)
    } else {
        0.0
    }
}

/// What trying a level out for walking found: see [`try_walking`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkCheck {
    /// How many places for the player to start the game has: the `spawn`
    /// entities in its map file or, when that has none, the level's
    /// markers whose names begin with `spawn`.
    pub spawn_points: usize,
    /// What its maker should change, a sentence each. Empty when a player
    /// can start at every spawn point and land on something.
    pub problems: Vec<String>,
}

/// Tries `level` out for walking, with no window: starts a
/// player at each spawn point, lets them stand for three seconds and says
/// which ones never landed. `lore check` prints what it finds.
///
/// The spawn points are the `spawn` entities in `map`
/// ([`ScriptGame::map`](crate::ScriptGame::map)) if it has any, else the
/// level's markers whose names begin with `spawn`, else the middle of the
/// level.
#[must_use]
pub fn try_walking(level: &Scene, map: &Map) -> WalkCheck {
    let ground = Ground::from_level(level, 60);
    WalkCheck {
        spawn_points: starts(level, map).len(),
        problems: ground.problems(level, map),
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    /// A marker called `name` at `position`, turned `yaw` radians to the left.
    fn marker(name: &str, position: [f32; 3], yaw: f32) -> Marker {
        let (sin, cos) = (libm::sinf(yaw), libm::cosf(yaw));
        let [x, y, z] = position;
        Marker {
            name: Some(name.to_owned()),
            transform: [
                [cos, 0.0, -sin, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [sin, 0.0, cos, 0.0],
                [x, y, z, 1.0],
            ],
            extras: None,
        }
    }

    /// A level that is a floor 20 m on a side at height zero, with `markers`.
    fn floor_with(markers: Vec<Marker>) -> Scene {
        use lore_assets::{Instance, Mesh, MeshPart};

        let mut scene = Scene::default();
        scene.meshes.push(Mesh {
            name: None,
            parts: vec![MeshPart {
                positions: vec![
                    [-10.0, 0.0, -10.0],
                    [10.0, 0.0, -10.0],
                    [10.0, 0.0, 10.0],
                    [-10.0, 0.0, 10.0],
                ],
                indices: vec![0, 2, 1, 0, 3, 2],
                ..MeshPart::default()
            }],
        });
        scene.instances.push(Instance {
            mesh: 0,
            transform: marker("", [0.0; 3], 0.0).transform,
            mirrored: false,
            name: None,
            extras: None,
        });
        scene.markers = markers;
        scene
    }

    #[test]
    fn markers_whose_names_begin_with_spawn_are_where_players_start() {
        let level = floor_with(vec![
            marker("Camera Spot", [1.0, 0.0, 1.0], 0.0),
            marker("spawn", [2.0, 0.0, 3.0], 0.0),
            marker("Spawn Red", [-4.0, 0.5, 0.0], FRAC_PI_2),
            marker("respawn timer", [5.0, 0.0, 5.0], 0.0),
            marker("spawn.001", [0.0, 0.0, -6.0], TAU / 2.0),
        ]);
        let ground = Ground::from_level(&level, 60);

        let places: Vec<_> = ground.spawns.to_vec();
        assert_eq!(
            places,
            [[2.0, 0.0, 3.0], [-4.0, 0.5, 0.0], [0.0, 0.0, -6.0]]
        );
        // A player looks the way the marker is turned: left is west (-X).
        assert_eq!(
            ground.facing_at(&Map::default(), [2.0, 0.01, 3.1]),
            Some(0.0)
        );
        let west = ground.facing_at(&Map::default(), [-4.0, 0.5, 0.0]).unwrap();
        assert!((west - FRAC_PI_2).abs() < 1.0e-6, "{west}");
        let south = ground.facing_at(&Map::default(), [0.0, 0.0, -6.0]).unwrap();
        assert!((south - TAU / 2.0).abs() < 1.0e-6, "{south}");
        assert_eq!(ground.facing_at(&Map::default(), [7.0, 0.0, 7.0]), None);
        assert_eq!(try_walking(&level, &Map::default()).spawn_points, 3);
        assert_eq!(
            try_walking(&level, &Map::default()).problems,
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_player_is_put_at_a_spawn_point_away_from_whoever_stands_on_one() {
        let level = floor_with(vec![
            marker("spawn", [-9.0, 0.0, 0.0], 0.0),
            marker("spawn.001", [9.0, 0.0, 0.0], 0.0),
        ]);
        let ground = Ground::from_level(&level, 60);
        for seed in 0..20 {
            let first = ground.spawn_in(&Map::default(), std::iter::empty(), seed);
            assert_eq!(first.position[0].abs(), 9.0);
            let taken = || std::iter::once(first.position);
            let second = ground.spawn_in(&Map::default(), taken(), seed);
            assert_eq!(second.position[0], -first.position[0], "seed {seed}");
            // The same question has the same answer.
            assert_eq!(ground.spawn_in(&Map::default(), taken(), seed), second);
        }
    }

    #[test]
    fn a_level_with_no_spawn_points_starts_players_at_its_middle_and_says_so() {
        let level = floor_with(vec![marker("Camera Spot", [1.0, 0.0, 1.0], 0.0)]);
        let ground = Ground::from_level(&level, 60);
        assert_eq!(
            ground
                .spawn_in(&Map::default(), std::iter::empty(), 7)
                .position,
            [0.0; 3]
        );

        let walked = try_walking(&level, &Map::default());
        assert_eq!(walked.spawn_points, 0);
        assert_eq!(walked.problems.len(), 1, "{:?}", walked.problems);
        let said = &walked.problems[0];
        assert!(
            said.starts_with(
                "This game has no spawn points, so the player starts at the level's middle"
            ),
            "{said}"
        );
        assert!(said.contains("an empty object named `spawn`"), "{said}");
    }

    #[test]
    fn a_spawn_point_with_no_floor_under_it_is_named() {
        let level = floor_with(vec![
            marker("spawn", [0.0, 2.0, 0.0], 0.0),
            marker("spawn.ledge", [30.0, 0.0, 0.5], 0.0),
        ]);
        let walked = try_walking(&level, &Map::default());
        assert_eq!(walked.problems.len(), 1, "{:?}", walked.problems);
        let said = &walked.problems[0];
        assert!(
            said.starts_with(
                "A player who starts at the spawn point \"spawn.ledge\" in level.glb \
                 (30.0, 0.0, 0.5) falls and has not landed after 3 seconds"
            ),
            "{said}"
        );
        assert!(said.contains("put a floor under it"), "{said}");
    }

    #[test]
    fn spawn_points_in_the_map_file_are_used_instead_of_the_levels_markers() {
        let level = floor_with(vec![marker("spawn", [-9.0, 0.0, 0.0], 0.0)]);
        let ground = Ground::from_level(&level, 60);
        let map = Map::parse(
            "map.toml",
            "version = 1\n\n\
             [[entity]]\nid = \"aaaa\"\ntype = \"spawn\"\nname = \"Blue spawn\"\n\
             position = [5.0, 0.0, 5.0]\nfacing = 90.0\n\n\
             [[entity]]\nid = \"bbbb\"\ntype = \"spawn\"\nposition = [50.0, 0.0, 0.0]\n\n\
             [[entity]]\nid = \"cccc\"\ntype = \"zone\"\nposition = [1.0, 0.0, 1.0]\n",
        )
        .unwrap();

        // With an empty map the level's marker is where the player starts.
        let empty = Map::default();
        assert_eq!(
            ground.spawn_in(&empty, std::iter::empty(), 3).position,
            [-9.0, 0.0, 0.0]
        );
        assert_eq!(ground.facing_at(&empty, [-9.0, 0.0, 0.0]), Some(0.0));

        // With spawn points in the map, only they are.
        for seed in 0..20 {
            let x = ground.spawn_in(&map, std::iter::empty(), seed).position[0];
            assert!(x == 5.0 || x == 50.0, "{x}");
        }
        let west = ground.facing_at(&map, [5.0, 0.0, 5.2]).unwrap();
        assert!((west - FRAC_PI_2).abs() < 1.0e-6, "{west}");
        assert_eq!(ground.facing_at(&map, [-9.0, 0.0, 0.0]), None);

        // The walk is tried from the map's points, and the one off the
        // floor is named by its file; one with no name goes by its id.
        let walked = try_walking(&level, &map);
        assert_eq!(walked.spawn_points, 2);
        assert_eq!(walked.problems.len(), 1, "{:?}", walked.problems);
        assert!(
            walked.problems[0].starts_with(
                "A player who starts at the spawn point \"bbbb\" in map.toml (50.0, 0.0, 0.0) \
                 falls"
            ),
            "{}",
            walked.problems[0]
        );
    }

    #[test]
    fn a_level_with_nothing_in_it_has_nothing_to_stand_on() {
        let mut level = Scene::default();
        level.markers.push(marker("spawn", [0.0; 3], 0.0));
        let walked = try_walking(&level, &Map::default());
        assert_eq!(walked.problems.len(), 1, "{:?}", walked.problems);
        assert!(
            walked.problems[0].starts_with("level.glb has no shapes in it"),
            "{}",
            walked.problems[0]
        );
    }
}
