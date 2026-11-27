//! A bot: something that plays in the player's place by pressing the
//! player's buttons, and nothing else.
//!
//! A bot is given a plan, a list of small steps (walk somewhere, look at
//! something, use it, press a button, attack someone, wait), and works
//! through it one step at a time. Each tick it reads what a player could
//! know (where its body is, the walkable ground, what is in view and what
//! the crosshair is on) and gives back [`Hands`]: how far the move stick is
//! pushed, how far the view turns and which buttons are held. Whoever runs
//! it turns those into the same input a person's devices give, so a bot
//! can do nothing a player cannot, and a recording of its play replays
//! exactly with no bot at all.
//!
//! It walks as an NPC walks ([`Walk`]): the same ways, the same steering,
//! jumping and crouching where the way says. A shut door on its way it
//! opens as a player must, by looking at it and pressing the use button.
//!
//! Every step ends done or stuck, and stuck has a cause as data
//! ([`Stuck`]) for whoever reports it to say in words. Everything counts
//! ticks, never a clock, and its dice are its own, from the seed it was
//! made with: the same plan in the same world plays the same game.

use std::collections::VecDeque;

use lore_nav::GateId;
use lore_physics::Player;

use crate::mind::Around;
use crate::walk::{Face, Goal, News, Trouble, Walk, across, ticks, wrapped, yaw_along};

/// How fast the view turns while the bot walks, in radians a second: as
/// fast as a body turns.
const WALK_TURN: f32 = 5.0;

/// How fast the view turns to look at something or aim, in radians a
/// second: a quick but human flick.
const LOOK_TURN: f32 = 8.0;

/// Within this many radians of where it wants to look, the bot is looking
/// there.
const LOOKING: f32 = 0.02;

/// How long the bot looks for something before it gives up, in seconds.
pub const LOOK_FOR: f32 = 5.0;

/// How long the bot hunts for someone to attack who is not in view before
/// it gives up, in seconds.
pub const HUNT_FOR: f32 = 20.0;

/// How long a door it opened is waited for, in seconds: a door takes 0.7
/// to swing open.
const DOOR_WAIT: f32 = 0.75;

/// The bot attacks from no further than this, in metres; further, or with
/// the one it attacks out of view, it walks nearer.
pub const KEEP_WITHIN: f32 = 10.0;

/// How near it walks when it walks up to someone to attack, in metres.
const CLOSE_TO: f32 = 7.0;

/// Nearer than this, in metres, it backs away while it attacks.
pub const KEEP_OFF: f32 = 2.5;

/// How long the trigger is held for a burst, in seconds.
pub const BURST: f32 = 0.3;
/// How long the trigger is let go between two bursts, in seconds.
pub const BETWEEN_BURSTS: f32 = 0.35;

/// The view must be this near the spot it aims at, in radians, for the
/// bot to pull the trigger.
const ON_TARGET: f32 = 0.03;

/// How long the bot reacts in, in seconds, at the middle of the
/// difficulty dial (an aim scale of 1).
pub const REACTION: f32 = 0.6;
/// How far off its aim is, in radians, either way, at the middle of the
/// difficulty dial.
pub const AIM_ERROR: f32 = 0.03;

/// One step of a bot's plan. `T` is how its user names a target (a place,
/// a thing, a person) and `A` an action.
#[derive(Debug, Clone, PartialEq)]
pub enum Step<T, A> {
    /// Walk to `to`, arriving within `within` metres of it; `run` holds
    /// the sprint button.
    GoTo {
        /// Where to: a place, a thing, or someone (followed as they move).
        to: T,
        /// How close is there, in metres across the ground.
        within: f32,
        /// Whether to sprint.
        run: bool,
    },
    /// Turn the view to look at `at`.
    LookAt {
        /// What to look at.
        at: T,
    },
    /// Walk up to `thing`, look at it and use it: press the use button, or
    /// hold it as long as the thing needs.
    Use {
        /// What to use.
        thing: T,
    },
    /// Hold `action` for `seconds` (a press, for one tick, at 0).
    Press {
        /// Which action.
        action: A,
        /// How long, in seconds.
        seconds: f32,
    },
    /// Attack `who` with what the player holds until they are down.
    Attack {
        /// Whom.
        who: T,
    },
    /// Do nothing for `seconds`.
    Wait {
        /// How long, in seconds.
        seconds: f32,
    },
}

impl<T, A> Step<T, A> {
    /// What the step is aimed at, if anything.
    #[must_use]
    pub fn target(&self) -> Option<&T> {
        match self {
            Self::GoTo { to: target, .. }
            | Self::LookAt { at: target }
            | Self::Use { thing: target }
            | Self::Attack { who: target } => Some(target),
            Self::Press { .. } | Self::Wait { .. } => None,
        }
    }
}

