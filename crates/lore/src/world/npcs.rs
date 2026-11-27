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
//! ([`NpcAsk`]) for bodies to make, move and take out, and to walk and
//! stop, and a new world makes a body for every NPC the host has.
//! Everything here happens in the tick, NPCs in the order of their ids, so
//! the same input gives the same NPCs. Each NPC is plain data: what a save
//! will keep of it.
//!
//! Walking ([`lore_npc::Walk`]): an NPC asks for a way, and at most
//! [`WAYS_PER_TICK`] asks are answered a tick, taking turns in the order
//! of the NPCs' ids, so a crowd sent off at once does not make one tick
//! slow. A shut door on its way it opens as the player does, by using it
//! (the script hears it), unless the script locked it (`usable = false`).
//! Bodies do not stand in each other: one walking is moved out of the way
//! of one standing and of the player, and of two walking the later in id
//! order gives way.

use lore_nav::{GateId, Openings, Scratch};
use lore_npc::{Around, Face, Goal, Going, Keenness, Mind, News, Senses, Upright};
use lore_physics::{Level, MoveSettings, Player};
use lore_script::{CharacterId, NpcAsk, ScriptHost, Toward, WalkAsk};

use super::bodies::Standing;
use super::things::Things;
use crate::noise::{Noise, Walking};

/// How many ways are found a tick, at most, for all the NPCs together.
/// One way across the starter level takes some tens of microseconds.
pub(crate) const WAYS_PER_TICK: usize = 8;

/// How long an NPC that opened a door waits for it to swing open, in
/// seconds: a door takes 0.7.
const DOOR_WAIT: f32 = 0.75;

/// How long a sliding thing (a door, a lift) waits for an NPC caught in
/// its way, in seconds: then the NPC steps aside onto free floor if there
/// is some, and the thing no longer waits for it, as it does not for the
/// player once they are clear: it goes on, and pushes them as it pushes
/// the player.
const CAUGHT_PATIENCE: f32 = 2.0;

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
    /// Who it walks to or follows, when that is someone: where they are is
    /// said to its walk each tick.
    pub(crate) chasing: Option<CharacterId>,
    /// When its feet come down, for the noise of its steps.
    steps: Walking,
    /// How many ticks running a sliding thing (a door, a lift) has had it
    /// caught: see [`CAUGHT_PATIENCE`].
    caught_for: u32,
    /// What it sees and has lately heard (see `super::sensing`).
    pub senses: Senses<CharacterId, crate::noise::Which>,
    /// What it has noticed, bit by bit (see `super::noticing`).
    pub awareness: lore_npc::Awareness<CharacterId>,
    /// How it is turning, as it stands, to what it noticed.
    pub(crate) turning: super::noticing::Turning,
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

/// Where the feet go of an NPC put with its feet at `feet` in `level`: on
/// the floor beneath, if it was put in the air (or a little into the
/// floor). One put inside the level's solid shapes, or with no floor
/// beneath it, stays where it was put and the log says so, naming it
/// (`name`) and the place, and what to change.
fn stand(
    level: &Level,
    rules: &MoveSettings,
    feet: [f32; 3],
    name: impl FnOnce() -> Option<String>,
) -> [f32; 3] {
    let [x, y, z] = feet;
    // From a little above the feet, so one put just into the floor is
    // stood on top of it.
    let lift = rules.step_height.max(0.05);
    let inside = level.inside([x, y + lift, z]);
    let floor = (!inside)
        .then(|| level.raycast([x, y + lift, z], [0.0, -1.0, 0.0], f32::MAX))
        .flatten();
    if let Some(floor) = floor {
        return [x, floor.point[1], z];
    }
    let name = name().unwrap_or_else(|| "someone".to_owned());
    if inside {
        log::warn!(
            "The NPC \"{name}\" is put at ({x:.1}, {y:.1}, {z:.1}), inside the level's solid \
             shapes, where it cannot stand. Change its `at` (in game.npc, or its place in the \
             map) to a spot just above a floor."
        );
    } else {
        log::warn!(
            "The NPC \"{name}\" is put at ({x:.1}, {y:.1}, {z:.1}) with no floor beneath it, so \
             it would fall for ever. Change its `at` (in game.npc, or its place in the map) to a \
             spot just above a floor, or put a floor under it."
        );
    }
    feet
}

