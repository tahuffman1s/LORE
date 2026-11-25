//! Weapons in a running game: the weapon the player holds is stepped by
//! what they press, its shots are judged against the level and whoever
//! else stands in the world, rockets and grenades are flown, and the
//! game's script is told what was fired, what was hit and when a reload
//! begins.
//!
//! Nothing here hurts anyone. A hit is handed to the script with what it is
//! worth, and the script decides what it means.

use std::rc::Rc;

use lore_physics::{Level, MoveSettings};
use lore_script::{CharacterId, HeldWeapon, ScriptHost, WeaponHit};
use lore_weapons::{
    Delivery, Flight, Projectile, Shot, Struck, Target, WeaponInput, WeaponSettings, blast_damage,
    hitscan, shot_seed,
};

use crate::noise::{Noise, NoiseKind};
use crate::world::{Body, Dummy};

/// The most rockets, grenades and slow bullets in flight at once. One more
/// takes the place of the oldest.
const MOST_IN_FLIGHT: usize = 1024;

/// Something launched and still on its way.
#[derive(Debug, Clone)]
struct InFlight {
    projectile: Projectile,
    /// The numbers of the weapon that launched it, as they were then.
    settings: WeaponSettings,
    /// The name of the weapon that launched it.
    weapon: Rc<str>,
}

/// What to tell the script once everything has been worked out.
#[derive(Debug, Clone)]
enum Told {
    Fired,
    Reloading,
    Hit(WeaponHit),
}

/// The weapons side of a game being played. Its lists are kept between
/// ticks so that a tick sets no memory aside.
#[derive(Debug, Default)]
pub(crate) struct Shooting {
    shots: Vec<Shot>,
    targets: Vec<Target>,
    in_flight: Vec<InFlight>,
    told: Vec<Told>,
    /// Where the camera is when it is not at the eyes (third person): shots
    /// go to what its crosshair is on, fired from the body.
    pub(crate) camera: Option<[f32; 3]>,
}

/// How far off the crosshair's mark is taken to be when it is on nothing:
/// the sky, far out.
const FAR_MARK: f32 = 1000.0;

/// Where a shot fired along `direction` should go, from `eyes`, when the
/// picture is seen from `camera`: the point the crosshair is on, found by a
/// ray from the camera that starts level with the body (so nothing between
/// the camera and the player is taken for the mark). With the camera at the
/// eyes, `direction` as it is.
fn toward_mark(
    level: &Level,
    targets: &[Target],
    camera: Option<[f32; 3]>,
    eyes: [f32; 3],
    direction: [f32; 3],
    settings: &lore_weapons::WeaponSettings,
) -> [f32; 3] {
    let Some(camera) = camera else {
        return direction;
    };
    let gap: [f32; 3] = std::array::from_fn(|i| eyes[i] - camera[i]);
    if gap.iter().map(|g| g * g).sum::<f32>() < 0.05 * 0.05 {
        return direction;
    }
    let skip = (0..3).map(|i| gap[i] * direction[i]).sum::<f32>().max(0.0);
    let from: [f32; 3] = std::array::from_fn(|i| camera[i] + direction[i] * skip);
    let mark = hitscan(level, targets, from, direction, settings, Some(PLAYER)).map_or_else(
        || std::array::from_fn(|i| from[i] + direction[i] * FAR_MARK),
        |hit| hit.point,
    );
    let way: [f32; 3] = std::array::from_fn(|i| mark[i] - eyes[i]);
    let length = way.iter().map(|w| w * w).sum::<f32>().sqrt();
    if length > 1e-3 && length.is_finite() {
        way.map(|w| w / length)
    } else {
        direction
    }
}

/// Closer than this to the crosshair's mark, a shot is taken to have got
/// there: only cover that stops it well short is worth telling the player.
const BLOCKED_SHORT: f32 = 0.3;