/// What the bot can tell of a target this tick, as a player would.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    /// Where to walk to for it: a place, or the feet of a thing or person.
    pub feet: [f32; 3],
    /// Where to look to look at it: a thing's middle, a person's chest.
    pub aim: [f32; 3],
    /// Whether it moves by itself (a person): a walk to it follows it.
    pub moves: bool,
    /// Whether nothing solid is between the player's eyes and `aim`.
    pub in_view: bool,
    /// Whether it is the usable thing the crosshair is on, in reach: what
    /// the use button would use now.
    pub meant: bool,
    /// How near the eyes must be to use it, in metres.
    pub reach: f32,
    /// How long the use button is held to use it, in seconds; 0 for a
    /// press.
    pub hold: f32,
    /// Whether it is a person whose health is gone.
    pub down: bool,
}

impl Spot {
    /// A bare place: walked to, looked at, never used.
    #[must_use]
    pub fn place(at: [f32; 3]) -> Self {
        Self {
            feet: at,
            aim: at,
            moves: false,
            in_view: true,
            meant: false,
            reach: 0.0,
            hold: 0.0,
            down: false,
        }
    }
}

/// What the player holds to attack with, as the bot reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arms {
    /// Rounds ready to fire.
    pub loaded: u32,
    /// Whether a reload is under way.
    pub reloading: bool,
}

/// How well the bot plays: how long it reacts in and how far off it aims.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Skill {
    /// Seconds from first having someone in view to the first shot.
    pub reaction: f32,
    /// How far off a burst may be aimed, in radians, either way.
    pub error: f32,
}

impl Skill {
    /// The skill at an aim scale of `aim` from the difficulty dial: 0.5 at
    /// the easiest, 1 in the middle, 1.5 at the hardest. The middle reacts
    /// in 0.6 s and aims within 0.03 rad (1.7°); the easiest takes 1.2 s
    /// and 0.06 rad (3.4°); the hardest 0.4 s and 0.02 rad (1.1°).
    #[must_use]
    pub fn of_aim(aim: f32) -> Self {
        let aim = if aim.is_finite() {
            aim.clamp(0.1, 10.0)
        } else {
            1.0
        };
        Self {
            reaction: REACTION / aim,
            error: AIM_ERROR / aim,
        }
    }
}

/// Everything the bot reads in one tick.
#[derive(Debug, Clone, Copy)]
pub struct Seen<'a, 'b> {
    /// The level, its walkable ground and everyone on it, as an NPC walking
    /// sees them. `around.player` should be far off (the bot is the player,
    /// and does not walk round itself).
    pub around: &'b Around<'a>,
    /// The bot's own place in `around.npcs`, if it is in it; any place past
    /// the end if not.
    pub me: usize,
    /// The player's body.
    pub body: &'b Player,
    /// Where the player looks: yaw (turning left from -Z) and pitch (up),
    /// in radians.
    pub view: [f32; 2],
    /// Where the player's eyes are.
    pub eyes: [f32; 3],
    /// What the step under way is aimed at, if it is aimed at something
    /// that is still there.
    pub target: Option<Spot>,
    /// The shut door the bot stands before, wanting it opened
    /// ([`Bot::door`]).
    pub door: Option<Spot>,
    /// What the player holds to attack with, if anything.
    pub arms: Option<Arms>,
    /// How well it plays.
    pub skill: Skill,
}

/// What the bot asks of the player's controls for one tick: the same
/// things a person's hands do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hands<A> {
    /// The move stick: right, then forward, each from -1 to 1.
    pub walk: [f32; 2],
    /// How far the view turns this tick: right, then up, in radians.
    pub turn: [f32; 2],
    /// Jump is held.
    pub jump: bool,
    /// Crouch is held.
    pub crouch: bool,
    /// Sprint is held.
    pub sprint: bool,
    /// The trigger is held.
    pub fire: bool,
    /// Aim (down the sights) is held.
    pub aim: bool,
    /// Reload is held.
    pub reload: bool,
    /// The use button.
    pub interact: bool,
    /// Any other action, held.
    pub action: Option<A>,
}

impl<A> Default for Hands<A> {
    fn default() -> Self {
        Self {
            walk: [0.0; 2],
            turn: [0.0; 2],
            jump: false,
            crouch: false,
            sprint: false,
            fire: false,
            aim: false,
            reload: false,
            interact: false,
            action: None,
        }
    }
}

impl<A> Hands<A> {
    /// Whether anything is pushed, turned or held.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.walk != [0.0; 2]
            || self.turn != [0.0; 2]
            || self.jump
            || self.crouch
            || self.sprint
            || self.fire
            || self.aim
            || self.reload
            || self.interact
            || self.action.is_some()
    }
}

