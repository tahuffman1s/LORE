//! Drops that lie on surfaces: landing on each other, spreading, merging,
//! being pushed apart, running down slopes, leaving trails and drying.

use crate::flow::{GRAVITY, MIN_VOLUME, Physics};
use crate::math::{add, dot, frame, in_plane, length, length_sq, madd, normalize, scale, sub};
use crate::store::Particle;
use crate::{Fluids, MOST_DROPS, MOST_KINDS};
use core::f32::consts::PI;
use lore_physics::Level;

/// Ticks a drop must lie unchanged before it sleeps and costs nothing.
pub(crate) const SLEEP_TICKS: u32 = 20;
/// The widest a merged patch may be, metres: wider ones push apart instead.
pub(crate) const R_MAX: f32 = 0.12;
/// An awake drop slower than this, m/s, is only updated every few ticks:
/// honey creeping down a wall moves a fifth of a millimetre between looks.
const CALM_SPEED: f32 = 0.005;
/// How many ticks apart a calm drop is updated.
const CALM_STRIDE: u32 = 4;
/// A move shorter than this counts as no move, metres.
const MOVED: f32 = 1.0e-5;
/// A drop on a surface this far off another's plane is on another surface.
const SAME_PLANE: f32 = 0.02;
/// Normals closer than this (dot product) are the same surface.
const SAME_FACING: f32 = 0.8;
/// Overlap under this is left alone, metres, so crowded drops can rest.
const LEAST_PUSH: f32 = 5.0e-4;
/// Trail drops leave this many store places free for the rest.
const TRAIL_ROOM: usize = 256;
/// How far above its surface a drop looks along its way, metres.
const LOOK_LIFT: f32 = 0.001;
/// How far past its move a drop looks for a corner, metres.
const LOOK_AHEAD: f32 = 0.002;
/// How far above its new place a drop looks for the surface, metres.
const SNAP_LIFT: f32 = 0.01;
/// How far down from there the surface may be, metres.
const SNAP_REACH: f32 = 0.03;

/// How many ticks of time drop `i` is updated for this tick: 1 for a drop
/// in the ordinary way, the stride when it is calm and it is its turn, and
/// `None` when it is calm and it is not.
fn pace(tick: u32, i: usize, p: &Particle) -> Option<u32> {
    if !p.calm {
        return Some(1);
    }
    tick.wrapping_add(i as u32)
        .is_multiple_of(CALM_STRIDE)
        .then_some(CALM_STRIDE)
}

/// How fast `p` runs down its surface, m/s.
fn run_speed(phys: &Physics, p: &Particle, radius: f32) -> f32 {
    let ny = p.normal[1];
    let sin_alpha = (1.0 - ny * ny).max(0.0).sqrt();
    phys.film_speed(sin_alpha, p.h, radius, p.volume)
}

/// How fast `p` may creep apart from neighbours, m/s.
fn creep_speed(phys: &Physics, p: &Particle, radius: f32) -> f32 {
    phys.spread_speed(p.h, radius)
        .max(run_speed(phys, p, radius))
}

impl Fluids {
    /// What a drop that has just landed does to the other kinds under it:
    /// washes them away, or lies on top and covers part of them.
    pub(crate) fn settle_landings(&mut self) {
        let Self {
            parts,
            grid,
            physics,
            ..
        } = self;
        for i in 0..parts.len() {
            let Some(p) = parts.get(i).copied() else {
                continue;
            };
            if !(p.landed_now && p.lying && !p.dead) {
                continue;
            }
            let washes = physics.get(usize::from(p.kind)).is_some_and(|f| f.washes);
            let radius = p.patch();
            let reach = radius + R_MAX;
            let lo = [p.pos[0] - reach, p.pos[1] - reach, p.pos[2] - reach];
            let hi = [p.pos[0] + reach, p.pos[1] + reach, p.pos[2] + reach];
            grid.near(lo, hi, &mut |j| {
                if j == i {
                    return;
                }
                let Some(q) = parts.get_mut(j) else {
                    return;
                };
                if !q.lying || q.dead || q.kind == p.kind {
                    return;
                }
                // Of two that land together, the newer lies on top.
                if q.landed_now && q.born > p.born {
                    return;
                }
                let off = sub(q.pos, p.pos);
                let height = dot(off, p.normal);
                if dot(q.normal, p.normal) <= SAME_FACING || height.abs() >= SAME_PLANE {
                    return;
                }
                let d = length(madd(off, p.normal, -height));
                let rq = q.patch();
                if d >= radius + rq {
                    return;
                }
                q.still = 0;
                q.calm = false;
                if washes {
                    q.dead = true;
                    return;
                }
                // The part of the older patch the new one covers: the
                // overlap of two discs, approximated.
                let small = radius.min(rq);
                let share = ((radius + rq - d) / (2.0 * small)).clamp(0.0, 1.0)
                    * (small * small / (rq * rq));
                q.volume *= 1.0 - share;
                if q.volume < MIN_VOLUME {
                    q.dead = true;
                }
            });
            if let Some(slot) = parts.get_mut(i) {
                slot.landed_now = false;
                slot.still = 0;
                slot.calm = false;
            }
        }
    }

