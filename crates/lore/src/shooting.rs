//! Weapons in a running game, on the server's side: the weapon each player
//! holds is stepped by what they press, its shots are judged against the
//! level and the other players, rockets and grenades are flown, and the
//! game's script is told what was fired, what was hit and who reloads.
//!
//! Nothing here hurts anyone. A hit is handed to the script with what it is
//! worth, and the script decides what it means.

use std::rc::Rc;

use lore_net::{MAX_PLAYERS, PlayerId, Server};
use lore_physics::{Level, MoveSettings, Player};
use lore_script::{HeldWeapon, ScriptHost, WeaponHit};
use lore_weapons::{
    Delivery, Flight, Projectile, Shot, Struck, Target, WeaponInput, WeaponSettings, blast_damage,
    hitscan, shot_seed,
};

use crate::noise::{Noise, NoiseKind};

/// Who each place on the server is to the script.
pub(crate) type Names = [Option<lore_script::PlayerId>; MAX_PLAYERS];

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
fn body(id: PlayerId, feet: [f32; 3], crouched: bool, rules: &MoveSettings) -> Target {
    Target {
        id: id.into(),
        feet,
        height: if crouched {
            rules.crouched_height
        } else {
            rules.standing_height
        },
        radius: rules.radius,
    }
}

/// Who a target is to the script.
pub(crate) fn name_of(names: &Names, target: u32) -> Option<lore_script::PlayerId> {
    names.get(usize::try_from(target).ok()?).copied().flatten()
}

/// Whether the player the script knows as `name` is in the world.
pub(crate) fn alive(host: &ScriptHost, name: lore_script::PlayerId) -> bool {
    host.mode().player(name).is_some_and(|player| player.alive)
}

/// What a player is doing with the weapon they hold this tick.
fn weapon_input(
    server: &Server,
    id: PlayerId,
    player: &Player,
    rules: &MoveSettings,
) -> WeaponInput {
    let asked = server.input(id).unwrap_or_default();
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
    /// Works the weapons for the tick `server` has just run: call it after
    /// [`Server::tick`], every tick. `step` is the length of a tick in
    /// seconds.
    ///
    /// Bullets are judged against where the shooter saw the others
    /// ([`Server::seen_by`]); rockets and grenades fly through the present.
    /// A player who is dead neither fires nor is hit.
    ///
    /// What the weapons were heard to do (each shot, the click of an empty
    /// magazine, a reload beginning) is added to `noises`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tick(
        &mut self,
        server: &Server,
        names: &Names,
        level: &Level,
        rules: &MoveSettings,
        step: f32,
        host: &mut ScriptHost,
        noises: &mut Vec<Noise>,
    ) {
        self.told.clear();
        for (id, player) in server.players() {
            self.fire(server, id, player, names, level, rules, step, host, noises);
        }
        self.fly(server, names, level, rules, step, host);
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
        server: &Server,
        id: PlayerId,
        player: &Player,
        names: &Names,
        level: &Level,
        rules: &MoveSettings,
        step: f32,
        host: &mut ScriptHost,
        noises: &mut Vec<Noise>,
    ) {
        let Some(name) = name_of(names, id.into()) else {
            return;
        };
        if !alive(host, name) {
            return;
        }
        let Some(mut held) = host.held_weapon(name) else {
            return;
        };
        let input = weapon_input(server, id, player, rules);
        self.shots.clear();
        let seed = shot_seed(id.into(), server.tick_number());
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
            tick: server.tick_number(),
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
            self.land(server, id, player, names, level, rules, host, name, &held);
        }
        // An empty magazine starts its reload after the shot that emptied it.
        if events.reload_started {
            self.told.push(Told::Reloading(name));
            noises.push(heard(NoiseKind::Reload, 0.0));
        }
    }

    /// Judges the shots a weapon has just fired: bullets land at once, and
    /// anything slower is launched.
    #[allow(clippy::too_many_arguments)]
    fn land(
        &mut self,
        server: &Server,
        id: PlayerId,
        player: &Player,
        names: &Names,
        level: &Level,
        rules: &MoveSettings,
        host: &ScriptHost,
        name: lore_script::PlayerId,
        held: &HeldWeapon,
    ) {
        let asked = server.input(id).unwrap_or_default();
        let (eyes, yaw, pitch) = (player.eye_position(), asked.movement.yaw, asked.pitch);
        match held.settings.delivery {
            Delivery::Hitscan { .. } => {
                // The others where the shooter saw them when they fired.
                self.targets.clear();
                self.targets.extend(
                    server
                        .seen_by(id)
                        .filter(|other| {
                            name_of(names, other.id.into()).is_some_and(|other| alive(host, other))
                        })
                        .map(|other| body(other.id, other.position, other.crouched, rules)),
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
                        Struck::Target { id, zone } => (name_of(names, id), Some(zone)),
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
    fn fly(
        &mut self,
        server: &Server,
        names: &Names,
        level: &Level,
        rules: &MoveSettings,
        step: f32,
        host: &ScriptHost,
    ) {
        if self.in_flight.is_empty() {
            return;
        }
        self.targets.clear();
        self.targets.extend(
            server
                .players()
                .filter(|(id, _)| {
                    name_of(names, (*id).into()).is_some_and(|name| alive(host, name))
                })
                .map(|(id, player)| body(id, player.position, player.crouched, rules)),
        );
        let (targets, told) = (&self.targets, &mut self.told);
        self.in_flight.retain_mut(|flying| {
            // Whoever launched it has left: there is nobody to credit.
            if !names.contains(&Some(flying.shooter)) {
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
                        Struck::Target { id, zone } => (name_of(names, id), Some(zone)),
                    };
                    told.push(hit(victim, part, struck.damage, struck.point, false));
                }
                Flight::Exploded { point, .. } => {
                    told.push(hit(None, None, 0.0, point, true));
                    for target in targets {
                        let damage = blast_damage(level, &flying.settings, point, target);
                        let victim = name_of(names, target.id);
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
