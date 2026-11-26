//! How a held weapon is seen to move: kicked by each shot, swaying as the
//! view turns and bobbing as the player walks, lifted when it is brought
//! up and tilted through a reload; its slide or bolt worked, its magazine
//! out and in, and spent cases thrown out of it to tumble to the ground.
//!
//! None of it is the game's rules: the rules are the weapon's own state
//! (`lore_weapons::Weapon`), stepped on the tick. This only shows that
//! state, so it may ease and bounce as it likes. What happens at a moment
//! (a shot, a switch, a case thrown) is timed here on the tick, and the
//! pose is worked out from those times for each frame.
//!
//! A model's moving parts are found by the names of its objects: one
//! whose name has `slide`, `bolt`, `pump`, `magazine` (or `mag`),
//! `trigger` or `hammer` in it moves as that part does.

use std::f32::consts::PI;
use std::rc::Rc;
use std::sync::Arc;

use lore_assets::{Scene, Transform};
use lore_physics::Level;
use lore_script::HeldWeapon;
use lore_weapons::Action;

use crate::props;

/// The parts of a weapon that move by themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Slide,
    Bolt,
    Pump,
    Magazine,
    Trigger,
    Hammer,
}

impl Part {
    /// The part an object called `name` is, if any.
    fn named(name: &str) -> Option<Self> {
        let name = name.to_lowercase();
        [
            ("slide", Self::Slide),
            ("bolt", Self::Bolt),
            ("pump", Self::Pump),
            ("magazine", Self::Magazine),
            ("mag_", Self::Magazine),
            ("trigger", Self::Trigger),
            ("hammer", Self::Hammer),
        ]
        .iter()
        .find(|(word, _)| name.contains(word))
        .map(|(_, part)| *part)
    }

    const ALL: [Self; 6] = [
        Self::Slide,
        Self::Bolt,
        Self::Pump,
        Self::Magazine,
        Self::Trigger,
        Self::Hammer,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|part| *part == self).unwrap_or(0)
    }
}

/// A weapon model in pieces: what stays still, and each part that moves.
#[derive(Debug, Clone)]
pub(crate) struct Pieces {
    pub(crate) still: Arc<Scene>,
    pub(crate) parts: Vec<(Part, Arc<Scene>)>,
}

/// `scene` in pieces, by the names of its objects. A model with no part
/// named is one still piece.
pub(crate) fn split(scene: &Arc<Scene>) -> Pieces {
    let part_of = |index: usize| {
        scene
            .instances
            .get(index)
            .and_then(|instance| instance.name.as_deref())
            .and_then(Part::named)
    };
    if (0..scene.instances.len()).all(|index| part_of(index).is_none()) {
        return Pieces {
            still: Arc::clone(scene),
            parts: Vec::new(),
        };
    }
    // Each piece keeps the materials and pictures, and only the shapes its
    // own objects use.
    let piece = |keep: &dyn Fn(Option<Part>) -> bool| {
        let mut out = Scene {
            materials: scene.materials.clone(),
            textures: scene.textures.clone(),
            images: scene.images.clone(),
            ..Scene::default()
        };
        let mut moved: Vec<Option<u32>> = vec![None; scene.meshes.len()];
        for (index, instance) in scene.instances.iter().enumerate() {
            if !keep(part_of(index)) {
                continue;
            }
            let Some(mesh) = scene.meshes.get(instance.mesh as usize) else {
                continue;
            };
            let slot = moved.get_mut(instance.mesh as usize);
            let number = match slot {
                Some(Some(number)) => *number,
                Some(slot) => {
                    // A few hundred shapes at most.
                    #[allow(clippy::cast_possible_truncation)]
                    let number = out.meshes.len() as u32;
                    out.meshes.push(mesh.clone());
                    *slot = Some(number);
                    number
                }
                None => continue,
            };
            let mut placed = instance.clone();
            placed.mesh = number;
            out.instances.push(placed);
        }
        Arc::new(out)
    };
    let still = piece(&|part| part.is_none());
    let parts = Part::ALL
        .iter()
        .filter(|part| (0..scene.instances.len()).any(|index| part_of(index) == Some(**part)))
        .map(|part| (*part, piece(&|of| of == Some(*part))))
        .collect();
    Pieces { still, parts }
}

