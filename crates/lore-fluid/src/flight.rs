//! Drops in the air: gravity, drag, being swept against the level, landing
//! and splashing.

use crate::flow::{GRAVITY, K_SPLASH, MIN_VOLUME, Physics};
use crate::lying::R_MAX;
use crate::math::{add, dot, frame, length, madd, normalize, scale, sub};
use crate::store::Particle;
use crate::{Fluids, MOST_DROPS};
use core::f32::consts::TAU;
use lore_physics::{Level, RayHit};

/// Seconds in the air after which a drop is gone: the level has no kill
/// plane, so a drop that falls for ever must end somewhere.
const FLIGHT_LIMIT: f32 = 10.0;
/// The most of a drop's volume a splash throws off.
const MOST_THROWN: f32 = 0.7;
/// How far a splash child starts off the surface, metres.
const CHILD_LIFT: f32 = 0.001;
/// Normals closer than this (dot product) are one surface, for landing
/// in the wet.
const WET_FACING: f32 = 0.8;
/// How far off a lying drop's plane a landing still counts as in it, metres.
const WET_PLANE: f32 = 0.02;
/// How far beside a lying drop a landing still runs into it, metres: the
/// crown of a drop landing in the wet is about this wide.
const WET_REACH: f32 = 0.02;
/// The fewest and the most drops one splash throws off.
const FEWEST_THROWN: usize = 2;
const MOST_CHILDREN: usize = 4;
/// Splashes throw nothing off once the store has fewer free places than
/// this: the stream and the puddles it makes matter more than spatter, so
/// a long spray fills the store slowly and with what is seen.
const SPLASH_ROOM: usize = 2000;

impl Fluids {
    /// Moves every flying drop one tick, landing, splashing or removing
    /// those that reach something.
    pub(crate) fn fly(&mut self, level: &Level, dt: f32) {
        // Drops made by splashing this tick start to fly next tick.
        let count = self.parts.len();
        for i in 0..count {
            let Some(p) = self.parts.get(i).copied() else {
                continue;
            };
            if p.lying || p.dead {
                continue;
            }
            let Some(phys) = self.physics.get(usize::from(p.kind)).copied() else {
                if let Some(slot) = self.parts.get_mut(i) {
                    slot.dead = true;
                }
                continue;
            };
            let p = self.fly_one(level, dt, p, &phys);
            if let Some(slot) = self.parts.get_mut(i) {
                *slot = p;
            }
        }
    }

    /// One flying drop for one tick: the drop as it is afterwards.
    fn fly_one(&mut self, level: &Level, dt: f32, mut p: Particle, phys: &Physics) -> Particle {
        let t = dt * (1.0 - p.head_start.clamp(0.0, 1.0));
        p.head_start = 0.0;
        let r = p.ball();

        // Gravity, then air drag taken implicitly so it cannot overshoot.
        p.vel[1] -= GRAVITY * t;
        let drag = phys.air_drag(r) * length(p.vel);
        p.vel = scale(p.vel, 1.0 / (1.0 + drag * t));

        p.flight_time += dt;
        // A piece of a jet pulls itself round as the jet breaks up.
        if p.long > 1.0 {
            p.long = 1.0 + (p.long - 1.0) * libm::expf(-t / phys.breakup_time(r));
        }
        let speed = length(p.vel);
        // The ray along the centre stops at the surface: a drop is
        // millimetres across and a ray is continuous, so nothing tunnels.
        let mut hit = level.raycast(p.pos, p.vel, speed * t + r);
        let mut end = hit.map_or_else(|| madd(p.pos, p.vel, t), |h| h.point);
        // Ice that bears a person bears a drop: it lands on a frozen lake,
        // which is not in the level's mesh, rather than on its bed.
        if let Some(ice) = level.ice_around(end)
            && p.pos[1] >= ice
            && end[1] < ice
        {
            let share = (p.pos[1] - ice) / (p.pos[1] - end[1]);
            let point = madd(p.pos, sub(end, p.pos), share);
            hit = Some(RayHit {
                distance: length(sub(point, p.pos)),
                point,
                normal: [0.0, 1.0, 0.0],
            });
            end = point;
        }
        if p.flight_time > FLIGHT_LIMIT || !end.iter().all(|c| c.is_finite()) {
            p.dead = true;
            return p;
        }
        // Under the surface of the fluid it is in, not of fluid over it: a
        // drop in a cellar under a pool is in the air. The pool takes it
        // in, and fast drops throw a little of the pool back up.
        let drowned = level
            .water_around(end)
            .is_some_and(|surface| end[1] < surface);
        if drowned && self.joins_pool(level, end, p.kind, p.volume, p.vel, p.thrown) {
            p.dead = true;
            return p;
        }
        match hit {
            Some(hit) => self.land(p, hit, phys),
            None => {
                p.pos = end;
                p
            }
        }
    }