/// Why a step could not be done.
#[derive(Debug, Clone, PartialEq)]
pub enum Stuck {
    /// The walk there failed: [`Trouble::cause`] says why in words.
    Walk {
        /// Why.
        why: Trouble,
        /// Where the walk was going.
        to: [f32; 3],
    },
    /// It looked for its target for this long and never had it where it
    /// wanted it: in view, or under the crosshair to use.
    NeverInView {
        /// How long it looked, in seconds.
        seconds: f32,
        /// Whether it had it in view, but could not use it from there.
        seen: bool,
    },
    /// It was to use something nobody can use: the game gave it no use.
    CannotBeUsed,
    /// It was to attack, and the player holds nothing to attack with.
    Unarmed,
    /// Its target is not in the world.
    Gone,
}

/// What a bot has to tell.
#[derive(Debug, Clone, PartialEq)]
pub enum BotNews<T, A> {
    /// A step is done.
    Did(Step<T, A>),
    /// The plan is done: every step in it.
    Done,
    /// A step could not be done, and the plan was given up.
    Stuck {
        /// The step.
        step: Step<T, A>,
        /// Why.
        why: Stuck,
    },
}

/// The step under way and how far along it is.
#[derive(Debug, Clone)]
struct Doing<T, A> {
    step: Step<T, A>,
    /// The tick it began on.
    began: u32,
    /// Whether the walk it began with is over (or it needed none).
    walked: bool,
    /// The tick it began looking on.
    looking_since: Option<u32>,
    /// Whether the target was ever in view while it looked.
    seen: bool,
    /// The button held for this step is let go on this tick.
    held_until: Option<u32>,
    /// Since when the one attacked has been in view without a break.
    in_view_since: Option<u32>,
    /// The tick the one attacked was last in view.
    last_in_view: u32,
    /// The trigger is held until this tick, then let go until `rest`.
    fire_until: u32,
    rest: u32,
    /// How far off this burst is aimed: yaw, then pitch, in radians.
    off: [f32; 2],
}

/// A bot: a plan, and the step under way.
#[derive(Debug, Clone)]
pub struct Bot<T, A> {
    plan: VecDeque<Step<T, A>>,
    doing: Option<Doing<T, A>>,
    walk: Walk,
    news: VecDeque<BotNews<T, A>>,
    /// The door being opened: the use button is let go on the tick here.
    door_press: Option<(GateId, u32)>,
    dice: u64,
}

impl<T: Clone, A: Copy> Bot<T, A> {
    /// A bot with nothing to do, whose dice start from `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            plan: VecDeque::new(),
            doing: None,
            walk: Walk::default(),
            news: VecDeque::new(),
            door_press: None,
            dice: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Adds `step` to the end of the plan.
    pub fn add(&mut self, step: Step<T, A>) {
        self.plan.push_back(step);
    }

    /// Gives up the plan, and stops where it is.
    pub fn stop(&mut self) {
        self.plan.clear();
        self.doing = None;
        self.walk.stop();
        self.door_press = None;
    }

    /// Whether it has anything left to do.
    #[must_use]
    pub fn playing(&self) -> bool {
        self.doing.is_some() || !self.plan.is_empty()
    }

    /// How many steps are left: those waiting, and the one under way.
    #[must_use]
    pub fn steps_left(&self) -> usize {
        self.plan.len() + usize::from(self.doing.is_some())
    }

    /// The step under way, beginning the next one in the plan if none is:
    /// what [`Seen::target`] is to be about this tick.
    pub fn step(&mut self, tick: u32) -> Option<&Step<T, A>> {
        if self.doing.is_none() {
            let step = self.plan.pop_front()?;
            self.doing = Some(Doing {
                step,
                began: tick,
                walked: false,
                looking_since: None,
                seen: false,
                held_until: None,
                in_view_since: None,
                last_in_view: tick,
                fire_until: 0,
                rest: 0,
                off: [0.0; 2],
            });
        }
        self.doing.as_ref().map(|doing| &doing.step)
    }

    /// The shut door the bot stands before and wants opened: what
    /// [`Seen::door`] is to be about.
    #[must_use]
    pub fn door(&self) -> Option<GateId> {
        self.walk.door().or(self.door_press.map(|(door, _)| door))
    }

    /// Its walk, for whoever finds it a way ([`Walk::wants_way`],
    /// [`Walk::way_mut`], [`Walk::found`]).
    pub fn walk_mut(&mut self) -> &mut Walk {
        &mut self.walk
    }

    /// Its walk.
    #[must_use]
    pub fn walk(&self) -> &Walk {
        &self.walk
    }

