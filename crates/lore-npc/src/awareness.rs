//! How much an NPC has noticed: bit by bit, from "what was that?" to
//! "there he is", and back to calm.
//!
//! Each NPC keeps a gauge from 0 to 1 for each one it has noticed, and one
//! more for a place (a noise nobody made, a body found). Its senses
//! ([`Senses`]) fill it: sight by how strongly someone is seen, for as long
//! as they are in view, so a glimpse at the edge of the cone in the dark
//! fills it slowly and someone in plain view close by at once; a noise by
//! how loud it reached the ears, the noise of a weapon more. Time drains
//! it, slowly, once nothing has fed it for a moment ([`GRACE_SECONDS`]).
//!
//! From the gauge come four levels ([`Aware`]): unaware, curious (something
//! is there: it looks that way), searching (someone was there: the place
//! they were last sensed is kept) and alarmed (it sees them). A level rises
//! as soon as the gauge reaches it and falls one step at a time, each at a
//! lower mark than it rose at, so it does not flicker at a boundary: after
//! being alarmed it searches before it is calm. Hearing alone never fills
//! the gauge past [`HEARING_TOPS`], which is searching and no further.
//!
//! Whether one may alarm at all is not the gauge's question: the world says
//! it of each by `wary` (see [`Awareness::step`]). One it is not wary of (a
//! friend, a neutral passer-by) is noticed, never more ([`CALM_TOPS`]), and
//! the noises they make go unremarked; the body of one it is not wary of,
//! found lying there, is an alarm about the place it lies.
//!
//! The numbers, at the middle of the difficulty dial and an alertness of 1:
//! someone seen at a strength of 1 alarms in 0.2 seconds, of 0.75 (standing
//! still, close, in plain light) in 0.36, of 0.3 (walking, near the end of
//! sight) in 2.2, of 0.1 (the faintest in view) in 20; after the last sight
//! of them it is alarmed 4.75 seconds more, searching till 16, curious till
//! 23.5, and then calm.
//!
//! Everything is plain data in storage of a fixed size and pure functions
//! of it, stepped once a tick: the same senses give the same levels on the
//! same ticks.

use crate::senses::{Heard, Senses, Sighting};

/// The most people one NPC keeps a gauge for. The player always has a
/// place; when they are full, a newcomer takes the place of the one noticed
/// least, if it is noticed less than the newcomer would be.
pub const MOST_NOTICED: usize = 8;

/// How many bodies an NPC remembers having found, so the same body in view
/// does not alarm it again and again.
pub const MOST_BODIES: usize = 4;

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
/// The fullest the gauge of one the NPC is not wary of goes: noticed, not
/// even curious.
pub const CALM_TOPS: f32 = 0.2;

/// How much a second of being seen at a strength of 1 fills the gauge;
/// the fill goes with the square of the strength, so a faint glimpse fills
/// it far more slowly than a clear look.
pub const SIGHT_FILL: f32 = 5.0;
/// How much one noise heard at a strength of 1 fills the gauge at once.
pub const HEARING_FILL: f32 = 0.6;
/// How many times more a weapon's noise (a shot, a blast) fills it.
pub const WEAPON_HEARD: f32 = 4.0;
/// How long the gauge holds after it was last fed, in seconds, before it
/// drains.
pub const GRACE_SECONDS: f32 = 1.0;
/// How much the gauge drains each second after that.
pub const DRAIN: f32 = 0.04;

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
    /// The tick anything last fed the gauge.
    fed: u32,
    /// The tick anything that may fill it past [`HEARING_TOPS`] last fed
    /// it: sight, a body, a script.
    fed_high: u32,
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
    /// By sight, up to `tops`.
    Sight(f32),
    /// By hearing, up to `tops`.
    Hearing(f32),
}