/// How a weapon is posed for one frame: the whole of it, in its own space,
/// and each moving part from where it was made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pose {
    pub(crate) whole: Transform,
    pub(crate) parts: [Transform; 6],
}

impl Pose {
    pub(crate) fn part(&self, part: Part) -> &Transform {
        &self.parts[part.index()]
    }
}

/// A spent case on its way to the ground, or lying there.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Case {
    at: [f32; 3],
    moving: [f32; 3],
    /// How it is turned, about its own axes: radians.
    turned: [f32; 3],
    spin: [f32; 3],
    /// Seconds since it was thrown out.
    age: f32,
    resting: bool,
}

/// How long the hands are empty after a throw, following it through,
/// before the weapon held comes back up: seconds.
pub(crate) const THROW_FOLLOW: f32 = 0.5;

/// How an armed grenade is held, wound back over the shoulder: up, back
/// and tipped back, in its own space.
pub(crate) fn wound_back() -> Transform {
    moved([0.05, 0.11, 0.03], 0.75, 0.0, -0.2)
}

/// Where a thing in flight is drawn, and how it is turned: along its
/// flight, turning as `spin` says with its age, or lying on its side once
/// it has stopped.
pub(crate) fn flying(spin: lore_weapons::Spin, projectile: &lore_weapons::Projectile) -> Transform {
    let [x, y, z] = projectile.velocity;
    let speed = (x * x + y * y + z * z).sqrt();
    let age = projectile.age;
    let [px, py, pz] = projectile.position;
    let mut place = if speed < 0.3 {
        // At rest: on its side, turned by where it lies.
        moved(
            [0.0; 3],
            0.0,
            (px * 3.1 + pz * 1.7).rem_euclid(2.0 * PI),
            PI * 0.5,
        )
    } else {
        // Its muzzle (or head) along its flight.
        let yaw = libm::atan2f(-x, -z);
        let pitch = libm::asinf((y / speed).clamp(-1.0, 1.0));
        let along = moved([0.0; 3], pitch, yaw, 0.0);
        let turning = match spin {
            lore_weapons::Spin::None => lore_anim::math::IDENTITY,
            // Head over handle, three turns a second.
            lore_weapons::Spin::EndOverEnd => moved([0.0; 3], -age * 6.0 * PI, 0.0, 0.0),
            lore_weapons::Spin::Tumble => {
                moved([0.0; 3], -age * 4.0 * PI, age * 2.5 * PI, age * 1.3 * PI)
            }
        };
        lore_anim::math::combine(&along, &turning)
    };
    place[3] = [px, py, pz, 1.0];
    place
}

/// The most spent cases kept: the oldest goes when another comes.
const MOST_CASES: usize = 32;

/// How long a spent case lies on the ground, in seconds.
const CASE_LIFE: f32 = 12.0;

/// What the motion of the weapon in the player's hands remembers.
#[derive(Debug)]
pub(crate) struct WeaponMotion {
    /// The weapon held at the last tick, to see a switch.
    held: Option<Rc<str>>,
    /// Seconds left of bringing the weapon up, and how long it takes.
    raising: f32,
    raise_time: f32,
    /// Seconds since the last shot, as seen here (the weapon's own clock
    /// starts at nought before any shot).
    since_shot: f32,
    shots_before: (u32, u32),
    /// Where the view looked, eased: the weapon lags behind it.
    eased_look: (f32, f32),
    look: (f32, f32),
    /// How far through a stride the walk is, in radians, and how hard.
    stride: f32,
    stride_strength: f32,
    /// Whether this shot's case has been thrown (a bolt throws it when it
    /// is worked, not when it fires).
    case_due: bool,
    /// Whether a thrown weapon was armed at the last tick, and seconds
    /// since one was last thrown.
    armed_before: bool,
    since_throw: f32,
    cases: Vec<Case>,
    case_model: Arc<Scene>,
}

