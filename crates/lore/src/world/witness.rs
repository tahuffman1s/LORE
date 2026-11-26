//! What the world's people make of what is done in front of them: who
//! holds what against whom, and how word of it goes round.
//!
//! The table of sides and each person's grudges are
//! [`lore_npc::Sides`], kept by the script host as the game's story state.
//! Each tick the world tells it the time, then each hurt someone dealt
//! someone since the last tick: the one hurt holds it against whoever did
//! it (unless it was a friend's stray blow), and each NPC who saw it done
//! to a friend does too. Every half second, everyone tells the people of
//! their side who stand near them what they hold against whom, so word
//! goes from person to person, never all at once.
//!
//! Who saw what is decided by [`sees`], a stand-in until NPCs have senses
//! of their own (#55): the one place to swap.

use lore_npc::Act;
use lore_physics::Level;
use lore_script::{CharacterId, Deed, ScriptHost};

use super::npcs::Npc;

/// How far an NPC sees someone do something, in metres, until NPCs have
/// senses of their own (#55).
pub(crate) const WITNESS_DISTANCE: f32 = 25.0;
/// How near two people of a side stand to tell each other what they
/// hold against whom, in metres: within speaking distance.
pub(crate) const TELLING_DISTANCE: f32 = 4.0;
/// How many ticks apart people tell each other what they know: half a
/// second at sixty a second.
pub(crate) const TELL_EVERY: u32 = 30;
/// How high above someone's feet an NPC looks to see them do something,
/// when they are not an NPC whose eyes are known: a grown person's chest.
const CHEST: f32 = 1.3;

/// What the world keeps from one tick to the next for its people's
/// feelings: lists filled each tick, so a tick sets no memory aside.
#[derive(Debug, Default)]
pub(crate) struct Witnessing {
    deeds: Vec<Deed>,
    near: Vec<(u32, [f32; 3])>,
}

/// Whether the NPC `npc` saw something done by someone whose body is at
/// `at`: within [`WITNESS_DISTANCE`] of its eyes, with nothing of the
/// level between.
///
/// A stand-in for NPCs' own senses (#55): it sees all the way round and in
/// the dark. When senses land, this answers from them (whether `npc`
/// perceives `at` this tick) and [`Witnessing::step`] is unchanged.
fn sees(npc: &Npc, at: [f32; 3], level: &Level) -> bool {
    let eyes = npc.eyes();
    let way = [at[0] - eyes[0], at[1] - eyes[1], at[2] - eyes[2]];
    let far = (way[0] * way[0] + way[1] * way[1] + way[2] * way[2]).sqrt();
    if far > WITNESS_DISTANCE {
        return false;
    }
    if far < 1e-3 {
        return true;
    }
    level.raycast(eyes, way, far).is_none()
}

impl Witnessing {
    /// Tells the host's sides of the tick: which tick it is (`tick`, each
    /// `step` seconds long), what each hurt someone dealt since the last
    /// tick made of the one hurt and of each NPC who saw it, and, every
    /// [`TELL_EVERY`] ticks, what people near each other said. NPCs in the
    /// order of their ids; `player_eyes` is where the player's eyes are.
    pub(crate) fn step(
        &mut self,
        host: &mut ScriptHost,
        npcs: &[Npc],
        level: &Level,
        player_eyes: [f32; 3],
        (tick, step): (u32, f32),
    ) {
        host.sides_mut().set_time(tick, step);
        host.take_deeds(&mut self.deeds);
        for deed in &self.deeds {
            let (victim, by) = (deed.victim.number(), deed.by.number());
            if !host.sides_mut().suffered(victim, by, deed.killed) {
                continue;
            }
            // Where the one who did it is seen: their eyes.
            let at = if deed.by == CharacterId::PLAYER {
                Some(player_eyes)
            } else if let Some(npc) = npcs.iter().find(|npc| npc.who == deed.by) {
                Some(npc.eyes())
            } else {
                host.character_position(deed.by)
                    .map(|[x, y, z]| [x, y + CHEST, z])
            };
            let Some(at) = at else { continue };
            let mut sides = host.sides_mut();
            for npc in npcs {
                if npc.down || npc.who == deed.victim || npc.who == deed.by {
                    continue;
                }
                if sees(npc, at, level) {
                    sides.perceive(npc.who.number(), Act::Hurt { victim, by });
                }
            }
        }
        if tick % TELL_EVERY == 0 {
            self.near.clear();
            (self.near).extend(
                (npcs.iter())
                    .filter(|npc| !npc.down)
                    .map(|npc| (npc.who.number(), npc.body.position)),
            );
            host.sides_mut().tell_near(&self.near, TELLING_DISTANCE);
        }
    }
}