    /// The next thing it has to tell, oldest first.
    pub fn take_news(&mut self) -> Option<BotNews<T, A>> {
        self.news.pop_front()
    }

    /// What the bot does with the player's controls this tick.
    pub fn play(&mut self, seen: &Seen<'_, '_>) -> Hands<A> {
        let mut hands = Hands::default();
        let tick = seen.around.tick;
        let dt = seen.around.seconds;
        if self.step(tick).is_none() {
            return hands;
        }
        let Some(mut doing) = self.doing.take() else {
            return hands;
        };
        let ended = match doing.step.clone() {
            Step::GoTo { within, run, .. } => match seen.target {
                None => Some(Err(Stuck::Gone)),
                Some(sight) => {
                    if !doing.walked && self.walk.goal().is_none() {
                        doing.walked = true;
                        self.walk.go(goal(sight, within));
                    }
                    self.walk.retarget(sight.feet);
                    self.stride(seen, &mut hands, run)
                }
            },
            Step::LookAt { .. } => match seen.target {
                None => Some(Err(Stuck::Gone)),
                Some(sight) => {
                    let since = *doing.looking_since.get_or_insert(tick);
                    let gap = self.look(seen, &mut hands, sight.aim, LOOK_TURN);
                    doing.seen |= sight.in_view;
                    if sight.in_view && gap < LOOKING {
                        Some(Ok(()))
                    } else {
                        overdue(since, tick, dt, doing.seen)
                    }
                }
            },
            Step::Use { .. } => match seen.target {
                None => Some(Err(Stuck::Gone)),
                Some(sight) => self.use_it(seen, &mut hands, &mut doing, sight),
            },
            Step::Press { action, seconds } => {
                let until = *doing
                    .held_until
                    .get_or_insert(tick.saturating_add(ticks(seconds, dt)));
                if tick < until {
                    hands.action = Some(action);
                    None
                } else {
                    // One tick let go, so a press after it is a press.
                    Some(Ok(()))
                }
            }
            Step::Attack { .. } => match (seen.target, seen.arms) {
                (None, _) => Some(Err(Stuck::Gone)),
                (Some(sight), _) if sight.down => Some(Ok(())),
                (_, None) => Some(Err(Stuck::Unarmed)),
                (Some(sight), Some(arms)) => self.attack(seen, &mut hands, &mut doing, sight, arms),
            },
            Step::Wait { seconds } => {
                (tick.saturating_sub(doing.began) >= ticks(seconds, dt)).then_some(Ok(()))
            }
        };
        match ended {
            None => self.doing = Some(doing),
            Some(Ok(())) => {
                self.walk.stop();
                self.door_press = None;
                self.news.push_back(BotNews::Did(doing.step));
                if self.plan.is_empty() {
                    self.news.push_back(BotNews::Done);
                }
            }
            Some(Err(why)) => {
                self.walk.stop();
                self.door_press = None;
                self.plan.clear();
                self.news.push_back(BotNews::Stuck {
                    step: doing.step,
                    why,
                });
            }
        }
        hands
    }

    /// A tick of walking: the stick, the view turned the way it walks, and
    /// a shut door on the way opened with the use button. `Some` when the
    /// walk is over: there, or stuck.
    fn stride(
        &mut self,
        seen: &Seen<'_, '_>,
        hands: &mut Hands<A>,
        run: bool,
    ) -> Option<Result<(), Stuck>> {
        if let Some(news) = self.walk.take_news() {
            return Some(self.walk_ended(news));
        }
        let tick = seen.around.tick;
        let dt = seen.around.seconds;
        let yaw = seen.view[0];
        let steered = self.walk.steer(seen.me, seen.body, yaw, seen.around);
        if let Some(news) = self.walk.take_news() {
            return Some(self.walk_ended(news));
        }
        let (moving, face) = steered?;
        // A door it is opening, or one it stands before.
        if let Some((door, until)) = self.door_press {
            if let Some(sight) = seen.door {
                self.look(seen, hands, sight.aim, LOOK_TURN);
            }
            if tick < until {
                hands.interact = true;
            } else {
                // Let go: it waits for the door to swing open.
                self.door_press = None;
                self.walk
                    .hold(tick.saturating_add(ticks(DOOR_WAIT, dt)), door);
            }
            return None;
        }
        if let (Some(door), Some(sight)) = (self.walk.door(), seen.door) {
            self.look(seen, hands, sight.aim, LOOK_TURN);
            if sight.meant {
                let held = ticks(sight.hold, dt).max(1);
                self.door_press = Some((door, tick.saturating_add(held)));
                hands.interact = true;
            }
            return None;
        }
        // The view turns the way it wants to face, level, and the stick is
        // pushed the way the walk goes, as seen from the view turned.
        let now = face.map_or(yaw, |face| {
            let gap = wrapped(face - yaw);
            let most = WALK_TURN * dt;
            yaw + gap.clamp(-most, most)
        });
        let most = LOOK_TURN * dt;
        hands.turn = [-(now - yaw), (-seen.view[1]).clamp(-most, most)];
        let way = along(yaw, moving.forward, moving.right);
        let (forward, right) = stick(now, way);
        hands.walk = [right, forward];
        hands.jump = moving.jump;
        hands.crouch = moving.crouch;
        hands.sprint = run && forward > 0.5 && !moving.crouch;
        None
    }