/// How tall a body is now: crouched or standing.
pub(super) fn height_of(body: &Player, rules: &MoveSettings) -> f32 {
    if body.crouched {
        rules.crouched_height
    } else {
        rules.standing_height
    }
}

/// Moves `body` across the ground by `[x, z]` metres, as far as the
/// level lets it go: at the height of the knees and up, so a step is not a
/// wall.
fn shove(body: &mut Player, [x, z]: [f32; 2], rules: &MoveSettings, level: &Level) {
    let lift = rules.step_height + rules.radius;
    let [px, py, pz] = body.position;
    let from = [px, py + lift, pz];
    let to = [px + x, py + lift, pz + z];
    let clear = level.ball_cast(from, to, rules.radius * 0.9).unwrap_or(1.0);
    body.position = [px + x * clear, py, pz + z * clear];
}

/// Moves `body` across the ground by `[x, z]` metres if the spot it would
/// land on is one it could stand on: a floor within a step of its feet,
/// not inside the level's shapes, and not in anyone standing at `others`
/// (feet, of bodies `rules.radius` round). Says whether it moved; a body
/// that cannot be moved aside stays where it is.
fn shove_safely(
    body: &mut Player,
    [x, z]: [f32; 2],
    rules: &MoveSettings,
    level: &Level,
    others: &[[f32; 3]],
) -> bool {
    let lift = rules.step_height + rules.radius;
    let [px, py, pz] = body.position;
    let clear = (level.ball_cast(
        [px, py + lift, pz],
        [px + x, py + lift, pz + z],
        rules.radius * 0.9,
    ))
    .unwrap_or(1.0);
    let to = [px + x * clear, py, pz + z * clear];
    let floor = level
        .raycast(
            [to[0], py + rules.step_height, to[2]],
            [0.0, -1.0, 0.0],
            rules.step_height * 2.0,
        )
        .is_some();
    let inside = level.inside([to[0], py + lift, to[2]]);
    let apart = 2.0 * rules.radius - 0.02;
    let crowded = others.iter().any(|at| {
        let near = |p: [f32; 3]| libm::hypotf(at[0] - p[0], at[2] - p[2]);
        (at[1] - py).abs() < rules.standing_height
            && near(to) < apart
            && near(to) < near(body.position)
    });
    if !floor || inside || crowded {
        return false;
    }
    body.position = to;
    true
}

/// How many ticks of `dt` seconds are `span` seconds, at least one.
fn ticks(span: f32, dt: f32) -> u32 {
    let ticks = (span / dt.max(1e-4)).ceil();
    // A second or so of ticks: far fewer than a u32 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = ticks.clamp(1.0, 1.0e6) as u32;
    ticks
}

/// Every NPC in a world, in the order of their ids.
#[derive(Debug, Clone, Default)]
pub(crate) struct Npcs {
    list: Vec<Npc>,
    /// Where each stood as the last tick began, and how each was moving,
    /// for their minds to read: kept so a tick sets no memory aside.
    places: Vec<[f32; 3]>,
    going: Vec<Going>,
    /// Memory kept from one search for a way to the next.
    scratch: Scratch,
    /// Which doors an NPC may walk through: those open, and those shut that
    /// it may open.
    may_open: Openings,
    /// The place in the list of the NPC whose ask for a way is answered
    /// first on the next tick: they take turns.
    turn: usize,
    /// The shut doors NPCs stand before this tick, wanting them opened:
    /// who, and which.
    doors: Vec<(CharacterId, GateId)>,
    /// The NPCs whose walk the world ended this tick without news (they
    /// went down): the host forgets where they were going.
    stopped: Vec<CharacterId>,
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
    /// The solid things, doors among them.
    pub(crate) things: &'a Things,
}

impl Npcs {
    /// A body for every NPC `host` has, standing where it says on `level`,
    /// with weapon numbers from `next_target` on. What the host had asked
    /// before is then no longer news.
    ///
    /// Each NPC the host's scripts sent walking sets off again: one sent
    /// off as its script loads sets off on the world's first tick.
    /// `player` is where the player's feet are.
    pub(crate) fn of(
        host: &mut ScriptHost,
        (level, rules): (&Level, &MoveSettings),
        player: [f32; 3],
        next_target: &mut u32,
    ) -> Self {
        let _ = host.take_npc_asks();
        let mut told = Vec::new();
        host.each_npc(|npc| told.push(npc));
        let mut npcs = Self::default();
        for npc in told {
            let feet = stand(level, rules, npc.feet, || host.character_name(npc.id));
            npcs.add(npc.id, feet, npc.facing, rules, next_target);
        }
        let mut walks = Vec::new();
        host.each_npc_walk(|id, walk| walks.push((id, walk)));
        for (id, walk) in walks {
            npcs.walk(host, id, walk, player);
        }
        npcs
    }

