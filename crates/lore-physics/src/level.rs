//! The level's triangles, and the few questions the player asks of them.

use lore_assets::{Scene, Transform, Water};
use parry3d::glamx::Vec3;
use parry3d::math::{Pose, Vector as Wide};
use parry3d::query::{self, Ray, RayCast, ShapeCastOptions};
use parry3d::shape::{Capsule, TriMesh};

/// A triangle thinner than this (twice its area, in square metres) is left
/// out: it has no direction to push the player in.
const THINNEST_TRIANGLE: f32 = 1.0e-10;

/// A place or a direction as the player's movement works with it.
///
/// Movement is single precision, like the positions it starts from and
/// ends at. The questions below are worked out in double precision
/// (`Wide`) and their answers handed back in single: a triangle may be a
/// kilometre across and the gap to it a centimetre, and in single
/// precision that gap came out wrong by up to 18 cm.
pub(crate) type Vector = Vec3;

/// The longest a ray is followed, in metres, however far was asked for.
const FARTHEST_RAY: f32 = 100_000.0;

/// What [`Level::raycast`] found in the way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// How far from the start, in metres.
    pub distance: f32,
    /// Where the surface was met.
    pub point: [f32; 3],
    /// Which way the surface faces there, one unit long and towards the
    /// side the ray came from.
    pub normal: [f32; 3],
}

/// The solid shapes of a level, ready for the player to collide with.
///
/// Every triangle of every placed mesh is solid, from both sides, unless the
/// placement says it is not ([`Instance::is_solid`](lore_assets::Instance::is_solid)). Build it
/// once when the level loads; it does not change afterwards, but for how
/// high rain has raised its water ([`set_water_rise`](Self::set_water_rise))
/// and how thick the ice on it is ([`set_ice`](Self::set_ice)).
#[derive(Clone)]
pub struct Level {
    /// `None` when there are no triangles, so nothing is ever hit.
    mesh: Option<TriMesh>,
    triangles: usize,
    /// The still water in the level: lakes and ponds, each in the shape
    /// it took when it filled its hollow.
    waters: Vec<Water>,
    /// How far rain has raised that water above where it was put.
    rise: Rise,
    /// How thick the ice on that water is.
    ice: Rise,
}

/// A height in metres that can be set through a level many hold: the level's
/// shapes are shared, not copied, and whoever steps a world says how high
/// its water stands, and how thick its ice is, before each step.
#[derive(Default)]
struct Rise(std::sync::atomic::AtomicU32);

impl Rise {
    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(std::sync::atomic::Ordering::Relaxed))
    }
}

impl Clone for Rise {
    fn clone(&self) -> Self {
        Self(std::sync::atomic::AtomicU32::new(self.get().to_bits()))
    }
}

impl std::fmt::Debug for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Level")
            .field("triangles", &self.triangles)
            .finish()
    }
}

impl Level {
    /// How thick ice must be to bear someone walking on it, in metres: five
    /// centimetres, which is where a careful person starts to trust it.
    pub const ICE_BEARS: f32 = 0.05;

    /// The collision shapes of a loaded scene: every mesh, where it stands,
    /// but for those marked as not solid.
    ///
    /// A scene with no meshes gives an empty level, in which a player falls
    /// forever; check [`Level::triangle_count`] to warn about that.
    #[must_use]
    pub fn from_scene(scene: &Scene) -> Self {
        let mut level = Self::solids(scene);
        // Water that lies level is a lake or a pond, to be waded and swum
        // in: it fills the hollow it is over. A scene that was filled
        // when it was loaded says where; one that was not is filled here.
        // Water that slopes is a stream: too shallow to matter here.
        level.waters = if scene.waters.is_empty() {
            crate::water::filled(&level, scene).0
        } else {
            scene.waters.clone()
        };
        level
    }

    /// The solid shapes of `scene`, with no water in them yet.
    pub(crate) fn solids(scene: &Scene) -> Self {
        let mut triangles = Vec::new();
        for instance in scene
            .instances
            .iter()
            .filter(|instance| instance.is_solid())
        {
            let Some(mesh) = scene.meshes.get(instance.mesh as usize) else {
                continue;
            };
            for part in &mesh.parts {
                // Both sides of a triangle are solid, so a mirrored
                // placement, which turns triangles inside out, needs no care.
                triangles.extend(
                    part.triangles()
                        .map(|corners| corners.map(|point| place(&instance.transform, point))),
                );
            }
        }
        Self::from_triangles(&triangles)
    }