    /// What a walk's news means for the step.
    fn walk_ended(&self, news: News) -> Result<(), Stuck> {
        match news {
            News::Arrived => Ok(()),
            News::CannotReach { why, to } => Err(Stuck::Walk { why, to }),
            News::Stopped => Err(Stuck::Walk {
                why: self.walk.trouble().cloned().unwrap_or(Trouble::Gone),
                to: self.walk.goal().map_or([0.0; 3], |goal| goal.to),
            }),
        }
    }

    /// Turns the view toward `at` at no more than `speed` radians a
    /// second: how far off it still is, in radians.
    fn look(&self, seen: &Seen<'_, '_>, hands: &mut Hands<A>, at: [f32; 3], speed: f32) -> f32 {
        self.look_off(seen, hands, at, [0.0; 2], speed)
    }

    /// [`look`](Self::look), aimed `off` (yaw, pitch) from `at`.
    fn look_off(
        &self,
        seen: &Seen<'_, '_>,
        hands: &mut Hands<A>,
        at: [f32; 3],
        off: [f32; 2],
        speed: f32,
    ) -> f32 {
        let Some([yaw, pitch]) = view_toward(seen.eyes, at) else {
            return 0.0;
        };
        let gap = [
            wrapped(yaw + off[0] - seen.view[0]),
            (pitch + off[1]).clamp(-1.5, 1.5) - seen.view[1],
        ];
        let most = speed * seen.around.seconds;
        hands.turn = [-gap[0].clamp(-most, most), gap[1].clamp(-most, most)];
        libm::hypotf(gap[0], gap[1])
    }

    /// A tick of using a thing: walk up to it, look at it, press or hold
    /// the use button once it is what the crosshair is on.
    fn use_it(
        &mut self,
        seen: &Seen<'_, '_>,
        hands: &mut Hands<A>,
        doing: &mut Doing<T, A>,
        sight: Spot,
    ) -> Option<Result<(), Stuck>> {
        let tick = seen.around.tick;
        let dt = seen.around.seconds;
        if sight.reach <= 0.0 {
            return Some(Err(Stuck::CannotBeUsed));
        }
        if let Some(until) = doing.held_until {
            // Held as long as it needs, then one tick let go.
            if tick < until {
                hands.interact = true;
                self.look(seen, hands, sight.aim, LOOK_TURN);
                return None;
            }
            return Some(Ok(()));
        }
        if !doing.walked {
            let near = distance(seen.eyes, sight.aim) <= sight.reach * 0.8;
            if near && self.walk.goal().is_none() {
                doing.walked = true;
            } else {
                if self.walk.goal().is_none() {
                    self.walk.go(goal(sight, (sight.reach * 0.5).max(0.5)));
                }
                self.walk.retarget(sight.feet);
                match self.stride(seen, hands, false)? {
                    Ok(()) => doing.walked = true,
                    Err(why) => return Some(Err(why)),
                }
                return None;
            }
        }
        let since = *doing.looking_since.get_or_insert(tick);
        doing.seen |= sight.in_view;
        self.look(seen, hands, sight.aim, LOOK_TURN);
        if sight.meant {
            // The last tick of holding the button, counted from the first.
            doing.held_until = Some(tick.saturating_add(ticks(sight.hold, dt).max(1) + 1));
            hands.interact = true;
            return None;
        }
        overdue(since, tick, dt, doing.seen)
    }

