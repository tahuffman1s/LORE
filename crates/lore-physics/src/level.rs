//! The level's triangles, and the few questions the player asks of them.

use lore_assets::{Scene, Transform, Water};

use crate::medium::Medium;
use parry3d::glamx::Vec3;
use parry3d::math::{Pose, Vector as Wide};
use parry3d::query::{self, Ray, RayCast, ShapeCastOptions};
use parry3d::shape::{Ball, Capsule, TriMesh};

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
    /// How each of the waters stands now, in their order.
    standing: Vec<Standing>,
    /// The hollows found while a game runs, and how each stands.
    found: Found,
}

/// Hollows that fluid was found lying in while a game ran, each with how
/// it stands: said through a level many hold, as the rise and the ice are.
#[derive(Default)]
struct Found(std::sync::RwLock<Vec<(std::sync::Arc<Water>, Standing)>>);

impl Found {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<(std::sync::Arc<Water>, Standing)>> {
        // A holder that panicked while it had the list left it whole.
        (self.0.read()).unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Vec<(std::sync::Arc<Water>, Standing)>> {
        (self.0.write()).unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Clone for Found {
    fn clone(&self) -> Self {
        Self(std::sync::RwLock::new(self.read().clone()))
    }
}

/// How one body of still fluid stands, said through a level many hold.
#[derive(Clone)]
struct Standing {
    /// How far over where it was put its surface is; under it, below
    /// nothing.
    shift: Rise,
    /// How heavy and how thick what it holds is.
    density: Rise,
    viscosity: Rise,
    /// Whether it is frozen thick enough to walk on: 1 if so.
    frozen: Rise,
}

impl Default for Standing {
    fn default() -> Self {
        let standing = Self {
            shift: Rise::default(),
            density: Rise::default(),
            viscosity: Rise::default(),
            frozen: Rise::default(),
        };
        standing.density.set(Medium::WATER.density);
        standing.viscosity.set(Medium::WATER.viscosity);
        standing
    }
}

/// How one body of still fluid stands: what [`Level::set_pool`] takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stand {
    /// How far over where its maker put it the surface is, in metres;
    /// under it, below nothing. It is kept between empty and the body's
    /// brim ([`Water::stood`]).
    pub shift: f32,
    /// Whether it is frozen thick enough to walk on.
    pub frozen: bool,
    /// What it holds, to someone in it.
    pub medium: Medium,
}

impl Default for Stand {
    /// Water, where its maker put it, not frozen.
    fn default() -> Self {
        Self {
            shift: 0.0,
            frozen: false,
            medium: Medium::WATER,
        }
    }
}

/// The still fluid at a place: what [`Level::pool_around`] finds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoolHere {
    /// Which of [`Level::waters`] it is.
    pub index: usize,
    /// How high its surface stands, in metres.
    pub surface: f32,
    /// How deep it is there from its surface to its bed, in metres.
    pub deep: f32,
    /// What it holds, to someone in it.
    pub medium: Medium,
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

    fn set(&self, value: f32) {
        (self.0).store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
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

    /// The most rain raises still water over where its maker put it, in
    /// metres. A body whose rim is lower than that runs out over it, and
    /// stands no higher ([`Water::brim`]).
    pub const MOST_WATER_RISE: f32 = 0.8;

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
        let waters = if scene.waters.is_empty() {
            crate::water::filled(&level, scene).0
        } else {
            scene.waters.clone()
        };
        level.set_waters(waters);
        level
    }

    /// Says what still water the level has, each body standing where it
    /// was put, as water, not frozen.
    pub(crate) fn set_waters(&mut self, waters: Vec<Water>) {
        self.standing = waters.iter().map(|_| Standing::default()).collect();
        self.waters = waters;
        // What was said of all of them still holds.
        self.set_water_rise(self.rise.get());
        self.set_ice(self.ice.get());
    }

    /// Lends every body of still fluid the level has, the level's own and
    /// then the hollows found since ([`set_found`](Self::set_found)): each
    /// with which it is, its shape, and how it stands now.
    fn bodies<R>(
        &self,
        with: impl FnOnce(&mut dyn Iterator<Item = (usize, &Water, &Standing)>) -> R,
    ) -> R {
        let found = self.found.read();
        let mut all = (self.waters.iter().zip(&self.standing))
            .chain(found.iter().map(|(water, standing)| (&**water, standing)))
            .enumerate()
            .map(|(index, (water, standing))| (index, water, standing));
        with(&mut all)
    }