    /// Does what the host's scripts asked of the NPCs since the last tick.
    /// `player` is where the player's feet are.
    pub(crate) fn hear(
        &mut self,
        host: &mut ScriptHost,
        (level, rules): (&Level, &MoveSettings),
        player: [f32; 3],
        next: &mut u32,
    ) {
        let before = self.list.len();
        let mut changed = false;
        for ask in host.take_npc_asks() {
            changed |= matches!(ask, NpcAsk::Removed { .. });
            match ask {
                NpcAsk::Made { id, feet, facing } => {
                    let feet = stand(level, rules, feet, || host.character_name(id));
                    self.add(id, feet, facing, rules, next);
                }
                NpcAsk::Put { id, feet, facing } => {
                    if let Some(npc) = self.list.iter_mut().find(|npc| npc.who == id) {
                        let feet = stand(level, rules, feet, || host.character_name(id));
                        npc.body = Player::at(feet, rules);
                        npc.mind = Mind::standing(facing);
                        npc.chasing = None;
                        npc.steps = Walking::default();
                        // Put somewhere new, it looks about afresh.
                        npc.senses.forget();
                        npc.awareness.forget();
                        npc.turning = super::noticing::Turning::default();
                        host.npc_blinded(id);
                    }
                }
                NpcAsk::Removed { id } => self.list.retain(|npc| npc.who != id),
                NpcAsk::Walk { id, walk } => self.walk(host, id, walk, player),
                NpcAsk::Stop { id } => self.stop(id),
            }
        }
        // Who was in whose way is kept by place in the list, which has
        // changed.
        if changed || self.list.len() != before {
            for npc in &mut self.list {
                npc.mind.walk.forget_blocker();
            }
        }
    }

    /// Sends the NPC `id` off as `walk` says. `player`: where the player's
    /// feet are.
    fn walk(&mut self, host: &ScriptHost, id: CharacterId, walk: WalkAsk, player: [f32; 3]) {
        let (to, chasing) = match walk.to {
            Toward::Place(at) => (Some(at), None),
            Toward::Who(who) => (self.where_is(who, player, host), Some(who)),
        };
        let Some(npc) = self.list.iter_mut().find(|npc| npc.who == id) else {
            return;
        };
        let face = match (walk.facing, chasing) {
            (Some(yaw), _) => Face::Yaw(yaw),
            (None, Some(_)) => Face::Goal,
            (None, None) => Face::AsWalked,
        };
        npc.chasing = chasing;
        npc.mind.walk.go(Goal {
            to: to.unwrap_or(npc.body.position),
            within: walk.within,
            face,
            moves: chasing.is_some(),
            follow: walk.follow,
            run: walk.run,
        });
        if to.is_none() {
            npc.mind.walk.lost_goal();
            npc.chasing = None;
        }
    }

