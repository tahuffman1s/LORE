//! Walking somewhere: following a way round corners and through doors,
//! stepping round people, giving way, and saying so when it cannot get
//! there.
//!
//! A [`Walk`] is what an NPC is walking to and how far along it is. It
//! never finds a way itself: it asks for one ([`Walk::wants_way`]) and the
//! world answers a few asks a tick ([`Walk::found`]), so fifty NPCs told
//! to go somewhere on one tick do not make that tick slow. Each tick it
//! turns the next corner of its way into what a player would press
//! ([`MoveInput`]), so an NPC walks exactly as the player does.
//!
//! Everything here reads the world as it was when the tick began, and
//! goes through the NPCs in the order of their ids, so the same input
//! gives the same walk.

use lore_nav::{GateId, Leg, NoWay, Way};
use lore_physics::{MoveInput, Player};

use crate::mind::Around;

/// How close to a corner, across the ground, counts as having reached it,
/// in metres: then the next one is walked to.
const CORNER: f32 = 0.4;

/// How close to the last corner of a way counts as there, when the place
/// asked for is off the ground and the way ends as near it as it can.
const WAY_END: f32 = 0.3;

/// How far a place to walk to may be above or below the feet and still be
/// arrived at, in metres.
const ARRIVE_HEIGHT: f32 = 1.6;

/// How far before a shut door an NPC stops to open it, in metres from
/// where its way goes into the door: out of the way of the door's swing.
const DOOR_REACH: f32 = 1.4;

/// How quickly a body turns, in radians a second: a quarter turn in a
/// third of a second.
const TURN_SPEED: f32 = 5.0;

/// The controller's top speeds, walking and sprinting, in metres a second
/// (`lore_physics::MoveSettings`' own): what a full push of the stick
/// gives, without and with sprint.
const TOP_WALK: f32 = 4.5;
const TOP_SPRINT: f32 = 6.5;

/// How fast an NPC walks, in metres a second: a person's everyday walk.
pub const WALK_PACE: f32 = 1.4;

/// How fast an NPC runs (`run = true`), in metres a second: a steady run,
/// the player's own walking speed.
pub const RUN_PACE: f32 = TOP_WALK;

/// How hard a walk pushes the stick, as a share of the controller's top
/// walking speed: [`WALK_PACE`].
pub const STROLL: f32 = WALK_PACE / TOP_WALK;

/// How far a follower lets its leader get beyond the distance it keeps
/// before it walks after them again, in metres.
const REJOIN: f32 = 1.0;

/// How much faster than its leader a follower goes for each metre it is
/// behind (beyond its distance), in metres a second: a 2 m gap closes at
/// some 1.2 m/s more than the leader goes, so it hurries only to close a
/// gap, and never past a sprint.
const CLOSE_GAP: f32 = 0.6;

/// How quickly a follower's idea of its leader's speed catches up with
/// what it sees, as a share each tick: about a quarter second.
const LEADER_SMOOTHING: f32 = 0.07;

/// How long a walk to a place just found unreachable is not looked for
/// again, in seconds: asked again within this, it ends at once, and is
/// not told again; asked after it, and still unreachable for the same
/// reason, it is not told again either.
const REFUSED_SECONDS: f32 = 2.0;

/// A place that moves (someone being followed) that has gone this far from
/// where the way was found to, in metres, or this share of the way left,
/// is found a new way to.
const MOVED: f32 = 1.0;
const MOVED_SHARE: f32 = 0.25;

/// The fewest seconds between two ways found for a place that moves.
const REPLAN_SECONDS: f32 = 0.5;

/// Walking that brings the way left this much shorter, in metres, is
/// progress.
const PROGRESS: f32 = 0.3;

/// With no progress for this many seconds, a walker finds its way again;
/// after [`MOST_TRIES`] of those, it gives up.
const STUCK_SECONDS: f32 = 3.0;
/// How many times a walker that makes no progress finds its way again
/// before it gives up.
const MOST_TRIES: u8 = 2;

/// How long a follower that cannot reach whoever it follows waits before
/// it tries again, in seconds.
const RETRY_SECONDS: f32 = 2.0;

/// Others nearer than this, in metres across the ground, are looked at
/// when walking.
const NEAR: f32 = 2.5;

/// What is between two people walking past each other, besides their own
/// widths, in metres.
const ROOM: f32 = 0.15;

/// Slower than this, in metres a second, someone is standing still.
const STILL: f32 = 0.3;

/// Someone who gives way to another coming the other way stays this far
/// from them, in metres, until they have passed.
const GIVE_WAY: f32 = 1.8;

/// Where a walk is going and how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Goal {
    /// Where to, in metres: the feet of whoever is followed.
    pub to: [f32; 3],
    /// How close to `to` across the ground is there, in metres.
    pub within: f32,
    /// Which way to face on arriving.
    pub face: Face,
    /// Whether `to` is someone who moves: the walk follows them there and
    /// finds a new way as they go.
    pub moves: bool,
    /// Whether to keep following once there, rather than arrive: then the
    /// walk never ends of itself.
    pub follow: bool,
    /// Whether to run rather than walk.
    pub run: bool,
}

impl Goal {
    /// A walk to `to`, arriving within `within` metres of it.
    #[must_use]
    pub fn to(to: [f32; 3], within: f32) -> Self {
        Self {
            to,
            within: within.max(0.1),
            face: Face::AsWalked,
            moves: false,
            follow: false,
            run: false,
        }
    }
}

/// Which way a walker faces when it gets where it was going.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Face {
    /// The way it was walking.
    AsWalked,
    /// This yaw, in radians, turning left from -Z.
    Yaw(f32),
    /// Toward where it was going: whoever it walked up to.
    Goal,
}

/// What a walk has to tell: it got there, or it cannot.
#[derive(Debug, Clone, PartialEq)]
pub enum News {
    /// It is where it was going, and has stopped.
    Arrived,
    /// It cannot get there. A walk to a place stops; one that follows
    /// someone waits and tries again.
    CannotReach {
        /// Why.
        why: Trouble,
        /// Where it was going, in metres.
        to: [f32; 3],
    },
    /// It was sent again where it just could not get, for the same reason
    /// as before, and stopped at once: that was told already.
    Stopped,
}