    /// Everything the awake lying drops do in one tick: spread, merge or
    /// push apart, run, follow their surface, leave trails and fall asleep.
    pub(crate) fn lie(&mut self, level: &Level, dt: f32) {
        self.spread(dt);
        self.crowd(dt);
        self.run(level, dt);
    }

    /// Lying drops spread towards the thickness of a puddle.
    pub(crate) fn spread(&mut self, dt: f32) {
        for (i, p) in self.parts.iter_mut().enumerate() {
            if !p.lying || p.dead || p.still >= SLEEP_TICKS {
                continue;
            }
            let Some(ticks) = pace(self.tick, i, p) else {
                continue;
            };
            let dt = dt * ticks as f32;
            let Some(phys) = self.physics.get(usize::from(p.kind)) else {
                p.dead = true;
                continue;
            };
            // A drop already thinner than a puddle is a lens and stays one.
            if p.h <= phys.h_eq {
                continue;
            }
            let r = p.patch();
            let r2 = r + phys.spread_speed(p.h, r) * dt;
            p.h = (p.volume / (PI * r2 * r2)).max(phys.h_eq);
            // Thickness is compared with where it last counted as changed,
            // so a slow spread still keeps a drop awake now and then.
            if (p.h - p.h_ref).abs() > 0.01 * p.h_ref {
                p.h_ref = p.h;
                p.changed = true;
            }
        }
    }

    /// Awake drops merge with same-kind drops they overlap, or push them
    /// apart when the merged patch would be too wide.
    pub(crate) fn crowd(&mut self, dt: f32) {
        let Self {
            parts,
            grid,
            physics,
            tick,
            ..
        } = self;
        for i in 0..parts.len() {
            let Some(mut a) = parts.get(i).copied() else {
                continue;
            };
            if !a.lying || a.dead || a.still >= SLEEP_TICKS {
                continue;
            }
            let Some(ticks) = pace(*tick, i, &a) else {
                continue;
            };
            let dt = dt * ticks as f32;
            let reach = a.patch() + R_MAX;
            let lo = [a.pos[0] - reach, a.pos[1] - reach, a.pos[2] - reach];
            let hi = [a.pos[0] + reach, a.pos[1] + reach, a.pos[2] + reach];
            grid.near(lo, hi, &mut |j| {
                if j != i && !a.dead && meet(parts, physics, &a, i, j, dt) {
                    // Something changed `a` (a merge): read it again.
                    if let Some(now) = parts.get(i) {
                        a = *now;
                    }
                }
            });
        }
    }