    /// A tick of attacking: nearer when out of view or far, aim (late and
    /// off by the bot's skill), fire in bursts, reload when empty.
    fn attack(
        &mut self,
        seen: &Seen<'_, '_>,
        hands: &mut Hands<A>,
        doing: &mut Doing<T, A>,
        sight: Spot,
        arms: Arms,
    ) -> Option<Result<(), Stuck>> {
        let tick = seen.around.tick;
        let dt = seen.around.seconds;
        if sight.in_view {
            doing.in_view_since.get_or_insert(tick);
            doing.last_in_view = tick;
        } else {
            doing.in_view_since = None;
            doing.fire_until = 0;
        }
        let far = across(seen.body.position, sight.feet);
        if !sight.in_view || far > KEEP_WITHIN {
            if tick.saturating_sub(doing.last_in_view) > ticks(HUNT_FOR, dt) {
                return Some(Err(Stuck::NeverInView {
                    seconds: HUNT_FOR,
                    seen: false,
                }));
            }
            if self.walk.goal().is_none() {
                self.walk.go(Goal {
                    moves: true,
                    ..goal(sight, CLOSE_TO)
                });
            }
            self.walk.retarget(sight.feet);
            // Arriving near them is not news for an attack.
            // It does not shoot while it closes in: the stick goes the way
            // the view turns as it walks.
            if let Some(Err(why)) = self.stride(seen, hands, false) {
                return Some(Err(why));
            }
            return None;
        } else if self.walk.goal().is_some() {
            self.walk.stop();
        }
        if far < KEEP_OFF {
            hands.walk = [0.0, -1.0];
        }
        let gap = self.look_off(seen, hands, sight.aim, doing.off, LOOK_TURN);
        if arms.loaded == 0 {
            // Empty: a press of reload, unless one is under way.
            hands.reload = !arms.reloading && tick.is_multiple_of(2);
            return None;
        }
        let reacted = doing
            .in_view_since
            .is_some_and(|since| tick.saturating_sub(since) >= ticks(seen.skill.reaction, dt));
        if tick < doing.fire_until {
            hands.fire = true;
        } else if tick >= doing.rest && reacted && gap < ON_TARGET {
            doing.fire_until = tick.saturating_add(ticks(BURST, dt));
            doing.rest = doing.fire_until.saturating_add(ticks(BETWEEN_BURSTS, dt));
            hands.fire = true;
            // The next burst is aimed afresh, off by up to the skill's error.
            let error = seen.skill.error;
            doing.off = [self.roll() * error, self.roll() * error];
        }
        None
    }

    /// A number from -1 to 1 from the bot's own dice.
    fn roll(&mut self) -> f32 {
        // splitmix64: the same numbers from the same seed on every machine.
        self.dice = self.dice.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.dice;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // The top 24 bits, exactly representable.
        #[allow(clippy::cast_precision_loss)]
        let unit = (z >> 40) as f32 / (1u64 << 24) as f32;
        unit * 2.0 - 1.0
    }
}

/// A walk to `sight` arriving within `within` metres, at full stick.
fn goal(sight: Spot, within: f32) -> Goal {
    Goal {
        moves: sight.moves,
        face: Face::AsWalked,
        // The pace of a run is the move stick pushed all the way.
        run: true,
        ..Goal::to(sight.feet, within)
    }
}