/// Why a walker cannot get where it is going: kept as data, so whatever
/// reports it (the script's event, `lore check`) can say it its own way.
#[derive(Debug, Clone, PartialEq)]
pub enum Trouble {
    /// There is no way over the ground, and why ([`NoWay`]). Doors the
    /// walker may open count as open, so the gates of a
    /// [`NoWay::Shut`] are doors it may not open.
    NoWay(NoWay),
    /// It walked and got no nearer for a while, found its way again and
    /// still got no nearer: something the ground does not know of is in
    /// the way.
    Stuck {
        /// Where it was when it gave up.
        at: [f32; 3],
        /// Who was in its way then, if anyone.
        by: Option<Blocker>,
    },
    /// Whoever it was walking to has left the world.
    Gone,
    /// A door on its way stayed shut after it opened it and waited: the
    /// game's script keeps it shut. It looked for a way round that door
    /// and found none.
    StayedShut {
        /// The door.
        door: GateId,
        /// The nearest it can get, in metres.
        nearest: [f32; 3],
    },
}

/// Someone in a walker's way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    /// The player.
    Player,
    /// The NPC at this place in [`Around::npcs`], as of the tick it was
    /// met.
    Npc(usize),
}

/// How an NPC is moving, as the others walking round it see it: one for
/// each NPC in [`Around::going`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Going {
    /// How fast it is moving across the ground, in metres a second along X
    /// and Z.
    pub velocity: [f32; 2],
    /// Whether it is walking through a doorway: it goes first, and is not
    /// stopped there.
    pub doorway: bool,
    /// Whether it is on its way somewhere: someone standing is walked
    /// round, and does not step aside.
    pub walking: bool,
    /// Whether it lies where it fell: it is stepped over, neither walked
    /// round nor named as in the way.
    pub down: bool,
}

/// What a walk is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Stage {
    /// Going nowhere.
    #[default]
    Idle,
    /// Waiting for a way to be found.
    Asking,
    /// Walking its way.
    Walking,
    /// Following someone and close enough to them: standing.
    Near,
    /// Following someone it cannot reach: waiting to try again on this
    /// tick.
    Waiting(u32),
}

/// What an NPC is walking to, the way it is taking and how far along it
/// is. Plain data: what a save keeps of a walk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Walk {
    goal: Option<Goal>,
    stage: Stage,
    /// The way, and the corner being walked to.
    way: Way,
    next: usize,
    /// A new way is wanted while the old one is walked: whoever is
    /// followed has moved on, or no progress was made.
    again: bool,
    /// Where the way was found to, and on which tick.
    planned_to: [f32; 3],
    planned_at: u32,
    /// The shortest the way left has been, and the tick it was.
    best: f32,
    best_at: u32,
    /// How many times it found its way again for making no progress.
    tries: u8,
    /// It stands still until this tick: a door is opening for it.
    hold_until: u32,
    /// The shut door it wants opened, as of its last tick.
    door: Option<GateId>,
    /// Whether it is walking through a doorway, as of its last tick.
    doorway: bool,
    /// Who was in its way on its last tick, and who was last in its way
    /// since it last made progress.
    blocker: Option<Blocker>,
    stuck_by: Option<Blocker>,
    /// Whether it gave way to someone on its last tick.
    giving_way: bool,
    /// A follower that cannot reach its leader has said so: it says so
    /// again only after it has got somewhere since.
    told: bool,
    news: Option<News>,
    trouble: Option<Trouble>,
    /// How far it had left to walk on its last tick: a new way for a walk
    /// under way moves the progress record by how much longer it is.
    last_left: f32,
    /// A new way came for a walk under way: its next measure moves the
    /// progress record.
    rebase: bool,
    /// Where whoever it follows was on its last tick, and how fast they
    /// are going, smoothed, in metres a second.
    leader_was: Option<[f32; 3]>,
    leader_speed: f32,
    /// The door it last waited for, and the one that stayed shut after
    /// that wait: found a way round, as a door it may not open.
    held_door: Option<GateId>,
    stayed_shut: Option<GateId>,
    /// The last walk that ended unreachable, why, and the tick it did.
    refused: Option<(Goal, Trouble, u32)>,
}

/// The yaw that faces along `d` across the ground: turning left from -Z.
fn yaw_along(d: [f32; 2]) -> f32 {
    libm::atan2f(-d[0], -d[1])
}