impl Shooting {
    /// Where a shot fired now along `direction` would land, and whether
    /// something stops it short of what the crosshair is on: cover that
    /// is in front of the body but not of the camera over its shoulder.
    /// With the camera at the eyes nothing can be in between.
    pub(crate) fn aim(
        &mut self,
        eyes: [f32; 3],
        dummies: &[Dummy],
        level: &Level,
        rules: &MoveSettings,
        direction: [f32; 3],
    ) -> ([f32; 3], bool) {
        let settings = lore_weapons::WeaponSettings::default();
        self.targets.clear();
        self.targets
            .extend(dummies.iter().map(|dummy| dummy_target(dummy, rules)));
        let way = toward_mark(
            level,
            &self.targets,
            self.camera,
            eyes,
            direction,
            &settings,
        );
        let landed = hitscan(level, &self.targets, eyes, way, &settings, None).map(|hit| hit.point);
        let far = |from: [f32; 3], way: [f32; 3]| -> [f32; 3] {
            std::array::from_fn(|i| from[i] + way[i] * FAR_MARK)
        };
        let Some(camera) = self.camera else {
            return (landed.unwrap_or_else(|| far(eyes, way)), false);
        };
        // The mark again, as the shot was aimed at it.
        let gap: [f32; 3] = std::array::from_fn(|i| eyes[i] - camera[i]);
        let skip = (0..3).map(|i| gap[i] * direction[i]).sum::<f32>().max(0.0);
        let from: [f32; 3] = std::array::from_fn(|i| camera[i] + direction[i] * skip);
        let mark = hitscan(
            level,
            &self.targets,
            from,
            direction,
            &settings,
            Some(PLAYER),
        )
        .map_or_else(|| far(from, direction), |hit| hit.point);
        let reach = |point: [f32; 3]| -> f32 {
            (0..3)
                .map(|i| (point[i] - eyes[i]).powi(2))
                .sum::<f32>()
                .sqrt()
        };
        match landed {
            Some(landed) if reach(landed) < reach(mark) - BLOCKED_SHORT => (landed, true),
            _ => (mark, false),
        }
    }
}

/// The number a weapon knows the player by: who launched a rocket, and
/// whose body a shot landed on. Dummies are numbered from 1.
const PLAYER: u32 = 0;

/// The player's body as something to shoot.
fn player_target(body: &Body, rules: &MoveSettings) -> Target {
    Target {
        id: PLAYER,
        feet: body.player.position,
        height: if body.player.crouched {
            rules.crouched_height
        } else {
            rules.standing_height
        },
        radius: rules.radius,
        // The head is where the eyes are, crouched or standing.
        head: Some({
            let eye = body.player.eye_position();
            [eye[0], eye[1] + 0.05, eye[2]]
        }),
    }
}

/// A training dummy as something to shoot: the size of a standing person.
fn dummy_target(dummy: &Dummy, rules: &MoveSettings) -> Target {
    Target {
        id: dummy.id.0,
        feet: dummy.feet,
        height: rules.standing_height,
        radius: rules.radius,
        // It has no skeleton: its head is the top of it.
        head: None,
    }
}

/// Who the target numbered `target` is to the script.
fn name_of(dummies: &[Dummy], target: u32) -> Option<CharacterId> {
    if target == PLAYER {
        return Some(CharacterId::PLAYER);
    }
    let dummy = dummies.iter().find(|dummy| dummy.id.0 == target)?;
    Some(dummy.name)
}

/// What the player is doing with the weapon they hold this tick.
fn weapon_input(body: &Body, rules: &MoveSettings) -> WeaponInput {
    let Body { player, asked } = body;
    let [x, _, z] = player.velocity;
    let speed = (x * x + z * z).sqrt();
    let moving = if rules.walk_speed > 0.0 {
        (speed / rules.walk_speed).clamp(0.0, 1.0)
    } else {
        0.0
    };
    WeaponInput {
        trigger: asked.fire,
        reload: asked.reload,
        aiming: asked.aim,
        moving: if moving.is_finite() { moving } else { 0.0 },
        airborne: !player.grounded,
        crouched: player.crouched,
    }
}

