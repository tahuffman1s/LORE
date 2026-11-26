//! The NPCs in a world: each a body like the player's, moved by what its
//! mind asks for.
//!
//! An NPC's body is a [`Player`], stepped through the same level with the
//! same rules, so it stands, falls and (once it walks) climbs steps exactly
//! as the player does. Each tick its [`Mind`] says what it asks of that
//! body ([`Mind::steer`]); that is the one place the rules that make an NPC
//! walk, follow and flee answer.
//!
//! Who is an NPC is the script host's to say: it hands the world asks
//! ([`NpcAsk`]) for bodies to make, move and take out, and a new world
//! makes a body for every NPC the host has. Everything here happens in the
//! tick, NPCs in the order of their ids, so the same input gives the same
//! NPCs. Each NPC is plain data: what a save will keep of it.

use lore_npc::{Around, Mind, Upright};
use lore_physics::{Level, MoveSettings, Player};
use lore_script::{CharacterId, NpcAsk, ScriptHost};

use super::bodies::Standing;

/// One NPC in a world. Not `Copy`: its mind will hold the way it is going.
#[derive(Debug, Clone, PartialEq)]
pub struct Npc {
    /// Who it is to the game's script.
    pub who: CharacterId,
    /// The number weapons know it by, shared with the training dummies.
    pub(crate) target: u32,
    /// Its body: where it stands and how it moves.
    pub body: Player,
    /// What it asks of its body each tick.
    pub mind: Mind,
    /// Its health is gone: it lies where it fell, and is not walked into,
    /// shot, struck, used or looked at by the lock-on.
    pub down: bool,
}

impl Npc {
    /// Where its eyes are, in metres: where it would see from.
    #[must_use]
    pub fn eyes(&self) -> [f32; 3] {
        self.body.eye_position()
    }

    /// Which way its body faces, in radians, turning left from -Z as the
    /// player's view turns.
    #[must_use]
    pub fn facing(&self) -> f32 {
        self.mind.facing
    }

    /// Its body as something to bump into.
    fn upright(&self, rules: &MoveSettings) -> Upright {
        Upright {
            feet: self.body.position,
            radius: rules.radius,
            height: height_of(&self.body, rules),
        }
    }
}

/// How tall a body is now: crouched or standing.
fn height_of(body: &Player, rules: &MoveSettings) -> f32 {
    if body.crouched {
        rules.crouched_height
    } else {
        rules.standing_height
    }
}

/// Every NPC in a world, in the order of their ids.
#[derive(Debug, Clone, Default)]
pub(crate) struct Npcs {
    list: Vec<Npc>,
    /// Where each stood as the last tick began, for their minds to read:
    /// kept so a tick sets no memory aside.
    places: Vec<[f32; 3]>,
}

/// What the world says of a tick to the NPCs' minds.
pub(crate) struct Tick<'a> {
    /// The ground: the level's shapes, where a body can walk on them and
    /// which doors are open.
    pub(crate) ground: &'a super::ground::Ground,
    /// Which tick this is.
    pub(crate) tick: u32,
    /// Where the player's feet are.
    pub(crate) player: [f32; 3],
}

impl Npcs {
    /// A body for every NPC `host` has, standing where it says, with
    /// weapon numbers from `next_target` on. What the host had asked
    /// before is then no longer news.
    pub(crate) fn of(host: &mut ScriptHost, rules: &MoveSettings, next_target: &mut u32) -> Self {
        let _ = host.take_npc_asks();
        let mut npcs = Self::default();
        host.each_npc(|told| {
            npcs.add(told.id, told.feet, told.facing, rules, next_target);
        });
        npcs
    }

    /// Does what the host's scripts asked of the NPCs since the last tick.
    pub(crate) fn hear(&mut self, host: &mut ScriptHost, rules: &MoveSettings, next: &mut u32) {
        for ask in host.take_npc_asks() {
            match ask {
                NpcAsk::Made { id, feet, facing } => self.add(id, feet, facing, rules, next),
                NpcAsk::Put { id, feet, facing } => {
                    if let Some(npc) = self.list.iter_mut().find(|npc| npc.who == id) {
                        npc.body = Player::at(feet, rules);
                        npc.mind = Mind::standing(facing);
                    }
                }
                NpcAsk::Removed { id } => self.list.retain(|npc| npc.who != id),
            }
        }
    }

    /// Gives the NPC `who` a body, unless it has one.
    fn add(
        &mut self,
        who: CharacterId,
        feet: [f32; 3],
        facing: f32,
        rules: &MoveSettings,
        next_target: &mut u32,
    ) {
        let Err(at) = self.list.binary_search_by(|npc| npc.who.cmp(&who)) else {
            return;
        };
        let target = *next_target;
        *next_target = next_target.saturating_add(1);
        self.list.insert(
            at,
            Npc {
                who,
                target,
                body: Player::at(feet, rules),
                mind: Mind::standing(facing),
                down: false,
            },
        );
    }