    /// The NPC `id` stops where it is.
    fn stop(&mut self, id: CharacterId) {
        if let Some(npc) = self.list.iter_mut().find(|npc| npc.who == id) {
            npc.mind.walk.stop();
            npc.chasing = None;
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
                chasing: None,
                steps: Walking::default(),
                caught_for: 0,
                senses: Senses::new(Keenness::default(), Some(CharacterId::PLAYER)),
                awareness: lore_npc::Awareness::new(Some(CharacterId::PLAYER)),
                turning: super::noticing::Turning::default(),
            },
        );
    }

    /// Where the feet of `who` are: the player's at `player`, an NPC's as
    /// its body stands, anyone else's as the host says.
    fn where_is(&self, who: CharacterId, player: [f32; 3], host: &ScriptHost) -> Option<[f32; 3]> {
        if who == CharacterId::PLAYER {
            return Some(player);
        }
        match self.list.binary_search_by(|npc| npc.who.cmp(&who)) {
            Ok(at) => self.list.get(at).map(|npc| npc.body.position),
            Err(_) => host.character_position(who),
        }
    }

    /// Moves every NPC on by a tick of `dt` seconds, in the order of their
    /// ids: each asks its mind what to do, knowing what `world` says of the
    /// tick, and its body does it. One whose health is gone asks for
    /// nothing and falls where it stands. Then nobody is left standing in
    /// anybody else.
    pub(crate) fn step(&mut self, host: &ScriptHost, world: &Tick<'_>, dt: f32) {
        let rules = &world.ground.rules;
        if self.list.is_empty() {
            return;
        }
        // Where everyone stands as the tick begins, in a list kept from one
        // tick to the next: no memory is set aside once it is long enough.
        self.places.clear();
        (self.places).extend(self.list.iter().map(|npc| npc.body.position));
        self.going.clear();
        (self.going).extend(self.list.iter().map(|npc| lore_npc::Going {
            down: npc.down,
            ..npc.mind.walk.going(&npc.body)
        }));
        // Whoever is walked to or followed is where they are now.
        for at in 0..self.list.len() {
            let Some(who) = self.list[at].chasing else {
                continue;
            };
            let now = if who == CharacterId::PLAYER {
                Some(world.player)
            } else {
                match self.list.binary_search_by(|npc| npc.who.cmp(&who)) {
                    Ok(other) => self.places.get(other).copied(),
                    Err(_) => host.character_position(who),
                }
            };
            let walk = &mut self.list[at].mind.walk;
            match now {
                Some(now) => walk.retarget(now),
                None => {
                    walk.lost_goal();
                    self.list[at].chasing = None;
                }
            }
        }
        // The walkable ground is built on a thread of its own when the
        // level loads; the first tick with an NPC waits for it, so every
        // mind reads the same ground on every run.
        let walkable = world.ground.walkable();
        self.find_ways(host, world, dt);
        let around = Around {
            level: &world.ground.level,
            walkable,
            openings: world.ground.openings(),
            tick: world.tick,
            seconds: dt,
            player: world.player,
            npcs: &self.places,
            going: &self.going,
        };
        self.doors.clear();
        for (me, npc) in self.list.iter_mut().enumerate() {
            npc.down = host.health(npc.who).is_none_or(|(now, _)| now <= 0.0);
            let asked = if npc.down {
                if npc.mind.walk.goal().is_some() {
                    npc.mind.walk.stop();
                    self.stopped.push(npc.who);
                }
                npc.chasing = None;
                lore_physics::MoveInput {
                    yaw: npc.mind.facing,
                    ..Default::default()
                }
            } else {
                npc.mind.steer(me, &npc.body, &around)
            };
            npc.body.step(&world.ground.level, rules, &asked, dt);
            if let Some(door) = npc.mind.walk.door() {
                self.doors.push((npc.who, door));
            }
        }
        self.part(rules, &world.ground.level, world.player);
    }

    /// Answers asks for a way, at most [`WAYS_PER_TICK`], taking turns:
    /// from the NPC after the last one answered, in the order of their ids.
    fn find_ways(&mut self, host: &ScriptHost, world: &Tick<'_>, dt: f32) {
        let count = self.list.len();
        let mut answered = 0;
        let mut doors_known = false;
        for k in 0..count {
            if answered >= WAYS_PER_TICK {
                break;
            }
            let at = (self.turn + k) % count;
            let npc = &mut self.list[at];
            if npc.down {
                continue;
            }
            // Sent again where it just could not get: it stops at once.
            if npc.mind.walk.refused_again(world.tick, dt) {
                npc.chasing = None;
                continue;
            }
            let Some((from, to)) = npc.mind.walk.wants_way(&npc.body) else {
                continue;
            };
            if !doors_known {
                // A shut door an NPC may open is a way, to it.
                doors_known = true;
                self.may_open.clone_from(world.ground.openings());
                for (gate, id) in world.things.door_ids().enumerate() {
                    if !host.door_may_open(id) {
                        continue;
                    }
                    // Fewer doors than fit in a u32.
                    #[allow(clippy::cast_possible_truncation)]
                    self.may_open.set_gate(GateId(gate as u32), true);
                }
            }
            // A door that stayed shut when it opened it is one it may not.
            let stayed = npc.mind.walk.stayed_shut();
            let was = stayed.map(|gate| self.may_open.gate_open(gate));
            if let Some(gate) = stayed {
                self.may_open.set_gate(gate, false);
            }
            let found = world.ground.walkable().way_into(
                from,
                to,
                &self.may_open,
                &mut self.scratch,
                npc.mind.walk.way_mut(),
            );
            if let (Some(gate), Some(was)) = (stayed, was) {
                self.may_open.set_gate(gate, was);
            }
            npc.mind.walk.found(found, world.tick, dt);
            answered += 1;
            self.turn = (at + 1) % count;
        }
    }

    /// Moves bodies out of each other, as far as the level lets them go.
    /// One walking gives way to the player and to one standing; of two
    /// walking, the later in id order gives way; two standing (one put on
    /// the other) are parted half each.
    fn part(&mut self, rules: &MoveSettings, level: &Level, player: [f32; 3]) {
        let player = Upright {
            feet: player,
            radius: rules.radius,
            height: rules.standing_height,
        };
        // Everyone where they stand now, the player last: a body is not
        // pushed into another, and each push is checked against where the
        // others have been pushed to.
        self.places.clear();
        (self.places).extend(self.list.iter().map(|npc| {
            if npc.down {
                [f32::INFINITY; 3]
            } else {
                npc.body.position
            }
        }));
        self.places.push(player.feet);
        let count = self.list.len();
        for one in 0..count {
            if self.list[one].down {
                continue;
            }
            let walking = self.list[one].mind.walk.walking();
            if walking && let Some(away) = lore_npc::push_out(self.list[one].upright(rules), player)
            {
                // Not off a ledge nor into a wall: one that cannot step
                // aside stays, and the player is the one stopped.
                self.shove(one, away, rules, level);
            }
            for two in one + 1..count {
                let (me, other) = (&self.list[one], &self.list[two]);
                if other.down {
                    continue;
                }
                let Some([x, z]) = lore_npc::push_out(me.upright(rules), other.upright(rules))
                else {
                    continue;
                };
                match (walking, other.mind.walk.walking()) {
                    (true, false) => {
                        self.shove(one, [x, z], rules, level);
                    }
                    (false, true) | (true, true) => {
                        self.shove(two, [-x, -z], rules, level);
                    }
                    (false, false) => {
                        self.shove(one, [x * 0.5, z * 0.5], rules, level);
                        self.shove(two, [-x * 0.5, -z * 0.5], rules, level);
                    }
                }
            }
        }
    }

    /// Moves the NPC at `at` in the list by `away` if it lands somewhere it
    /// can stand, clear of everyone in `places`, which says where it went.
    fn shove(&mut self, at: usize, away: [f32; 2], rules: &MoveSettings, level: &Level) -> bool {
        let Some(npc) = self.list.get_mut(at) else {
            return false;
        };
        let mine = std::mem::replace(&mut self.places[at], [f32::INFINITY; 3]);
        let moved = shove_safely(&mut npc.body, away, rules, level, &self.places);
        self.places[at] = if moved { npc.body.position } else { mine };
        moved
    }

    /// Opens the doors NPCs stand before, through the same use as the
    /// player's (so the script hears who used it), each for the first NPC
    /// that wants it; each such NPC waits for it to swing open. A door the
    /// script locked (`usable = false`) is not opened: the NPC finds its
    /// way again, and is told there is none if that was the only one.
    pub(crate) fn open_doors(
        &mut self,
        things: &mut Things,
        host: &mut ScriptHost,
        tick: u32,
        dt: f32,
    ) {
        if self.doors.is_empty() {
            return;
        }
        let wait = tick.saturating_add(ticks(DOOR_WAIT, dt));
        for at in 0..self.doors.len() {
            let (who, gate) = self.doors[at];
            let Ok(npc) = self.list.binary_search_by(|npc| npc.who.cmp(&who)) else {
                continue;
            };
            let Some(id) = things.door_ids().nth(gate.0 as usize).map(str::to_owned) else {
                self.list[npc].mind.walk.find_again();
                continue;
            };
            let walk = &mut self.list[npc].mind.walk;
            if things.door(&id) == Some(true) {
                // Another opened it this tick.
                walk.hold(wait, gate);
            } else if host.door_may_open(&id) {
                host.use_thing_by(&id, who);
                things.open(&id, true);
                walk.hold(wait, gate);
            } else {
                walk.find_again();
            }
        }
        self.doors.clear();
    }

    /// Tells `host` what came of the NPCs' walks since the last tick: who
    /// got where they were going (`npc:onArrive`) and who cannot, and why
    /// (`npc:onCannotReach`).
    pub(crate) fn tell_news(&mut self, host: &mut ScriptHost) {
        for who in self.stopped.drain(..) {
            host.npc_stopped(who);
        }
        for at in 0..self.list.len() {
            let Some(news) = self.list[at].mind.walk.take_news() else {
                continue;
            };
            let who = self.list[at].who;
            match news {
                News::Arrived => host.npc_arrived(who),
                News::Stopped => host.npc_stopped(who),
                News::CannotReach { why, to } => {
                    let list = &self.list;
                    let cause = why.cause(
                        to,
                        |gate| host.door_name(gate.0),
                        |other| list.get(other).and_then(|npc| host.character_name(npc.who)),
                    );
                    let ends = self.list[at].mind.walk.goal().is_none();
                    host.npc_cannot_reach(who, &cause, ends);
                }
            }
        }
    }

    /// Adds to `noises` the steps of every NPC on its feet over the tick
    /// numbered `tick`, of `dt` seconds, each heard as made by them.
    pub(crate) fn footsteps(&mut self, tick: u32, dt: f32, noises: &mut Vec<Noise>) {
        let said = lore_script::MoveSaid::default();
        for npc in self.list.iter_mut().filter(|npc| !npc.down) {
            let before = noises.len();
            npc.steps.tick(&npc.body, tick, dt, &said, noises);
            for noise in &mut noises[before..] {
                noise.by = Some(npc.who);
            }
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
            shove(player, [x, z], rules, level);
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

    /// Marks in `caught` (one for each of `things`) each sliding thing that
    /// has an NPC on its feet caught against something, unless the player
    /// was caught first: it waits for them.
    ///
    /// One caught for [`CAUGHT_PATIENCE`] steps aside onto free floor
    /// beside it if there is some, and is no longer waited for.
    pub(crate) fn caught_by(
        &mut self,
        things: &Things,
        level: &Level,
        rules: &MoveSettings,
        (caught, dt): (&mut [Option<CharacterId>], f32),
    ) {
        let patience = ticks(CAUGHT_PATIENCE, dt);
        for at in 0..self.list.len() {
            let npc = &self.list[at];
            if npc.down {
                continue;
            }
            let mut by: Option<([f32; 3], usize)> = None;
            npc.body
                .touching(level, rules, 0.08, |body, toward, apart| {
                    if toward[1] >= -0.7 && apart < -super::things::CAUGHT && things.sliding(body) {
                        by = Some((toward, body));
                    }
                });
            let Some((toward, body)) = by else {
                self.list[at].caught_for = 0;
                continue;
            };
            let npc = &mut self.list[at];
            npc.caught_for = npc.caught_for.saturating_add(1);
            if npc.caught_for <= patience {
                if let Some(slot) = caught.get_mut(body)
                    && slot.is_none()
                {
                    *slot = Some(npc.who);
                }
                continue;
            }
            // Waited long enough: a step aside, either way across what
            // has it caught, if there is floor there.
            let across = [-toward[2], toward[0]];
            let long = libm::hypotf(across[0], across[1]).max(1e-4);
            let step = 2.0 * rules.radius + 0.1;
            let side = [across[0] / long * step, across[1] / long * step];
            self.places.clear();
            (self.places).extend(self.list.iter().map(|npc| {
                if npc.down {
                    [f32::INFINITY; 3]
                } else {
                    npc.body.position
                }
            }));
            if !self.shove(at, side, rules, level) {
                self.shove(at, [-side[0], -side[1]], rules, level);
            }
        }
    }

    /// The level changed: every walk under way finds its way afresh, on
    /// the new ground with its own doors.
    pub(crate) fn level_changed(&mut self) {
        self.doors.clear();
        for npc in &mut self.list {
            npc.mind.walk.level_changed();
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

    /// Every NPC, in the order of their ids, to change.
    pub(crate) fn list_mut(&mut self) -> &mut [Npc] {
        &mut self.list
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
        let npcs = Npcs::of(&mut host, (&floor(), &rules), [0.0; 3], &mut 1);
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
        npcs.hear(&mut host, (&floor(), &rules), [0.0; 3], &mut next);
        let order: Vec<u32> = npcs.list().iter().map(|npc| npc.target).collect();
        assert_eq!(order, vec![1, 2]);
        assert!(npcs.list()[0].who < npcs.list()[1].who);
        // Told twice, still one body each.
        let again = Npcs::of(&mut host, (&floor(), &rules), [0.0; 3], &mut next);
        assert_eq!(again.list().len(), 2);
    }
}