impl Shooting {
    /// Everything in flight or lying where it landed: the weapon that
    /// launched it, how it flies, and where it is.
    pub(crate) fn in_flight(
        &self,
    ) -> impl Iterator<Item = (&Rc<str>, &WeaponSettings, &Projectile)> + '_ {
        self.in_flight
            .iter()
            .map(|flying| (&flying.weapon, &flying.settings, &flying.projectile))
    }

    /// Works the player's weapon for the tick numbered `tick`, which has
    /// just moved them: call it every tick, after the player has moved.
    /// `step` is the length of a tick in seconds.
    ///
    /// Bullets, rockets and grenades are all judged against where the
    /// player and the `dummies` are now.
    ///
    /// What the weapon was heard to do (each shot, the click of an empty
    /// magazine, a reload beginning) is added to `noises`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tick(
        &mut self,
        player: &Body,
        dummies: &[Dummy],
        tick: u32,
        level: &Level,
        rules: &MoveSettings,
        step: f32,
        host: &mut ScriptHost,
        noises: &mut Vec<Noise>,
    ) {
        self.told.clear();
        self.fire(player, dummies, tick, level, rules, step, host, noises);
        self.fly(player, dummies, level, rules, step);
        // Told last, and in the order it happened: a function of the
        // script's may hand out or take away weapons, which must not change
        // what this tick's shots were.
        for told in self.told.drain(..) {
            match told {
                Told::Fired => host.weapon_fired(CharacterId::PLAYER),
                Told::Reloading => host.weapon_reloading(CharacterId::PLAYER),
                Told::Hit(hit) => {
                    host.weapon_hit(&hit);
                    // A hit hurts through the one path every hurt takes.
                    if let Some(victim) = hit.victim {
                        host.damage(victim, hit.damage, Some(hit.shooter), &hit.weapon);
                    }
                }
            }
        }
    }

    /// Steps the weapon the player holds and deals with the shots it fired.
    #[allow(clippy::too_many_arguments)]
    fn fire(
        &mut self,
        body: &Body,
        dummies: &[Dummy],
        tick: u32,
        level: &Level,
        rules: &MoveSettings,
        step: f32,
        host: &mut ScriptHost,
        noises: &mut Vec<Noise>,
    ) {
        // What is in the hands by the trigger, and then what is thrown
        // past it by the item button: a grenade while holding a rifle.
        for thrown in [false, true] {
            self.work(
                body, dummies, tick, level, rules, step, host, noises, thrown,
            );
        }
    }

    /// Steps one of the weapons the player carries: the one in their hands,
    /// or the one they throw (`thrown`).
    #[allow(clippy::too_many_arguments)]
    fn work(
        &mut self,
        body: &Body,
        dummies: &[Dummy],
        tick: u32,
        level: &Level,
        rules: &MoveSettings,
        step: f32,
        host: &mut ScriptHost,
        noises: &mut Vec<Noise>,
        thrown: bool,
    ) {
        let held = if thrown {
            host.thrown_weapon(CharacterId::PLAYER)
        } else {
            host.held_weapon(CharacterId::PLAYER)
        };
        let Some(mut held) = held else {
            return;
        };
        let mut input = weapon_input(body, rules);
        if thrown {
            input.trigger = body.asked.use_item;
            input.reload = false;
            input.aiming = false;
        }
        self.shots.clear();
        let seed = shot_seed(PLAYER, tick);
        let events = held
            .state
            .step(&held.settings, &input, seed, step, &mut self.shots);
        if thrown {
            host.set_thrown_weapon_state(CharacterId::PLAYER, held.state);
        } else {
            host.set_weapon_state(CharacterId::PLAYER, held.state);
        }
        let eyes = body.player.eye_position();
        let heard = |kind, delay: f32| Noise {
            kind,
            position: eyes,
            loudness: 1.0,
            pitch: 1.0,
            delay: if delay.is_finite() {
                delay.clamp(0.0, step.max(0.0))
            } else {
                0.0
            },
            tick,
        };
        // A blast of pellets is one shot, and is heard once: when its
        // first pellet left.
        let pellets = (self.shots.len() / events.shots.max(1) as usize).max(1);
        for shot in 0..events.shots as usize {
            self.told.push(Told::Fired);
            let left = self.shots.get(shot * pellets).map_or(0.0, |shot| shot.time);
            noises.push(heard(NoiseKind::Gunshot, left));
        }
        if events.dry_fire {
            noises.push(heard(NoiseKind::DryFire, 0.0));
        }
        if !self.shots.is_empty() {
            self.land(body, dummies, level, rules, &held);
        }
        // An empty magazine starts its reload after the shot that emptied it.
        if events.reload_started {
            self.told.push(Told::Reloading);
            noises.push(heard(NoiseKind::Reload, 0.0));
        }
    }

    /// Judges the shots a weapon has just fired: bullets land at once, and
    /// anything slower is launched.
    fn land(
        &mut self,
        body: &Body,
        dummies: &[Dummy],
        level: &Level,
        rules: &MoveSettings,
        held: &HeldWeapon,
    ) {
        let Body { player, asked } = *body;
        let (eyes, yaw, pitch) = (player.eye_position(), asked.movement.yaw, asked.pitch);
        match held.settings.delivery {
            Delivery::Hitscan { .. } => {
                // Everyone but the player, where they are.
                self.targets.clear();
                self.targets
                    .extend(dummies.iter().map(|dummy| dummy_target(dummy, rules)));
                for shot in &self.shots {
                    let direction = toward_mark(
                        level,
                        &self.targets,
                        self.camera,
                        eyes,
                        shot.direction(yaw, pitch),
                        &held.settings,
                    );
                    let Some(hit) =
                        hitscan(level, &self.targets, eyes, direction, &held.settings, None)
                    else {
                        continue;
                    };
                    let (victim, part) = match hit.struck {
                        Struck::Level => (None, None),
                        Struck::Target { id, zone } => (name_of(dummies, id), Some(zone)),
                    };
                    self.told.push(Told::Hit(WeaponHit {
                        shooter: CharacterId::PLAYER,
                        victim,
                        weapon: Rc::clone(&held.name),
                        damage: hit.damage,
                        part,
                        position: hit.point,
                        distance: hit.distance,
                        cover: hit.cover,
                        explosion: false,
                    }));
                }
            }
            Delivery::Projectile(_) => {
                self.targets.clear();
                self.targets
                    .extend(dummies.iter().map(|dummy| dummy_target(dummy, rules)));
                for shot in &self.shots {
                    let direction = toward_mark(
                        level,
                        &self.targets,
                        self.camera,
                        eyes,
                        shot.direction(yaw, pitch),
                        &held.settings,
                    );
                    let Some(mut projectile) =
                        Projectile::launch(&held.settings, eyes, direction, Some(PLAYER))
                    else {
                        continue;
                    };
                    // A stick grenade held after its cord was pulled has
                    // burnt that much of its fuse already.
                    projectile.cooked = shot.cooked;
                    if self.in_flight.len() >= MOST_IN_FLIGHT {
                        self.in_flight.remove(0);
                    }
                    self.in_flight.push(InFlight {
                        projectile,
                        settings: held.settings,
                        weapon: Rc::clone(&held.name),
                    });
                }
            }
        }
    }

    /// Flies everything launched by one tick, through where everyone is now.
    fn fly(
        &mut self,
        player: &Body,
        dummies: &[Dummy],
        level: &Level,
        rules: &MoveSettings,
        step: f32,
    ) {
        if self.in_flight.is_empty() {
            return;
        }
        self.targets.clear();
        self.targets.push(player_target(player, rules));
        self.targets
            .extend(dummies.iter().map(|dummy| dummy_target(dummy, rules)));
        let (targets, told) = (&self.targets, &mut self.told);
        self.in_flight.retain_mut(|flying| {
            let flight = flying
                .projectile
                .step(level, targets, &flying.settings, step);
            let hit = |victim, part, damage, position, explosion| {
                Told::Hit(WeaponHit {
                    shooter: CharacterId::PLAYER,
                    victim,
                    weapon: Rc::clone(&flying.weapon),
                    damage,
                    part,
                    position,
                    distance: flying.projectile.travelled,
                    cover: 0.0,
                    explosion,
                })
            };
            match flight {
                Flight::Struck(struck) => {
                    let (victim, part) = match struck.struck {
                        Struck::Level => (None, None),
                        Struck::Target { id, zone } => (name_of(dummies, id), Some(zone)),
                    };
                    told.push(hit(victim, part, struck.damage, struck.point, false));
                }
                Flight::Exploded { point, .. } => {
                    told.push(hit(None, None, 0.0, point, true));
                    for target in targets {
                        let damage = blast_damage(level, &flying.settings, point, target);
                        let victim = name_of(dummies, target.id);
                        if damage > 0.0 && victim.is_some() {
                            told.push(hit(victim, None, damage, point, true));
                        }
                    }
                }
                Flight::Flying | Flight::Bounced { .. } | Flight::Expired => {}
            }
            !flight.is_over()
        });
    }
}

