//! How much an NPC has noticed: bit by bit, from "what was that?" to
//! "there he is", and back to calm.
//!
//! Each NPC keeps a gauge from 0 to 1 for each one it has noticed, and one
//! more for a place (a noise nobody made, a body found). Its senses
//! ([`Senses`]) fill it: sight by how strongly someone is seen, for as long
//! as they are in view, so a glimpse at the edge of the cone in the dark
//! fills it slowly and someone in plain view close by at once; a noise by
//! how loud it reached the ears. Time drains it, slowly, once nothing has
//! fed it for a moment ([`GRACE_SECONDS`]).
//!
//! From the gauge come four levels ([`Aware`]): unaware, curious (something
//! is there: it looks that way), searching (someone was there: the place
//! they were last sensed is kept) and alarmed (it sees them). A level rises
//! as soon as the gauge reaches it and falls one step at a time, each at a
//! lower mark than it rose at, so it does not flicker at a boundary: after
//! being alarmed it searches before it is calm. Hearing alone never fills
//! the gauge past [`HEARING_TOPS`], which is searching and no further.
//!
//! How far the gauge of someone may go is how the NPC feels about them
//! (the world asks its table of sides, see [`Awareness::step`]): only an
//! enemy is searched for or alarms it. A neutral (a stranger, the player
//! in a game with no sides) is at most a curiosity ([`CURIOUS_TOPS`]): it
//! looks at them, and forgets them a few seconds after they go
//! ([`QUICK_DRAIN`]). A friend is only noticed ([`CALM_TOPS`]). The noises
//! of those it is not an enemy of go unremarked, but a weapon's noise is
//! never ignored: it is an interest in the place it came from. The body of
//! one it is not an enemy of, found lying there, is an alarm about the
//! place it lies, once.
//!
//! The numbers, at the middle of the difficulty dial and an alertness of 1:
//! someone seen at a strength of 1 alarms in 0.7 seconds, of 0.75 (standing
//! still, close, in plain light) in 1.15, of 0.6 (moving, 20 metres off) in
//! 1.7, of 0.3 in 6.6; the first look at someone counts half
//! ([`FIRST_LOOK`]). After the last sight of an enemy it is alarmed 4.75
//! seconds more, searching till 16, curious till 23.5, and then calm.
//!
//! Everything is plain data in storage of a fixed size and pure functions
//! of it, stepped once a tick: the same senses give the same levels on the
//! same ticks.

use crate::senses::{Heard, Senses, Sighting};
use crate::sides::Feeling;

/// The most people one NPC keeps a gauge for. Who gets a place when they
/// are full: the player always; one who may alarm it (an enemy) before one
/// who may not, who gives way, the least noticed first; among enemies, the
/// least noticed and longest unsensed gives way, but never one alarming it
/// or sensed within [`GRACE_SECONDS`]; one who may not alarm it only takes
/// the place of another such who is unaware and noticed less.
pub const MOST_NOTICED: usize = 8;

/// How many bodies an NPC remembers having found, so the same body in view
/// does not alarm it again and again.
pub const MOST_BODIES: usize = 8;

/// From this full the NPC is curious.
pub const CURIOUS_AT: f32 = 0.25;
/// From this full it is searching.
pub const SEARCHING_AT: f32 = 0.6;
/// Full: it is alarmed.
pub const ALARMED_AT: f32 = 1.0;
/// Below this a curious NPC is calm again.
pub const CURIOUS_BELOW: f32 = 0.1;
/// Below this a searching NPC is only curious.
pub const SEARCHING_BELOW: f32 = 0.4;
/// Below this an alarmed NPC is searching.
pub const ALARMED_BELOW: f32 = 0.85;

/// The fullest hearing alone fills the gauge: searching, never alarmed.
pub const HEARING_TOPS: f32 = 0.8;
/// The fullest the gauge of a neutral goes: curious, never searching.
pub const CURIOUS_TOPS: f32 = 0.5;
/// The fullest the gauge of a friend goes: noticed, not even curious.
pub const CALM_TOPS: f32 = 0.2;

/// How much a second of being seen at a strength of 1 fills the gauge;
/// the fill goes with the square of the strength, so a faint glimpse fills
/// it far more slowly than a clear look.
pub const SIGHT_FILL: f32 = 1.7;
/// How much the time from the first look that sees someone to the next
/// counts: half, as they may have left view just after it.
pub const FIRST_LOOK: f32 = 0.5;
/// How much one noise heard at a strength of 1 fills the gauge at once.
pub const HEARING_FILL: f32 = 0.6;
/// How many times more a weapon's noise (a shot, a blast) fills it.
pub const WEAPON_HEARD: f32 = 4.0;
/// How strongly a weapon's noise heard through a wall must arrive to make
/// the NPC search at once, as one heard in the open does; fainter, it is
/// curious at once. About 16 metres off for a shot.
pub const WEAPON_NEAR: f32 = 0.25;
/// How long the gauge holds after it was last fed, in seconds, before it
/// drains.
pub const GRACE_SECONDS: f32 = 1.0;
/// How much the gauge drains each second after that.
pub const DRAIN: f32 = 0.04;
/// How much the gauge of one who may not alarm the NPC (a neutral, a
/// friend) drains each second after that, and how fast any gauge fuller
/// than it may be drains: a neutral is forgotten about three seconds after
/// they go.
pub const QUICK_DRAIN: f32 = 0.2;

/// How aware an NPC is of someone or something: the four levels, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Aware {
    /// It has noticed nothing.
    #[default]
    Unaware,
    /// Something is there: it looks that way.
    Curious,
    /// Someone was there: it knows where they were last sensed.
    Searching,
    /// It sees them.
    Alarmed,
}

impl Aware {
    /// All four, from the least.
    pub const ALL: [Self; 4] = [Self::Unaware, Self::Curious, Self::Searching, Self::Alarmed];

    /// The word a script reads: "unaware", "curious", "searching" or
    /// "alarmed".
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Unaware => "unaware",
            Self::Curious => "curious",
            Self::Searching => "searching",
            Self::Alarmed => "alarmed",
        }
    }

    /// The level a gauge this full rises to.
    #[must_use]
    pub fn rising_to(gauge: f32) -> Self {
        if gauge >= ALARMED_AT {
            Self::Alarmed
        } else if gauge >= SEARCHING_AT {
            Self::Searching
        } else if gauge >= CURIOUS_AT {
            Self::Curious
        } else {
            Self::Unaware
        }
    }

    /// Below how full the gauge must fall for this level to drop a step.
    fn falls_below(self) -> f32 {
        match self {
            Self::Unaware => f32::NEG_INFINITY,
            Self::Curious => CURIOUS_BELOW,
            Self::Searching => SEARCHING_BELOW,
            Self::Alarmed => ALARMED_BELOW,
        }
    }

    /// One step lower.
    fn lower(self) -> Self {
        match self {
            Self::Unaware | Self::Curious => Self::Unaware,
            Self::Searching => Self::Curious,
            Self::Alarmed => Self::Searching,
        }
    }
}