/// Stuck, if it has looked since `since` for longer than [`LOOK_FOR`].
fn overdue(since: u32, tick: u32, dt: f32, seen: bool) -> Option<Result<(), Stuck>> {
    (tick.saturating_sub(since) > ticks(LOOK_FOR, dt)).then_some(Err(Stuck::NeverInView {
        seconds: LOOK_FOR,
        seen,
    }))
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    libm::sqrtf(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
}

/// The yaw and pitch that look from `eyes` at `at`.
fn view_toward(eyes: [f32; 3], at: [f32; 3]) -> Option<[f32; 2]> {
    let d = [at[0] - eyes[0], at[1] - eyes[1], at[2] - eyes[2]];
    let flat = libm::hypotf(d[0], d[2]);
    (flat.is_finite() && (flat > 1e-4 || d[1].abs() > 1e-4))
        .then(|| [yaw_along([d[0], d[2]]), libm::atan2f(d[1], flat)])
}

/// The way across the ground that `forward` and `right` push a body
/// facing `yaw`.
fn along(yaw: f32, forward: f32, right: f32) -> [f32; 2] {
    let (sin, cos) = (libm::sinf(yaw), libm::cosf(yaw));
    let ahead = [-sin, -cos];
    let aside = [cos, -sin];
    [
        ahead[0] * forward + aside[0] * right,
        ahead[1] * forward + aside[1] * right,
    ]
}

/// The stick, forward and right, that pushes a body facing `yaw` along
/// `way`.
fn stick(yaw: f32, way: [f32; 2]) -> (f32, f32) {
    let (sin, cos) = (libm::sinf(yaw), libm::cosf(yaw));
    let forward = -sin * way[0] - cos * way[1];
    let right = cos * way[0] - sin * way[1];
    (forward.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use lore_nav::{Body, Openings, Walkable};
    use lore_physics::{Level, MoveSettings};

    use super::*;

    const DT: f32 = 1.0 / 60.0;

    type TestBot = Bot<[f32; 3], u8>;

    struct Place {
        level: Level,
        walkable: Walkable,
        openings: Openings,
    }

    fn flat() -> Place {
        let r = 20.0;
        let level = Level::from_triangles(&[
            [[-r, 0.0, -r], [r, 0.0, r], [r, 0.0, -r]],
            [[-r, 0.0, -r], [-r, 0.0, r], [r, 0.0, r]],
        ]);
        Place {
            level,
            walkable: Walkable::flat([-r, -r], [r, r], 0.0, Body::default()),
            openings: Openings::all_open(),
        }
    }

    fn around<'a>(place: &'a Place, tick: u32) -> Around<'a> {
        Around {
            level: &place.level,
            walkable: &place.walkable,
            openings: &place.openings,
            tick,
            seconds: DT,
            player: [f32::INFINITY; 3],
            npcs: &[],
            going: &[],
        }
    }

    fn body_at(feet: [f32; 3]) -> Player {
        Player::at(feet, &MoveSettings::default())
    }

    fn seen<'a, 'b>(around: &'b Around<'a>, body: &'b Player, view: [f32; 2]) -> Seen<'a, 'b> {
        Seen {
            around,
            me: 0,
            body,
            view,
            eyes: body.eye_position(),
            target: None,
            door: None,
            arms: None,
            skill: Skill::of_aim(1.0),
        }
    }

    #[test]
    fn the_skill_follows_the_dial() {
        let easy = Skill::of_aim(0.5);
        let middle = Skill::of_aim(1.0);
        let hard = Skill::of_aim(1.5);
        assert!((middle.reaction - 0.6).abs() < 1e-6 && (middle.error - 0.03).abs() < 1e-6);
        assert!(easy.reaction > middle.reaction && middle.reaction > hard.reaction);
        assert!(easy.error > middle.error && middle.error > hard.error);
        assert_eq!(Skill::of_aim(f32::NAN), middle);
    }

    #[test]
    fn a_press_holds_the_action_for_its_time_then_lets_go_and_is_done() {
        let place = flat();
        let body = body_at([0.0; 3]);
        let mut bot = TestBot::new(1);
        bot.add(Step::Press {
            action: 7,
            seconds: 0.1,
        });
        let mut held = 0;
        let mut tick = 1;
        while bot.playing() {
            let around = around(&place, tick);
            let hands = bot.play(&seen(&around, &body, [0.0; 2]));
            if hands.action == Some(7) {
                held += 1;
            }
            tick += 1;
            assert!(tick < 100);
        }
        assert_eq!(held, 6);
        assert_eq!(
            bot.take_news(),
            Some(BotNews::Did(Step::Press {
                action: 7,
                seconds: 0.1
            }))
        );
        assert_eq!(bot.take_news(), Some(BotNews::Done));
    }

    #[test]
    fn a_wait_waits_and_presses_nothing() {
        let place = flat();
        let body = body_at([0.0; 3]);
        let mut bot = TestBot::new(1);
        bot.add(Step::Wait { seconds: 1.0 });
        let mut tick = 1;
        while bot.playing() {
            let around = around(&place, tick);
            assert!(!bot.play(&seen(&around, &body, [0.0; 2])).busy());
            tick += 1;
        }
        assert_eq!(tick, 62);
    }

    #[test]
    fn looking_turns_the_view_at_a_human_pace_until_it_is_on_the_target() {
        let place = flat();
        let body = body_at([0.0; 3]);
        let mut bot = TestBot::new(1);
        // Straight behind: half a turn.
        let at = [0.0, 1.6, 10.0];
        bot.add(Step::LookAt { at });
        let mut view = [0.0f32, 0.0];
        let mut tick = 1;
        while bot.playing() {
            let around = around(&place, tick);
            let mut seen = seen(&around, &body, view);
            seen.target = Some(Spot::place(at));
            let hands = bot.play(&seen);
            assert!(hands.turn[0].abs() <= LOOK_TURN * DT + 1e-6);
            view = [view[0] - hands.turn[0], view[1] + hands.turn[1]];
            tick += 1;
            assert!(tick < 200);
        }
        // Half a turn at 8 radians a second: about 0.4 s.
        assert!((20..40).contains(&tick), "{tick}");
        assert!(wrapped(view[0] - std::f32::consts::PI).abs() < LOOKING);
    }

    #[test]
    fn something_never_in_view_is_given_up_on_after_five_seconds() {
        let place = flat();
        let body = body_at([0.0; 3]);
        let mut bot = TestBot::new(1);
        bot.add(Step::LookAt { at: [0.0; 3] });
        bot.add(Step::Wait { seconds: 1.0 });
        let mut tick = 1;
        while bot.playing() {
            let around = around(&place, tick);
            let mut seen = seen(&around, &body, [0.0; 2]);
            seen.target = Some(Spot {
                in_view: false,
                ..Spot::place([0.0, 1.0, -5.0])
            });
            bot.play(&seen);
            tick += 1;
        }
        assert!((300..310).contains(&tick), "{tick}");
        match bot.take_news() {
            Some(BotNews::Stuck {
                why: Stuck::NeverInView { seconds, seen },
                ..
            }) => assert!(seconds == LOOK_FOR && !seen),
            other => panic!("{other:?}"),
        }
        // The rest of the plan is given up.
        assert!(!bot.playing());
        assert_eq!(bot.take_news(), None);
    }

    #[test]
    fn attacking_with_nothing_held_says_so() {
        let place = flat();
        let body = body_at([0.0; 3]);
        let mut bot = TestBot::new(1);
        bot.add(Step::Attack { who: [0.0; 3] });
        let around = around(&place, 1);
        let mut seen = seen(&around, &body, [0.0; 2]);
        seen.target = Some(Spot::place([0.0, 0.0, -5.0]));
        bot.play(&seen);
        assert!(matches!(
            bot.take_news(),
            Some(BotNews::Stuck {
                why: Stuck::Unarmed,
                ..
            })
        ));
    }

    /// Ticks of attacking someone standing 6 m ahead, in view, until the
    /// first shot, at an aim scale of `aim`.
    fn first_shot(aim: f32) -> u32 {
        let place = flat();
        let body = body_at([0.0; 3]);
        let mut bot = TestBot::new(3);
        bot.add(Step::Attack { who: [0.0; 3] });
        let mut view = [0.0f32, 0.0];
        for tick in 1..600 {
            let around = around(&place, tick);
            let mut seen = seen(&around, &body, view);
            seen.target = Some(Spot {
                moves: true,
                ..Spot::place([0.0, 1.3, -6.0])
            });
            seen.arms = Some(Arms {
                loaded: 30,
                reloading: false,
            });
            seen.skill = Skill::of_aim(aim);
            let hands = bot.play(&seen);
            view = [view[0] - hands.turn[0], view[1] + hands.turn[1]];
            if hands.fire {
                return tick;
            }
        }
        panic!("never fired");
    }

    #[test]
    fn the_first_shot_comes_after_the_reaction_time_of_the_dial() {
        let middle = first_shot(1.0);
        let easiest = first_shot(0.5);
        assert!((36..=40).contains(&middle), "{middle}");
        assert!((72..=76).contains(&easiest), "{easiest}");
    }

    #[test]
    fn an_empty_weapon_is_reloaded_not_fired() {
        let place = flat();
        let body = body_at([0.0; 3]);
        let mut bot = TestBot::new(3);
        bot.add(Step::Attack { who: [0.0; 3] });
        let mut reloads = 0;
        for tick in 1..120 {
            let around = around(&place, tick);
            let mut seen = seen(&around, &body, [0.0; 2]);
            seen.target = Some(Spot::place([0.0, 1.3, -6.0]));
            seen.arms = Some(Arms {
                loaded: 0,
                reloading: false,
            });
            let hands = bot.play(&seen);
            assert!(!hands.fire);
            reloads += u32::from(hands.reload);
        }
        assert!(reloads > 0);
    }

    #[test]
    fn a_walk_pushes_the_stick_toward_the_place_and_arrives() {
        let place = flat();
        let mut body = body_at([0.0; 3]);
        let rules = MoveSettings::default();
        let to = [6.0, 0.0, -6.0];
        let mut bot = TestBot::new(1);
        bot.add(Step::GoTo {
            to,
            within: 0.5,
            run: false,
        });
        let mut view = [0.0f32, 0.0];
        let mut tick = 1;
        while bot.playing() {
            if let Some((from, to)) = bot.walk().wants_way(&body) {
                let found = place.walkable.way(from, to, &place.openings);
                let found = found.map(|way| *bot.walk_mut().way_mut() = way);
                bot.walk_mut().found(found, tick, DT);
            }
            let around = around(&place, tick);
            let mut seen = seen(&around, &body, view);
            seen.target = Some(Spot::place(to));
            let hands = bot.play(&seen);
            view = [
                (view[0] - hands.turn[0]).rem_euclid(std::f32::consts::TAU),
                view[1] + hands.turn[1],
            ];
            let asked = lore_physics::MoveInput {
                forward: hands.walk[1],
                right: hands.walk[0],
                yaw: view[0],
                ..Default::default()
            };
            body.step(&place.level, &rules, &asked, DT);
            tick += 1;
            assert!(tick < 60 * 10, "{:?}", body.position);
        }
        assert_eq!(
            bot.take_news().map(|news| matches!(news, BotNews::Did(_))),
            Some(true)
        );
        assert!(across(body.position, to) < 0.6, "{:?}", body.position);
    }
}