#[cfg(test)]
mod aim_tests {
    use super::*;

    /// A wall across the way at z = -10.
    fn wall() -> Level {
        Level::from_triangles(&[
            [
                [-20.0, -5.0, -10.0],
                [20.0, -5.0, -10.0],
                [20.0, 20.0, -10.0],
            ],
            [
                [-20.0, -5.0, -10.0],
                [20.0, 20.0, -10.0],
                [-20.0, 20.0, -10.0],
            ],
        ])
    }

    #[test]
    fn cover_the_camera_sees_past_is_said_to_block_the_shot() {
        let rules = MoveSettings::default();
        let eyes = [0.0, 1.6, 0.0];
        let ahead = [0.0, 0.0, -1.0];
        let mut shooting = Shooting {
            // Behind and over the right shoulder.
            camera: Some([0.6, 1.75, 3.0]),
            ..Shooting::default()
        };
        // In the open the shot lands on the far wall, where the crosshair is.
        let (point, blocked) = shooting.aim(eyes, &[], &wall(), &rules, ahead);
        assert!(!blocked);
        assert!(
            (point[2] + 10.0).abs() < 1e-3 && (point[0] - 0.6).abs() < 1e-3,
            "{point:?}"
        );

        // A pillar a metre in front of the body, left of where the camera
        // looks: the crosshair is still on the far wall, the shot is not.
        let mut cover = vec![
            [[-0.3, 0.0, -1.0], [0.3, 0.0, -1.0], [0.3, 3.0, -1.0]],
            [[-0.3, 0.0, -1.0], [0.3, 3.0, -1.0], [-0.3, 3.0, -1.0]],
        ];
        // With the far wall behind it.
        cover.extend([
            [
                [-20.0, -5.0, -10.0],
                [20.0, -5.0, -10.0],
                [20.0, 20.0, -10.0],
            ],
            [
                [-20.0, -5.0, -10.0],
                [20.0, 20.0, -10.0],
                [-20.0, 20.0, -10.0],
            ],
        ]);
        let level = Level::from_triangles(&cover);
        let (point, blocked) = shooting.aim(eyes, &[], &level, &rules, ahead);
        assert!(blocked, "{point:?}");
        assert!((point[2] + 1.0).abs() < 1e-3, "on the pillar: {point:?}");

        // From the eyes there is nothing to see past.
        shooting.camera = None;
        let (point, blocked) = shooting.aim(eyes, &[], &level, &rules, ahead);
        assert!(!blocked);
        assert!((point[2] + 1.0).abs() < 1e-3, "{point:?}");
        // And the sky is a long way off.
        let (point, blocked) = shooting.aim(eyes, &[], &Level::from_triangles(&[]), &rules, ahead);
        assert!(!blocked && point[2] < -900.0, "{point:?}");
    }

