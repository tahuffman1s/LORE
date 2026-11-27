//! What the NPCs notice, each tick, and how they turn to it.
//!
//! The pure part is `lore_npc`'s [`Awareness`](lore_npc::Awareness): this
//! steps it for every NPC right after its senses, in the order of their
//! ids, tells the script each level that changed, and turns a standing NPC
//! toward what it is curious about. Last, each NPC's awareness is handed to
//! the script host for `npc:awarenessOf` and the marks to read.

use lore_npc::{Change, Feeling, Pace, Sides, facing_toward, turn_toward};
use lore_script::{AwareAsk, CharacterId, ScriptHost};

use super::npcs::{Npc, Npcs};

/// How fast a standing NPC turns its body toward what it noticed, in
/// radians a second: half a turn in a second, as a person turns to a sound.
pub(crate) const TURN_SPEED: f32 = std::f32::consts::PI;

/// How far off what it noticed must be before the body turns too, in
/// radians (20 degrees): nearer than that, head and chest turn alone.
const TURN_FROM: f32 = 0.35;

/// How near the way it faces must come to the way it turns to before the
/// turn ends, in radians.
const TURNED: f32 = 0.02;

/// What noticing keeps from tick to tick, so a tick sets no memory aside.
#[derive(Debug, Default)]
pub(crate) struct Noticing {
    /// What the scripts asked this tick.
    asks: Vec<AwareAsk>,
    /// What each NPC's awareness said changed this tick, in order, to tell
    /// the script.
    told: Vec<(CharacterId, Change<CharacterId>)>,
}

/// How a standing NPC is turning to what it noticed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Turning {
    /// The way it faced before it first turned to something, to turn back
    /// to once it is calm.
    rest: Option<f32>,
    /// Whether it is turning now.
    on: bool,
}

/// What the world says of a tick to the NPCs' awareness.
pub(crate) struct Tick {
    /// The tick it is.
    pub(crate) tick: u32,
    /// How long a tick is, in seconds.
    pub(crate) seconds: f32,
    /// The difficulty dial's `notice`: how much faster the player is
    /// noticed.
    pub(crate) notice: f32,
}

impl Noticing {
    /// What the scripts asked is done; then each NPC's awareness takes in
    /// what its senses gave this tick, it turns, and the script is told.
    pub(crate) fn tick(&mut self, npcs: &mut Npcs, host: &mut ScriptHost, world: &Tick) {
        if npcs.list().is_empty() {
            return;
        }
        let tick = world.tick;
        host.take_awareness_asks(&mut self.asks);
        self.told.clear();
        let told = &mut self.told;
        for ask in &self.asks {
            let (npc, about) = match *ask {
                AwareAsk::Alert { npc, about, .. } | AwareAsk::Calm { npc, about } => (npc, about),
            };
            let Some(one) = npcs.list_mut().iter_mut().find(|one| one.who == npc) else {
                continue;
            };
            match *ask {
                AwareAsk::Alert { at, .. } => one.awareness.raise(about, at, tick),
                AwareAsk::Calm {
                    about: Some(who), ..
                } => {
                    one.awareness
                        .calm(Some(who), tick, |change| told.push((npc, change)));
                }
                AwareAsk::Calm { about: None, .. } => {
                    one.awareness
                        .calm_all(tick, |change| told.push((npc, change)));
                }
            }
        }
        {
            let sides = host.sides();
            for npc in npcs.list_mut() {
                let who = npc.who;
                if npc.down || !host.is_npc(who) {
                    // A body notices nothing, and says nothing of it.
                    if npc.awareness.noticed().next().is_some() {
                        npc.awareness.forget();
                    }
                    npc.turning = Turning::default();
                    continue;
                }
                let pace = Pace {
                    tick,
                    seconds: world.seconds,
                    alertness: host.npc_senses(who).alertness,
                    player: Some(CharacterId::PLAYER),
                    player_scale: world.notice,
                };
                let wary = |them: CharacterId| wary(&sides, who, them);
                (npc.awareness).step(&npc.senses, &pace, wary, |change| told.push((who, change)));
                // One with somewhere to go faces the way it walks; it turns
                // to what it noticed again from where it ends up.
                if npc.mind.walk.goal().is_some() {
                    npc.turning = Turning::default();
                } else {
                    turn_to_notice(npc, world.seconds);
                }
            }
        }
        for npc in npcs.list() {
            // One the script took out this tick is no longer told of.
            if host.is_npc(npc.who) {
                host.set_npc_awareness(npc.who, &npc.awareness);
            }
        }
        for (npc, change) in &self.told {
            host.npc_awareness_changed(*npc, change);
        }
    }
}

/// Whether `them` may alarm `me`: an enemy, and the player unless a
/// friend. A friend, and a neutral NPC passing by, is noticed but never
/// alarming, so a town does not alarm itself; whether one alarmed about is
/// an enemy is the rules' question (#57), asked of the same table.
fn wary(sides: &Sides, me: CharacterId, them: CharacterId) -> bool {
    match sides.feels(me.number(), them.number()) {
        Feeling::Enemy => true,
        Feeling::Neutral => them == CharacterId::PLAYER,
        Feeling::Friend => false,
    }
}

/// Turns `npc`'s body, as it stands, toward what it is curious about, at a
/// person's turning speed, so its sight finds what it heard; and back the
/// way it faced once it is calm. Head and chest turn first, in the picture
/// ([`crate::npc_figures`]); the body follows when what it noticed is
/// further round than they reach alone.
///
/// Only for an NPC with nowhere to go: one walking faces the way it goes
/// (walking writes `mind.facing` itself), so the tick calls this for the
/// others and forgets the turn of one that sets off.
pub(crate) fn turn_to_notice(npc: &mut Npc, seconds: f32) {
    let facing = npc.mind.facing;
    let goal = match npc.awareness.interest() {
        Some(notice) => {
            if npc.turning.rest.is_none() {
                npc.turning.rest = Some(facing);
            }
            facing_toward(npc.body.position, notice.at)
        }
        None => npc.turning.rest,
    };
    let Some(goal) = goal else {
        npc.turning.on = false;
        return;
    };
    let calm = npc.awareness.interest().is_none();
    let off = libm::fabsf(turn_toward(facing, goal, f32::INFINITY) - facing);
    let off = off.min(std::f32::consts::TAU - off);
    if off > TURN_FROM || (calm && off > TURNED) {
        npc.turning.on = true;
    }
    if calm && !npc.turning.on {
        npc.turning.rest = None;
    }
    if npc.turning.on {
        npc.mind.facing = turn_toward(facing, goal, TURN_SPEED * seconds);
        let left = libm::fabsf(turn_toward(npc.mind.facing, goal, f32::INFINITY) - npc.mind.facing);
        if left.min(std::f32::consts::TAU - left) <= TURNED {
            npc.turning.on = false;
            if calm {
                npc.turning.rest = None;
            }
        }
    }
}
