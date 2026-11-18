//! Drops in the air: gravity, drag, being swept against the level, landing
//! and splashing.

use crate::flow::{GRAVITY, K_SPLASH, MIN_VOLUME, Physics};
use crate::math::{add, dot, frame, length, madd, normalize, scale};
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
        let speed = length(p.vel);
        // The ray along the centre stops at the surface: a drop is
        // millimetres across and a ray is continuous, so nothing tunnels.
        let hit = level.raycast(p.pos, p.vel, speed * t + r);
        let end = hit.map_or_else(|| madd(p.pos, p.vel, t), |h| h.point);
        let drowned = level
            .water_at(end[0], end[2])
            .is_some_and(|surface| end[1] < surface);
        if drowned || p.flight_time > FLIGHT_LIMIT || !end.iter().all(|c| c.is_finite()) {
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
        if k > K_SPLASH {
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
        let children = (libm::floorf(root_we / 8.0) as usize).clamp(2, 6);
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
            let child = Particle::flying(
                p.kind,
                madd(point, normal, CHILD_LIFT),
                scale(way, speed),
                each,
                self.born,
            );
            self.parts.push(child);
            p.volume -= each;
        }
        p
    }
}