    /// How high the surface of the still water over `x`, `z` is, if there
    /// is any there: a lake or a pond that the level marks as water.
    #[must_use]
    pub fn water_at(&self, x: f32, z: f32) -> Option<f32> {
        if self.frozen() {
            return None;
        }
        let rise = self.rise.get();
        self.waters
            .iter()
            .filter(|water| water.depth_at(x, z) + rise > 0.0)
            .map(|water| water.level + rise)
            .reduce(f32::max)
    }

    /// Says that rain has raised the level's still water `rise` metres
    /// above where its maker put it. Each lake and pond stands that much
    /// higher, and spreads over whatever ground beside it is lower than
    /// that: a body of water knows how far above it the ground round it
    /// stands. Less than nothing, or what is no number, is nothing.
    ///
    /// The level's shapes are shared by everyone who holds it, and so is
    /// this: a world says it before each step it takes.
    pub fn set_water_rise(&self, rise: f32) {
        let rise = if rise.is_finite() { rise.max(0.0) } else { 0.0 };
        (self.rise.0).store(rise.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// How far rain has raised the level's still water, in metres.
    #[must_use]
    pub fn water_rise(&self) -> f32 {
        self.rise.get()
    }

    /// Says how thick the ice on the level's still water is, in metres.
    /// Once it is [`ICE_BEARS`](Self::ICE_BEARS) thick it bears whoever
    /// walks on it: the surface of every lake and pond is ground, slippery
    /// ground, and nobody wades or swims in it
    /// ([`water_at`](Self::water_at) and the rest find no water). Thinner
    /// than that it is only seen. Less than nothing, or what is no number,
    /// is no ice.
    ///
    /// Like the water's rise this is shared by everyone who holds the
    /// level: a world says it before each step it takes.
    pub fn set_ice(&self, thickness: f32) {
        let thickness = if thickness.is_finite() {
            thickness.max(0.0)
        } else {
            0.0
        };
        (self.ice.0).store(thickness.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// How thick the ice on the level's still water is, in metres.
    #[must_use]
    pub fn ice(&self) -> f32 {
        self.ice.get()
    }

    /// Whether the ice on the level's still water is thick enough to walk
    /// on.
    #[must_use]
    pub fn frozen(&self) -> bool {
        self.ice.get() >= Self::ICE_BEARS
    }

    /// How high the ice that `point` is on, over or frozen into stands, if
    /// there is still water at its `x` and `z` that is frozen thick enough
    /// to walk on: the height of the water's surface. A point under the
    /// water's own bed, in a cellar under a pool, has no ice over it that
    /// matters to it.
    #[must_use]
    pub fn ice_around(&self, [x, y, z]: [f32; 3]) -> Option<f32> {
        if !self.frozen() {
            return None;
        }
        // Feet rest a little in the bed they stand on.
        let slack = 0.5;
        let rise = self.rise.get();
        self.waters
            .iter()
            .filter(|water| {
                let deep = water.depth_at(x, z);
                deep + rise > 0.0 && y > water.level - deep - slack
            })
            .map(|water| water.level + rise)
            .reduce(f32::max)
    }

    /// How high the surface of the still water that `point` is in or
    /// under the surface of stands, if it is in any: [`water_at`](Self::water_at)
    /// for a point that has a height. A point under the water's own bed,
    /// in a cellar under a pool or a cave under a lake, is not in it.
    #[must_use]
    pub fn water_around(&self, [x, y, z]: [f32; 3]) -> Option<f32> {
        if self.frozen() {
            return None;
        }
        // Feet rest a little in the bed they stand on.
        let slack = 0.5;
        let rise = self.rise.get();
        self.waters
            .iter()
            .filter(|water| {
                // The bed is where it was; the surface is higher.
                let deep = water.depth_at(x, z);
                deep + rise > 0.0 && y > water.level - deep - slack
            })
            .map(|water| water.level + rise)
            .reduce(f32::max)
    }

    /// How deep the still water at `x`, `z` is from its surface to its
    /// bed, in metres; nothing where there is none.
    #[must_use]
    pub fn water_depth(&self, x: f32, z: f32) -> f32 {
        if self.frozen() {
            return 0.0;
        }
        let rise = self.rise.get();
        self.waters
            .iter()
            .map(|water| water.depth_at(x, z) + rise)
            .fold(0.0, f32::max)
    }

    /// The still water of the level: each lake, pond and pool in the
    /// shape it took when it filled the hollow it was put in.
    #[must_use]
    pub fn waters(&self) -> &[Water] {
        &self.waters
    }

    /// A level made of loose triangles, each three corner positions in metres.
    ///
    /// Triangles with no area, or with a corner that is not a number, are
    /// left out.
    #[must_use]
    pub fn from_triangles(triangles: &[[[f32; 3]; 3]]) -> Self {
        let mut points = Vec::with_capacity(triangles.len() * 3);
        let mut indices = Vec::with_capacity(triangles.len());
        for triangle in triangles {
            let [a, b, c] = triangle.map(Vector::from_array);
            let area_doubled = (b - a).cross(c - a).length();
            if !(area_doubled.is_finite() && area_doubled > THINNEST_TRIANGLE) {
                continue;
            }
            let Ok(first) = u32::try_from(points.len()) else {
                break;
            };
            points.extend([a, b, c].map(Vec3::as_dvec3));
            indices.push([first, first + 1, first + 2]);
        }
        let count = indices.len();
        let mesh = TriMesh::new(points, indices).ok();
        Self {
            triangles: if mesh.is_some() { count } else { 0 },
            mesh,
            waters: Vec::new(),
            rise: Rise::default(),
            ice: Rise::default(),
        }
    }

    /// How many triangles the player can collide with.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.triangles
    }

    /// Looks from `from` along `direction` and reports the first surface in
    /// the way, no further off than `reach` metres: what a bullet hits.
    ///
    /// `direction` need not be one unit long. Nothing is found when it has
    /// no length, when a number is not a number, or in an empty level.
    #[must_use]
    pub fn raycast(&self, from: [f32; 3], direction: [f32; 3], reach: f32) -> Option<RayHit> {
        let mesh = self.mesh.as_ref()?;
        let from = Vector::from_array(from);
        let heading = Vector::from_array(direction).try_normalize()?;
        if !(from.is_finite() && heading.is_finite() && reach > 0.0) {
            return None;
        }
        let ray = Ray::new(from.as_dvec3(), heading.as_dvec3());
        let reach = f64::from(reach.min(FARTHEST_RAY));
        let hit = mesh.cast_local_ray_and_get_normal(&ray, reach, false)?;
        let normal = hit.normal.as_vec3().try_normalize()?;
        // A triangle seen from behind reports its back; face the looker.
        let normal = if normal.dot(heading) > 0.0 {
            -normal
        } else {
            normal
        };
        let distance = hit.time_of_impact as f32;
        distance.is_finite().then(|| RayHit {
            distance,
            point: (from + heading * distance).to_array(),
            normal: normal.to_array(),
        })
    }

    /// Moves `shape` from `centre` along `travel` and reports the first
    /// thing in its way, stopping `gap` short of it. Something less than
    /// `gap` past the end of `travel` is in the way too.
    ///
    /// Stopping short rather than touching matters: a shape that rests a
    /// hair away from a wall can slide along it without the wall being
    /// found again on every move, where its direction would be guesswork.
    pub(crate) fn sweep(
        &self,
        shape: &Capsule,
        centre: Vector,
        travel: Vector,
        gap: f32,
    ) -> Option<Touch> {
        let mesh = self.mesh.as_ref()?;
        let (travel, gap) = (travel.as_dvec3(), f64::from(gap));
        let length = travel.length();
        let heading = travel.try_normalize()?;
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: 0.0,
            // Something already touching is only in the way if we head into it.
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let hit = query::cast_shapes(
            &Pose::IDENTITY,
            Wide::ZERO,
            mesh,
            &Pose::from_translation(centre.as_dvec3()),
            heading * (length + gap),
            shape,
            options,
        )
        .ok()
        .flatten()?;
        let normal = hit.normal1.try_normalize()?;
        // Back along the path far enough to leave `gap` square to the surface.
        let head_on = (-heading.dot(normal)).max(1.0e-3);
        let reached = hit.time_of_impact.clamp(0.0, 1.0) * (length + gap);
        let fraction = ((reached - gap / head_on) / length).clamp(0.0, 1.0);
        (fraction.is_finite() && hit.witness1.is_finite()).then_some(Touch {
            fraction: fraction as f32,
            normal: normal.as_vec3(),
            point: hit.witness1.as_vec3(),
        })
    }

    /// Whether `shape` at `centre` reaches into anything solid.
    #[cfg(test)]
    pub(crate) fn overlaps(&self, shape: &Capsule, centre: Vector) -> bool {
        self.mesh.as_ref().is_some_and(|mesh| {
            query::intersection_test(
                &Pose::IDENTITY,
                mesh,
                &Pose::from_translation(centre.as_dvec3()),
                shape,
            )
            .is_ok_and(|found| found.intersecting)
        })
    }

    /// The surface nearest to `shape` at `centre`, if it is closer than
    /// `gap`: which way it faces, and how far off it is (negative when the
    /// shape is inside it).
    pub(crate) fn nearest(
        &self,
        shape: &Capsule,
        centre: Vector,
        gap: f32,
    ) -> Option<(Vector, f32)> {
        let mesh = self.mesh.as_ref()?;
        let contact = query::contact(
            &Pose::IDENTITY,
            mesh,
            &Pose::from_translation(centre.as_dvec3()),
            shape,
            f64::from(gap),
        )
        .ok()
        .flatten()?;
        let normal = contact.normal1.try_normalize()?;
        contact
            .dist
            .is_finite()
            .then_some((normal.as_vec3(), contact.dist as f32))
    }

    /// Which way the surface straight below `from` faces, looking no
    /// further down than `reach`.
    pub(crate) fn surface_below(&self, from: Vector, reach: f32) -> Option<Vector> {
        let ray = Ray::new(from.as_dvec3(), Wide::NEG_Y);
        let hit =
            self.mesh
                .as_ref()?
                .cast_local_ray_and_get_normal(&ray, f64::from(reach), false)?;
        // A triangle seen from behind reports its back; either side is floor.
        let normal = hit.normal.as_vec3().try_normalize()?;
        Some(if normal.y < 0.0 { -normal } else { normal })
    }
}

/// What a moving shape ran into.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Touch {
    /// How much of the move was made before touching, from 0 to 1.
    pub fraction: f32,
    /// Which way the surface faces, pointing at the shape.
    pub normal: Vector,
    /// The place on the surface that was touched.
    pub point: Vector,
}

/// `point` moved to where `transform` puts it.
pub(crate) fn place(transform: &Transform, point: [f32; 3]) -> [f32; 3] {
    let mut out = [transform[3][0], transform[3][1], transform[3][2]];
    for (column, value) in point.into_iter().enumerate() {
        for (row, cell) in out.iter_mut().enumerate() {
            *cell += transform[column][row] * value;
        }
    }
    out
}

#[cfg(test)]
mod rise_tests {
    use super::*;

    #[test]
    fn rain_raises_the_water_over_the_ground_beside_it() {
        let mut level = Level::from_triangles(&[]);
        // A pond a metre deep with a bank 0.3 m over it on one side and a
        // wall on the other, as three points a metre apart.
        level.waters = vec![Water {
            name: None,
            sources: Vec::new(),
            level: 2.0,
            origin: [0.0, 0.0],
            cell: 1.0,
            size: [3, 1],
            depths: vec![-0.3, 1.0, Water::DRY],
            colour: [0.0; 4],
        }];
        assert_eq!(level.water_at(1.0, 0.0), Some(2.0));
        assert_eq!(level.water_at(0.0, 0.0), None, "the bank is dry");
        level.set_water_rise(0.5);
        assert_eq!(level.water_rise(), 0.5);
        assert_eq!(level.water_at(1.0, 0.0), Some(2.5));
        assert_eq!(level.water_at(0.0, 0.0), Some(2.5), "the bank is under");
        assert_eq!(level.water_at(2.0, 0.0), None, "the wall is not");
        assert!((level.water_depth(1.0, 0.0) - 1.5).abs() < 1e-6);
        assert!((level.water_depth(0.0, 0.0) - 0.2).abs() < 1e-6);
        // Someone standing on the bank is in the water now.
        assert_eq!(level.water_around([0.0, 2.3, 0.0]), Some(2.5));
        // And it goes down again; what is no height is none.
        for none in [0.0, -1.0, f32::NAN] {
            level.set_water_rise(none);
            assert_eq!(level.water_rise(), 0.0);
            assert_eq!(level.water_at(0.0, 0.0), None);
            assert_eq!(level.water_at(1.0, 0.0), Some(2.0));
        }
    }
}