    /// Moves every NPC on by a tick of `dt` seconds, in the order of their
    /// ids: each asks its mind what to do, knowing what `world` says of the
    /// tick, and its body does it. One whose health is gone asks for
    /// nothing and falls where it stands.
    pub(crate) fn step(&mut self, host: &ScriptHost, world: &Tick<'_>, dt: f32) {
        let rules = &world.ground.rules;
        if self.list.is_empty() {
            return;
        }
        // Where everyone stands as the tick begins, in a list kept from one
        // tick to the next: no memory is set aside once it is long enough.
        self.places.clear();
        (self.places).extend(self.list.iter().map(|npc| npc.body.position));
        // The walkable ground is built on a thread of its own when the
        // level loads; the first tick with an NPC waits for it, so every
        // mind reads the same ground on every run.
        let around = Around {
            level: &world.ground.level,
            walkable: world.ground.walkable(),
            openings: world.ground.openings(),
            tick: world.tick,
            player: world.player,
            npcs: &self.places,
        };
        for npc in &mut self.list {
            npc.down = host.health(npc.who).is_none_or(|(now, _)| now <= 0.0);
            let asked = if npc.down {
                lore_physics::MoveInput {
                    yaw: npc.mind.facing,
                    ..Default::default()
                }
            } else {
                npc.mind.steer(&npc.body, &around)
            };
            npc.body.step(&world.ground.level, rules, &asked, dt);
        }
    }

    /// Tells `host` where each NPC is and how it moves, as the script reads
    /// it and the zones count it.
    pub(crate) fn tell(&self, host: &mut ScriptHost) {
        for npc in &self.list {
            let aim = super::aim_of(npc.mind.facing, 0.0);
            host.set_position(npc.who, npc.body.position);
            host.set_motion(npc.who, super::motion_of(&npc.body, aim));
        }
    }

    /// Moves `player` (of the size `rules` says) out of any NPC that is on
    /// its feet: the player walks into a person and stops, rather than
    /// through them. They are not moved into a wall to do it.
    pub(crate) fn keep_out(&self, player: &mut Player, rules: &MoveSettings, level: &Level) {
        for npc in self.list.iter().filter(|npc| !npc.down) {
            let me = Upright {
                feet: player.position,
                radius: rules.radius,
                height: height_of(player, rules),
            };
            let Some([x, z]) = lore_npc::push_out(me, npc.upright(rules)) else {
                continue;
            };
            // Across at the height of the knees and up, so a step is not a
            // wall; as far as the way is clear.
            let lift = rules.step_height + rules.radius;
            let [px, py, pz] = player.position;
            let from = [px, py + lift, pz];
            let to = [px + x, py + lift, pz + z];
            let clear = level.ball_cast(from, to, rules.radius * 0.9).unwrap_or(1.0);
            player.position = [px + x * clear, py, pz + z * clear];
            // What is left of their speed toward the NPC is spent on it.
            let long = libm::hypotf(x, z);
            if long > 1e-6 {
                let out = [x / long, z / long];
                let into = player.velocity[0] * out[0] + player.velocity[2] * out[1];
                if into < 0.0 {
                    player.velocity[0] -= out[0] * into;
                    player.velocity[2] -= out[1] * into;
                }
            }
        }
    }

    /// Adds every NPC that stands, as something to be shot and struck.
    pub(crate) fn standing(&self, rules: &MoveSettings, into: &mut Vec<Standing>) {
        into.extend(self.list.iter().filter(|npc| !npc.down).map(|npc| {
            let eye = npc.body.eye_position();
            Standing {
                target: npc.target,
                who: npc.who,
                feet: npc.body.position,
                height: height_of(&npc.body, rules),
                head: Some([eye[0], eye[1] + 0.05, eye[2]]),
            }
        }));
    }

    /// Every NPC, in the order of their ids.
    pub(crate) fn list(&self) -> &[Npc] {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor() -> Level {
        Level::from_triangles(&[
            [[-50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [50.0, 0.0, -50.0]],
            [[-50.0, 0.0, -50.0], [-50.0, 0.0, 50.0], [50.0, 0.0, 50.0]],
        ])
    }

    #[test]
    fn a_player_walking_into_an_npc_stops_against_them() {
        let rules = MoveSettings::default();
        let mut host = ScriptHost::from_source(
            "game.npc { name = \"Guard\", at = vector.create(0, 0, 0) }",
            lore_input::ActionMap::standard(),
        );
        let npcs = Npcs::of(&mut host, &rules, &mut 1);
        let mut player = Player::at([0.3, 0.0, 0.0], &rules);
        player.velocity = [-4.0, 0.0, 0.0];
        npcs.keep_out(&mut player, &rules, &floor());
        assert!(
            (player.position[0] - 0.7).abs() < 1e-4,
            "{:?}",
            player.position
        );
        assert!(player.velocity[0].abs() < 1e-6, "{:?}", player.velocity);
    }

    #[test]
    fn the_npcs_keep_the_order_of_their_ids_and_each_has_its_own_weapon_number() {
        let rules = MoveSettings::default();
        let mut next = 1;
        let mut npcs = Npcs::default();
        let mut host = ScriptHost::from_source(
            "local a = game.npc { name = \"A\", at = vector.create(0, 0, 0) }\n\
             local b = game.npc { name = \"B\", at = vector.create(2, 0, 0) }",
            lore_input::ActionMap::standard(),
        );
        npcs.hear(&mut host, &rules, &mut next);
        let order: Vec<u32> = npcs.list().iter().map(|npc| npc.target).collect();
        assert_eq!(order, vec![1, 2]);
        assert!(npcs.list()[0].who < npcs.list()[1].who);
        // Told twice, still one body each.
        let again = Npcs::of(&mut host, &rules, &mut next);
        assert_eq!(again.list().len(), 2);
    }
}