/// How full the gauge of one the NPC feels `feeling` about may go.
#[must_use]
pub fn tops_for(feeling: Feeling) -> f32 {
    match feeling {
        Feeling::Enemy => ALARMED_AT,
        Feeling::Neutral => CURIOUS_TOPS,
        Feeling::Friend => CALM_TOPS,
    }
}

/// What an NPC has noticed of one person, or of a place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Notice<W> {
    /// Who: `None` for a place (a noise nobody made, a body found, a place
    /// a script raised the alarm about).
    pub about: Option<W>,
    /// How full the gauge is, from 0 to 1.
    pub gauge: f32,
    /// The level it is at.
    pub level: Aware,
    /// The tick it came to that level on.
    pub since: u32,
    /// Where they (or it) were last sensed, in metres: where a search
    /// looks.
    pub at: [f32; 3],
    /// The tick they were last seen on, if ever.
    pub seen: Option<u32>,
    /// The tick they were last heard on, if ever.
    pub heard: Option<u32>,
    /// For a place: whose body was found there, if that is what it is.
    pub body: Option<W>,
    /// How full the gauge may go now: [`tops_for`] how the NPC feels about
    /// them, 1 for a place and for one a script raised the alarm about.
    pub tops: f32,
    /// The tick anything last fed the gauge.
    fed: u32,
    /// The tick anything that may fill it past [`HEARING_TOPS`] last fed
    /// it: sight, a body, a script.
    fed_high: u32,
    /// Whether a script raised the alarm about them: they may alarm it,
    /// whoever they are, until it is calm again.
    raised: bool,
}

impl<W> Notice<W> {
    /// The tick they were last sensed on, seen or heard, if ever.
    #[must_use]
    pub fn sensed(&self) -> Option<u32> {
        match (self.seen, self.heard) {
            (Some(a), Some(b)) => Some(if b.wrapping_sub(a) < u32::MAX / 2 {
                b
            } else {
                a
            }),
            (one, other) => one.or(other),
        }
    }

    /// How many ticks it has been at its level, as of `tick`.
    #[must_use]
    pub fn ticks_at_level(&self, tick: u32) -> u32 {
        tick.wrapping_sub(self.since)
    }

    /// Whether it is a place rather than a person.
    #[must_use]
    pub fn is_place(&self) -> bool {
        self.about.is_none()
    }

    /// Whether it may go past curious: a place, an enemy, or one a script
    /// raised the alarm about. What the body turns to, and what the marks
    /// over NPCs show.
    #[must_use]
    pub fn may_alarm(&self) -> bool {
        self.tops > CURIOUS_TOPS
    }
}

/// A level that changed: what scripts are told, and where a spoken line
/// (#66) will be picked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Change<W> {
    /// Who it is about: `None` for a place.
    pub about: Option<W>,
    /// Where they (or it) were last sensed, in metres.
    pub at: [f32; 3],
    /// The level now.
    pub now: Aware,
    /// The level before.
    pub before: Aware,
}

/// What a tick says to an NPC's awareness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace<W> {
    /// The tick it is.
    pub tick: u32,
    /// How long a tick is, in seconds.
    pub seconds: f32,
    /// How quickly this NPC notices: 1 as a person does, 0.5 a sleepy
    /// guard. Every fill is multiplied by it.
    pub alertness: f32,
    /// The player, whom the difficulty dial is about.
    pub player: Option<W>,
    /// What every fill of the player's gauge is multiplied by besides: the
    /// difficulty dial's `notice` (0.5 at the easiest, 1 in the middle, 1.5
    /// at the hardest).
    pub player_scale: f32,
}

/// How a gauge is fed.
#[derive(Clone, Copy, PartialEq)]
enum Feed {
    /// By sight.
    Sight,
    /// By hearing.
    Hearing,
}

/// Everything one NPC has noticed, in storage of a fixed size.
#[derive(Debug, Clone, PartialEq)]
pub struct Awareness<W> {
    people: [Option<Notice<W>>; MOST_NOTICED],
    place: Option<Notice<W>>,
    /// The bodies it found, so the same one does not alarm it twice: who,
    /// until it sees them on their feet again.
    bodies: [Option<W>; MOST_BODIES],
    /// Where the next body found is remembered.
    next_body: usize,
    /// The player, who always has a place.
    player: Option<W>,
    /// Those let go to make room while more than unaware: told as calm at
    /// the next step.
    dropped: [Option<Change<W>>; MOST_NOTICED],
    /// How many ticks [`GRACE_SECONDS`] is, as of the last step.
    grace: u32,
}

impl<W: Copy + PartialEq> Awareness<W> {
    /// An NPC that has noticed nothing yet. `player` is the one who always
    /// has a place.
    #[must_use]
    pub fn new(player: Option<W>) -> Self {
        Self {
            people: [None; MOST_NOTICED],
            place: None,
            bodies: [None; MOST_BODIES],
            next_body: 0,
            player,
            dropped: [None; MOST_NOTICED],
            grace: 60,
        }
    }

    /// What it has noticed of `who`, if anything.
    #[must_use]
    pub fn of(&self, who: W) -> Option<&Notice<W>> {
        self.people.iter().flatten().find(|n| n.about == Some(who))
    }

    /// How aware it is of `who`.
    #[must_use]
    pub fn level_of(&self, who: W) -> Aware {
        self.of(who).map_or(Aware::Unaware, |n| n.level)
    }

    /// How full its gauge for `who` is, from 0 to 1.
    #[must_use]
    pub fn gauge_of(&self, who: W) -> f32 {
        self.of(who).map_or(0.0, |n| n.gauge)
    }

    /// What it has noticed of a place: a noise nobody made, a body, a place
    /// a script raised the alarm about.
    #[must_use]
    pub fn place(&self) -> Option<&Notice<W>> {
        self.place.as_ref()
    }

    /// Everyone and the place it has noticed: the people first, in no
    /// order that means anything.
    pub fn noticed(&self) -> impl Iterator<Item = &Notice<W>> {
        self.people.iter().flatten().chain(self.place.as_ref())
    }