/// `angle` brought within a half turn of zero.
fn wrapped(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (angle + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// How far apart two points are across the ground.
fn across(a: [f32; 3], b: [f32; 3]) -> f32 {
    libm::hypotf(b[0] - a[0], b[2] - a[2])
}

/// The way from `a` to `b` across the ground, at its length, or `None`
/// when they are in the same spot.
fn toward(a: [f32; 3], b: [f32; 3]) -> Option<[f32; 2]> {
    let d = [b[0] - a[0], b[2] - a[2]];
    let long = libm::hypotf(d[0], d[1]);
    (long > 1e-4 && long.is_finite()).then(|| [d[0] / long, d[1] / long])
}

/// The direction a quarter turn to the right of `d`, seen from above.
fn right_of(d: [f32; 2]) -> [f32; 2] {
    [-d[1], d[0]]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// `d` at length one, or `None` if it has none.
fn unit(d: [f32; 2]) -> Option<[f32; 2]> {
    let long = libm::hypotf(d[0], d[1]);
    (long > 1e-4 && long.is_finite()).then(|| [d[0] / long, d[1] / long])
}

/// How many ticks of `seconds` each are `span` seconds, at least one.
fn ticks(span: f32, seconds: f32) -> u32 {
    let ticks = (span / seconds.max(1e-4)).ceil();
    // A few seconds of ticks: far fewer than a u32 holds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = ticks.clamp(1.0, 1.0e6) as u32;
    ticks
}

/// Whether two walks go to the same place in the same way: a place that
/// moves counts as the same within [`MOVED`].
fn same_goal(a: &Goal, b: &Goal) -> bool {
    let near = if a.moves { MOVED } else { 0.05 };
    a.within == b.within
        && a.face == b.face
        && a.moves == b.moves
        && a.follow == b.follow
        && a.run == b.run
        && across(a.to, b.to) <= near
        && (a.to[1] - b.to[1]).abs() <= near
}

/// How fast to go, in metres a second, as a push of the stick (a share of
/// the top walking speed, or of the top sprint when `sprint`).
fn push_for(pace: f32) -> (f32, bool) {
    if pace > TOP_WALK {
        ((pace / TOP_SPRINT).min(1.0), true)
    } else {
        (pace / TOP_WALK, false)
    }
}

/// What a walker does about the others near it: which way it goes in
/// the end, how hard it pushes, who is in its way and whether it is
/// giving way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Passing {
    /// The way to go across the ground, at length one.
    pub way: [f32; 2],
    /// How much of its pace it keeps, 0 to 1.
    pub pace: f32,
    /// The nearest in its way, if any.
    pub blocker: Option<Blocker>,
    /// Whether it is stepping aside for someone who goes first.
    pub giving_way: bool,
}

/// How the walker `me` (its place in [`Around::npcs`]), with its feet at
/// `feet` and wanting to go `wanted` (at length one) across the ground,
/// gets past the others around it. `doorway`: it is walking through a
/// doorway.
///
/// The rules, the same for everyone, so two never wait for each other:
/// - Someone standing, and the player, are walked round: on the side
///   they are not on, to the right when they are straight ahead.
/// - Someone going the same way is not run into: the walker keeps behind
///   at their pace.
/// - Two coming at each other each keep to their right. Of the two, one
///   goes first: the one in a doorway, else the one first in id order.
///   The other stops short and steps back and aside until they have passed,
///   so two never meet in a doorway and stop there.
#[must_use]
pub fn pass(
    me: usize,
    feet: [f32; 3],
    wanted: [f32; 2],
    doorway: bool,
    around: &Around<'_>,
) -> Passing {
    let radius = around.walkable.body().radius;
    let lane = 2.0 * radius + ROOM;
    let mut way = wanted;
    let mut pace: f32 = 1.0;
    let mut nearest = f32::INFINITY;
    let mut blocker = None;
    let mut giving_way = false;
    let side = right_of(wanted);
    let others = around
        .npcs
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != me)
        .map(|(other, at)| (Blocker::Npc(other), *at, around.going.get(other).copied()))
        .chain(std::iter::once((Blocker::Player, around.player, None)));
    for (who, at, going) in others {
        // One lying where it fell is stepped over.
        if going.is_some_and(|going| going.down) || (at[1] - feet[1]).abs() > ARRIVE_HEIGHT {
            continue;
        }
        let rel = [at[0] - feet[0], at[2] - feet[2]];
        let apart = libm::hypotf(rel[0], rel[1]);
        if !apart.is_finite() || apart >= NEAR {
            continue;
        }
        let ahead = dot(rel, wanted);
        let aside = dot(rel, side);
        // Touching, ahead or beside: in the way, whichever side.
        if ahead > -0.1 && apart < lane + 0.3 && apart < nearest {
            nearest = apart;
            blocker = Some(who);
        }
        // Behind, or off to one side of the way it goes: no matter.
        if ahead <= 0.0 || aside.abs() >= lane {
            continue;
        }
        if ahead < nearest {
            nearest = ahead;
            blocker = Some(who);
        }
        let velocity = going.map_or([0.0; 2], |going| going.velocity);
        let speed = libm::hypotf(velocity[0], velocity[1]);
        let walking = going.is_some_and(|going| going.walking) && speed > STILL;
        let along = if speed > STILL {
            dot(velocity, wanted) / speed
        } else {
            0.0
        };
        // How near it is, from nothing far off to all of it touching.
        let close = (1.0 - (ahead - lane) / (NEAR - lane)).clamp(0.0, 1.0);
        if walking && along > 0.5 {
            // Going the same way: keep behind them.
            let keep = ((ahead - lane) / 0.6).clamp(0.0, 1.0);
            pace = pace.min(keep.max(dot(velocity, wanted).max(0.0) / 6.5));
        } else if walking && along < -0.5 {
            // Coming the other way: both keep right, and one goes first.
            let Blocker::Npc(other) = who else { continue };
            let their_doorway = going.is_some_and(|going| going.doorway);
            let first = (doorway && !their_doorway) || (doorway == their_doorway && me < other);
            if first {
                way = [
                    way[0] + side[0] * 0.4 * close,
                    way[1] + side[1] * 0.4 * close,
                ];
            } else if ahead < GIVE_WAY + lane {
                giving_way = true;
                way = [side[0] - wanted[0] * 0.6, side[1] - wanted[1] * 0.6];
                pace = pace.min(0.8);
            } else {
                way = [
                    way[0] + side[0] * 0.6 * close,
                    way[1] + side[1] * 0.6 * close,
                ];
            }
        } else {
            // Standing, the player, or crossing: round it, on the side it
            // is not on; to the right when it is straight ahead.
            let round = if aside > 0.05 { -1.0 } else { 1.0 };
            let push = 1.6 * close;
            way = [
                way[0] + side[0] * round * push,
                way[1] + side[1] * round * push,
            ];
            if ahead < lane + 0.05 {
                pace = pace.min(0.5);
            }
        }
    }
    Passing {
        way: unit(way).unwrap_or(wanted),
        pace,
        blocker,
        giving_way,
    }
}

impl Walk {
    /// Starts walking to `goal`, from wherever it is now: the way is asked
    /// for, and found on this tick or a later one.
    pub fn go(&mut self, goal: Goal) {
        *self = Self {
            way: std::mem::take(&mut self.way),
            goal: Some(goal),
            stage: Stage::Asking,
            best: f32::INFINITY,
            refused: self.refused.take(),
            ..Self::default()
        };
        self.way.points.clear();
    }

    /// Stops walking, where it is.
    pub fn stop(&mut self) {
        let way = std::mem::take(&mut self.way);
        *self = Self {
            way,
            refused: self.refused.take(),
            ..Self::default()
        };
        self.way.points.clear();
    }

    /// Whether it was sent, on tick `tick` of `seconds` each, where it
    /// could not get a moment ago: then it stops at once, without looking
    /// for a way, and says only that it stopped ([`News::Stopped`]).
    /// Asked before each search for a way.
    pub fn refused_again(&mut self, tick: u32, seconds: f32) -> bool {
        let (Some(goal), Some((refused, _, at))) = (self.goal, &self.refused) else {
            return false;
        };
        if self.stage != Stage::Asking
            || goal.follow
            || !same_goal(&goal, refused)
            || tick.saturating_sub(*at) >= ticks(REFUSED_SECONDS, seconds)
        {
            return false;
        }
        self.stop();
        self.news = Some(News::Stopped);
        true
    }

    /// The door it waited for that stayed shut, which its way must go
    /// round: a door it may not open.
    #[must_use]
    pub fn stayed_shut(&self) -> Option<GateId> {
        self.stayed_shut
    }

    /// The level changed under it: the way, the door numbers on it and any
    /// door it waited for mean nothing now. A walk under way finds its way
    /// afresh.
    pub fn level_changed(&mut self) {
        self.door = None;
        self.held_door = None;
        self.stayed_shut = None;
        self.hold_until = 0;
        self.blocker = None;
        self.stuck_by = None;
        self.refused = None;
        self.way.points.clear();
        if self.goal.is_some() {
            self.stage = Stage::Asking;
            self.again = false;
            self.best = f32::INFINITY;
        }
    }

    /// Says where the one being walked to or followed is now.
    pub fn retarget(&mut self, to: [f32; 3]) {
        if let Some(goal) = &mut self.goal
            && goal.moves
            && to.iter().all(|part| part.is_finite())
        {
            goal.to = to;
        }
    }

    /// Where it is going, if anywhere.
    #[must_use]
    pub fn goal(&self) -> Option<&Goal> {
        self.goal.as_ref()
    }

    /// Whether it is on its way somewhere and moving there (not waiting
    /// for a way, nor standing near whoever it follows).
    #[must_use]
    pub fn walking(&self) -> bool {
        self.goal.is_some() && self.stage == Stage::Walking
    }

    /// Where it wants a way from and to, if it wants one now: from its
    /// feet (`body`) to where it is going.
    #[must_use]
    pub fn wants_way(&self, body: &Player) -> Option<([f32; 3], [f32; 3])> {
        let goal = self.goal?;
        (self.stage == Stage::Asking || self.again).then_some((body.position, goal.to))
    }

    /// The way it walks, to be filled in by the world's search
    /// (`lore_nav::Walkable::way_into`) when it wants one.
    pub fn way_mut(&mut self) -> &mut Way {
        &mut self.way
    }

    /// The way it walks: its corners, and how each stretch is crossed.
    #[must_use]
    pub fn way(&self) -> &Way {
        &self.way
    }

    /// Says what came of the search for the way it wanted, on tick `tick`
    /// of `seconds` each: found (in [`way_mut`](Self::way_mut)), or why
    /// there is none.
    pub fn found(&mut self, result: Result<(), NoWay>, tick: u32, seconds: f32) {
        let Some(goal) = self.goal else { return };
        self.again = false;
        match result {
            Ok(()) => {
                // A new way for a walk under way keeps its record of
                // progress, moved by how much longer the new way is: only
                // walking nearer forgives a stall.
                let under_way = self.stage == Stage::Walking && self.best.is_finite();
                self.stage = Stage::Walking;
                self.next = 1.min(self.way.points.len().saturating_sub(1));
                self.planned_to = goal.to;
                self.planned_at = tick;
                if under_way {
                    self.rebase = true;
                } else {
                    self.best = f32::INFINITY;
                    self.best_at = tick;
                }
            }
            Err(no) if goal.follow => {
                self.way.points.clear();
                self.stage = Stage::Waiting(tick.saturating_add(ticks(RETRY_SECONDS, seconds)));
                if !self.told {
                    self.told = true;
                    self.give_up(Trouble::NoWay(no), false, tick);
                }
            }
            Err(no) => {
                let why = match (no, self.stayed_shut) {
                    (NoWay::Shut { gates, nearest, .. }, Some(door)) if gates.contains(&door) => {
                        Trouble::StayedShut { door, nearest }
                    }
                    (no, _) => Trouble::NoWay(no),
                };
                self.give_up(why, true, tick);
            }
        }
    }

    /// Stops for `trouble` on tick `tick`, and says so: for good if
    /// `ends`, else (a follower) to try again. A walk that ends for the
    /// same reason as the last one to the same place says only that it
    /// stopped: that was told already.
    fn give_up(&mut self, trouble: Trouble, ends: bool, tick: u32) {
        let Some(goal) = self.goal else { return };
        let told = (self.refused.as_ref())
            .is_some_and(|(before, why, _)| same_goal(&goal, before) && why.same_cause(&trouble));
        self.trouble = Some(trouble.clone());
        if ends {
            self.goal = None;
            self.stage = Stage::Idle;
            self.way.points.clear();
            if !goal.follow && trouble != Trouble::Gone {
                self.refused = Some((goal, trouble.clone(), tick));
            }
        }
        self.news = Some(if ends && told {
            News::Stopped
        } else {
            News::CannotReach {
                why: trouble,
                to: goal.to,
            }
        });
    }

    /// Whoever it walked to has left the world: it stops, and says why.
    pub fn lost_goal(&mut self) {
        if self.goal.is_some() {
            self.give_up(Trouble::Gone, true, 0);
        }
    }

    /// The shut door it wants opened, as of its last tick: it stands
    /// before it, out of its swing, and waits.
    #[must_use]
    pub fn door(&self) -> Option<GateId> {
        self.door
    }

    /// Stands still until tick `until`: the door `door` is opening for it.
    pub fn hold(&mut self, until: u32, door: GateId) {
        self.hold_until = until;
        self.held_door = Some(door);
        self.door = None;
    }

    /// Forgets who was in its way: the NPCs were numbered afresh.
    pub fn forget_blocker(&mut self) {
        self.blocker = None;
        self.stuck_by = None;
    }

    /// Finds its way again: a door it wanted opened may not be.
    pub fn find_again(&mut self) {
        if self.goal.is_some() {
            self.again = true;
            self.door = None;
        }
    }

    /// What it has to tell since this was last asked.
    pub fn take_news(&mut self) -> Option<News> {
        self.news.take()
    }

    /// Why it last could not get where it was going, until it next gets
    /// somewhere.
    #[must_use]
    pub fn trouble(&self) -> Option<&Trouble> {
        self.trouble.as_ref()
    }

    /// Who was in its way on its last tick.
    #[must_use]
    pub fn blocker(&self) -> Option<Blocker> {
        self.blocker
    }

    /// Whether it gave way to someone on its last tick.
    #[must_use]
    pub fn giving_way(&self) -> bool {
        self.giving_way
    }

    /// How it moves, as the others walking round it see it.
    #[must_use]
    pub fn going(&self, body: &Player) -> Going {
        Going {
            velocity: [body.velocity[0], body.velocity[2]],
            doorway: self.doorway,
            walking: self.walking(),
            down: false,
        }
    }

    /// How far it has left to walk along its way, from `feet`.
    fn left(&self, feet: [f32; 3]) -> f32 {
        let points = &self.way.points;
        let Some(next) = points.get(self.next) else {
            return 0.0;
        };
        let mut left = across(feet, next.point);
        for pair in points[self.next..].windows(2) {
            left += across(pair[0].point, pair[1].point);
        }
        left
    }

    /// What the walker `me` (its place in `around.npcs`), whose body is
    /// `body` and which faces `facing`, asks of its body this tick, and
    /// which way it then wants to face: `None` when it does not walk,
    /// which the mind answers by standing.
    pub(crate) fn steer(
        &mut self,
        me: usize,
        body: &Player,
        facing: f32,
        around: &Around<'_>,
    ) -> Option<(MoveInput, Option<f32>)> {
        self.door = None;
        self.doorway = false;
        self.blocker = None;
        self.giving_way = false;
        let goal = self.goal?;
        let feet = body.position;
        let tick = around.tick;
        let seconds = around.seconds;
        let standing = |face: Option<f32>| Some((MoveInput::default(), face));
        let face_goal = toward(feet, goal.to).map(yaw_along);
        let there =
            across(feet, goal.to) <= goal.within && (feet[1] - goal.to[1]).abs() <= ARRIVE_HEIGHT;
        if goal.moves {
            // How fast whoever it follows goes, as seen tick by tick.
            let seen = self
                .leader_was
                .map_or(0.0, |was| across(was, goal.to) / seconds.max(1e-4));
            if seen.is_finite() {
                self.leader_speed +=
                    (seen.min(TOP_SPRINT * 2.0) - self.leader_speed) * LEADER_SMOOTHING;
            }
            self.leader_was = Some(goal.to);
        }
        match self.stage {
            Stage::Idle => return None,
            Stage::Asking => return standing(None),
            Stage::Waiting(until) => {
                if tick >= until {
                    self.stage = Stage::Asking;
                }
                return standing(face_goal);
            }
            Stage::Near => {
                if across(feet, goal.to) > goal.within + REJOIN {
                    self.stage = Stage::Asking;
                }
                return standing(face_goal);
            }
            Stage::Walking => {}
        }
        let last = self.way.points.len().saturating_sub(1);
        let way_end = self.way.points.last().map_or(goal.to, |end| end.point);
        let at_end = self.next >= last && across(feet, way_end) <= WAY_END;
        // At the end of a way to someone who has not moved on since it was
        // found: as near them as the ground goes (they stand on a crate).
        let moved_on = goal.moves && across(self.planned_to, goal.to) > MOVED;
        if there || (at_end && !moved_on) {
            return self.arrive(goal, face_goal);
        }
        // Whoever is followed has moved on a good way: a new way, now and
        // then, and the old one walked meanwhile.
        if goal.moves && tick.saturating_sub(self.planned_at) >= ticks(REPLAN_SECONDS, seconds) {
            let gone = across(self.planned_to, goal.to);
            if gone > MOVED.max(MOVED_SHARE * across(feet, goal.to)) {
                self.again = true;
            }
        }
        // The corners already reached are passed. Cutting a corner is not
        // progress: the record moves with the way left.
        let skipped_from = self.next;
        let before_skip = self.left(feet);
        while self.next < last
            && let Some(corner) = self.way.points.get(self.next)
            && across(feet, corner.point) < CORNER
        {
            self.next += 1;
        }
        if self.next != skipped_from && self.best.is_finite() && !self.rebase {
            self.best += self.left(feet) - before_skip;
        }
        let Some(corner) = self.way.points.get(self.next).copied() else {
            self.again = true;
            return standing(None);
        };
        let on = (self.next.checked_sub(1))
            .and_then(|before| self.way.points.get(before))
            .map_or(Leg::Walk, |before| before.leg);
        let to_corner = across(feet, corner.point);
        let wanted = toward(feet, corner.point).or_else(|| toward(feet, goal.to));
        self.doorway = matches!(on, Leg::Gate(_));
        // No progress for a while: the way again, then give up.
        let left = self.left(feet);
        if self.rebase {
            // A new way under way: the record moves by how much more (or
            // less) there is to walk on it from here.
            self.rebase = false;
            self.best += left - self.last_left;
        }
        self.last_left = left;
        if left < self.best - PROGRESS {
            // Real progress, not the first measure of a way just found,
            // forgives the stalls before it, and lets a follower say again
            // when it next cannot get through.
            if self.best.is_finite() {
                self.tries = 0;
                self.told = false;
            }
            self.best = left;
            self.best_at = tick;
            self.stuck_by = None;
        } else if tick.saturating_sub(self.best_at) > ticks(STUCK_SECONDS, seconds) {
            self.tries = self.tries.saturating_add(1);
            if self.tries > MOST_TRIES {
                let by = self.stuck_by;
                self.tries = 0;
                if goal.follow {
                    self.stage = Stage::Waiting(tick.saturating_add(ticks(RETRY_SECONDS, seconds)));
                    self.best = f32::INFINITY;
                    if !self.told {
                        self.told = true;
                        self.give_up(Trouble::Stuck { at: feet, by }, false, tick);
                    }
                } else {
                    self.give_up(Trouble::Stuck { at: feet, by }, true, tick);
                }
                return standing(None);
            }
            self.again = true;
            self.best = left;
            self.best_at = tick;
        }
        // A door it holds for while it opens.
        // Its wait counts toward getting nowhere, so a door that never
        // opens is given up on.
        if tick < self.hold_until {
            return standing(wanted.map(yaw_along));
        }
        // The door it waited for is still shut: the game keeps it so. It
        // is a door it may not open, and a way round it is found.
        if let Some(held) = self.held_door.take()
            && !around.openings.gate_open(held)
        {
            self.stayed_shut = Some(held);
            self.again = true;
            return standing(wanted.map(yaw_along));
        }
        // A shut door on the way, or in it: it stops before it, out of its
        // swing, and wants it opened.
        let shut = |leg: Leg| match leg {
            Leg::Gate(gate) if !around.openings.gate_open(gate) => Some(gate),
            _ => None,
        };
        let door = shut(on).or_else(|| shut(corner.leg).filter(|_| to_corner < DOOR_REACH));
        if let Some(door) = door {
            self.door = Some(door);
            return standing(wanted.map(yaw_along));
        }
        let Some(wanted) = wanted else {
            return standing(None);
        };
        let passing = pass(me, feet, wanted, self.doorway, around);
        self.blocker = passing.blocker;
        self.stuck_by = passing.blocker.or(self.stuck_by);
        self.giving_way = passing.giving_way;
        // How fast it goes: a walk or a run. A follower keeps its leader's
        // pace, a little faster the further behind it is, and slows as it
        // comes up to them; never slower than a slow walk while it walks.
        let mut pace = if goal.run { RUN_PACE } else { WALK_PACE };
        if goal.moves {
            let behind = (across(feet, goal.to) - goal.within).max(0.0);
            let keeping = self.leader_speed + CLOSE_GAP * behind;
            let easing = WALK_PACE * (behind / REJOIN).clamp(0.5, 1.0);
            pace = keeping.max(easing).max(if goal.run && behind > REJOIN {
                RUN_PACE
            } else {
                0.0
            });
            pace = pace.min(TOP_SPRINT);
        } else if self.next >= last {
            // Easing into the end, so it does not walk past it.
            pace *= (to_corner / 1.2).clamp(0.5, 1.0);
        }
        pace *= passing.pace;
        let (mut push, sprint) = push_for(pace);
        // It turns as it goes, and goes the way it wants meanwhile, slower
        // while it faces well away from it.
        let wants = yaw_along(passing.way);
        let turned = wrapped(wants - facing);
        push *= libm::cosf(turned).max(0.35);
        let (sin, cos) = (libm::sinf(facing), libm::cosf(facing));
        let ahead = [-sin, -cos];
        let aside = [cos, -sin];
        let input = MoveInput {
            forward: dot(passing.way, ahead) * push,
            right: dot(passing.way, aside) * push,
            yaw: facing,
            crouch: matches!(on, Leg::Crouch),
            sprint: sprint && !passing.giving_way,
            jump: matches!(on, Leg::Link(_, lore_nav::LinkKind::Jump)) && body.grounded,
            ..MoveInput::default()
        };
        // It faces the way it walks, not the way it steps aside.
        let face = if passing.giving_way {
            Some(yaw_along(wanted))
        } else {
            Some(wants)
        };
        Some((input, face))
    }

    /// It is there: it stops and says so, or a follower stands by.
    fn arrive(&mut self, goal: Goal, face_goal: Option<f32>) -> Option<(MoveInput, Option<f32>)> {
        let face = match goal.face {
            Face::AsWalked => None,
            Face::Yaw(yaw) => Some(yaw),
            Face::Goal => face_goal,
        };
        self.way.points.clear();
        self.again = false;
        if goal.follow {
            self.stage = Stage::Near;
            self.told = false;
            return Some((MoveInput::default(), face_goal));
        }
        self.goal = None;
        self.stage = Stage::Idle;
        self.trouble = None;
        self.news = Some(News::Arrived);
        Some((MoveInput::default(), face))
    }
}

/// `facing` turned toward `wanted` by as much as a body turns in `seconds`.
#[must_use]
pub fn turned_toward(facing: f32, wanted: f32, seconds: f32) -> f32 {
    let most = TURN_SPEED * seconds;
    let gap = wrapped(wanted - facing);
    if gap.abs() <= most {
        wanted
    } else {
        wrapped(facing + gap.clamp(-most, most))
    }
}

/// "4 m", "0.5 m": a distance as a person would say it.
fn metres(distance: f32) -> String {
    if distance < 0.95 {
        format!("{distance:.1} m")
    } else {
        format!("{distance:.0} m")
    }
}

impl Trouble {
    /// Why, in words that follow "Guard cannot reach Gate: ", for a walk
    /// that was going to `to`. `door` gives what a door (a gate of the
    /// walkable ground) is called, and `npc` what the NPC at a place in
    /// [`Around::npcs`] is called.
    #[must_use]
    pub fn cause(
        &self,
        to: [f32; 3],
        door: impl Fn(GateId) -> Option<String>,
        npc: impl Fn(usize) -> Option<String>,
    ) -> String {
        let short = |nearest: [f32; 3]| {
            let gap = libm::hypotf(nearest[0] - to[0], nearest[2] - to[2])
                .max((nearest[1] - to[1]).abs());
            if gap.is_finite() && gap >= 0.05 {
                format!(" (the nearest it can get is {} short)", metres(gap))
            } else {
                String::new()
            }
        };
        let place = |at: [f32; 3]| format!("({:.1}, {:.1}, {:.1})", at[0], at[1], at[2]);
        match self {
            Self::NoWay(NoWay::StartOff) => "where it stands is not on ground a person can walk \
                                             on: it may have been put inside a wall or in the \
                                             air. Put it on a floor"
                .to_owned(),
            Self::NoWay(NoWay::EndOff) => "there is no floor a person can stand on within 2 m \
                                           of it. Move the place onto a floor, or choose one \
                                           that is on one"
                .to_owned(),
            Self::NoWay(NoWay::Shut { gates, nearest, .. }) => {
                let named: Vec<String> = gates
                    .iter()
                    .map(|gate| match door(*gate) {
                        Some(name) => format!("the door \"{name}\""),
                        None => "a door".to_owned(),
                    })
                    .collect();
                let what = match named.as_slice() {
                    [] => "the way there is shut".to_owned(),
                    [one] => format!(
                        "{one} is locked (it has usable = false; set door.usable = true to let \
                         people through)"
                    ),
                    [first @ .., last] => format!(
                        "{} and {last} are locked (they have usable = false; set door.usable = \
                         true to let people through)",
                        first.join(", ")
                    ),
                };
                format!("{what}{}", short(*nearest))
            }
            Self::NoWay(NoWay::Apart { nearest }) => format!(
                "nothing a person can walk joins where it stands to there: a slope steeper than \
                 45°, a step higher than 0.4 m or a gap narrower than 0.7 m is in the way{}. \
                 Make a ramp or steps, widen the gap, or send it somewhere on the same ground",
                short(*nearest)
            ),
            Self::Stuck { at, by } => {
                let who = match by {
                    Some(Blocker::Player) => "the player is".to_owned(),
                    Some(Blocker::Npc(other)) => npc(*other)
                        .map_or_else(|| "someone is".to_owned(), |name| format!("{name} is")),
                    None => "something is".to_owned(),
                };
                format!(
                    "{who} in the way at {}, and it found no way round. Move whoever is in the \
                     way, or send it somewhere else with goTo",
                    place(*at)
                )
            }
            Self::Gone => "whoever it was going to has left the world. Give it somewhere else \
                           to go with goTo"
                .to_owned(),
            Self::StayedShut {
                door: gate,
                nearest,
            } => {
                let named = door(*gate).map_or_else(
                    || "a door on the way".to_owned(),
                    |name| format!("the door \"{name}\""),
                );
                format!(
                    "{named} stayed shut after it opened it, and there is no other way: the \
                     game's script keeps it shut (an onUse that shuts it again, or a close \
                     every tick){}. Let the door open, or send it another way",
                    short(*nearest)
                )
            }
        }
    }

    /// Whether `other` is the same reason as this, if not in every
    /// number: the same kind, the same doors, the same one in the way.
    #[must_use]
    pub fn same_cause(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::NoWay(NoWay::Shut { gates: a, .. }),
                Self::NoWay(NoWay::Shut { gates: b, .. }),
            ) => a == b,
            (Self::NoWay(a), Self::NoWay(b)) => {
                std::mem::discriminant(a) == std::mem::discriminant(b)
            }
            (Self::Stuck { by: a, .. }, Self::Stuck { by: b, .. }) => a == b,
            (Self::StayedShut { door: a, .. }, Self::StayedShut { door: b, .. }) => a == b,
            (Self::Gone, Self::Gone) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use lore_nav::{Body, Openings, Walkable};
    use lore_physics::{Level, MoveSettings};

    use super::*;

    const STEP: f32 = 1.0 / 60.0;

    /// A floor 40 m on a side at height 0, and the walls `walls` on it,
    /// each from its low corner to its high one; the walkable ground knows
    /// only the floor.
    fn floor(walls: &[([f32; 3], [f32; 3])]) -> (Level, Walkable) {
        let r = 20.0;
        let mut triangles = vec![
            [[-r, 0.0, -r], [r, 0.0, r], [r, 0.0, -r]],
            [[-r, 0.0, -r], [-r, 0.0, r], [r, 0.0, r]],
        ];
        for &(low, high) in walls {
            let c = |x: bool, y: bool, z: bool| {
                [
                    if x { high[0] } else { low[0] },
                    if y { high[1] } else { low[1] },
                    if z { high[2] } else { low[2] },
                ]
            };
            // The four upright faces are enough to walk into.
            for (a, b) in [
                (c(false, false, false), c(true, false, false)),
                (c(true, false, false), c(true, false, true)),
                (c(true, false, true), c(false, false, true)),
                (c(false, false, true), c(false, false, false)),
            ] {
                let (ta, tb) = ([a[0], high[1], a[2]], [b[0], high[1], b[2]]);
                triangles.push([a, b, tb]);
                triangles.push([a, tb, ta]);
            }
        }
        let level = Level::from_triangles(&triangles);
        (
            level,
            Walkable::flat([-r, -r], [r, r], 0.0, Body::default()),
        )
    }

    /// Walks `walk` with a body from `from` for at most `ticks`, finding
    /// its way at once whenever it asks, with `others` standing about;
    /// gives the body.
    fn walked(
        walk: &mut Walk,
        from: [f32; 3],
        ticks: u32,
        world: (&Level, &Walkable),
        others: &[[f32; 3]],
    ) -> (Player, f32) {
        let (level, walkable) = world;
        let rules = MoveSettings::default();
        let mut body = Player::at(from, &rules);
        let mut facing = 0.0;
        let open = Openings::all_open();
        let mut npcs = vec![from];
        npcs.extend_from_slice(others);
        let mut going = vec![Going::default(); npcs.len()];
        for tick in 1..=ticks {
            if let Some((from, to)) = walk.wants_way(&body) {
                let found = walkable.way_into(
                    from,
                    to,
                    &open,
                    &mut lore_nav::Scratch::default(),
                    walk.way_mut(),
                );
                walk.found(found, tick, STEP);
            }
            npcs[0] = body.position;
            going[0] = walk.going(&body);
            let around = Around {
                level,
                walkable,
                openings: &open,
                tick,
                seconds: STEP,
                player: [100.0, 0.0, 100.0],
                npcs: &npcs,
                going: &going,
            };
            let (mut asked, face) = walk.steer(0, &body, facing, &around).unwrap_or_default();
            if let Some(face) = face {
                facing = turned_toward(facing, face, STEP);
            }
            asked.yaw = facing;
            body.step(level, &rules, &asked, STEP);
            if walk.goal().is_none() {
                break;
            }
        }
        (body, facing)
    }

    fn around<'a>(
        level: &'a Level,
        walkable: &'a Walkable,
        open: &'a Openings,
        npcs: &'a [[f32; 3]],
        going: &'a [Going],
    ) -> Around<'a> {
        Around {
            level,
            walkable,
            openings: open,
            tick: 1,
            seconds: STEP,
            player: [100.0, 0.0, 100.0],
            npcs,
            going,
        }
    }

    #[test]
    fn a_walk_arrives_and_then_faces_the_way_asked() {
        let (level, walkable) = floor(&[]);
        let mut walk = Walk::default();
        walk.go(Goal {
            face: Face::Yaw(1.0),
            ..Goal::to([0.0, 0.0, -10.0], 0.5)
        });
        let (body, facing) = walked(&mut walk, [0.0; 3], 60 * 20, (&level, &walkable), &[]);
        assert!(
            across(body.position, [0.0, 0.0, -10.0]) <= 0.5,
            "{:?}",
            body.position
        );
        assert_eq!(walk.take_news(), Some(News::Arrived));
        assert!(walk.goal().is_none());
        // Turning to face the way asked takes a few ticks more.
        let mut facing = facing;
        for _ in 0..30 {
            facing = turned_toward(facing, 1.0, STEP);
        }
        assert_eq!(facing, 1.0);
    }

    #[test]
    fn a_body_turns_a_little_each_tick_and_the_short_way_round() {
        let once = turned_toward(0.0, 3.0, STEP);
        assert!((once - TURN_SPEED * STEP).abs() < 1e-6, "{once}");
        // From just left of behind to just right of it: through behind.
        let round = turned_toward(3.0, -3.0, STEP);
        assert!(round > 3.0 || round < -3.0, "{round}");
    }

    #[test]
    fn a_walker_that_gets_nowhere_finds_its_way_again_then_gives_up_saying_where() {
        // A wall the ground does not know of, right across a corridor
        // the walker cannot leave: two more walls along it.
        let (level, walkable) = floor(&[
            ([-3.0, 0.0, -4.2], [3.0, 3.0, -3.8]),
            ([-1.2, 0.0, -4.0], [-0.8, 3.0, 1.0]),
            ([0.8, 0.0, -4.0], [1.2, 3.0, 1.0]),
        ]);
        let mut walk = Walk::default();
        walk.go(Goal::to([0.0, 0.0, -10.0], 0.5));
        walked(&mut walk, [0.0; 3], 60 * 30, (&level, &walkable), &[]);
        match walk.take_news() {
            Some(News::CannotReach {
                why: Trouble::Stuck { at, by: None },
                to,
            }) => {
                assert!(at[2] < -3.0, "{at:?}");
                assert_eq!(to, [0.0, 0.0, -10.0]);
            }
            other => panic!("{other:?}"),
        }
        assert!(walk.goal().is_none());
    }

    #[test]
    fn someone_standing_ahead_is_walked_round_on_the_side_they_are_not_on() {
        let (level, walkable) = floor(&[]);
        let open = Openings::all_open();
        let going = [Going::default(); 2];
        // Straight ahead: round them to the right, which going -Z is +X.
        let npcs = [[0.0; 3], [0.0, 0.0, -1.2]];
        let passing = pass(
            0,
            [0.0; 3],
            [0.0, -1.0],
            false,
            &around(&level, &walkable, &open, &npcs, &going),
        );
        assert!(passing.way[0] > 0.2, "{passing:?}");
        assert_eq!(passing.blocker, Some(Blocker::Npc(1)));
        assert!(!passing.giving_way);
        // A little to the right: round them to the left.
        let npcs = [[0.0; 3], [0.3, 0.0, -1.2]];
        let passing = pass(
            0,
            [0.0; 3],
            [0.0, -1.0],
            false,
            &around(&level, &walkable, &open, &npcs, &going),
        );
        assert!(passing.way[0] < -0.2, "{passing:?}");
        // Well off to the side: nothing to go round.
        let npcs = [[0.0; 3], [2.0, 0.0, -1.2]];
        let passing = pass(
            0,
            [0.0; 3],
            [0.0, -1.0],
            false,
            &around(&level, &walkable, &open, &npcs, &going),
        );
        assert_eq!(passing.way, [0.0, -1.0]);
    }

    #[test]
    fn of_two_coming_at_each_other_the_first_in_order_goes_first_unless_the_other_is_in_a_doorway()
    {
        let (level, walkable) = floor(&[]);
        let open = Openings::all_open();
        let npcs = [[0.0; 3], [0.0, 0.0, -1.5]];
        let walker = |velocity: [f32; 2], doorway: bool| Going {
            velocity,
            doorway,
            walking: true,
            down: false,
        };
        // 0 walks -Z, 1 walks +Z: 0 goes first and 1 steps aside.
        let going = [walker([0.0, -1.8], false), walker([0.0, 1.8], false)];
        let near = around(&level, &walkable, &open, &npcs, &going);
        let first = pass(0, npcs[0], [0.0, -1.0], false, &near);
        let second = pass(1, npcs[1], [0.0, 1.0], false, &near);
        assert!(
            !first.giving_way && second.giving_way,
            "{first:?} {second:?}"
        );
        // Stepping aside is to its own right (going +Z, that is -X), and back.
        assert!(second.way[0] < 0.0 && second.way[1] < 0.0, "{second:?}");
        // The later one in a doorway goes first.
        let going = [walker([0.0, -1.8], false), walker([0.0, 1.8], true)];
        let near = around(&level, &walkable, &open, &npcs, &going);
        let first = pass(0, npcs[0], [0.0, -1.0], false, &near);
        let second = pass(1, npcs[1], [0.0, 1.0], true, &near);
        assert!(
            first.giving_way && !second.giving_way,
            "{first:?} {second:?}"
        );
    }

    #[test]
    fn someone_going_the_same_way_is_kept_behind_not_walked_into() {
        let (level, walkable) = floor(&[]);
        let open = Openings::all_open();
        let npcs = [[0.0; 3], [0.0, 0.0, -0.8]];
        let going = [
            Going::default(),
            Going {
                velocity: [0.0, -1.0],
                doorway: false,
                walking: true,
                down: false,
            },
        ];
        let passing = pass(
            0,
            npcs[0],
            [0.0, -1.0],
            false,
            &around(&level, &walkable, &open, &npcs, &going),
        );
        assert!(passing.pace < 0.5, "{passing:?}");
        assert_eq!(passing.way, [0.0, -1.0]);
    }

    #[test]
    fn a_shut_door_on_the_way_stops_the_walker_short_of_it_wanting_it_opened() {
        let (level, walkable) = floor(&[]);
        let mut walk = Walk::default();
        walk.go(Goal::to([0.0, 0.0, -6.0], 0.5));
        // A way through a door, as the ground gives one.
        let gate = GateId(3);
        walk.way_mut().points = vec![
            lore_nav::Waypoint {
                point: [0.0; 3],
                leg: Leg::Walk,
            },
            lore_nav::Waypoint {
                point: [0.0, 0.0, -2.0],
                leg: Leg::Gate(gate),
            },
            lore_nav::Waypoint {
                point: [0.0, 0.0, -2.8],
                leg: Leg::Walk,
            },
            lore_nav::Waypoint {
                point: [0.0, 0.0, -6.0],
                leg: Leg::Walk,
            },
        ];
        walk.found(Ok(()), 1, STEP);
        let mut open = Openings::all_open();
        open.set_gate(gate, false);
        let rules = MoveSettings::default();
        let mut body = Player::at([0.0; 3], &rules);
        let npcs = [[0.0; 3]];
        let going = [Going::default()];
        for tick in 2..300 {
            let near = Around {
                tick,
                ..around(&level, &walkable, &open, &npcs, &going)
            };
            let (asked, _) = walk.steer(0, &body, 0.0, &near).unwrap();
            body.step(&level, &rules, &asked, STEP);
            if walk.door().is_some() {
                break;
            }
        }
        assert_eq!(walk.door(), Some(gate));
        let short = -2.0 - body.position[2];
        assert!(
            (-(DOOR_REACH + 0.2)..=-(DOOR_REACH - 0.3)).contains(&short),
            "{:?}",
            body.position
        );
    }

    #[test]
    fn why_it_cannot_get_there_is_said_with_names_and_how_far_short() {
        let door = |gate: GateId| (gate.0 == 0).then(|| "Cellar door".to_owned());
        let npc = |at: usize| (at == 2).then(|| "Smith".to_owned());
        let locked = Trouble::NoWay(NoWay::Shut {
            gates: vec![GateId(0)],
            links: Vec::new(),
            nearest: [0.0, 0.0, -2.0],
        });
        assert_eq!(
            locked.cause([0.0, 0.0, -6.0], door, npc),
            "the door \"Cellar door\" is locked (it has usable = false; set door.usable = true to \
             let people through) (the nearest it can get is 4 m short)"
        );
        let apart = Trouble::NoWay(NoWay::Apart {
            nearest: [0.0, 0.0, -9.5],
        });
        assert!(
            apart
                .cause([0.0, 0.0, -10.0], door, npc)
                .contains("(the nearest it can get is 0.5 m short). Make")
        );
        let stuck = Trouble::Stuck {
            at: [1.0, 0.0, -2.0],
            by: Some(Blocker::Npc(2)),
        };
        assert_eq!(
            stuck.cause([0.0; 3], door, npc),
            "Smith is in the way at (1.0, 0.0, -2.0), and it found no way round. Move whoever is \
             in the way, or send it somewhere else with goTo"
        );
    }
}
