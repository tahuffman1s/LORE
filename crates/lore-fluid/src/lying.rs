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
/// How much of the smaller of two neighbouring patches of one kind may lie
/// under the other before they push apart.
const SHEET_OVERLAP: f32 = 0.5;
/// A patch as wide as it may be splits in two once it is this many times as
/// thick as a puddle of its kind.
const SPLIT_THICK: f32 = 2.0;
/// A small drop that meets a patch of its own kind at most this share of
/// the patch's volume joins it even when the two would be wider than a
/// patch may be: the patch grows thicker, and then splits.
const JOIN_SHARE: f32 = 0.25;
/// The thickest a patch may grow by small drops joining it, in puddle
/// thicknesses: past the split, so a patch splits before it stops taking.
const JOIN_THICKEST: f32 = 2.0 * SPLIT_THICK;
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

/// How much of itself a corrosive drop uses up per unit of volume it eats,
/// times its corrosiveness: a modelling choice, not a measured number.
const ACID_SPENT: f32 = 0.5;

impl Fluids {
    /// What a drop that has just landed does to the other kinds under it:
    /// washes them away, eats them (if it is corrosive), or lies on top and
    /// covers part of them.
    pub(crate) fn settle_landings(&mut self) {
        let Self {
            parts,
            grid,
            physics,
            landed,
            ..
        } = self;
        // Where it all landed is summed here and divided once below.
        let mut middle = [0.0_f32; 3];
        for i in 0..parts.len() {
            let Some(p) = parts.get(i).copied() else {
                continue;
            };
            if !(p.landed_now && p.lying && !p.dead) {
                continue;
            }
            let litres = p.volume * 1000.0;
            landed.litres += litres;
            middle = madd(middle, p.pos, litres);
            if litres > landed.biggest {
                (landed.biggest, landed.kind) = (litres, p.kind);
            }
            // What ran out of a pool is the pool's mix, one kind a stream:
            // the water in it does not wash the honey in it away, and what
            // left the pool is what lies below it.
            if p.of_a_mix {
                continue;
            }
            let (washes, corrosive) = physics
                .get(usize::from(p.kind))
                .map_or((false, 0.0), |f| (f.washes, f.corrosive));
            // Volume of other kinds this drop has eaten.
            let mut eaten = 0.0f32;
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
                // The part of the older patch the new one covers: the
                // overlap of two discs, approximated. Covering hides that
                // share; washing also carries off the thin rim around it,
                // so it takes twice the share (a modelling choice, not a
                // measured number). The rest of the drop stays.
                let small = radius.min(rq);
                let mut share = ((radius + rq - d) / (2.0 * small)).clamp(0.0, 1.0)
                    * (small * small / (rq * rq));
                let mut times = if washes { 2.0 } else { 1.0 };
                if corrosive > 0.0 {
                    times = f32::max(times, 1.0 + corrosive);
                }
                if times > 1.0 {
                    share = (times * share).min(1.0);
                }
                eaten += q.volume * share;
                q.volume *= 1.0 - share;
                if q.volume < MIN_VOLUME {
                    q.dead = true;
                }
            });
            if let Some(slot) = parts.get_mut(i) {
                if corrosive > 0.0 {
                    slot.volume = (slot.volume - corrosive * ACID_SPENT * eaten).max(0.0);
                    if slot.volume < MIN_VOLUME {
                        slot.dead = true;
                    }
                }
                slot.still = 0;
                slot.calm = false;
            }
        }
        if landed.litres > 0.0 {
            landed.place = scale(middle, 1.0 / landed.litres);
        }
        // Only now, so that which of two drops landing on each other this
        // tick lies on top does not hang on where each is kept.
        for p in parts.iter_mut() {
            p.landed_now = false;
        }
    }

    /// Everything the awake lying drops do in one tick: spread, merge or
    /// push apart, run, follow their surface, leave trails and fall asleep.
    pub(crate) fn lie(&mut self, level: &Level, dt: f32) {
        self.spread(level, dt);
        self.crowd(level, dt);
        self.run(level, dt);
    }

    /// Lying drops spread towards the thickness of a puddle. A drop that
    /// has spread as wide as a patch may be and is still much thicker than
    /// a puddle splits in two side by side, so that what a stream pours
    /// into one place spreads into a wider puddle instead of piling up.
    pub(crate) fn spread(&mut self, level: &Level, dt: f32) {
        for i in 0..self.parts.len() {
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
                if let Some(slot) = self.parts.get_mut(i) {
                    slot.dead = true;
                }
                continue;
            };
            // A drop already thinner than a puddle is a lens and stays one.
            if p.h <= phys.h_eq {
                continue;
            }
            let r = p.patch();
            // Never wider than R_MAX: a big drop settles thick, not wide.
            let r2 = (r + phys.spread_speed(p.h, r) * dt).min(R_MAX);
            p.h = (p.volume / (PI * r2 * r2)).max(phys.h_eq);
            if r2 >= R_MAX
                && p.h > SPLIT_THICK * phys.h_eq
                && self.parts.len() + TRAIL_ROOM < MOST_DROPS
            {
                // Half of it goes to one side and half to the other, each
                // as thick as the whole was: they then spread on their own
                // and push apart, quickly if thin and slowly if thick.
                let (u, v) = frame(p.normal);
                let turn = core::f32::consts::TAU * self.rolls.next();
                let way = add(scale(u, libm::cosf(turn)), scale(v, libm::sinf(turn)));
                let half = 0.5 * R_MAX;
                // Each half stops short of a wall and lies on the surface
                // it reaches; where one would go off an edge it stays whole.
                let there = moved_on(level, p.pos, p.normal, way, half);
                let here = moved_on(level, p.pos, p.normal, scale(way, -1.0), half);
                if let (Some((there, there_facing)), Some((here, here_facing))) = (there, here) {
                    p.volume *= 0.5;
                    self.born += 1;
                    let mut other =
                        Particle::lying(p.kind, there, there_facing, p.volume, p.h, self.born);
                    other.dry_left = p.dry_left;
                    p.pos = here;
                    p.normal = here_facing;
                    p.changed = true;
                    p.h_ref = p.h;
                    self.parts.push(other);
                }
            }
            // Thickness is compared with where it last counted as changed,
            // so a slow spread still keeps a drop awake now and then.
            if (p.h - p.h_ref).abs() > 0.01 * p.h_ref {
                p.h_ref = p.h;
                p.changed = true;
            }
            if let Some(slot) = self.parts.get_mut(i) {
                *slot = p;
            }
        }
    }

    /// Awake drops merge with same-kind drops they overlap, or push them
    /// apart when the merged patch would be too wide.
    pub(crate) fn crowd(&mut self, level: &Level, dt: f32) {
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
                // Read drop `i` afresh each time: an earlier meeting may have
                // pushed it or merged into it.
                if let Some(now) = parts.get(i) {
                    a = *now;
                }
                if j != i && !a.dead {
                    meet(level, parts, physics, &a, i, j, dt);
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
                // Off the ceiling's plane, or the ray it falls along next
                // meets the ceiling where it starts and it lands there again.
                p.pos = madd(p.pos, normal, LOOK_LIFT);
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
                    let facing = normalize(hit.normal).unwrap_or(normal);
                    // A surface whose own downhill runs back into this one
                    // (the foot of a wall, the bottom of a ditch) is the
                    // bottom: going round onto it would only bring the drop
                    // back, tick after tick.
                    let bottom = normalize(madd([0.0, -1.0, 0.0], facing, facing[1]))
                        .is_some_and(|down| dot(down, normal) < 0.0);
                    if ran * 2.0 >= step_len && !bottom {
                        // Running into a corner: the drop goes round it.
                        p.pos = hit.point;
                        p.normal = facing;
                    } else {
                        // Crowded against a wall, or at the bottom: it stays
                        // on its surface, and once it is there it is still.
                        let along = (hit.distance - LOOK_LIFT).max(0.0);
                        p.pos = madd(p.pos, way, along);
                        if along <= MOVED {
                            moved = false;
                            ran = 0.0;
                        }
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
                    self.leave_trail(level, &mut p, &phys, speed, before, normal, radius);
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
    #[expect(clippy::too_many_arguments)]
    fn leave_trail(
        &mut self,
        level: &Level,
        p: &mut Particle,
        phys: &Physics,
        speed: f32,
        before: [f32; 3],
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
        // The film lies behind the drop, clear of it: where the drop was a
        // tick ago it would overlap it and be taken straight back in.
        let trail_radius = (volume / (PI * film)).sqrt();
        let run = in_plane(sub(p.pos, before), normal);
        let ran = length(run);
        let Some(back) = normalize(scale(run, -1.0)) else {
            return;
        };
        let gap = radius + trail_radius - ran;
        let (place, facing) = if gap > 0.0 {
            match moved_on(level, before, normal, back, gap) {
                Some(found) => found,
                None => return,
            }
        } else {
            (before, normal)
        };
        self.born += 1;
        let mut trail = Particle::lying(p.kind, place, facing, volume, film, self.born);
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

/// Where a drop lying at `pos` on the surface facing `normal` comes to when
/// it is moved `far` along `way`: stopped short of a wall in the way, and on
/// the surface there. `None` where that surface has ended.
fn moved_on(
    level: &Level,
    pos: [f32; 3],
    normal: [f32; 3],
    way: [f32; 3],
    far: f32,
) -> Option<([f32; 3], [f32; 3])> {
    let along = level
        .raycast(madd(pos, normal, LOOK_LIFT), way, far + LOOK_AHEAD)
        .filter(|hit| dot(hit.normal, normal) < 0.9995)
        .map_or(far, |hit| (hit.distance - LOOK_LIFT - LOOK_AHEAD).max(0.0));
    let next = madd(pos, way, along);
    let hit = level.raycast(
        madd(next, normal, SNAP_LIFT),
        scale(normal, -1.0),
        SNAP_REACH,
    )?;
    Some((hit.point, normalize(hit.normal).unwrap_or(normal)))
}

/// Whether something solid stands between two lying drops, a little over
/// the surface they lie on.
fn parted(level: &Level, a: &Particle, b: &Particle) -> bool {
    let from = madd(a.pos, a.normal, LOOK_LIFT);
    let to = madd(b.pos, b.normal, LOOK_LIFT);
    let off = sub(to, from);
    let far = length_sq(off).sqrt();
    normalize(off).is_some_and(|way| level.raycast(from, way, far).is_some())
}

/// Drop `i` (awake, and read as `a`) meets drop `j`: they merge, or are
/// pushed apart. Says whether drop `i` was changed by a merge.
fn meet(
    level: &Level,
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
    // A small drop joins a full patch only while the patch is not already
    // as thick as a split makes it: with the store too full to split, the
    // two push apart instead of piling up without end.
    let h_eq = physics.get(usize::from(a.kind)).map_or(0.0, |f| f.h_eq);
    let joined_h = h.max(volume / (PI * R_MAX * R_MAX));
    let small_joins = a.volume.min(b.volume) <= JOIN_SHARE * a.volume.max(b.volume)
        && joined_h <= JOIN_THICKEST * h_eq;
    if wide <= R_MAX || small_joins {
        // Two drops with a wall between them, or round a corner from each
        // other, are not one puddle: joined, they would lie where the wall
        // stands, and seep on through it.
        if parted(level, a, &b) {
            return false;
        }
        // Never wider than a patch may be: what does not fit lies thicker.
        let h = h.max(volume / (PI * R_MAX * R_MAX));
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
    // Patches of one puddle are left overlapping by a share of the smaller,
    // so that together they lie as one sheet of liquid with no dry seams.
    let overlap = ra + rb - d - SHEET_OVERLAP * ra.min(rb);
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