    /// What it is most aware of: the highest level, then the fullest
    /// gauge; on a tie, a person before the place.
    #[must_use]
    pub fn highest(&self) -> Option<&Notice<W>> {
        most_aware(self.noticed())
    }

    /// The highest level it is at, about anyone or anything.
    #[must_use]
    pub fn level(&self) -> Aware {
        self.highest().map_or(Aware::Unaware, |n| n.level)
    }

    /// What it looks toward, head and chest: what it is most aware of, once
    /// that is curious or more.
    #[must_use]
    pub fn interest(&self) -> Option<&Notice<W>> {
        self.highest().filter(|n| n.level >= Aware::Curious)
    }

    /// What it turns its whole body toward, as it stands: what it is most
    /// aware of among those that may alarm it ([`Notice::may_alarm`]),
    /// once that is curious or more. A neutral it is curious about is
    /// only looked at.
    #[must_use]
    pub fn turns_to(&self) -> Option<&Notice<W>> {
        most_aware(self.noticed().filter(|n| n.may_alarm())).filter(|n| n.level >= Aware::Curious)
    }

    /// Forgets everything it noticed, telling nothing: for an NPC whose
    /// health is gone, or that was put somewhere new. The bodies it found
    /// it still knows of, so one found before does not alarm it again.
    pub fn forget(&mut self) {
        let (bodies, next_body) = (self.bodies, self.next_body);
        *self = Self::new(self.player);
        self.bodies = bodies;
        self.next_body = next_body;
    }

    /// One tick: fills each gauge by what `senses` says now, drains those
    /// nothing fed, and moves the levels, telling each change to `tell`.
    /// `feels` says how the NPC feels about someone, which caps their gauge
    /// (see the module's notes): the world asks its table of sides.
    pub fn step<N: Copy>(
        &mut self,
        senses: &Senses<W, N>,
        pace: &Pace<W>,
        feels: impl FnMut(W) -> Feeling,
        tell: impl FnMut(Change<W>),
    ) {
        self.take_in(senses.in_view(), senses.heard(), pace, feels, tell);
    }

