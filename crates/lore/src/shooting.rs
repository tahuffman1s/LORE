//! Weapons in a running game: the weapon each player holds is stepped by
//! what they press, its shots are judged against the level and the other
//! players, rockets and grenades are flown, and the game's script is told
//! what was fired, what was hit and who reloads.
//!
//! Nothing here hurts anyone. A hit is handed to the script with what it is
//! worth, and the script decides what it means.

use std::rc::Rc;

use lore_physics::{Level, MoveSettings};
use lore_script::{HeldWeapon, ScriptHost, WeaponHit};
use lore_weapons::{
    Delivery, Flight, Projectile, Shot, Struck, Target, WeaponInput, WeaponSettings, blast_damage,
    hitscan, shot_seed,
};

use crate::noise::{Noise, NoiseKind};
use crate::world::{Bodies, Body, PlayerId};

/// The most rockets, grenades and slow bullets in flight at once. One more
/// takes the place of the oldest.
const MOST_IN_FLIGHT: usize = 1024;

/// Something launched and still on its way.
#[derive(Debug, Clone)]
struct InFlight {
    projectile: Projectile,
    /// The numbers of the weapon that launched it, as they were then.
    settings: WeaponSettings,
    /// Who launched it, to the script.
    shooter: lore_script::PlayerId,
    /// The name of the weapon that launched it.
    weapon: Rc<str>,
}

/// What to tell the script once everything has been worked out.
#[derive(Debug, Clone)]
enum Told {
    Fired(lore_script::PlayerId),
    Reloading(lore_script::PlayerId),
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

/// A player's body as something to shoot.
fn target(id: PlayerId, body: &Body, rules: &MoveSettings) -> Target {
    Target {
        id: id.into(),
        feet: body.player.position,
        height: if body.player.crouched {
            rules.crouched_height
        } else {
            rules.standing_height
        },
        radius: rules.radius,
    }
}

/// What a player is doing with the weapon they hold this tick.
fn weapon_input(body: &Body, rules: &MoveSettings) -> WeaponInput {
    let Body { player, asked, .. } = body;
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
    /// Works the weapons for the tick numbered `tick`, which has just moved
    /// `bodies`: call it every tick, after everyone has moved. `step` is
    /// the length of a tick in seconds.
    ///
    /// Bullets, rockets and grenades are all judged against where everyone
    /// is now.
    ///
    /// What the weapons were heard to do (each shot, the click of an empty
    /// magazine, a reload beginning) is added to `noises`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tick(
        &mut self,
        bodies: &Bodies,
        tick: u32,
        level: &Level,
        rules: &MoveSettings,
        step: f32,
        host: &mut ScriptHost,
        noises: &mut Vec<Noise>,
    ) {
        self.told.clear();
        for (id, body) in bodies.iter() {
            self.fire(bodies, tick, id, body, level, rules, step, host, noises);
        }
        self.fly(bodies, level, rules, step);
        // Told last, and in the order it happened: a function of the
        // script's may hand out or take away weapons, which must not change
        // what this tick's shots were.
        for told in self.told.drain(..) {
            match told {
                Told::Fired(player) => host.weapon_fired(player),
                Told::Reloading(player) => host.weapon_reloading(player),
                Told::Hit(hit) => host.weapon_hit(&hit),
            }
        }
    }