    /// The still fluid there is at a place, of the bodies that are not
    /// frozen over: the one whose surface stands highest, of those `there`
    /// says how deep they are for (their own depth where they were put).
    fn highest(&self, there: impl Fn(&Water, f32) -> Option<f32>) -> Option<PoolHere> {
        self.bodies(|all| {
            all.filter(|(_, _, standing)| standing.frozen.get() < 0.5)
                .filter_map(|(index, water, standing)| {
                    let shift = standing.shift.get();
                    there(water, shift).map(|deep| PoolHere {
                        index,
                        surface: water.level + shift,
                        deep: deep + shift,
                        medium: Medium {
                            density: standing.density.get(),
                            viscosity: standing.viscosity.get(),
                        },
                    })
                })
                .reduce(|one, other| {
                    if other.surface > one.surface {
                        other
                    } else {
                        one
                    }
                })
        })
    }

    /// Says how body `index` stands: how far from where it was put,
    /// whether it is frozen, and what it holds. The level's own waters
    /// ([`waters`](Self::waters)) are numbered first, then the hollows
    /// found since ([`set_found`](Self::set_found)). A body there is none
    /// of is nobody's to say.
    ///
    /// Like the rise and the ice this is shared by everyone who holds the
    /// level: a world says it before each step it takes, after
    /// [`set_water_rise`](Self::set_water_rise) and
    /// [`set_ice`](Self::set_ice), which say the same of every body.
    pub fn set_pool(&self, index: usize, stand: Stand) {
        let medium = stand.medium.cleaned();
        self.bodies(|all| {
            if let Some((_, water, standing)) = all.nth(index) {
                standing.shift.set(water.stood(stand.shift));
                standing.frozen.set(f32::from(u8::from(stand.frozen)));
                standing.density.set(medium.density);
                standing.viscosity.set(medium.viscosity);
            }
        });
    }

    /// How body `index` stands, if there is one.
    #[must_use]
    pub fn pool(&self, index: usize) -> Option<Stand> {
        self.bodies(|all| {
            all.nth(index).map(|(_, _, standing)| Stand {
                shift: standing.shift.get(),
                frozen: standing.frozen.get() >= 0.5,
                medium: Medium {
                    density: standing.density.get(),
                    viscosity: standing.viscosity.get(),
                },
            })
        })
    }

    /// Says which hollows have been found since the level was made: fluid
    /// lay in them, and they hold it as the level's own waters do
    /// ([`hollow`](crate::hollow) finds one). They are numbered after the
    /// level's own waters, in this order. One that was already said keeps
    /// how it stands; a new one stands empty until it is said otherwise.
    ///
    /// Shared by everyone who holds the level, as how the waters stand is:
    /// a world says it before each step it takes.
    pub fn set_found(&self, hollows: &[std::sync::Arc<Water>]) {
        let mut found = self.found.write();
        if found.len() == hollows.len()
            && (found.iter().zip(hollows)).all(|((have, _), new)| std::sync::Arc::ptr_eq(have, new))
        {
            return;
        }
        let mut next = Vec::with_capacity(hollows.len());
        for hollow in hollows {
            let standing = found
                .iter()
                .find(|(have, _)| std::sync::Arc::ptr_eq(have, hollow))
                .map_or_else(
                    || {
                        let standing = Standing::default();
                        standing.shift.set(-hollow.deepest());
                        standing
                    },
                    |(_, standing)| standing.clone(),
                );
            next.push((std::sync::Arc::clone(hollow), standing));
        }
        *found = next;
    }

    /// How many hollows have been found since the level was made.
    #[must_use]
    pub fn found(&self) -> usize {
        self.found.read().len()
    }

    /// The still fluid that `point` is in or under the surface of, if it
    /// is in any that is not frozen over: the one whose surface stands
    /// highest. A point under the fluid's own bed, in a cellar under a
    /// pool or a cave under a lake, is not in it.
    #[must_use]
    pub fn pool_around(&self, [x, y, z]: [f32; 3]) -> Option<PoolHere> {
        // Feet rest a little in the bed they stand on.
        let slack = 0.5;
        self.highest(|water, shift| {
            // The bed is where it was; the surface is higher or lower.
            let deep = water.depth_at(x, z);
            (deep + shift > 0.0 && y > water.level - deep - slack).then_some(deep)
        })
    }