    /// [`step`](Self::step) with those in view and the noises lately heard
    /// given one by one: what a step reads of the senses.
    pub fn take_in<'a, N: 'a>(
        &mut self,
        in_view: impl Iterator<Item = &'a Sighting<W>>,
        heard: impl Iterator<Item = &'a Heard<W, N>>,
        pace: &Pace<W>,
        mut feels: impl FnMut(W) -> Feeling,
        mut tell: impl FnMut(Change<W>),
    ) where
        W: 'a,
    {
        let tick = pace.tick;
        if pace.seconds > 0.0 {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a second is a few hundred ticks at most"
            )]
            let grace = (GRACE_SECONDS / pace.seconds).round().min(1e6) as u32;
            self.grace = grace;
        }
        let scale = |who: Option<W>| {
            let player = if who.is_some() && who == pace.player {
                pace.player_scale.max(0.0)
            } else {
                1.0
            };
            pace.alertness.max(0.0) * player
        };
        for seen in in_view.filter(|seen| seen.in_view) {
            let feeling = feels(seen.who);
            if seen.down_since.is_some() {
                if feeling != Feeling::Enemy {
                    self.found_body(seen.who, seen.at, tick);
                }
                continue;
            }
            self.up_again(seen.who);
            let strength = seen.strength.clamp(0.0, 1.0);
            let first = if seen.seen == seen.since {
                FIRST_LOOK
            } else {
                1.0
            };
            let fill =
                SIGHT_FILL * strength * strength * first * scale(Some(seen.who)) * pace.seconds;
            let tops = tops_for(feeling);
            self.feed(Some(seen.who), fill, 0.0, seen.at, Feed::Sight, tops, tick);
        }
        for heard in heard.filter(|h| h.tick == tick && !h.of_world) {
            let weapon = if heard.weapon { WEAPON_HEARD } else { 1.0 };
            let fill = HEARING_FILL * heard.strength * weapon * scale(heard.by);
            // A weapon's noise, heard at all, makes it curious at once, and
            // searching when it came through no wall or was loud.
            let least = if !heard.weapon {
                0.0
            } else if !heard.walled || heard.strength >= WEAPON_NEAR {
                SEARCHING_AT
            } else {
                CURIOUS_AT
            };
            let enemy = heard.by.filter(|&by| feels(by) == Feeling::Enemy);
            match (enemy, heard.by) {
                (Some(by), _) => self.feed(
                    Some(by),
                    fill,
                    least,
                    heard.at,
                    Feed::Hearing,
                    HEARING_TOPS,
                    tick,
                ),
                // The noises of those it is no enemy of go unremarked, but
                // not a weapon's: that is an interest in where it was.
                (None, Some(_)) if !heard.weapon => {}
                (None, _) => self.feed(
                    None,
                    fill,
                    least,
                    heard.at,
                    Feed::Hearing,
                    HEARING_TOPS,
                    tick,
                ),
            }
        }
        for gone in &mut self.dropped {
            if let Some(gone) = gone.take() {
                tell(gone);
            }
        }
        let quiet = |since: u32| tick.wrapping_sub(since) as f32 * pace.seconds > GRACE_SECONDS;
        for slot in self
            .people
            .iter_mut()
            .chain(std::iter::once(&mut self.place))
        {
            let Some(notice) = slot else { continue };
            if let Some(who) = notice.about {
                notice.tops = if notice.raised {
                    ALARMED_AT
                } else {
                    tops_for(feels(who))
                };
            }
            let held_by = if notice.gauge > HEARING_TOPS {
                notice.fed_high
            } else {
                notice.fed
            };
            if notice.gauge > notice.tops {
                // Fuller than it may be (an enemy forgiven): down to it.
                notice.gauge = (notice.gauge - QUICK_DRAIN * pace.seconds).max(notice.tops);
            } else if quiet(held_by) {
                let drain = if notice.may_alarm() {
                    DRAIN
                } else {
                    QUICK_DRAIN
                };
                notice.gauge = (notice.gauge - drain * pace.seconds).max(0.0);
            }
            let before = notice.level;
            let rising = Aware::rising_to(notice.gauge);
            if rising > before {
                notice.level = rising;
            } else if notice.gauge < before.falls_below() {
                notice.level = before.lower();
            }
            if notice.level != before {
                notice.since = tick;
                tell(Change {
                    about: notice.about,
                    at: notice.at,
                    now: notice.level,
                    before,
                });
            }
            if notice.level == Aware::Unaware {
                notice.raised = false;
                if notice.gauge <= 0.0 {
                    *slot = None;
                }
            }
        }
    }

    /// Fills the gauge of `about` (a place when `None`) by `fill`, to at
    /// least `least`, no further than `tops`, as sensed at `at` on `tick`.
    #[allow(clippy::too_many_arguments, reason = "one private helper")]
    fn feed(
        &mut self,
        about: Option<W>,
        fill: f32,
        least: f32,
        at: [f32; 3],
        how: Feed,
        tops: f32,
        tick: u32,
    ) {
        let fill = if fill.is_finite() { fill.max(0.0) } else { 0.0 };
        let may_alarm = about.is_none() || tops > CURIOUS_TOPS;
        let Some(notice) = self.slot_for(about, fill.max(least), may_alarm, at, tick) else {
            return;
        };
        // A body found is not forgotten for a noise elsewhere.
        if about.is_none() && notice.body.is_some() && notice.gauge > HEARING_TOPS {
            return;
        }
        let tops = if notice.raised || about.is_none() {
            match how {
                Feed::Sight => ALARMED_AT,
                Feed::Hearing => HEARING_TOPS,
            }
        } else {
            tops
        };
        if notice.gauge < tops {
            notice.gauge = (notice.gauge + fill).max(least).min(tops);
        }
        notice.at = at;
        notice.fed = tick;
        match how {
            Feed::Sight => {
                notice.seen = Some(tick);
                if tops > HEARING_TOPS {
                    notice.fed_high = tick;
                }
            }
            Feed::Hearing => {
                notice.heard = Some(tick);
                if about.is_none() {
                    notice.body = None;
                }
            }
        }
    }

    /// The gauge of `about`, made if there is none: in an empty place, or
    /// in place of one who gives way to it (see [`MOST_NOTICED`]). `None`
    /// when there is no room. `may_alarm` says whether `about` may.
    fn slot_for(
        &mut self,
        about: Option<W>,
        fill: f32,
        may_alarm: bool,
        at: [f32; 3],
        tick: u32,
    ) -> Option<&mut Notice<W>> {
        let fresh = Notice {
            about,
            gauge: 0.0,
            level: Aware::Unaware,
            since: tick,
            at,
            seen: None,
            heard: None,
            body: None,
            tops: if may_alarm { ALARMED_AT } else { CURIOUS_TOPS },
            fed: tick,
            fed_high: tick,
            raised: false,
        };
        let Some(who) = about else {
            return Some(self.place.get_or_insert(fresh));
        };
        if let Some(at) = self
            .people
            .iter()
            .position(|n| n.is_some_and(|n| n.about == about))
        {
            return self.people[at].as_mut();
        }
        let room = match self.people.iter().position(Option::is_none) {
            Some(empty) => Some(empty),
            None => self.gives_way(Some(who) == self.player, may_alarm, fill, tick),
        };
        let slot = room?;
        if let Some(gone) = self.people[slot]
            && gone.level != Aware::Unaware
            && let Some(free) = self.dropped.iter_mut().find(|d| d.is_none())
        {
            *free = Some(Change {
                about: gone.about,
                at: gone.at,
                now: Aware::Unaware,
                before: gone.level,
            });
        }
        self.people[slot] = Some(fresh);
        self.people[slot].as_mut()
    }

    /// The place of the one who gives way to a newcomer (the player when
    /// `player`; one who may alarm the NPC when `may_alarm`) filling its
    /// gauge by `fill` on `tick`, if anyone does: see [`MOST_NOTICED`].
    /// The player never gives way.
    fn gives_way(&self, player: bool, may_alarm: bool, fill: f32, tick: u32) -> Option<usize> {
        let unsensed = |n: &Notice<W>| n.sensed().map_or(u32::MAX, |t| tick.wrapping_sub(t));
        let (slot, n) = (0..MOST_NOTICED)
            .filter_map(|slot| self.people[slot].map(|n| (slot, n)))
            .filter(|(_, n)| self.player.is_none() || n.about != self.player)
            .min_by(|(_, a), (_, b)| {
                (a.may_alarm().cmp(&b.may_alarm()))
                    .then(a.level.cmp(&b.level))
                    .then(a.gauge.total_cmp(&b.gauge))
                    .then(unsensed(b).cmp(&unsensed(a)))
            })?;
        let lately = |n: &Notice<W>| unsensed(n) <= self.grace;
        let gives = if player {
            true
        } else if n.may_alarm() {
            // An enemy gives way to an enemy only while it is not alarming
            // the NPC and was not sensed a moment ago.
            may_alarm && n.level < Aware::Alarmed && !lately(&n)
        } else {
            may_alarm || (n.level == Aware::Unaware && n.gauge < fill)
        };
        gives.then_some(slot)
    }

    /// A body found lying there: an alarm about the place, unless it is
    /// one already found.
    fn found_body(&mut self, who: W, at: [f32; 3], tick: u32) {
        if self.bodies.iter().flatten().any(|&k| k == who) {
            return;
        }
        self.bodies[self.next_body % MOST_BODIES] = Some(who);
        self.next_body = (self.next_body + 1) % MOST_BODIES;
        self.raise(None, at, tick);
        if let Some(place) = &mut self.place {
            place.body = Some(who);
            place.seen = Some(tick);
        }
    }

    /// One seen on their feet: should they lie there again, that is a body
    /// found anew.
    fn up_again(&mut self, who: W) {
        for body in &mut self.bodies {
            if *body == Some(who) {
                *body = None;
            }
        }
    }

    /// Raises the alarm about `about` (a place when `None`) at `at`, as a
    /// script's `npc:alert` does or a body found: the gauge is full, and
    /// the level rises to alarmed at the next step. Whoever they are, they
    /// may alarm it until it is calm again.
    pub fn raise(&mut self, about: Option<W>, at: [f32; 3], tick: u32) {
        let Some(notice) = self.slot_for(about, f32::INFINITY, true, at, tick) else {
            return;
        };
        notice.gauge = 1.0;
        notice.tops = ALARMED_AT;
        notice.raised = true;
        notice.at = at;
        notice.fed = tick;
        notice.fed_high = tick;
        notice.body = None;
    }

    /// Settles it about `about` (a place when `None`) at once: the gauge is
    /// empty and the level unaware, told to `tell` if it was not.
    pub fn calm(&mut self, about: Option<W>, tick: u32, tell: impl FnMut(Change<W>)) {
        let slot = match about {
            None => Some(&mut self.place),
            Some(_) => self
                .people
                .iter_mut()
                .find(|n| n.is_some_and(|n| n.about == about)),
        };
        if let Some(slot) = slot {
            settle(slot, tick, tell);
        }
    }

    /// Settles it about everyone and everything at once, telling each that
    /// was not unaware to `tell`, people first.
    pub fn calm_all(&mut self, tick: u32, mut tell: impl FnMut(Change<W>)) {
        for slot in self
            .people
            .iter_mut()
            .chain(std::iter::once(&mut self.place))
        {
            settle(slot, tick, &mut tell);
        }
    }
}