#[cfg(test)]
mod tests {
    use lore_assets::{Instance, Mesh, Scene};
    use lore_input::{ActionMap, InputFrame};

    use crate::ScriptGame;
    use crate::game::Game;
    use crate::world::World;

    /// A level of blocks, each from its west-north-bottom corner to its
    /// east-south-top one.
    fn blocks(boxes: &[([f32; 3], [f32; 3])]) -> Scene {
        let mut scene = Scene::default();
        scene.meshes.push(Mesh {
            name: None,
            parts: vec![crate::standin::block()],
        });
        for &(low, high) in boxes {
            let size: [f32; 3] = std::array::from_fn(|i| high[i] - low[i]);
            let foot = [(low[0] + high[0]) / 2.0, low[1], (low[2] + high[2]) / 2.0];
            scene.instances.push(Instance {
                mesh: 0,
                transform: crate::standin::at(foot, size),
                mirrored: false,
                name: None,
                extras: None,
            });
        }
        scene
    }

    /// The player hurts a villager on tick 5. A guard stands in sight of
    /// it; another stands three metres from that one, behind a wall.
    const SCRIPT: &str = r#"
        game.sides { Town = { friends = { "Guards" } }, Guards = {} }
        local villager = game.npc { name = "Villager", at = vector.create(0, 0, -2), side = "Town" }
        game.npc { name = "Seer", at = vector.create(4.5, 0, 0), side = "Guards" }
        game.npc { name = "Walled", at = vector.create(7.5, 0, 0), side = "Guards" }
        game.onStart(function() game.player:teleport(vector.create(0, 0, 0)) end)
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 5 then villager:damage(10, { from = game.player }) end
        end)
    "#;

    fn world() -> World {
        let level = blocks(&[
            ([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0]),
            ([5.9, 0.0, -10.0], [6.1, 3.0, 10.0]),
        ]);
        let game = ScriptGame::from_source(SCRIPT, &ActionMap::standard());
        World::new(game, 60).with_level(&level)
    }

    /// Whether the villager, the guard in sight and the one behind the
    /// wall count the player an enemy.
    fn enemies(world: &World) -> [bool; 3] {
        let sides = world.game.host().sides();
        let mut found = [false; 3];
        for (said, npc) in found.iter_mut().zip(world.npcs()) {
            *said = sides.is_enemy(npc.who.number(), 0);
        }
        found
    }

    #[test]
    fn a_guard_who_sees_the_player_hurt_a_villager_counts_them_an_enemy_and_tells() {
        let mut world = world();
        let still = InputFrame::default();
        let mut seen_on = None;
        let mut told_on = None;
        for _ in 0..70 {
            world.tick(&still, 1.0 / 60.0);
            let [villager, seer, walled] = enemies(&world);
            assert_eq!(villager, seer, "the one hurt and the one who saw, together");
            if seer && seen_on.is_none() {
                seen_on = Some(world.tick);
                assert!(!walled, "the wall hides it");
            }
            if walled && told_on.is_none() {
                told_on = Some(world.tick);
            }
        }
        assert_eq!(world.game.host().last_error(), None);
        let seen_on = seen_on.unwrap_or_else(|| panic!("the guard never saw it"));
        let told_on = told_on.unwrap_or_else(|| panic!("the guard behind the wall was never told"));
        assert!(seen_on < told_on, "{seen_on} {told_on}");
        assert_eq!(told_on % super::TELL_EVERY, 0, "told on a telling tick");
        // The guard behind the wall counts the player an enemy only
        // because a friend told: the villager is too far to tell them.
    }

    #[test]
    fn the_same_input_gives_the_same_feelings_on_the_same_ticks() {
        let run = || {
            let mut world = world();
            let still = InputFrame::default();
            (0..70)
                .map(|_| {
                    world.tick(&still, 1.0 / 60.0);
                    (world.tick, enemies(&world))
                })
                .collect::<Vec<_>>()
        };
        let first = run();
        assert_eq!(first, run());
        assert!(first.iter().any(|(_, [_, _, walled])| *walled));
    }
}