    /// The hollow of still fluid that `x`, `z` is over, whether or not
    /// anything stands in it now: which body it is, and how high its bed
    /// is there. Of several, the one whose bed is highest; none that is
    /// frozen over. An empty basin is found by this and by nothing else
    /// here.
    #[must_use]
    pub fn hollow_at(&self, x: f32, z: f32) -> Option<(usize, f32)> {
        self.bodies(|all| {
            all.filter(|(_, _, standing)| standing.frozen.get() < 0.5)
                .filter_map(|(index, water, _)| {
                    let deep = water.depth_at(x, z);
                    (deep > 0.0).then_some((index, water.level - deep))
                })
                .reduce(|one, other| if other.1 > one.1 { other } else { one })
        })
    }

    /// The body of still fluid whose hollow `point` is in, whether or not
    /// anything stands there now, frozen or not: over its bed where it
    /// covers the ground when it is full, and no higher than a little over
    /// its brim. Of several, the one whose bed is highest there. A point
    /// under the bed, in a cellar under a pool, is in none.
    #[must_use]
    pub fn pool_holding(&self, [x, y, z]: [f32; 3]) -> Option<usize> {
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return None;
        }
        // Feet rest a little in the bed they stand on, and a hand held
        // just over a full pool is at it.
        let slack = 0.5;
        self.bodies(|all| {
            all.filter_map(|(index, water, _)| {
                let deep = water.depth_at(x, z);
                let bed = water.level - deep;
                let covered = deep + (water.brim - water.level) > 0.0;
                (covered && y > bed - slack && y < water.brim + slack).then_some((index, bed))
            })
            .reduce(|one, other| if other.1 > one.1 { other } else { one })
            .map(|(index, _)| index)
        })
    }