    /// Awake drops run down their surface and follow it, leave trails, and
    /// those that have not changed for a while go to sleep.
    pub(crate) fn run(&mut self, level: &Level, dt: f32) {
        let count = self.parts.len();
        for i in 0..count {
            let Some(mut p) = self.parts.get(i).copied() else {
                continue;
            };
            if !p.lying || p.dead || p.still >= SLEEP_TICKS {
                continue;
            }
            let Some(ticks) = pace(self.tick, i, &p) else {
                continue;
            };
            let dt = dt * ticks as f32;
            let Some(phys) = self.physics.get(usize::from(p.kind)).copied() else {
                continue;
            };
            let before = p.pos;
            let normal = p.normal;
            let radius = p.patch();
            let ny = normal[1];

            // A drop hanging from a ceiling lets go once it is too heavy
            // for its pinning.
            if ny < -0.05 && phys.rho * GRAVITY * p.volume * -ny > phys.hold(radius) {
                p.lying = false;
                p.vel = [0.0; 3];
                p.flight_time = 0.0;
                p.run = 0.0;
                p.push = [0.0; 3];
                if let Some(slot) = self.parts.get_mut(i) {
                    *slot = p;
                }
                continue;
            }

            let speed = run_speed(&phys, &p, radius);
            let mut step = p.push;
            p.push = [0.0; 3];
            let mut ran = 0.0;
            if speed > 0.0 {
                // Straight down the surface: gravity less its part along the normal.
                if let Some(down) = normalize(madd([0.0, -1.0, 0.0], normal, ny)) {
                    ran = speed * dt;
                    step = madd(step, down, ran);
                }
            }
            let mut moved = false;
            let mut flies = false;
            let step_sq = length_sq(step);
            if step_sq > MOVED * MOVED {
                moved = true;
                let step_len = step_sq.sqrt();
                let way = scale(step, 1.0 / step_len);
                let corner = level
                    .raycast(madd(p.pos, normal, LOOK_LIFT), way, step_len + LOOK_AHEAD)
                    .filter(|hit| dot(hit.normal, normal) < 0.9995);
                if let Some(hit) = corner {
                    if ran * 2.0 >= step_len {
                        // Running into a corner: the drop goes round it.
                        p.pos = hit.point;
                        p.normal = normalize(hit.normal).unwrap_or(normal);
                    } else {
                        // Merely crowded against a wall: it stays on its surface.
                        let along = (hit.distance - LOOK_LIFT).max(0.0);
                        p.pos = madd(p.pos, way, along);
                    }
                } else {
                    let next = add(p.pos, step);
                    let seen = level.raycast(
                        madd(next, normal, SNAP_LIFT),
                        scale(normal, -1.0),
                        SNAP_REACH,
                    );
                    match seen {
                        Some(hit) => {
                            p.pos = hit.point;
                            p.normal = normalize(hit.normal).unwrap_or(normal);
                        }
                        None => {
                            // The surface has ended: it lets go and drips.
                            p.pos = next;
                            p.vel = scale(step, 1.0 / dt);
                            flies = true;
                        }
                    }
                }
            }
            if flies {
                p.lying = false;
                p.flight_time = 0.0;
                p.run = 0.0;
                p.long = 1.0;
                if let Some(slot) = self.parts.get_mut(i) {
                    *slot = p;
                }
                continue;
            }

            // A thin trail is left every two radii run.
            if ran > 0.0 {
                p.run += ran;
                if p.run >= 2.0 * radius {
                    p.run = 0.0;
                    self.leave_trail(&mut p, &phys, speed, before, normal, radius);
                }
            }

            if moved || p.changed {
                p.still = 0;
                p.changed = false;
            } else {
                p.still += ticks;
            }
            p.calm = length(step) < CALM_SPEED * dt;
            if let Some(slot) = self.parts.get_mut(i) {
                *slot = p;
            }
        }
    }

    /// A running drop `p` leaves a thin lying drop where it was, taking its
    /// volume from itself (a Landau-Levich-Derjaguin film), when there is
    /// room to spare and the film is worth keeping.
    fn leave_trail(
        &mut self,
        p: &mut Particle,
        phys: &Physics,
        speed: f32,
        place: [f32; 3],
        normal: [f32; 3],
        radius: f32,
    ) {
        if self.parts.len() + TRAIL_ROOM >= MOST_DROPS {
            return;
        }
        let film = phys.trail_film(speed, p.h);
        let volume = 4.0 * radius * radius * film;
        if volume < MIN_VOLUME || p.volume - volume < MIN_VOLUME {
            return;
        }
        self.born += 1;
        let mut trail = Particle::lying(p.kind, place, normal, volume, film, self.born);
        // A thin film dries sooner.
        trail.dry_left = 0.3 * phys.dries;
        p.volume -= volume;
        self.parts.push(trail);
    }