    /// Steps the weapon one player holds and deals with the shots it fired.
    #[allow(clippy::too_many_arguments)]
    fn fire(
        &mut self,
        bodies: &Bodies,
        tick: u32,
        id: PlayerId,
        body: &Body,
        level: &Level,
        rules: &MoveSettings,
        step: f32,
        host: &mut ScriptHost,
        noises: &mut Vec<Noise>,
    ) {
        let Body { player, name, .. } = *body;
        let Some(mut held) = host.held_weapon(name) else {
            return;
        };
        let input = weapon_input(body, rules);
        self.shots.clear();
        let seed = shot_seed(id.into(), tick);
        let events = held
            .state
            .step(&held.settings, &input, seed, step, &mut self.shots);
        host.set_weapon_state(name, held.state);
        let heard = |kind, delay: f32| Noise {
            kind,
            by: id,
            position: player.eye_position(),
            loudness: 1.0,
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
            self.told.push(Told::Fired(name));
            let left = self.shots.get(shot * pellets).map_or(0.0, |shot| shot.time);
            noises.push(heard(NoiseKind::Gunshot, left));
        }
        if events.dry_fire {
            noises.push(heard(NoiseKind::DryFire, 0.0));
        }
        if !self.shots.is_empty() {
            self.land(bodies, id, body, level, rules, &held);
        }
        // An empty magazine starts its reload after the shot that emptied it.
        if events.reload_started {
            self.told.push(Told::Reloading(name));
            noises.push(heard(NoiseKind::Reload, 0.0));
        }
    }

    /// Judges the shots a weapon has just fired: bullets land at once, and
    /// anything slower is launched.
    fn land(
        &mut self,
        bodies: &Bodies,
        id: PlayerId,
        body: &Body,
        level: &Level,
        rules: &MoveSettings,
        held: &HeldWeapon,
    ) {
        let Body {
            player,
            asked,
            name,
        } = *body;
        let (eyes, yaw, pitch) = (player.eye_position(), asked.movement.yaw, asked.pitch);
        match held.settings.delivery {
            Delivery::Hitscan { .. } => {
                // Everyone else, where they are.
                self.targets.clear();
                self.targets.extend(
                    bodies
                        .iter()
                        .filter(|(other, _)| *other != id)
                        .map(|(other, body)| target(other, body, rules)),
                );
                for shot in &self.shots {
                    let direction = shot.direction(yaw, pitch);
                    let Some(hit) =
                        hitscan(level, &self.targets, eyes, direction, &held.settings, None)
                    else {
                        continue;
                    };
                    let (victim, part) = match hit.struck {
                        Struck::Level => (None, None),
                        Struck::Target { id, zone } => (bodies.name_of(id), Some(zone)),
                    };
                    self.told.push(Told::Hit(WeaponHit {
                        shooter: name,
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
                        Projectile::launch(&held.settings, eyes, direction, Some(id.into()))
                    else {
                        continue;
                    };
                    if self.in_flight.len() >= MOST_IN_FLIGHT {
                        self.in_flight.remove(0);
                    }
                    self.in_flight.push(InFlight {
                        projectile,
                        settings: held.settings,
                        shooter: name,
                        weapon: Rc::clone(&held.name),
                    });
                }
            }
        }
    }

    /// Flies everything launched by one tick, through where everyone is now.
    fn fly(&mut self, bodies: &Bodies, level: &Level, rules: &MoveSettings, step: f32) {
        if self.in_flight.is_empty() {
            return;
        }
        self.targets.clear();
        self.targets
            .extend(bodies.iter().map(|(id, body)| target(id, body, rules)));
        let (targets, told) = (&self.targets, &mut self.told);
        self.in_flight.retain_mut(|flying| {
            // Whoever launched it has left: there is nobody to credit.
            if bodies.named(flying.shooter).is_none() {
                return false;
            }
            let flight = flying
                .projectile
                .step(level, targets, &flying.settings, step);
            let hit = |victim, part, damage, position, explosion| {
                Told::Hit(WeaponHit {
                    shooter: flying.shooter,
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
                        Struck::Target { id, zone } => (bodies.name_of(id), Some(zone)),
                    };
                    told.push(hit(victim, part, struck.damage, struck.point, false));
                }
                Flight::Exploded { point, .. } => {
                    told.push(hit(None, None, 0.0, point, true));
                    for target in targets {
                        let damage = blast_damage(level, &flying.settings, point, target);
                        let victim = bodies.name_of(target.id);
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