    /// A drop meets a surface: some of it may be thrown off as smaller drops
    /// and the rest lies there.
    fn land(&mut self, mut p: Particle, hit: RayHit, phys: &Physics) -> Particle {
        let normal = normalize(hit.normal).unwrap_or([0.0, 1.0, 0.0]);
        let impact = length(p.vel);
        let normal_speed = (-dot(p.vel, normal)).max(0.0);
        let (k, root_we) = phys.splash(normal_speed, 2.0 * p.ball());
        if k > K_SPLASH
            && !p.thrown
            && self.parts.len() + SPLASH_ROOM < MOST_DROPS
            && !self.wet_at(hit.point, normal, p.kind)
        {
            p = self.splash(p, hit.point, normal, impact, k, root_we);
        }
        if p.volume < MIN_VOLUME {
            p.dead = true;
            return p;
        }
        // The ball's radius is how thick it starts; it then spreads.
        let h = p.ball();
        let mut lying = Particle::lying(p.kind, hit.point, normal, p.volume, h, p.born);
        lying.dry_left = phys.dries;
        lying.landed_now = true;
        lying
    }

    /// Whether a lying drop of `kind` already covers `point` on the surface
    /// facing `normal`. A drop that lands there runs into the liquid rather
    /// than onto a dry wall, and does not throw spatter off: Mundo's
    /// splashing threshold is for a dry wall, and a jet on one spot soon
    /// wets it, after which what it brings joins the puddle it feeds.
    ///
    /// It reads the grid of the last step, which still holds every lying
    /// drop where it was: flying drops land before anything lying moves.
    fn wet_at(&self, point: [f32; 3], normal: [f32; 3], kind: u8) -> bool {
        let lo = point.map(|c| c - R_MAX);
        let hi = point.map(|c| c + R_MAX);
        let mut wet = false;
        self.grid.near(lo, hi, &mut |j| {
            let Some(q) = self.parts.get(j) else {
                return;
            };
            if wet || !q.lying || q.dead || q.kind != kind || dot(q.normal, normal) <= WET_FACING {
                return;
            }
            let off = sub(point, q.pos);
            let height = dot(off, normal);
            let across = length(madd(off, normal, -height));
            wet = height.abs() < WET_PLANE && across < q.patch() + WET_REACH;
        });
        wet
    }

    /// Throws a share of `p` off the surface as children and returns `p`
    /// with what is left (Mundo, Sommerfeld and Tropea 1995).
    fn splash(
        &mut self,
        mut p: Particle,
        point: [f32; 3],
        normal: [f32; 3],
        impact: f32,
        k: f32,
        root_we: f32,
    ) -> Particle {
        let share = (0.1 * (k / K_SPLASH - 1.0)).min(MOST_THROWN);
        let children = (libm::floorf(root_we / 8.0) as usize).clamp(FEWEST_THROWN, MOST_CHILDREN);
        let each = share * p.volume / children as f32;
        if each < MIN_VOLUME {
            return p;
        }
        let (u, v) = frame(normal);
        let turn = self.rolls.next() * TAU;
        for c in 0..children {
            // Only into free room; what finds none stays in the lying drop.
            if self.parts.len() >= MOST_DROPS {
                break;
            }
            let azimuth = turn + TAU * c as f32 / children as f32 + 0.3 * (self.rolls.next() - 0.5);
            let lift = (20.0 + 20.0 * self.rolls.next()).to_radians();
            let speed = impact * (0.2 + 0.3 * self.rolls.next());
            let round = add(scale(u, libm::cosf(azimuth)), scale(v, libm::sinf(azimuth)));
            let way = add(
                scale(round, libm::cosf(lift)),
                scale(normal, libm::sinf(lift)),
            );
            self.born += 1;
            let mut child = Particle::flying(
                p.kind,
                madd(point, normal, CHILD_LIFT),
                scale(way, speed),
                each,
                self.born,
            );
            child.thrown = true;
            self.parts.push(child);
            p.volume -= each;
        }
        p
    }
}
