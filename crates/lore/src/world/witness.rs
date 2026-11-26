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
//! Who saw what is decided by [`witnessed`], from the NPC's own senses
//! (#55): it sees the one who did it, or heard them make a noise just now
//! and the deed was done near. Word between people is by nearness alone:
//! anyone of the side within [`TELLING_DISTANCE`] hears it, wall or no wall.

use lore_npc::{Act, Feeling};
use lore_script::{CharacterId, Deed, ScriptHost};

use super::npcs::Npc;

/// How far from an NPC a deed is done for the noise of it, heard from the
/// one who did it, to make the NPC a witness, in metres: a guard round the
/// corner who hears the shot that kills a friend minds it; one in the next
/// town does not.
pub(crate) const HEARD_DEED_DISTANCE: f32 = 30.0;
/// How long a noise made by the one who did a deed still counts as the
/// deed's own, in seconds.
const NOISE_COUNTS_FOR: f32 = 1.0;
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

/// Whether the NPC `npc` witnessed a deed done by `doer`, whose eyes were
/// at `at`, on tick `tick` (each `step` seconds long), by its own senses:
/// it holds a sighting of `doer` in view ([`lore_npc::Senses::sees`]), or it
/// heard a noise `doer` made within the last [`NOISE_COUNTS_FOR`] second
/// and the deed was done within [`HEARD_DEED_DISTANCE`] of its body.
/// A blind NPC that is also deaf witnesses nothing. Allocates nothing.
fn witnessed(npc: &Npc, doer: CharacterId, at: [f32; 3], (tick, step): (u32, f32)) -> bool {
    if npc.senses.sees(doer) {
        return true;
    }
    let lately = (NOISE_COUNTS_FOR / step.max(1e-6)).round().max(1.0) as u32;
    let Some(heard) = npc.senses.heard_from(doer) else {
        return false;
    };
    if tick.wrapping_sub(heard.tick) > lately {
        return false;
    }
    let me = npc.body.position;
    let way = [at[0] - me[0], at[1] - me[1], at[2] - me[2]];
    way[0] * way[0] + way[1] * way[1] + way[2] * way[2] <= HEARD_DEED_DISTANCE * HEARD_DEED_DISTANCE
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
        player_eyes: [f32; 3],
        (tick, step): (u32, f32),
    ) {
        host.sides_mut().set_time(tick, step);
        host.take_deeds(&mut self.deeds);
        // Who is down matters only to grudges: while nobody holds one and
        // nothing was done, a crowd with no quarrel pays nothing.
        if self.deeds.is_empty() && host.sides().grudges() == 0 {
            return;
        }
        {
            let mut sides = host.sides_mut();
            for npc in npcs {
                sides.set_down(npc.who.number(), npc.down);
            }
        }
        for deed in &self.deeds {
            let (victim, by, killed) = (deed.victim.number(), deed.by.number(), deed.killed);
            if !host.sides_mut().suffered(victim, by, killed).minded() {
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
                // Only what is done to a friend is minded.
                let witness = npc.who.number();
                if sides.feels(witness, victim) != Feeling::Friend {
                    continue;
                }
                if witnessed(npc, deed.by, at, (tick, step)) {
                    sides.perceive(witness, Act::Hurt { victim, by, killed });
                }
            }
        }
        if tick % TELL_EVERY == 0 && host.sides().grudges() > 0 {
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

    /// The player hurts a villager on tick 30 (NPCs have looked by then), saying nothing. A guard
    /// stands facing the player, in sight of the deed; another stands three
    /// metres from that one, facing the player too, behind a wall.
    const SCRIPT: &str = r#"
        game.sides { Town = { friends = { "Guards" } }, Guards = {} }
        local villager = game.npc { name = "Villager", at = vector.create(0, 0, -2), side = "Town" }
        game.npc { name = "Seer", at = vector.create(4.5, 0, 0), facing = 90, side = "Guards" }
        game.npc { name = "Walled", at = vector.create(7.5, 0, 0), facing = 90, side = "Guards" }
        game.onStart(function() game.player:teleport(vector.create(0, 0, 0)) end)
        local ticks = 0
        game.onTick(function()
            ticks += 1
            if ticks == 30 then villager:damage(10, { from = game.player }) end
        end)
    "#;

    fn world() -> World {
        world_of(SCRIPT)
    }

    fn world_of(script: &str) -> World {
        let level = blocks(&[
            ([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0]),
            ([5.9, 0.0, -10.0], [6.1, 3.0, 10.0]),
        ]);
        let game = ScriptGame::from_source(script, &ActionMap::standard());
        World::new(game, 60).with_level(&level)
    }

    /// A villager of the town 2 m north of the player, who stands at the
    /// middle; a guard 3 m east of the player with the script's `guard`
    /// line, who is not told anything. The deed (`deed`) is done on tick 30.
    fn guard_script(guard: &str, deed: &str) -> String {
        format!(
            r#"
            game.sides {{ Town = {{ friends = {{ "Guards" }} }}, Guards = {{}} }}
            local villager = game.npc {{ name = "Villager", at = vector.create(0, 0, -2), side = "Town" }}
            local guard = game.npc {{ name = "Guard", at = vector.create(3, 0, 0), side = "Guards", {guard} }}
            game.weapon("quiet", {{ from = "pistol" }})
            game.onSpawn(function() game.player:giveWeapon("quiet") end)
            game.onStart(function() game.player:teleport(vector.create(0, 0, 0)) end)
            local ticks = 0
            game.onTick(function()
                ticks += 1
                if ticks == 30 then {deed} end
            end)
            "#
        )
    }

    const SILENT_BLOW: &str = "villager:damage(10, { from = game.player })";
    const NOTHING: &str = "";

    /// Runs the world for 50 ticks, the player firing on tick 30 if they
    /// `shoot`, and says whether the guard (the second NPC) came to count
    /// the player an enemy.
    fn guard_minds(script: &str, shoot: bool) -> bool {
        let mut world = world_of(script);
        let still = InputFrame::default();
        for tick in 0..50 {
            if tick == 29 || tick == 31 {
                let mut asked = world.asked();
                asked.fire = shoot && tick == 29;
                world.ask(asked);
            }
            world.tick(&still, 1.0 / 60.0);
        }
        assert_eq!(world.game.host().last_error(), None);
        let guard = world.npcs()[1].who.number();
        world.game.host().sides().is_enemy(guard, 0)
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
        for _ in 0..100 {
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
            (0..100)
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

    /// The lock-on in front of a player at the middle looking north: a
    /// villager 3 m off and a wolf 6 m off, with or without sides.
    fn first_locks(sides: &str) -> Vec<String> {
        let script = format!(
            r#"
            {sides}
            game.npc {{ name = "Villager", at = vector.create(0, 0, -3), side = VILLAGER }}
            game.npc {{ name = "Wolf", at = vector.create(0.5, 0, -6), side = WOLF }}
            game.onStart(function() game.player:teleport(vector.create(0, 0, 0)) end)
            "#
        );
        let level = blocks(&[([-10.0, -0.2, -10.0], [10.0, 0.0, 10.0])]);
        let game = ScriptGame::from_source(&script, &ActionMap::standard());
        let mut world = World::new(game, 60).with_level(&level);
        for _ in 0..3 {
            world.tick(&InputFrame::default(), 1.0 / 60.0);
        }
        assert_eq!(world.game.host().last_error(), None);
        let eye = world.player().eye_position();
        let mut names = Vec::new();
        for _ in 0..3 {
            let next = super::super::lock_after(world.game.host(), eye, 0.0);
            world.game.host_mut().lock_on(next);
            names.push(
                next.and_then(|who| {
                    world
                        .npcs()
                        .iter()
                        .position(|npc| npc.who == who)
                        .map(|at| ["Villager", "Wolf"][at].to_owned())
                })
                .unwrap_or_else(|| "nobody".to_owned()),
            );
        }
        names
    }

    #[test]
    fn the_lock_on_picks_enemies_first_and_the_nearest_without_sides() {
        assert_eq!(
            first_locks(
                r#"game.sides { Town = { enemies = "Wolves" }, Wolves = {} }
                game.player.side = "Town"
                VILLAGER, WOLF = "Town", "Wolves""#
            ),
            ["Wolf", "Villager", "nobody"]
        );
        assert_eq!(
            first_locks("VILLAGER, WOLF = nil, nil"),
            ["Villager", "Wolf", "nobody"]
        );
    }

    #[test]
    fn a_quiet_crowd_holds_no_grudges_and_one_who_falls_is_no_longer_minded() {
        let mut world = world();
        let still = InputFrame::default();
        for _ in 0..28 {
            world.tick(&still, 1.0 / 60.0);
        }
        assert_eq!(world.game.host().sides().grudges(), 0, "nothing done yet");
        for _ in 0..100 {
            world.tick(&still, 1.0 / 60.0);
        }
        assert!(world.game.host().sides().grudges() >= 3);
        // Grudges against someone who leaves the world end with them.
        world.game.host_mut().sides_mut().forget(0);
        assert_eq!(world.game.host().sides().grudges(), 0);
        assert_eq!(enemies(&world), [false; 3]);
    }

    #[test]
    fn a_guard_facing_away_does_not_see_a_silent_blow_and_one_facing_it_does() {
        let facing = |way: &str| guard_script(way, SILENT_BLOW);
        assert!(guard_minds(&facing("facing = 90"), false), "west, at it");
        assert!(!guard_minds(&facing("facing = 270"), false), "east, away");
    }

    #[test]
    fn a_blind_guard_who_is_not_deaf_hears_a_shot_but_not_a_silent_blow() {
        let blind = |deed: &str| guard_script("facing = 270, sightDistance = 0", deed);
        assert!(!guard_minds(&blind(SILENT_BLOW), false));
        // The shot is made by the player and kills nobody here: a bullet
        // hurts the villager 2 m ahead of the player.
        assert!(guard_minds(&blind(NOTHING), true), "heard the shot");
    }

    #[test]
    fn a_blind_and_deaf_guard_witnesses_nothing() {
        let both = |deed: &str| guard_script("facing = 90, sightDistance = 0, hearing = 0", deed);
        assert!(!guard_minds(&both(SILENT_BLOW), false));
        assert!(!guard_minds(&both(NOTHING), true));
    }

    #[test]
    fn a_shot_heard_from_further_than_the_distance_is_not_witnessed() {
        let script = guard_script("facing = 270, sightDistance = 0, hearing = 3", NOTHING)
            .replace("vector.create(3, 0, 0)", "vector.create(0, 0, 40)");
        assert!(!guard_minds(&script, true));
    }
}