impl Default for WeaponMotion {
    fn default() -> Self {
        Self {
            held: None,
            raising: 0.0,
            raise_time: 0.0,
            since_shot: f32::MAX,
            shots_before: (0, 0),
            eased_look: (0.0, 0.0),
            look: (0.0, 0.0),
            stride: 0.0,
            stride_strength: 0.0,
            case_due: false,
            armed_before: false,
            since_throw: f32::MAX,
            cases: Vec::with_capacity(MOST_CASES),
            case_model: props::spent_case(),
        }
    }
}

/// `x` eased in and out between 0 and 1.
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A hump from 0 up to 1 and back over `t` from `from` to `to`.
fn hump(t: f32, from: f32, to: f32) -> f32 {
    if t <= from || t >= to || to <= from {
        return 0.0;
    }
    libm::sinf(PI * (t - from) / (to - from))
}

/// The shortest turn from `a` to `b`, in radians.
fn turn_to(a: f32, b: f32) -> f32 {
    let d = (b - a).rem_euclid(2.0 * PI);
    if d > PI { d - 2.0 * PI } else { d }
}

/// A move by `at` after turns about +X (`pitch`, muzzle up), +Y (`yaw`,
/// muzzle left) and +Z (`roll`, top to the left), in a weapon's space.
fn moved(at: [f32; 3], pitch: f32, yaw: f32, roll: f32) -> Transform {
    let (sp, cp) = (libm::sinf(pitch), libm::cosf(pitch));
    let (sy, cy) = (libm::sinf(yaw), libm::cosf(yaw));
    let (sr, cr) = (libm::sinf(roll), libm::cosf(roll));
    let about_x = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, cp, sp, 0.0],
        [0.0, -sp, cp, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let about_y = [
        [cy, 0.0, -sy, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [sy, 0.0, cy, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let about_z = [
        [cr, sr, 0.0, 0.0],
        [-sr, cr, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut out = lore_anim::math::combine(&about_y, &lore_anim::math::combine(&about_x, &about_z));
    out[3] = [at[0], at[1], at[2], 1.0];
    out
}

/// How far into working a bolt or pump between shots a weapon is, from 0
/// to 1, or `None` when it is not being worked.
fn working(held: &HeldWeapon, since_shot: f32) -> Option<f32> {
    if !matches!(held.settings.action, Action::Bolt | Action::Pump) {
        return None;
    }
    let between = 60.0 / held.settings.rounds_per_minute.max(1.0);
    // Worked from a fifth of the way to the next shot until nearly then.
    let t = (since_shot / between - 0.2) / 0.7;
    (0.0..1.0).contains(&t).then_some(t)
}

impl WeaponMotion {
    /// Steps what moves with time by one tick of `step` seconds: `held`
    /// is the weapon in the player's hands (if any) as it is after the
    /// tick, `look` where they look, `speed` how fast they walk (metres a
    /// second, along the ground) and `walk` their walking speed. A case a
    /// shot throws out comes from `eject`, the weapon as last drawn.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tick(
        &mut self,
        held: Option<&HeldWeapon>,
        thrown: Option<&HeldWeapon>,
        look: (f32, f32),
        speed: f32,
        walk: f32,
        moving: [f32; 3],
        eject: Option<&Transform>,
        level: &Level,
        step: f32,
    ) {
        self.step_cases(level, step);
        let name = held.map(|held| Rc::clone(&held.name));
        if name != self.held {
            // Brought up from below: what was held is gone at once.
            self.raise_time = held.map_or(0.0, |held| held.settings.raise_time.max(0.0));
            self.raising = self.raise_time;
            self.since_shot = f32::MAX;
            self.case_due = false;
            // A weapon brought up has fired nothing yet.
            self.shots_before =
                held.map_or((0, 0), |held| (held.state.magazine, held.state.reserve));
            self.held = name;
        }
        // A throw: armed at the last tick, not now. The hands follow it
        // through, then the weapon held comes up again.
        let armed = [held, thrown]
            .iter()
            .flatten()
            .any(|weapon| weapon.state.armed.is_some());
        if self.armed_before && !armed {
            self.since_throw = 0.0;
        } else {
            let was = self.since_throw;
            self.since_throw = self.since_throw.saturating_add_seconds(step);
            if was < THROW_FOLLOW && self.since_throw >= THROW_FOLLOW {
                self.raising = self.raise_time;
            }
        }
        self.armed_before = armed;
        self.raising = (self.raising - step).max(0.0);
        // The view eases after the look: the weapon trails a turn.
        self.look = look;
        let ease = (step * 14.0).min(1.0);
        self.eased_look.0 += turn_to(self.eased_look.0, look.0) * ease;
        self.eased_look.1 += (look.1 - self.eased_look.1) * ease;
        // A stride is about 1.4 m: the weapon bobs twice in it.
        let strength = if walk > 0.0 {
            (speed / walk).min(1.5)
        } else {
            0.0
        };
        self.stride_strength += (strength - self.stride_strength) * (step * 8.0).min(1.0);
        self.stride = (self.stride + speed * step * (2.0 * PI / 1.4)).rem_euclid(2.0 * PI);
        let Some(held) = held else {
            return;
        };
        // A shot: the magazine went down, or a one-round weapon emptied.
        let shots = (held.state.magazine, held.state.reserve);
        let fired = held.state.since_shot < step * 1.5 && shots != self.shots_before;
        self.shots_before = shots;
        self.since_shot = if fired {
            0.0
        } else {
            self.since_shot.saturating_add_seconds(step)
        };
        if fired && held.settings.ejects {
            if matches!(held.settings.action, Action::Automatic) {
                self.throw_case(held, eject, moving);
            } else {
                self.case_due = true;
            }
        }
        // A bolt or pump throws the case when it is pulled back.
        if self.case_due && working(held, self.since_shot).is_some_and(|t| t > 0.35) {
            self.case_due = false;
            self.throw_case(held, eject, moving);
        }
    }

    /// Throws a spent case out of `held`'s ejection port, to the right and
    /// up, carried along with the player.
    fn throw_case(&mut self, held: &HeldWeapon, weapon: Option<&Transform>, moving: [f32; 3]) {
        let Some(weapon) = weapon else {
            return;
        };
        let at = lore_anim::math::apply(weapon, held.settings.eject_at);
        let column = |i: usize| [weapon[i][0], weapon[i][1], weapon[i][2]];
        let (right, up, back) = (column(0), column(1), column(2));
        // A little different each time, from how many have been thrown.
        let n = self.cases.len() as f32;
        let wobble = libm::sinf(n * 2.3) * 0.3;
        let thrown: [f32; 3] = std::array::from_fn(|i| {
            moving[i] + right[i] * (2.2 + wobble) + up[i] * (1.6 - wobble) + back[i] * 0.4
        });
        if self.cases.len() == MOST_CASES {
            self.cases.remove(0);
        }
        self.cases.push(Case {
            at,
            moving: thrown,
            turned: [0.0, n * 0.7, 0.0],
            spin: [14.0 + wobble * 10.0, 3.0, 9.0],
            age: 0.0,
            resting: false,
        });
    }

    /// Moves each case one tick on: falling, bouncing off what it meets,
    /// and lying still once it has nearly stopped.
    fn step_cases(&mut self, level: &Level, step: f32) {
        for case in &mut self.cases {
            case.age += step;
            if case.resting {
                continue;
            }
            case.moving[1] -= 9.8 * step;
            let travel: [f32; 3] = case.moving.map(|v| v * step);
            let far =
                (travel[0] * travel[0] + travel[1] * travel[1] + travel[2] * travel[2]).sqrt();
            let hit = (far > 1e-6)
                .then(|| level.raycast(case.at, travel.map(|v| v / far), far + 0.01))
                .flatten();
            match hit {
                Some(hit) => {
                    case.at = std::array::from_fn(|i| hit.point[i] + hit.normal[i] * 0.005);
                    // Bounces back off the surface, losing most of its way.
                    let into = (0..3).map(|i| case.moving[i] * hit.normal[i]).sum::<f32>();
                    case.moving = std::array::from_fn(|i| {
                        (case.moving[i] - 2.0 * into * hit.normal[i]) * 0.35
                    });
                    case.spin = case.spin.map(|v| v * 0.5);
                    let speed = case.moving.iter().map(|v| v * v).sum::<f32>().sqrt();
                    if speed < 0.4 && hit.normal[1] > 0.7 {
                        case.resting = true;
                        // Lying on its side.
                        case.turned = [PI * 0.5, case.turned[1], 0.0];
                    }
                }
                None => case.at = std::array::from_fn(|i| case.at[i] + travel[i]),
            }
            if !case.resting {
                case.turned = std::array::from_fn(|i| case.turned[i] + case.spin[i] * step);
            }
        }
        self.cases.retain(|case| case.age < CASE_LIFE);
    }

    /// Whether a throw was let go on the last tick: when the body's throw
    /// clip is played.
    pub(crate) fn just_thrown(&self) -> bool {
        self.since_throw == 0.0
    }

    /// Whether the hands are following a throw through, holding nothing.
    pub(crate) fn following_through(&self) -> bool {
        self.since_throw < THROW_FOLLOW
    }

    /// The model of a spent case, and where each one is: for drawing.
    pub(crate) fn cases(&self) -> (&Arc<Scene>, impl Iterator<Item = Transform> + '_) {
        let placed = self.cases.iter().map(|case| {
            let mut place = moved([0.0; 3], case.turned[0], case.turned[1], case.turned[2]);
            place[3] = [case.at[0], case.at[1], case.at[2], 1.0];
            place
        });
        (&self.case_model, placed)
    }

    /// How `held` is posed this frame.
    pub(crate) fn pose(&self, held: &HeldWeapon) -> Pose {
        let state = &held.state;
        let settings = &held.settings;
        let mut parts = [lore_anim::math::IDENTITY; 6];
        let mut set = |part: Part, at: [f32; 3], pitch: f32| {
            parts[part.index()] = moved(at, pitch, 0.0, 0.0);
        };
        // Each shot: back into the shoulder and up, settling at once.
        let kick = if self.since_shot < 0.4 {
            libm::expf(-self.since_shot * 16.0)
        } else {
            0.0
        };
        let mut at = [0.0, 0.0, 0.035 * kick];
        let mut pitch = 0.08 * kick + state.kick[0] * 0.5;
        let mut yaw = state.kick[1] * 0.5;
        let mut roll = 0.0;
        // The slide or bolt goes back with the shot and home again; with
        // nothing left in the magazine a slide stays back.
        let empty = state.magazine == 0 && state.reload.is_none();
        let between = (60.0 / settings.rounds_per_minute.max(1.0)).min(0.1);
        // Thrown back at once by the shot, eased home before the next.
        let cycle = if self.since_shot < between {
            1.0 - smooth(self.since_shot / between)
        } else {
            0.0
        };
        match settings.action {
            Action::Automatic => {
                let back = if empty { 1.0 } else { cycle };
                set(Part::Slide, [0.0, 0.0, 0.028 * back], 0.0);
                set(Part::Bolt, [0.0, 0.0, 0.045 * back], 0.0);
                set(Part::Hammer, [0.0, 0.0, 0.004 * back], -0.6 * back);
            }
            Action::Bolt => {
                if let Some(t) = working(held, self.since_shot) {
                    // Up, back, forward, down; the weapon tipped to it.
                    let back = hump(t, 0.15, 0.85);
                    set(Part::Bolt, [0.0, 0.0, 0.075 * back], 0.0);
                    roll += 0.25 * hump(t, 0.0, 1.0);
                    pitch -= 0.06 * hump(t, 0.0, 1.0);
                }
            }
            Action::Pump => {
                if let Some(t) = working(held, self.since_shot) {
                    set(Part::Pump, [0.0, 0.0, 0.08 * hump(t, 0.05, 0.95)], 0.0);
                    pitch += 0.04 * hump(t, 0.0, 1.0);
                }
            }
            Action::None => {}
        }
        if state.trigger_held {
            set(Part::Trigger, [0.0, 0.0, 0.004], 0.2);
        }
        // A reload: tipped over and lowered, the magazine out and back in.
        if let Some(t) = state.reload_progress(settings) {
            let tip = hump(t, 0.0, 1.0);
            at[1] -= 0.05 * tip;
            roll += 0.45 * tip;
            pitch -= 0.15 * tip;
            let out = smooth((t - 0.1) / 0.2) * (1.0 - smooth((t - 0.55) / 0.25));
            set(Part::Magazine, [0.0, -0.22 * out, 0.03 * out], 0.3 * out);
        }
        // Brought up from below after a switch.
        if self.raise_time > 0.0 {
            let down = smooth(self.raising / self.raise_time);
            at[1] -= 0.18 * down;
            pitch -= 0.7 * down;
        }
        // Trailing a turn of the view, and bobbing with the stride.
        let lag_yaw = turn_to(self.look.0, self.eased_look.0).clamp(-0.12, 0.12);
        let lag_pitch = (self.eased_look.1 - self.look.1).clamp(-0.1, 0.1);
        yaw += lag_yaw * 0.6;
        pitch += lag_pitch * 0.6;
        at[0] += lag_yaw * 0.04;
        let bob = self.stride_strength;
        at[0] += libm::sinf(self.stride) * 0.008 * bob;
        at[1] -= libm::fabsf(libm::cosf(self.stride)) * 0.01 * bob;
        Pose {
            whole: moved(at, pitch, yaw, roll),
            parts,
        }
    }
}

/// Adds seconds to a time that may be "never".
trait Seconds {
    fn saturating_add_seconds(self, seconds: f32) -> Self;
}

impl Seconds for f32 {
    fn saturating_add_seconds(self, seconds: f32) -> Self {
        if self >= f32::MAX - seconds {
            f32::MAX
        } else {
            self + seconds
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lore_weapons::{Weapon, WeaponSettings};

    fn held(settings: WeaponSettings) -> HeldWeapon {
        HeldWeapon {
            name: Rc::from("gun"),
            settings,
            model: None,
            hold: lore_script::HoldSaid::default(),
            sounds: lore_script::WeaponSounds::default(),
            state: Weapon::new(&settings),
        }
    }

    fn flat() -> Level {
        let w = 50.0;
        Level::from_triangles(&[
            [[-w, 0.0, -w], [w, 0.0, -w], [w, 0.0, w]],
            [[-w, 0.0, -w], [w, 0.0, w], [-w, 0.0, w]],
        ])
    }

    #[test]
    fn parts_are_found_by_name() {
        assert_eq!(Part::named("service_pistol_slide_a"), Some(Part::Slide));
        assert_eq!(Part::named("Bolt_Action_Rifle_bolt_a"), Some(Part::Bolt));
        assert_eq!(Part::named("rifle_mag_01"), Some(Part::Magazine));
        assert_eq!(Part::named("stock"), None);
        let pieces = split(&props::weapon("pistol").unwrap());
        assert!(pieces.parts.iter().any(|(part, _)| *part == Part::Slide));
        let still: usize = pieces.still.instances.len();
        let moving: usize = pieces.parts.iter().map(|(_, p)| p.instances.len()).sum();
        assert_eq!(
            still + moving,
            props::weapon("pistol").unwrap().instances.len()
        );
    }

    #[test]
    fn a_shot_kicks_the_weapon_back_works_the_slide_and_throws_a_case() {
        let level = flat();
        let mut motion = WeaponMotion::default();
        let mut gun = held(WeaponSettings::pistol());
        let step = 1.0 / 60.0;
        let weapon = moved([0.0, 1.4, 0.0], 0.0, 0.0, 0.0);
        motion.tick(
            Some(&gun),
            None,
            (0.0, 0.0),
            0.0,
            4.5,
            [0.0; 3],
            Some(&weapon),
            &level,
            step,
        );
        for _ in 0..60 {
            motion.tick(
                Some(&gun),
                None,
                (0.0, 0.0),
                0.0,
                4.5,
                [0.0; 3],
                Some(&weapon),
                &level,
                step,
            );
        }
        let still = motion.pose(&gun);
        assert!(
            still.whole[3][2].abs() < 1e-4,
            "nothing moves before a shot"
        );
        // A shot.
        gun.state.magazine -= 1;
        gun.state.since_shot = 0.0;
        motion.tick(
            Some(&gun),
            None,
            (0.0, 0.0),
            0.0,
            4.5,
            [0.0; 3],
            Some(&weapon),
            &level,
            step,
        );
        let kicked = motion.pose(&gun);
        assert!(
            kicked.whole[3][2] > 0.02,
            "back into the shoulder: {:?}",
            kicked.whole[3]
        );
        assert!(kicked.part(Part::Slide)[3][2] > 0.01, "the slide goes back");
        assert_eq!(motion.cases.len(), 1);
        // The case falls to the floor and lies there.
        for _ in 0..240 {
            gun.state.since_shot += step;
            motion.tick(
                Some(&gun),
                None,
                (0.0, 0.0),
                0.0,
                4.5,
                [0.0; 3],
                Some(&weapon),
                &level,
                step,
            );
        }
        let case = motion.cases[0];
        assert!(case.resting && case.at[1].abs() < 0.02, "{case:?}");
        let settled = motion.pose(&gun);
        assert!(settled.whole[3][2].abs() < 1e-3);
        assert!(settled.part(Part::Slide)[3][2].abs() < 1e-4);
    }

    #[test]
    fn an_empty_pistol_holds_its_slide_back_and_a_reload_drops_the_magazine() {
        let mut gun = held(WeaponSettings::pistol());
        gun.state.magazine = 0;
        let motion = WeaponMotion::default();
        assert!(motion.pose(&gun).part(Part::Slide)[3][2] > 0.02);
        gun.state.reload = Some(lore_weapons::Reload {
            elapsed: gun.settings.empty_reload_time * 0.4,
            from_empty: true,
            loaded: false,
        });
        let pose = motion.pose(&gun);
        assert!(
            pose.part(Part::Magazine)[3][1] < -0.1,
            "{:?}",
            pose.part(Part::Magazine)
        );
    }

    #[test]
    fn a_bolt_is_worked_after_the_shot_and_throws_its_case_then() {
        let level = flat();
        let settings = WeaponSettings {
            action: Action::Bolt,
            rounds_per_minute: 45.0,
            ..WeaponSettings::default()
        };
        let mut gun = held(settings);
        let mut motion = WeaponMotion::default();
        let step = 1.0 / 60.0;
        let weapon = moved([0.0, 1.4, 0.0], 0.0, 0.0, 0.0);
        motion.tick(
            Some(&gun),
            None,
            (0.0, 0.0),
            0.0,
            4.5,
            [0.0; 3],
            Some(&weapon),
            &level,
            step,
        );
        gun.state.magazine -= 1;
        gun.state.since_shot = 0.0;
        motion.tick(
            Some(&gun),
            None,
            (0.0, 0.0),
            0.0,
            4.5,
            [0.0; 3],
            Some(&weapon),
            &level,
            step,
        );
        assert!(
            motion.cases.is_empty(),
            "a bolt throws its case when worked"
        );
        let mut furthest = 0.0_f32;
        for _ in 0..80 {
            gun.state.since_shot += step;
            motion.tick(
                Some(&gun),
                None,
                (0.0, 0.0),
                0.0,
                4.5,
                [0.0; 3],
                Some(&weapon),
                &level,
                step,
            );
            furthest = furthest.max(motion.pose(&gun).part(Part::Bolt)[3][2]);
        }
        assert!(furthest > 0.06, "the bolt went back {furthest} m");
        assert_eq!(motion.cases.len(), 1);
    }
}
