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
    }
}

/// A training dummy as something to shoot: the size of a standing person.
fn dummy_target(dummy: &Dummy, rules: &MoveSettings) -> Target {
    Target {
        id: dummy.id.0,
        feet: dummy.feet,
        height: rules.standing_height,
        radius: rules.radius,
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
                Told::Hit(hit) => host.weapon_hit(&hit),
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
        let Some(mut held) = host.held_weapon(CharacterId::PLAYER) else {
            return;
        };
        let input = weapon_input(body, rules);
        self.shots.clear();
        let seed = shot_seed(PLAYER, tick);
        let events = held
            .state
            .step(&held.settings, &input, seed, step, &mut self.shots);
        host.set_weapon_state(CharacterId::PLAYER, held.state);
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
                    let direction = shot.direction(yaw, pitch);
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
                        explosion: false,
                    }));
                }
            }
            Delivery::Projectile(_) => {
                for shot in &self.shots {
                    let direction = shot.direction(yaw, pitch);
                    let Some(projectile) =
                        Projectile::launch(&held.settings, eyes, direction, Some(PLAYER))
                    else {
                        continue;
                    };
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