    /// Lying drops of kinds that dry lose volume until they are gone.
    pub(crate) fn dry(&mut self, dt: f32) {
        for p in self.parts.iter_mut() {
            if !p.lying || p.dead {
                continue;
            }
            let dries = self
                .physics
                .get(usize::from(p.kind))
                .is_some_and(|f| f.dries > 0.0);
            if !dries {
                continue;
            }
            if p.dry_left <= dt {
                p.dead = true;
                continue;
            }
            p.volume *= (p.dry_left - dt) / p.dry_left;
            p.dry_left -= dt;
            if p.volume < MIN_VOLUME {
                p.dead = true;
            }
        }
    }
}

/// Drop `i` (awake, and read as `a`) meets drop `j`: they merge, or are
/// pushed apart. Says whether drop `i` was changed by a merge.
fn meet(
    parts: &mut [Particle],
    physics: &[Physics; MOST_KINDS],
    a: &Particle,
    i: usize,
    j: usize,
    dt: f32,
) -> bool {
    let Some(b) = parts.get(j) else {
        return false;
    };
    if b.dead || !b.lying || a.kind != b.kind {
        return false;
    }
    // A pair that are both awake is handled once, from the lower index.
    if b.still < SLEEP_TICKS && j < i {
        return false;
    }
    let off = sub(b.pos, a.pos);
    let height = dot(off, a.normal);
    if height.abs() >= SAME_PLANE || dot(a.normal, b.normal) <= SAME_FACING {
        return false;
    }
    let across = in_plane(off, a.normal);
    let d_sq = length_sq(across);
    // A square-root-free look first: (ra + rb)^2 is never above 2 (ra^2 + rb^2).
    let (ra_sq, rb_sq) = (a.volume / (PI * a.h), b.volume / (PI * b.h));
    if d_sq >= 2.0 * (ra_sq + rb_sq) {
        return false;
    }
    let (ra, rb) = (ra_sq.sqrt(), rb_sq.sqrt());
    if d_sq >= (ra + rb) * (ra + rb) {
        return false;
    }
    let d = d_sq.sqrt();
    let b = *b;
    let volume = a.volume + b.volume;
    let h = a.h.max(b.h);
    let wide = (volume / (PI * h)).sqrt();
    if wide <= R_MAX {
        let (keep, gone, kept_at, gone_at) = if i < j { (*a, b, i, j) } else { (b, *a, j, i) };
        let share = gone.volume / volume;
        let mut merged = keep;
        merged.pos = madd(keep.pos, sub(gone.pos, keep.pos), share);
        merged.normal = normalize(madd(keep.normal, sub(gone.normal, keep.normal), share))
            .unwrap_or(keep.normal);
        merged.volume = volume;
        merged.h = h;
        merged.h_ref = h;
        merged.dry_left = keep.dry_left + (gone.dry_left - keep.dry_left) * share;
        merged.born = keep.born.max(gone.born);
        merged.push = add(keep.push, gone.push);
        merged.still = 0;
        merged.calm = false;
        if let Some(slot) = parts.get_mut(kept_at) {
            *slot = merged;
        }
        if let Some(slot) = parts.get_mut(gone_at) {
            slot.dead = true;
        }
        return true;
    }
    let overlap = ra + rb - d;
    if overlap <= LEAST_PUSH {
        return false;
    }
    let Some(phys) = physics.get(usize::from(a.kind)) else {
        return false;
    };
    let way = normalize(across).unwrap_or_else(|| frame(a.normal).0);
    // Thin fluids part at once; thick ones creep apart.
    let by_a = (0.5 * overlap).min(creep_speed(phys, a, ra) * dt);
    let by_b = (0.5 * overlap).min(creep_speed(phys, &b, rb) * dt);
    // Too thick to move this far in a tick: it stays where it is.
    if by_a <= MOVED && by_b <= MOVED {
        return false;
    }
    if let Some(slot) = parts.get_mut(i) {
        slot.push = madd(slot.push, way, -by_a);
        slot.calm = false;
    }
    if let Some(slot) = parts.get_mut(j) {
        slot.push = madd(slot.push, way, by_b);
        if by_b > MOVED {
            slot.still = 0;
            slot.calm = false;
        }
    }
    false
}