    #[test]
    fn a_shot_from_the_body_lands_where_the_crosshair_over_the_shoulder_is() {
        let settings = lore_weapons::WeaponSettings::default();
        let eyes = [0.0, 1.6, 0.0];
        // Behind and over the right shoulder, looking straight ahead (-Z).
        let camera = [0.45, 1.75, 3.0];
        let ahead = [0.0, 0.0, -1.0];
        let way = toward_mark(&wall(), &[], Some(camera), eyes, ahead, &settings);
        // The crosshair is on the wall at (0.45, 1.75, -10); the shot from the
        // eyes goes there.
        let hit = hitscan(&wall(), &[], eyes, way, &settings, None).unwrap();
        assert!((hit.point[0] - 0.45).abs() < 1e-3, "{:?}", hit.point);
        assert!((hit.point[1] - 1.75).abs() < 1e-3, "{:?}", hit.point);
        // At the eyes, nothing changes.
        assert_eq!(
            toward_mark(&wall(), &[], None, eyes, ahead, &settings),
            ahead
        );
    }

    #[test]
    fn what_is_between_the_camera_and_the_body_is_not_the_mark() {
        let settings = lore_weapons::WeaponSettings::default();
        // A wall right behind the player, between them and the camera.
        let behind = Level::from_triangles(&[
            [[-20.0, -5.0, 1.0], [20.0, -5.0, 1.0], [20.0, 20.0, 1.0]],
            [[-20.0, -5.0, 1.0], [20.0, 20.0, 1.0], [-20.0, 20.0, 1.0]],
        ]);
        let way = toward_mark(
            &behind,
            &[],
            Some([0.0, 1.6, 3.0]),
            [0.0, 1.6, 0.0],
            [0.0, 0.0, -1.0],
            &settings,
        );
        assert!(way[2] < -0.99, "still straight ahead: {way:?}");
    }
}