/// Everything one NPC has noticed, in storage of a fixed size.
#[derive(Debug, Clone, PartialEq)]
pub struct Awareness<W> {
    people: [Option<Notice<W>>; MOST_NOTICED],
    place: Option<Notice<W>>,
    /// The bodies it found, by who and the tick they went down, so the same
    /// one does not alarm it twice.
    bodies: [Option<(W, u32)>; MOST_BODIES],
    /// Where the next body found is remembered.
    next_body: usize,
    /// The player, who always has a place and is never the one let go to
    /// make room.
    player: Option<W>,
    /// One let go to make room for the player while it was more than
    /// unaware: told as calm at the next step.
    dropped: Option<Change<W>>,
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
            dropped: None,
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
        self.noticed()
            .fold(None, |best: Option<&Notice<W>>, one| match best {
                Some(best) if (best.level, best.gauge) >= (one.level, one.gauge) => Some(best),
                _ => Some(one),
            })
    }

    /// The highest level it is at, about anyone or anything.
    #[must_use]
    pub fn level(&self) -> Aware {
        self.highest().map_or(Aware::Unaware, |n| n.level)
    }

    /// What it turns to look toward: what it is most aware of, once that is
    /// curious or more.
    #[must_use]
    pub fn interest(&self) -> Option<&Notice<W>> {
        self.highest().filter(|n| n.level >= Aware::Curious)
    }

    /// Forgets everything, telling nothing: for an NPC whose health is
    /// gone, or that was put somewhere new.
    pub fn forget(&mut self) {
        *self = Self::new(self.player);
    }

    /// One tick: fills each gauge by what `senses` says now, drains those
    /// nothing fed, and moves the levels, telling each change to `tell`.
    /// `wary` says whether one may alarm the NPC (see the module's notes):
    /// the world asks its table of sides.
    pub fn step<N: Copy>(
        &mut self,
        senses: &Senses<W, N>,
        pace: &Pace<W>,
        wary: impl FnMut(W) -> bool,
        tell: impl FnMut(Change<W>),
    ) {
        self.take_in(senses.in_view(), senses.heard(), pace, wary, tell);
    }

    /// [`step`](Self::step) with those in view and the noises lately heard
    /// given one by one: what a step reads of the senses.
    pub fn take_in<'a, N: 'a>(
        &mut self,
        in_view: impl Iterator<Item = &'a Sighting<W>>,
        heard: impl Iterator<Item = &'a Heard<W, N>>,
        pace: &Pace<W>,
        mut wary: impl FnMut(W) -> bool,
        mut tell: impl FnMut(Change<W>),
    ) where
        W: 'a,
    {
        let tick = pace.tick;
        let scale = |who: Option<W>| {
            let player = if who.is_some() && who == pace.player {
                pace.player_scale.max(0.0)
            } else {
                1.0
            };
            pace.alertness.max(0.0) * player
        };
        for seen in in_view.filter(|seen| seen.in_view) {
            if seen.down_since.is_some() {
                if !wary(seen.who) {
                    self.found_body(seen.who, seen.down_since.unwrap_or(tick), seen.at, tick);
                }
                continue;
            }
            let strength = seen.strength.clamp(0.0, 1.0);
            let fill = SIGHT_FILL * strength * strength * scale(Some(seen.who)) * pace.seconds;
            let tops = if wary(seen.who) { 1.0 } else { CALM_TOPS };
            self.feed(Some(seen.who), fill, seen.at, Feed::Sight(tops), tick);
        }
        for heard in heard.filter(|h| h.tick == tick && !h.of_world) {
            let weapon = if heard.weapon { WEAPON_HEARD } else { 1.0 };
            let fill = HEARING_FILL * heard.strength * weapon * scale(heard.by);
            match heard.by {
                // The noises of those it is not wary of go unremarked.
                Some(by) if !wary(by) => {}
                Some(by) => self.feed(Some(by), fill, heard.at, Feed::Hearing(HEARING_TOPS), tick),
                None => self.feed(None, fill, heard.at, Feed::Hearing(HEARING_TOPS), tick),
            }
        }
        if let Some(gone) = self.dropped.take() {
            tell(gone);
        }
        let drain = DRAIN * pace.seconds;
        let grace = GRACE_SECONDS;
        let quiet = |since: u32| tick.wrapping_sub(since) as f32 * pace.seconds > grace;
        for slot in self
            .people
            .iter_mut()
            .chain(std::iter::once(&mut self.place))
        {
            let Some(notice) = slot else { continue };
            let held_by = if notice.gauge > HEARING_TOPS {
                notice.fed_high
            } else {
                notice.fed
            };
            if quiet(held_by) {
                notice.gauge = (notice.gauge - drain).max(0.0);
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
            if notice.level == Aware::Unaware && notice.gauge <= 0.0 {
                *slot = None;
            }
        }
    }

    /// Fills the gauge of `about` (a place when `None`) by `fill`, no
    /// further than the feed allows, as sensed at `at` on `tick`.
    fn feed(&mut self, about: Option<W>, fill: f32, at: [f32; 3], how: Feed, tick: u32) {
        let fill = if fill.is_finite() { fill.max(0.0) } else { 0.0 };
        let Some(notice) = self.slot_for(about, fill, at, tick) else {
            return;
        };
        let tops = match how {
            Feed::Sight(tops) | Feed::Hearing(tops) => tops,
        };
        // A body found is not forgotten for a noise elsewhere.
        if about.is_none() && notice.body.is_some() && notice.gauge > HEARING_TOPS {
            return;
        }
        if notice.gauge < tops {
            notice.gauge = (notice.gauge + fill).min(tops);
        }
        notice.at = at;
        notice.fed = tick;
        match how {
            Feed::Sight(tops) => {
                notice.seen = Some(tick);
                if tops > HEARING_TOPS {
                    notice.fed_high = tick;
                }
            }
            Feed::Hearing(_) => {
                notice.heard = Some(tick);
                if about.is_none() {
                    notice.body = None;
                }
            }
        }
    }

    /// The gauge of `about`, made if there is none: in an empty place, or
    /// in place of the one noticed least (never the player, nor one
    /// noticed more than `fill`). `None` when there is no room.
    fn slot_for(
        &mut self,
        about: Option<W>,
        fill: f32,
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
            fed: tick,
            fed_high: tick,
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
        let room = if let Some(empty) = self.people.iter().position(Option::is_none) {
            Some(empty)
        } else {
            let player = self.player.is_some() && Some(who) == self.player;
            self.least().filter(|&slot| {
                // Only one still unaware is let go, so no level a script
                // was told of vanishes untold; the player takes any.
                self.people[slot]
                    .is_some_and(|n| player || (n.level == Aware::Unaware && n.gauge < fill))
            })
        };
        let slot = room?;
        if let Some(gone) = self.people[slot]
            && gone.level != Aware::Unaware
        {
            self.dropped = Some(Change {
                about: gone.about,
                at: gone.at,
                now: Aware::Unaware,
                before: gone.level,
            });
        }
        self.people[slot] = Some(fresh);
        self.people[slot].as_mut()
    }

    /// The place of the one noticed least, the player aside: the lowest
    /// level, then the emptiest gauge.
    fn least(&self) -> Option<usize> {
        (0..MOST_NOTICED)
            .filter_map(|slot| self.people[slot].map(|n| (slot, n)))
            .filter(|(_, n)| self.player.is_none() || n.about != self.player)
            .min_by(|(_, a), (_, b)| (a.level.cmp(&b.level)).then(a.gauge.total_cmp(&b.gauge)))
            .map(|(slot, _)| slot)
    }

    /// A body found lying there: an alarm about the place, unless it is
    /// one already found.
    fn found_body(&mut self, who: W, down_since: u32, at: [f32; 3], tick: u32) {
        if self
            .bodies
            .iter()
            .flatten()
            .any(|&k| k == (who, down_since))
        {
            return;
        }
        self.bodies[self.next_body % MOST_BODIES] = Some((who, down_since));
        self.next_body = (self.next_body + 1) % MOST_BODIES;
        self.raise(None, at, tick);
        if let Some(place) = &mut self.place {
            place.body = Some(who);
            place.seen = Some(tick);
        }
    }

    /// Raises the alarm about `about` (a place when `None`) at `at`, as a
    /// script's `npc:alert` does or a body found: the gauge is full, and
    /// the level rises to alarmed at the next step.
    pub fn raise(&mut self, about: Option<W>, at: [f32; 3], tick: u32) {
        let Some(notice) = self.slot_for(about, f32::INFINITY, at, tick) else {
            return;
        };
        notice.gauge = 1.0;
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

    fn pace(tick: u32) -> Pace<u32> {
        Pace {
            tick,
            seconds: TICK,
            alertness: 1.0,
            player: Some(PLAYER),
            player_scale: 1.0,
        }
    }

    fn seen(who: u32, strength: f32, tick: u32) -> Sighting<u32> {
        Sighting {
            who,
            strength,
            in_view: true,
            down_since: None,
            at: [1.0, 0.0, 2.0],
            seen: tick,
            since: 0,
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
            aware.take_in(
                s.iter(),
                n.iter(),
                &pace(tick),
                |who| who < 100,
                |c| {
                    changes.push((tick, c));
                },
            );
        }
    }

    fn first(changes: &[(u32, Change<u32>)], now: Aware) -> Option<u32> {
        changes.iter().find(|(_, c)| c.now == now).map(|(t, _)| *t)
    }

    #[test]
    fn sight_fills_with_the_square_of_how_strongly_one_is_seen() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            200,
            |t| vec![seen(PLAYER, 0.3, t)],
            |_| vec![],
            &mut changes,
        );
        // 0.25 / (5 * 0.09) = 0.56 s; 1 / 0.45 = 2.2 s.
        let curious = first(&changes, Aware::Curious).unwrap();
        let alarmed = first(&changes, Aware::Alarmed).unwrap();
        assert!((33..=35).contains(&curious), "{curious}");
        assert!((132..=135).contains(&alarmed), "{alarmed}");
        // Plain view, close: at once.
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
        assert!(first(&changes, Aware::Alarmed).unwrap() <= 12);
    }

    #[test]
    fn once_out_of_sight_it_searches_before_it_is_calm_and_says_each_step() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            30,
            |t| vec![seen(PLAYER, 1.0, t)],
            |_| vec![],
            &mut changes,
        );
        assert_eq!(aware.level_of(PLAYER), Aware::Alarmed);
        let lost = 30;
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
        assert!(
            (secs(changes[0].0) - 4.75).abs() < 0.1,
            "{}",
            secs(changes[0].0)
        );
        assert!(
            (secs(changes[1].0) - 16.0).abs() < 0.1,
            "{}",
            secs(changes[1].0)
        );
        assert!(
            (secs(changes[2].0) - 23.5).abs() < 0.1,
            "{}",
            secs(changes[2].0)
        );
        // Where to look stays where they were last seen, and when.
        assert!(aware.of(PLAYER).is_none(), "forgotten once calm and empty");
    }

    #[test]
    fn the_level_does_not_flicker_at_a_boundary() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        // Seen just long enough to be curious, then glimpsed on and off.
        run(
            &mut aware,
            0,
            600,
            |t| {
                if t % 120 < 40 {
                    vec![seen(PLAYER, 0.3, t)]
                } else {
                    vec![]
                }
            },
            |_| vec![],
            &mut changes,
        );
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
        // A shot nearby is searching at once; a footstep a few metres off only
        // adds a little.
        let mut aware = Awareness::new(Some(PLAYER));
        run(
            &mut aware,
            0,
            1,
            |_| vec![],
            |t| vec![heard(Some(PLAYER), 0.5, t, true)],
            &mut changes,
        );
        assert_eq!(aware.level_of(PLAYER), Aware::Searching);
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
        assert!(place.is_place());
        assert_eq!(place.level, Aware::Searching);
        assert_eq!(changes[0].1.about, None);
        assert_eq!(aware.interest().unwrap().at, [5.0, 0.0, 5.0]);
    }

    #[test]
    fn one_it_is_not_wary_of_is_noticed_never_alarming() {
        let friend = 200;
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(
            &mut aware,
            0,
            600,
            |t| vec![seen(friend, 1.0, t)],
            |t| vec![heard(Some(friend), 1.0, t, true)],
            &mut changes,
        );
        assert!(changes.is_empty(), "{changes:?}");
        assert_eq!(aware.level_of(friend), Aware::Unaware);
        assert!((aware.gauge_of(friend) - CALM_TOPS).abs() < 1e-6);
        assert!(aware.interest().is_none());
    }

    #[test]
    fn a_body_found_is_an_alarm_about_its_place_once() {
        let friend = 200;
        let mut body = seen(friend, 0.5, 0);
        body.down_since = Some(0);
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        run(&mut aware, 1, 1, |_| vec![body], |_| vec![], &mut changes);
        let place = aware.place().unwrap();
        assert_eq!((place.level, place.body), (Aware::Alarmed, Some(friend)));
        assert_eq!(aware.level_of(friend), Aware::Unaware);
        // Calmed by a script, the same body still in view does not alarm it
        // again.
        aware.calm_all(2, |c| changes.push((2, c)));
        run(&mut aware, 3, 60, |_| vec![body], |_| vec![], &mut changes);
        assert!(aware.place().is_none());
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
                    |_| true,
                    |_| {},
                );
                if aware.level_of(who) == Aware::Alarmed {
                    return tick;
                }
            }
            u32::MAX
        };
        // 1 / (5 * 0.09 * scale) seconds.
        assert!((265..=268).contains(&ticks_to_alarm(PLAYER, 0.5)));
        assert!((132..=135).contains(&ticks_to_alarm(PLAYER, 1.0)));
        assert!((88..=90).contains(&ticks_to_alarm(PLAYER, 1.5)));
        assert_eq!(ticks_to_alarm(5, 0.5), ticks_to_alarm(5, 1.5));
    }

    #[test]
    fn alertness_scales_every_fill() {
        let mut aware = Awareness::new(Some(PLAYER));
        let s = [seen(5, 1.0, 0)];
        let mut sleepy = pace(0);
        sleepy.alertness = 0.5;
        aware.take_in(
            s.iter(),
            std::iter::empty::<&Heard<u32>>(),
            &sleepy,
            |_| true,
            |_| {},
        );
        assert!((aware.gauge_of(5) - 0.5 * SIGHT_FILL * TICK).abs() < 1e-6);
    }

    #[test]
    fn the_player_always_has_a_place_when_more_are_noticed_than_there_is_room_for() {
        let mut aware = Awareness::new(Some(PLAYER));
        let mut changes = Vec::new();
        // Ten strangers fill every place, then the player is heard.
        run(
            &mut aware,
            0,
            20,
            |t| {
                (1..=10)
                    .map(|who| seen(who, 0.2 + who as f32 * 0.05, t))
                    .collect()
            },
            |_| vec![],
            &mut changes,
        );
        assert_eq!(aware.noticed().count(), MOST_NOTICED);
        assert!(
            aware.of(1).is_none() && aware.of(2).is_none(),
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
        // No stranger takes it again while it is not forgotten.
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