/// The one of `noticed` it is most aware of: the highest level, then the
/// fullest gauge; on a tie, the first.
fn most_aware<'a, W: 'a>(noticed: impl Iterator<Item = &'a Notice<W>>) -> Option<&'a Notice<W>> {
    noticed.fold(None, |best: Option<&Notice<W>>, one| match best {
        Some(best) if (best.level, best.gauge) >= (one.level, one.gauge) => Some(best),
        _ => Some(one),
    })
}

/// Empties `slot`, telling `tell` if it was more than unaware.
fn settle<W: Copy>(slot: &mut Option<Notice<W>>, _tick: u32, mut tell: impl FnMut(Change<W>)) {
    if let Some(notice) = slot.take()
        && notice.level != Aware::Unaware
    {
        tell(Change {
            about: notice.about,
            at: notice.at,
            now: Aware::Unaware,
            before: notice.level,
        });
    }
}
/// The way to face, `facing` turned toward `goal` by at most `most`
/// radians, the short way round: both in radians, turning left from -Z.
#[must_use]
pub fn turn_toward(facing: f32, goal: f32, most: f32) -> f32 {
    if !(facing.is_finite() && goal.is_finite()) {
        return if facing.is_finite() { facing } else { 0.0 };
    }
    let tau = std::f32::consts::TAU;
    let mut left = (goal - facing) % tau;
    if left > std::f32::consts::PI {
        left -= tau;
    } else if left < -std::f32::consts::PI {
        left += tau;
    }
    let most = most.max(0.0);
    let turned = facing + left.clamp(-most, most);
    // Kept within one turn either way, so it never grows without end.
    if turned > std::f32::consts::PI {
        turned - tau
    } else if turned < -std::f32::consts::PI {
        turned + tau
    } else {
        turned
    }
}

/// The way to face to look from `from` to `to` across the ground, in
/// radians turning left from -Z; `None` when they are on top of each other.
#[must_use]
pub fn facing_toward(from: [f32; 3], to: [f32; 3]) -> Option<f32> {
    let (x, z) = (to[0] - from[0], to[2] - from[2]);
    (x * x + z * z > 1e-6).then(|| libm::atan2f(-x, -z))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: f32 = 1.0 / 60.0;
    const PLAYER: u32 = 0;
    /// Who is under 100 is an enemy, under 200 a neutral, else a friend.
    const NEUTRAL: u32 = 150;
    const FRIEND: u32 = 200;

    fn feels(who: u32) -> Feeling {
        match who {
            0..100 => Feeling::Enemy,
            100..200 => Feeling::Neutral,
            _ => Feeling::Friend,
        }
    }

    fn pace(tick: u32) -> Pace<u32> {
        Pace {
            tick,
            seconds: TICK,
            alertness: 1.0,
            player: Some(PLAYER),
            player_scale: 1.0,
        }
    }

    /// `who` in view since tick 0, looked at every twelve ticks.
    fn seen(who: u32, strength: f32, tick: u32) -> Sighting<u32> {
        seen_from(who, strength, tick, 0)
    }

    /// `who` in view since `from`, looked at every twelve ticks from then.
    fn seen_from(who: u32, strength: f32, tick: u32, from: u32) -> Sighting<u32> {
        Sighting {
            who,
            strength,
            in_view: true,
            down_since: None,
            at: [1.0, 0.0, 2.0],
            seen: tick - (tick - from) % 12,
            since: from,
        }
    }

    fn heard(by: Option<u32>, strength: f32, tick: u32, weapon: bool) -> Heard<u32> {
        Heard {
            at: [5.0, 0.0, 5.0],
            strength,
            loudness: 1.0,
            tick,
            by,
            walled: false,
            what: (),
            of_world: false,
            weapon,
        }
    }

    /// Steps `aware` over `ticks` from `from`, with `sights` in view and
    /// `noises` heard on each, every change kept with its tick.
    fn run(
        aware: &mut Awareness<u32>,
        from: u32,
        ticks: u32,
        mut sights: impl FnMut(u32) -> Vec<Sighting<u32>>,
        mut noises: impl FnMut(u32) -> Vec<Heard<u32>>,
        changes: &mut Vec<(u32, Change<u32>)>,
    ) {
        for tick in from..from + ticks {
            let (s, n) = (sights(tick), noises(tick));
            aware.take_in(s.iter(), n.iter(), &pace(tick), feels, |c| {
                changes.push((tick, c));
            });
        }
    }

    fn first(changes: &[(u32, Change<u32>)], now: Aware) -> Option<u32> {
        changes.iter().find(|(_, c)| c.now == now).map(|(t, _)| *t)
    }

    /// The tick a gauge seen at `strength` from tick 0 reaches `gauge`:
    /// the first look's twelve ticks count half.
    fn expected(gauge: f32, strength: f32) -> f32 {
        let rate = SIGHT_FILL * strength * strength;
        (0.1 + gauge / rate) * 60.0
    }

    fn near(tick: u32, want: f32) -> bool {
        (tick as f32 - want).abs() <= 2.0
    }

    #[test]
    fn sight_fills_with_the_square_of_how_strongly_one_is_seen() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            600,
            |t| vec![seen(PLAYER, 0.3, t)],
            |_| vec![],
            &mut changes,
        );
        let curious = first(&changes, Aware::Curious).unwrap();
        let alarmed = first(&changes, Aware::Alarmed).unwrap();
        assert!(near(curious, expected(CURIOUS_AT, 0.3)), "{curious}");
        assert!(near(alarmed, expected(1.0, 0.3)), "{alarmed}");
        // Plain view, close: noticed at once, curious in a quarter of a
        // second and alarmed in 0.7.
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            60,
            |t| vec![seen(PLAYER, 1.0, t)],
            |_| vec![],
            &mut changes,
        );
        assert_eq!(changes.len(), 3, "{changes:?}");
        assert!(first(&changes, Aware::Curious).unwrap() <= 16);
        assert!(near(first(&changes, Aware::Alarmed).unwrap(), 41.3));
    }

    #[test]
    fn the_worked_cases_a_sprint_far_off_is_searched_for_and_one_standing_near_alarms() {
        // Sprinting across the middle of a guard's view 20 metres off in
        // day for a second and a half: seen at 0.6.
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            90,
            |t| vec![seen(PLAYER, 0.601, t)],
            |_| vec![],
            &mut changes,
        );
        assert_eq!(aware.level_of(PLAYER), Aware::Searching, "{changes:?}");
        // Standing still in plain view 5 metres off: seen at 0.75, alarmed
        // in about a second.
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            120,
            |t| vec![seen(PLAYER, 0.747, t)],
            |_| vec![],
            &mut changes,
        );
        let alarmed = first(&changes, Aware::Alarmed).unwrap();
        assert!((60..=75).contains(&alarmed), "{alarmed}");
        // Seen clearly at one look and gone by the next: the gap counts
        // half, so not even curious.
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            12,
            |t| vec![seen(PLAYER, 1.0, t)],
            |_| vec![],
            &mut changes,
        );
        run(&mut aware, 12, 60, |_| vec![], |_| vec![], &mut changes);
        assert!(changes.is_empty(), "{changes:?}");
        assert!((aware.gauge_of(PLAYER) - SIGHT_FILL * 0.1).abs() < 1e-4);
    }

    #[test]
    fn once_out_of_sight_it_searches_before_it_is_calm_and_says_each_step() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            60,
            |t| vec![seen(PLAYER, 1.0, t)],
            |_| vec![],
            &mut changes,
        );
        assert_eq!(aware.level_of(PLAYER), Aware::Alarmed);
        let lost = 60;
        changes.clear();
        run(
            &mut aware,
            lost,
            30 * 60,
            |_| vec![],
            |_| vec![],
            &mut changes,
        );
        let steps: Vec<(Aware, Aware)> = changes.iter().map(|(_, c)| (c.before, c.now)).collect();
        assert_eq!(
            steps,
            [
                (Aware::Alarmed, Aware::Searching),
                (Aware::Searching, Aware::Curious),
                (Aware::Curious, Aware::Unaware)
            ]
        );
        let secs = |t: u32| (t - lost) as f32 * TICK;
        for (change, want) in changes.iter().zip([4.75, 16.0, 23.5]) {
            assert!((secs(change.0) - want).abs() < 0.1, "{}", secs(change.0));
        }
        assert!(aware.of(PLAYER).is_none(), "forgotten once calm and empty");
    }

    #[test]
    fn the_level_does_not_flicker_at_a_boundary() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        // Glimpsed on and off.
        run(
            &mut aware,
            0,
            1200,
            |t| {
                if t % 120 < 40 {
                    vec![seen_from(PLAYER, 0.6, t, t - t % 120)]
                } else {
                    vec![]
                }
            },
            |_| vec![],
            &mut changes,
        );
        assert!(first(&changes, Aware::Curious).is_some());
        let flips = changes.iter().filter(|(_, c)| c.now < c.before).count();
        assert_eq!(flips, 0, "{changes:?}");
    }

    #[test]
    fn hearing_alone_raises_it_to_searching_and_no_further() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            600,
            |_| vec![],
            |t| {
                if t % 10 == 0 {
                    vec![heard(Some(PLAYER), 1.0, t, true)]
                } else {
                    vec![]
                }
            },
            &mut changes,
        );
        assert_eq!(aware.level_of(PLAYER), Aware::Searching);
        assert!(aware.gauge_of(PLAYER) <= HEARING_TOPS);
        assert!(first(&changes, Aware::Alarmed).is_none());
        let notice = aware.of(PLAYER).unwrap();
        assert_eq!(
            notice.at,
            [5.0, 0.0, 5.0],
            "searching goes where it was heard"
        );
        assert!(notice.seen.is_none() && notice.heard == Some(590));
        // A footstep a few metres off only adds a little.
        let mut aware = Awareness::new(Some(PLAYER));
        run(
            &mut aware,
            0,
            1,
            |_| vec![],
            |t| vec![heard(Some(PLAYER), 0.2, t, false)],
            &mut changes,
        );
        assert_eq!(aware.level_of(PLAYER), Aware::Unaware);
        assert!((aware.gauge_of(PLAYER) - 0.12).abs() < 1e-5);
    }

    #[test]
    fn a_weapon_heard_at_all_makes_it_curious_and_in_the_open_or_near_searching() {
        let once = |strength: f32, walled: bool| {
            let mut aware = Awareness::new(Some(PLAYER));
            let mut shot = heard(Some(PLAYER), strength, 0, true);
            shot.walled = walled;
            aware.take_in(std::iter::empty(), [shot].iter(), &pace(0), feels, |_| {});
            aware.level_of(PLAYER)
        };
        // Thirty metres off behind a wall, faint: curious at once.
        assert_eq!(once(0.046, true), Aware::Curious);
        // Thirty metres off in the open: searching.
        assert_eq!(once(0.133, false), Aware::Searching);
        // Behind a wall but near: searching.
        assert_eq!(once(WEAPON_NEAR, true), Aware::Searching);
        // And never alarmed without sight.
        assert_eq!(once(1.0, false), Aware::Searching);
    }

    #[test]
    fn a_noise_nobody_made_is_an_interest_in_a_place() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            1,
            |_| vec![],
            |t| vec![heard(None, 0.5, t, true)],
            &mut changes,
        );
        let place = aware.place().unwrap();
        assert!(place.is_place() && place.may_alarm());
        assert_eq!(place.level, Aware::Searching);
        assert_eq!(changes[0].1.about, None);
        assert_eq!(aware.interest().unwrap().at, [5.0, 0.0, 5.0]);
        assert_eq!(aware.turns_to().unwrap().at, [5.0, 0.0, 5.0]);
    }

    #[test]
    fn a_friend_is_noticed_and_its_noises_unremarked_but_not_its_shots() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            600,
            |t| vec![seen(FRIEND, 1.0, t)],
            |t| vec![heard(Some(FRIEND), 1.0, t, false)],
            &mut changes,
        );
        assert!(changes.is_empty(), "{changes:?}");
        assert_eq!(aware.level_of(FRIEND), Aware::Unaware);
        assert!((aware.gauge_of(FRIEND) - CALM_TOPS).abs() < 1e-6);
        assert!(aware.interest().is_none() && !aware.of(FRIEND).unwrap().may_alarm());
        // A shot it fires is an interest in where it was fired, not in it.
        run(
            &mut aware,
            600,
            1,
            |t| vec![seen(FRIEND, 1.0, t)],
            |t| vec![heard(Some(FRIEND), 0.5, t, true)],
            &mut changes,
        );
        assert_eq!(aware.place().unwrap().level, Aware::Searching);
        assert_eq!(aware.level_of(FRIEND), Aware::Unaware);
    }

    #[test]
    fn a_neutral_is_at_most_a_curiosity_looked_at_and_soon_forgotten() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            600,
            |t| vec![seen(NEUTRAL, 1.0, t)],
            |t| vec![heard(Some(NEUTRAL), 1.0, t, false)],
            &mut changes,
        );
        assert_eq!(aware.level_of(NEUTRAL), Aware::Curious);
        assert!((aware.gauge_of(NEUTRAL) - CURIOUS_TOPS).abs() < 1e-6);
        assert_eq!(changes.len(), 1, "{changes:?}");
        // Looked at, head and chest; the body does not turn to it.
        assert_eq!(aware.interest().unwrap().about, Some(NEUTRAL));
        assert!(aware.turns_to().is_none());
        // Gone from view: calm three seconds later.
        changes.clear();
        run(&mut aware, 600, 300, |_| vec![], |_| vec![], &mut changes);
        assert_eq!(changes.len(), 1, "{changes:?}");
        let calm = (changes[0].0 - 600) as f32 * TICK;
        assert!((calm - 3.0).abs() < 0.1, "{calm}");
    }

    #[test]
    fn a_neutral_who_becomes_an_enemy_is_noticed_on_from_where_it_was() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            300,
            |t| vec![seen(NEUTRAL, 1.0, t)],
            |_| vec![],
            &mut changes,
        );
        let mut grudge = |who: u32| {
            if who == NEUTRAL {
                Feeling::Enemy
            } else {
                feels(who)
            }
        };
        let s = [seen(NEUTRAL, 1.0, 300)];
        aware.take_in(
            s.iter(),
            std::iter::empty::<&Heard<u32>>(),
            &pace(300),
            &mut grudge,
            |_| {},
        );
        let after = aware.gauge_of(NEUTRAL);
        assert!(
            after > CURIOUS_TOPS && after < CURIOUS_TOPS + 0.05,
            "{after}"
        );
        assert!(aware.of(NEUTRAL).unwrap().may_alarm());
        let mut level = Aware::Unaware;
        for tick in 301..340 {
            let s = [seen(NEUTRAL, 1.0, tick)];
            aware.take_in(
                s.iter(),
                std::iter::empty::<&Heard<u32>>(),
                &pace(tick),
                &mut grudge,
                |_| {},
            );
            level = aware.level_of(NEUTRAL);
        }
        assert_eq!(level, Aware::Alarmed);
    }

    #[test]
    fn a_body_found_is_an_alarm_about_its_place_once() {
        let mut body = seen(FRIEND, 0.5, 0);
        body.down_since = Some(0);
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(&mut aware, 1, 1, |_| vec![body], |_| vec![], &mut changes);
        let place = aware.place().unwrap();
        assert_eq!((place.level, place.body), (Aware::Alarmed, Some(FRIEND)));
        assert_eq!(aware.level_of(FRIEND), Aware::Unaware);
        // Calmed by a script, the same body still in view does not alarm it
        // again.
        aware.calm_all(2, |c| changes.push((2, c)));
        run(&mut aware, 3, 60, |_| vec![body], |_| vec![], &mut changes);
        assert!(aware.place().is_none());
        // Passed again on a later round, the senses having forgotten it
        // (it is found anew, on a new tick): still not again.
        let mut again = body;
        again.down_since = Some(5000);
        let told = changes.len();
        run(
            &mut aware,
            5000,
            30,
            |_| vec![again],
            |_| vec![],
            &mut changes,
        );
        assert_eq!(changes.len(), told, "{changes:?}");
        // Seen on their feet, and down again: that is a new body.
        let mut up = body;
        up.down_since = None;
        run(&mut aware, 5030, 1, |_| vec![up], |_| vec![], &mut changes);
        run(
            &mut aware,
            5031,
            1,
            |_| vec![again],
            |_| vec![],
            &mut changes,
        );
        assert_eq!(aware.place().unwrap().level, Aware::Alarmed);
        // An enemy's body alarms nobody.
        let mut enemy = seen(7, 0.5, 0);
        enemy.down_since = Some(0);
        let mut aware = Awareness::new(Some(PLAYER));
        run(&mut aware, 1, 1, |_| vec![enemy], |_| vec![], &mut changes);
        assert!(aware.place().is_none());
    }

    #[test]
    fn the_difficulty_dial_scales_how_fast_the_player_is_noticed_and_nothing_else() {
        let ticks_to_alarm = |who: u32, scale: f32| {
            let mut aware = Awareness::new(Some(PLAYER));
            for tick in 0..10_000 {
                let s = [seen(who, 0.3, tick)];
                let mut pace = pace(tick);
                pace.player_scale = scale;
                aware.take_in(
                    s.iter(),
                    std::iter::empty::<&Heard<u32>>(),
                    &pace,
                    |_| Feeling::Enemy,
                    |_| {},
                );
                if aware.level_of(who) == Aware::Alarmed {
                    return tick;
                }
            }
            u32::MAX
        };
        // 0.1 + 1 / (1.7 * 0.09 * scale) seconds.
        for scale in [0.5, 1.0, 1.5] {
            let want = (0.1 + 1.0 / (SIGHT_FILL * 0.09 * scale)) * 60.0;
            let got = ticks_to_alarm(PLAYER, scale);
            assert!((got as f32 - want).abs() <= 3.0, "{scale} {got} {want}");
        }
        assert_eq!(ticks_to_alarm(5, 0.5), ticks_to_alarm(5, 1.5));
    }

    #[test]
    fn alertness_scales_every_fill() {
        let mut aware = Awareness::new(Some(PLAYER));
        let s = [seen(5, 1.0, 12)];
        let mut sleepy = pace(12);
        sleepy.alertness = 0.5;
        aware.take_in(
            s.iter(),
            std::iter::empty::<&Heard<u32>>(),
            &sleepy,
            feels,
            |_| {},
        );
        assert!((aware.gauge_of(5) - 0.5 * SIGHT_FILL * TICK).abs() < 1e-6);
    }

    #[test]
    fn the_player_always_has_a_place_when_more_are_noticed_than_there_is_room_for() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        // Ten enemies fill every place, then the player is heard.
        run(
            &mut aware,
            0,
            20,
            |t| (1..=10).map(|who| seen(who, 0.5, t)).collect(),
            |_| vec![],
            &mut changes,
        );
        assert_eq!(aware.noticed().count(), MOST_NOTICED);
        assert!(
            aware.of(9).is_none() && aware.of(10).is_none(),
            "the first come keep their place"
        );
        run(
            &mut aware,
            20,
            1,
            |_| vec![],
            |t| vec![heard(Some(PLAYER), 0.1, t, false)],
            &mut changes,
        );
        assert!(aware.of(PLAYER).is_some());
        assert_eq!(aware.noticed().count(), MOST_NOTICED);
        // No one takes it again while it is not forgotten.
        run(
            &mut aware,
            21,
            50,
            |t| (11..=20).map(|who| seen(who, 1.0, t)).collect(),
            |_| vec![],
            &mut changes,
        );
        assert!(aware.of(PLAYER).is_some());
    }

    /// A guard among `crowd` townsfolk (neutrals and friends) it has looked
    /// at a while; then `enemy` walks into view. It is alarmed by them in
    /// as long as alone, and no townsperson is told twice.
    fn among_townsfolk(crowd: u32, enemy: u32) {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        let town = |t: u32| -> Vec<Sighting<u32>> {
            (0..crowd)
                .map(|i| {
                    let who = if i % 2 == 0 { 100 + i } else { 200 + i };
                    seen(who, 0.9, t)
                })
                .collect()
        };
        run(&mut aware, 0, 600, town, |_| vec![], &mut changes);
        assert_eq!(aware.noticed().count(), MOST_NOTICED.min(crowd as usize));
        assert!(aware.noticed().all(|n| n.level <= Aware::Curious));
        let told = changes.len();
        run(
            &mut aware,
            600,
            600,
            |t| {
                let mut all = town(t);
                all.push(seen_from(enemy, 0.9, t, 600));
                all
            },
            |_| vec![],
            &mut changes,
        );
        let alarmed = changes
            .iter()
            .find(|(_, c)| c.about == Some(enemy) && c.now == Aware::Alarmed)
            .map(|(t, _)| t - 600);
        let want = expected(1.0, 0.9);
        assert!(
            alarmed.is_some_and(|t| (t as f32 - want).abs() <= 2.0),
            "{alarmed:?} {changes:?}"
        );
        // A townsperson let go is told so once, and is not taken back.
        let town_told: Vec<_> = changes[told..]
            .iter()
            .filter(|(_, c)| c.about != Some(enemy))
            .collect();
        assert!(town_told.len() <= 1, "{town_told:?}");
        assert!(town_told.iter().all(|(_, c)| c.now == Aware::Unaware));
    }

    #[test]
    fn an_enemy_walking_into_a_crowd_of_townsfolk_always_has_a_place() {
        among_townsfolk(8, 50);
        among_townsfolk(12, 50);
        among_townsfolk(12, PLAYER);
    }

    #[test]
    fn a_script_raises_the_alarm_and_settles_it() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        aware.raise(Some(PLAYER), [3.0, 0.0, 3.0], 5);
        run(&mut aware, 5, 1, |_| vec![], |_| vec![], &mut changes);
        assert_eq!(changes[0].1.now, Aware::Alarmed);
        assert_eq!(aware.highest().unwrap().about, Some(PLAYER));
        assert_eq!(aware.of(PLAYER).unwrap().since, 5);
        aware.raise(None, [9.0, 0.0, 9.0], 6);
        aware.calm(Some(PLAYER), 6, |c| changes.push((6, c)));
        assert_eq!(changes[1].1.now, Aware::Unaware);
        assert_eq!(aware.level_of(PLAYER), Aware::Unaware);
        run(&mut aware, 6, 1, |_| vec![], |_| vec![], &mut changes);
        assert_eq!(aware.level(), Aware::Alarmed);
        aware.calm_all(7, |c| changes.push((7, c)));
        assert_eq!(aware.level(), Aware::Unaware);
        assert_eq!(changes.len(), 4);
        // About a neutral too: alarmed while they are in view, and searching
        // once they have been gone as long as an enemy would be.
        aware.raise(Some(NEUTRAL), [3.0, 0.0, 3.0], 10);
        run(
            &mut aware,
            10,
            120,
            |t| vec![seen(NEUTRAL, 0.5, t)],
            |_| vec![],
            &mut changes,
        );
        assert_eq!(aware.level_of(NEUTRAL), Aware::Alarmed);
        assert!(aware.turns_to().is_some());
        changes.clear();
        run(&mut aware, 130, 2000, |_| vec![], |_| vec![], &mut changes);
        let fell = first(&changes, Aware::Searching).unwrap() - 130;
        assert!((fell as f32 * TICK - 4.75).abs() < 0.1, "{fell}");
        assert_eq!(aware.level_of(NEUTRAL), Aware::Unaware);
    }

    #[test]
    fn calmed_while_an_enemy_is_in_view_it_is_alarmed_again_within_a_second() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            60,
            |t| vec![seen(PLAYER, 1.0, t)],
            |_| vec![],
            &mut changes,
        );
        aware.calm_all(60, |c| changes.push((60, c)));
        changes.clear();
        run(
            &mut aware,
            60,
            60,
            |t| vec![seen(PLAYER, 1.0, t)],
            |_| vec![],
            &mut changes,
        );
        let again = first(&changes, Aware::Alarmed).unwrap() - 60;
        assert!(again <= 40, "{again}");
    }

    #[test]
    fn a_turn_goes_the_short_way_and_no_faster_than_allowed() {
        use std::f32::consts::PI;
        assert!((turn_toward(0.0, 1.0, 0.1) - 0.1).abs() < 1e-6);
        assert!((turn_toward(0.0, 0.05, 0.1) - 0.05).abs() < 1e-6);
        // From just left of behind to just right of it: across the back.
        let turned = turn_toward(PI - 0.1, -PI + 0.1, 0.05);
        assert!((turned - (PI - 0.05)).abs() < 1e-5, "{turned}");
        let turned = turn_toward(PI - 0.02, -PI + 0.1, 0.05);
        assert!((turned - (-PI + 0.03)).abs() < 1e-5, "{turned}");
        assert_eq!(turn_toward(f32::NAN, 1.0, 0.1), 0.0);
        // -Z is 0; +X is a quarter turn right.
        assert!(facing_toward([0.0; 3], [0.0, 0.0, -5.0]).unwrap().abs() < 1e-6);
        assert!((facing_toward([0.0; 3], [5.0, 0.0, 0.0]).unwrap() + PI / 2.0).abs() < 1e-6);
        assert!(facing_toward([0.0; 3], [0.0, 3.0, 0.0]).is_none());
    }
}