    /// The still fluid over `x`, `z`, if there is any that is not frozen
    /// over: the one whose surface stands highest.
    #[must_use]
    pub fn pool_at(&self, x: f32, z: f32) -> Option<PoolHere> {
        self.highest(|water, shift| {
            let deep = water.depth_at(x, z);
            (deep + shift > 0.0).then_some(deep)
        })
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
                // Both sides of a triangle are solid, but which way it faces
                // says which side of it is the inside of a closed shape
                // ([`met_from_behind`](Self::met_from_behind)): a mirrored
                // placement turns triangles inside out, and is turned back.
                triangles.extend(part.triangles().map(|corners| {
                    let [a, b, c] = corners.map(|point| place(&instance.transform, point));
                    if instance.mirrored {
                        [a, c, b]
                    } else {
                        [a, b, c]
                    }
                }));
            }
        }
        Self::from_triangles(&triangles)
    }

    /// How high the surface of the still water over `x`, `z` is, if there
    /// is any there: a lake or a pond that the level marks as water.
    #[must_use]
    pub fn water_at(&self, x: f32, z: f32) -> Option<f32> {
        self.pool_at(x, z).map(|pool| pool.surface)
    }

    /// Says that rain has raised the level's still water `rise` metres
    /// above where its maker put it. Each lake and pond stands that much
    /// higher, and spreads over whatever ground beside it is lower than
    /// that: a body of water knows how far above it the ground round it
    /// stands. One whose rim is lower runs out over it, and stands at its
    /// brim ([`Water::risen`]). Less than nothing, or what is no number,
    /// is nothing.
    ///
    /// The level's shapes are shared by everyone who holds it, and so is
    /// this: a world says it before each step it takes.
    pub fn set_water_rise(&self, rise: f32) {
        let rise = if rise.is_finite() { rise.max(0.0) } else { 0.0 };
        self.rise.set(rise);
        // The level's own waters: a hollow found since holds what was
        // poured into it, and rain does not fill it.
        for (water, standing) in self.waters.iter().zip(&self.standing) {
            standing.shift.set(water.risen(rise));
        }
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
        self.ice.set(thickness);
        let frozen = f32::from(u8::from(thickness >= Self::ICE_BEARS));
        self.bodies(|all| {
            for (_, _, standing) in all {
                standing.frozen.set(frozen);
            }
        });
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
        // Feet rest a little in the bed they stand on.
        let slack = 0.5;
        self.bodies(|all| {
            all.filter(|(_, _, standing)| standing.frozen.get() >= 0.5)
                .filter_map(|(_, water, standing)| {
                    let (deep, shift) = (water.depth_at(x, z), standing.shift.get());
                    (deep + shift > 0.0 && y > water.level - deep - slack)
                        .then_some(water.level + shift)
                })
                .reduce(f32::max)
        })
    }

    /// How high the surface of the still water that `point` is in or
    /// under the surface of stands, if it is in any: [`water_at`](Self::water_at)
    /// for a point that has a height. A point under the water's own bed,
    /// in a cellar under a pool or a cave under a lake, is not in it.
    #[must_use]
    pub fn water_around(&self, point: [f32; 3]) -> Option<f32> {
        self.pool_around(point).map(|pool| pool.surface)
    }

    /// How deep the still water at `x`, `z` is from its surface to its
    /// bed, in metres; nothing where there is none.
    #[must_use]
    pub fn water_depth(&self, x: f32, z: f32) -> f32 {
        self.bodies(|all| {
            all.filter(|(_, _, standing)| standing.frozen.get() < 0.5)
                .map(|(_, water, standing)| water.depth_at(x, z) + standing.shift.get())
                .fold(0.0, f32::max)
        })
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
            standing: Vec::new(),
            found: Found::default(),
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

    /// Moves a ball `radius` metres across from `from` towards `to` and says
    /// how far along it got before touching anything, from 0 (stuck at the
    /// start) to 1 (all the way): what a third-person camera uses to stay
    /// out of walls. `None` when nothing is in the way, or a number is not a
    /// number.
    #[must_use]
    pub fn ball_cast(&self, from: [f32; 3], to: [f32; 3], radius: f32) -> Option<f32> {
        let mesh = self.mesh.as_ref()?;
        let (from, to) = (Vector::from_array(from), Vector::from_array(to));
        if !(from.is_finite() && to.is_finite() && radius.is_finite() && radius > 0.0) {
            return None;
        }
        let travel = (to - from).as_dvec3();
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: 0.0,
            stop_at_penetration: true,
            compute_impact_geometry_on_penetration: false,
        };
        let hit = query::cast_shapes(
            &Pose::IDENTITY,
            Wide::ZERO,
            mesh,
            &Pose::from_translation(from.as_dvec3()),
            travel,
            &Ball::new(f64::from(radius)),
            options,
        )
        .ok()
        .flatten()?;
        let fraction = hit.time_of_impact as f32;
        fraction.is_finite().then(|| fraction.clamp(0.0, 1.0))
    }

    /// Whether the first surface a ray from `from` along `direction` meets,
    /// no further off than `reach` metres, is met from behind: from the
    /// side its triangles do not face, which is the inside of a closed
    /// shape (a rock, a block of ground) or the rock over a cave's roof.
    /// `None` if nothing is met.
    pub(crate) fn met_from_behind(
        &self,
        from: [f32; 3],
        direction: [f32; 3],
        reach: f32,
    ) -> Option<bool> {
        let mesh = self.mesh.as_ref()?;
        let from = Vector::from_array(from);
        let heading = Vector::from_array(direction).try_normalize()?;
        if !(from.is_finite() && heading.is_finite() && reach > 0.0) {
            return None;
        }
        let ray = Ray::new(from.as_dvec3(), heading.as_dvec3());
        let reach = f64::from(reach.min(FARTHEST_RAY));
        let hit = mesh.cast_local_ray_and_get_normal(&ray, reach, false)?;
        // The way the triangle faces as it was wound, not the way the hit
        // reports, which is always towards the ray.
        let faces = mesh.triangle(hit.subshape).scaled_normal();
        Some(faces.dot(heading.as_dvec3()) > 0.0)
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
        level.set_waters(vec![Water {
            name: None,
            sources: Vec::new(),
            level: 2.0,
            brim: 2.0 + Level::MOST_WATER_RISE,
            origin: [0.0, 0.0],
            cell: 1.0,
            size: [3, 1],
            depths: vec![-0.3, 1.0, Water::DRY],
            colour: [0.0; 4],
            fluid: None,
            amount: 1.0,
            spill: None,
        }]);
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
