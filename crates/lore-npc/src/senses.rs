//! What an NPC sees and hears.
//!
//! Sight is a cone from the eyes along the way the body faces, with a
//! reach, blocked by the level, what stands fixed in it and the bodies of
//! people standing in the way. It is aimed at three points of whoever is
//! looked at (head, chest and knees), so someone behind a low wall is seen
//! by the part that shows. Distance, fog, darkness, crouching and keeping
//! still make it weaker. Hearing takes the very noises the player hears,
//! each with its loudness and reach: one is heard inside its reach scaled
//! by how well the NPC hears, weaker with distance and through a wall.
//!
//! Everything here is plain data and pure functions. The world says each
//! tick who can be seen ([`Seeable`]), what was heard ([`Sound`]) and how
//! light it is; [`Senses`] keeps, per NPC and in storage of a fixed size,
//! whom it sees now and saw last ([`Sighting`]) and what it lately heard
//! ([`Heard`]). That is what awareness and an NPC's rules read, without
//! going through the script.
//!
//! An NPC looks a few times a second, not every tick: [`Senses::looks_on`]
//! staggers the looks of a crowd by id, so a town's sight costs about the
//! same on every tick. It listens every tick, because a noise lasts one.
//!
//! Who is kept in mind as seen is decided once a look, after every one in
//! sight has been weighed: the player always has a place; then those in
//! view, the stronger first; one already kept stays unless a newcomer is
//! clearly ([`CLEARLY`]) stronger, so two of nearly the same strength do
//! not take turns; and one lately lost from view keeps their place till
//! [`KEEP_LOST`] ticks after they were last seen, against any newcomer but
//! the player. One look never
//! tells both that someone came into view and that they went out of it.

use lore_physics::Level;

/// How many ticks pass between two looks of one NPC, until the world says
/// otherwise ([`Senses::every`]): 12 is five looks a second at 60 ticks a
/// second.
pub const LOOK_EVERY: u32 = 12;

/// The most people one NPC keeps in mind as seen: those in view now and
/// the last seen of those who went out of it.
pub const MOST_SIGHTED: usize = 8;

/// The most noises one NPC keeps as lately heard: at most one for each
/// maker, so the newest of the player's stays through a noisy second.
pub const MOST_HEARD: usize = 4;

/// How strongly someone must be seen to come into view, from 0 to 1.
pub const SEEN_FROM: f32 = 0.1;

/// Below how strongly someone in view goes out of it. Lower than
/// [`SEEN_FROM`], so someone at the edge of sight does not flicker in and
/// out of view from one look to the next.
pub const LOST_BELOW: f32 = 0.05;

/// How much more strongly a newcomer must be seen than someone already kept
/// in view to take their place when there is no room for both, from 0 to
/// 1: so two seen nearly as strongly do not take turns from look to look.
pub const CLEARLY: f32 = 0.1;

/// How many ticks after they were last seen someone lost from view keeps
/// their place in mind against a newcomer, the player excepted: 180, three
/// seconds at 60 ticks a second, so "lost sight of them a moment ago" outlives a crowd walking
/// past.
pub const KEEP_LOST: u32 = 180;

/// How faintly a noise may arrive and still be heard, from 0 to 1. Far
/// below what the player notices, so a noise is heard right up to its
/// reach, as the player hears it.
pub const HEARD_FROM: f32 = 0.001;

/// How much of a noise comes through a wall between it and the ears.
pub const THROUGH_WALL: f32 = 0.35;

/// How dark it is on a clear night, with no lamp: moon and stars, from 0
/// to 1, where 1 is day.
pub const NIGHT: f32 = 0.1;

/// Within this many metres someone is seen whatever the dark.
const CLOSE_IN_DARK: f32 = 3.0;

/// How far up or down from level the eyes take in, in radians: 70 degrees.
/// Eyes that see all round (a width of 360) see straight up and down too.
const LOOKS_UP_AND_DOWN: f32 = 1.221_730_5;

/// How much a noise's place is raised before the ray to the ears is cast,
/// in metres, so a footstep is not heard through the floor it was made on.
const NOISE_LIFT: f32 = 0.3;

/// The most standing bodies one look tests as hiding what is behind them:
/// the nearest in front of the eyes.
const MOST_HIDERS: usize = 16;

/// How far from the middle of a standing body a line of sight passes and
/// is still blocked by it, in metres: narrower than the body is wide, since
/// a person is not a pillar.
const BODY_HIDES_WITHIN: f32 = 0.22;

/// How a game sets an NPC's senses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keenness {
    /// How far it sees in full light, in metres. 0 is blind.
    pub sight: f32,
    /// How wide it sees, in degrees from one edge to the other: 120 is a
    /// person's, 360 sees all round, up and down too.
    pub width: f32,
    /// How well it hears: 1 hears a noise as far as the player does, 2
    /// twice as far, 0 not at all.
    pub hearing: f32,
    /// How well it sees in the dark, from 0 to 1: 0 as a person does, 1
    /// as well as by day.
    pub night_sight: f32,
}

impl Default for Keenness {
    /// A person's: 30 metres, 120 degrees wide, hearing as the player
    /// does, no better in the dark.
    fn default() -> Self {
        Self {
            sight: 30.0,
            width: 120.0,
            hearing: 1.0,
            night_sight: 0.0,
        }
    }
}

/// Someone an NPC may see, as the world says of them on a tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seeable<W> {
    /// Who it is.
    pub who: W,
    /// Where their feet are, in metres.
    pub feet: [f32; 3],
    /// How tall they are now, in metres: less crouched, and least lying
    /// down.
    pub height: f32,
    /// How fast they move across the ground, in metres a second.
    pub speed: f32,
    /// Whether they crouch.
    pub crouched: bool,
    /// Whether their health is gone: a body lying there, not a person
    /// standing.
    pub down: bool,
}

impl<W> Seeable<W> {
    /// The three points a look is aimed at: head, chest and knees.
    fn points(&self) -> [[f32; 3]; 3] {
        let [x, y, z] = self.feet;
        let tall = self.height.max(0.1);
        [
            [x, y + tall * 0.93, z],
            [x, y + tall * 0.7, z],
            [x, y + tall * 0.2, z],
        ]
    }
}

/// How much each of the three points counts toward what is in view.
const POINT_SHARES: [f32; 3] = [0.3, 0.4, 0.3];

/// The NPC that looks or listens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Looker<W> {
    /// Who it is: it does not see itself, nor hear its own noises.
    pub who: W,
    /// Where its eyes (and ears) are, in metres.
    pub eyes: [f32; 3],
    /// Which way its body faces, in radians, turning left from -Z: the
    /// simulation's facing, never the head turn a picture draws.
    pub facing: f32,
}

/// What the world shows the NPCs that look on a tick.
#[derive(Debug, Clone, Copy)]
pub struct Sight<'a, W> {
    /// The level's shapes, and the things fixed in it, that hide.
    pub level: &'a Level,
    /// The tick it is.
    pub tick: u32,
    /// Everyone who may be seen, the same list for every NPC that tick.
    /// Those standing in it also hide who is behind them.
    pub seeable: &'a [Seeable<W>],
    /// How much of what is behind each metre of air the weather hides:
    /// [`haze`].
    pub haze: f32,
}

/// A noise as an NPC may hear it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sound<W, N = ()> {
    /// Where it was made, in metres.
    pub at: [f32; 3],
    /// How loud it was made, from 0 up: 1 is a gunshot.
    pub loudness: f32,
    /// Inside this many metres it is heard at its full loudness.
    pub near: f32,
    /// Past this many metres it is not heard at all: its reach.
    pub far: f32,
    /// Who made it, when someone did.
    pub by: Option<W>,
    /// What it was, as the world names noises: kept with what was heard.
    pub what: N,
    /// Whether it is a sound of the world going on (thunder, rain, running
    /// water) rather than something someone did: kept, but it does not draw
    /// an NPC's attention ([`Senses::last_heard`]) nor is it told.
    pub of_world: bool,
    /// Whether a weapon made it: a shot, a blast, a bullet striking.
    pub weapon: bool,
}

/// Someone an NPC sees, or saw last.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sighting<W> {
    /// Who.
    pub who: W,
    /// How strongly they were seen at the last look, from 0 to 1: 0 once
    /// out of view.
    pub strength: f32,
    /// Whether they are in view now.
    pub in_view: bool,
    /// When they were last seen lying there, health gone: the tick of the
    /// look that first saw them down (whether it saw them fall or found
    /// them so). `None` when they were last seen standing.
    pub down_since: Option<u32>,
    /// Where their feet were when last seen, in metres.
    pub at: [f32; 3],
    /// The tick they were last seen on.
    pub seen: u32,
    /// The tick they last came into view on.
    pub since: u32,
}

impl<W> Sighting<W> {
    /// Whether they are out of view but were seen within [`KEEP_LOST`]
    /// ticks of `tick`, so their place in mind is kept.
    fn lately_lost(&self, tick: u32) -> bool {
        !self.in_view && tick.wrapping_sub(self.seen) < KEEP_LOST
    }
}

/// A noise an NPC heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heard<W, N = ()> {
    /// Where it was made, in metres.
    pub at: [f32; 3],
    /// How loud it reached the NPC, from 0 to 1: after distance, how well
    /// it hears, and any wall.
    pub strength: f32,
    /// How loud it was made, from 0 up.
    pub loudness: f32,
    /// The tick it was heard on.
    pub tick: u32,
    /// Who made it, when someone did.
    pub by: Option<W>,
    /// Whether a wall stood between it and the NPC.
    pub walled: bool,
    /// What it was, as the world names noises.
    pub what: N,
    /// Whether it is a sound of the world going on: see [`Sound::of_world`].
    pub of_world: bool,
    /// Whether a weapon made it.
    pub weapon: bool,
}

/// What an NPC's senses tell of a look or a listen, for the script's
/// events.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sensed<W> {
    /// Someone came into view.
    Saw(W),
    /// Someone went out of view; `at` is where their feet were last seen.
    Lost {
        /// Who.
        who: W,
        /// Where they were last seen, in metres.
        at: [f32; 3],
    },
    /// The noise at this place in the list the listen was given was heard.
    /// A sound of the world ([`Sound::of_world`]) is kept but not told.
    Heard(usize),
}

/// What one NPC sees and has lately heard, and how keen it is: kept from
/// tick to tick in storage of a fixed size, so sensing sets no memory
/// aside.
#[derive(Debug, Clone, PartialEq)]
pub struct Senses<W, N = ()> {
    /// How keen its senses are.
    pub keen: Keenness,
    /// How many ticks pass between its looks: [`LOOK_EVERY`] until the
    /// world says otherwise. A world that looks less often for NPCs far
    /// from the player raises it.
    pub every: u32,
    /// The player, who always has a place in mind and is never the one let
    /// go to make room.
    player: Option<W>,
    sighted: [Option<Sighting<W>>; MOST_SIGHTED],
    heard: [Option<Heard<W, N>>; MOST_HEARD],
    /// In what order each of `heard` came: higher is newer.
    order: [u32; MOST_HEARD],
    /// The order the next noise kept gets.
    next_order: u32,
}

/// One seen this look, with how strongly, and where in the list given.
#[derive(Clone, Copy)]
struct Weighed<W> {
    target: Seeable<W>,
    strength: f32,
    nth: usize,
}

/// A standing body that may hide who is behind it.
#[derive(Clone, Copy)]
struct Hider<W> {
    who: W,
    feet: [f32; 3],
    height: f32,
    /// How far from the eyes, squared, across the ground.
    away: f32,
}

impl<W: Copy + PartialEq, N: Copy> Senses<W, N> {
    /// Senses as keen as `keen`, that have seen and heard nothing yet.
    /// `player` is the one who always has a place in mind.
    #[must_use]
    pub fn new(keen: Keenness, player: Option<W>) -> Self {
        Self {
            keen,
            every: LOOK_EVERY,
            player,
            sighted: [None; MOST_SIGHTED],
            heard: [None; MOST_HEARD],
            order: [0; MOST_HEARD],
            next_order: 0,
        }
    }

    /// Whether the NPC known by the number `id` looks on `tick`. The NPCs
    /// of a crowd take turns: of every [`every`](Self::every) ticks, each
    /// looks on one, set by its own number, so one leaving the world does
    /// not move when any other looks.
    #[must_use]
    pub fn looks_on(&self, tick: u32, id: u32) -> bool {
        let every = self.every.max(1);
        tick.wrapping_add(id).is_multiple_of(every)
    }

    /// Everyone it keeps in mind as seen: in view now, or last seen a while
    /// ago. In no order that means anything.
    pub fn sighted(&self) -> impl Iterator<Item = &Sighting<W>> {
        self.sighted.iter().flatten()
    }

    /// Those in view now.
    pub fn in_view(&self) -> impl Iterator<Item = &Sighting<W>> {
        self.sighted().filter(|seen| seen.in_view)
    }

    /// What it keeps of `who`, if it has seen them.
    #[must_use]
    pub fn sighting(&self, who: W) -> Option<&Sighting<W>> {
        self.sighted().find(|seen| seen.who == who)
    }

    /// Whether `who` is in view now (as of its last look).
    #[must_use]
    pub fn sees(&self, who: W) -> bool {
        self.sighting(who).is_some_and(|seen| seen.in_view)
    }

    /// Every noise it lately heard, newest first: at most one of each
    /// maker's, sounds of the world among them.
    pub fn heard(&self) -> impl Iterator<Item = &Heard<W, N>> {
        let mut slots: [usize; MOST_HEARD] = std::array::from_fn(|slot| slot);
        slots.sort_unstable_by_key(|&slot| std::cmp::Reverse(self.order[slot]));
        slots
            .into_iter()
            .filter_map(|slot| self.heard[slot].as_ref())
    }

    /// The newest noise it heard that draws attention: anything but a
    /// sound of the world ([`Heard::of_world`]). What "heard something"
    /// means.
    #[must_use]
    pub fn last_heard(&self) -> Option<&Heard<W, N>> {
        self.heard().find(|heard| !heard.of_world)
    }

    /// The newest noise `by` made that it heard, if it still keeps one.
    #[must_use]
    pub fn heard_from(&self, by: W) -> Option<&Heard<W, N>> {
        self.heard().find(|heard| heard.by == Some(by))
    }

    /// Forgets everything seen and heard, telling nothing: for an NPC whose
    /// health is gone, or that was put somewhere new.
    pub fn forget(&mut self) {
        self.sighted = [None; MOST_SIGHTED];
        self.heard = [None; MOST_HEARD];
        self.order = [0; MOST_HEARD];
        self.next_order = 0;
    }

    /// Looks once: how strongly `me` sees each of `sight.seeable` now.
    /// `light` gives how light it is, from 0 to 1, where the one at that
    /// place in the list stands; it is asked only of those within reach and
    /// in the cone. Each ray cast at the level is counted in `rays`. Who
    /// came into view and who went out of it is told to `tell`.
    ///
    /// Who is kept in mind is decided once every strength of the look is
    /// known (see the module's notes), so one look never tells someone
    /// both seen and lost.
    pub fn look(
        &mut self,
        me: &Looker<W>,
        sight: &Sight<'_, W>,
        mut light: impl FnMut(usize) -> f32,
        rays: &mut u32,
        mut tell: impl FnMut(Sensed<W>),
    ) {
        let tick = sight.tick;
        let hiders = hiders(me, &self.keen, sight.seeable);
        let hiders = hiders.iter().flatten();
        // How strongly each one kept is seen now (`None`: gone from the
        // world), and the strongest of the rest who could come into view.
        let mut kept: [Option<Weighed<W>>; MOST_SIGHTED] = [None; MOST_SIGHTED];
        let mut newcomers: [Option<Weighed<W>>; MOST_SIGHTED] = [None; MOST_SIGHTED];
        let mut player: Option<Weighed<W>> = None;
        for (nth, target) in sight.seeable.iter().enumerate() {
            if target.who == me.who {
                continue;
            }
            let open = seen_openly(me, &self.keen, target, sight.haze);
            let mut strength = 0.0;
            if open > 0.0 {
                let away = distance(me.eyes, target.points()[1]);
                let most = open * light_helps(light(nth), self.keen.night_sight, away);
                // Too faint to be seen even with nothing in the way: no ray.
                if most >= LOST_BELOW {
                    let shows = showing_past(sight.level, me.eyes, target, hiders.clone(), rays);
                    strength = (most * shows).min(1.0);
                }
            }
            let weighed = Weighed {
                target: *target,
                strength,
                nth,
            };
            if let Some(slot) = self.slot_of(target.who) {
                kept[slot] = Some(weighed);
            } else if strength >= SEEN_FROM {
                if Some(target.who) == self.player {
                    player = Some(weighed);
                } else {
                    offer(&mut newcomers, weighed);
                }
            }
        }
        // What each one kept is now, before anyone is let go.
        let mut was_in_view = [false; MOST_SIGHTED];
        for (slot, now) in kept.iter().enumerate() {
            let Some(seen) = &mut self.sighted[slot] else {
                continue;
            };
            was_in_view[slot] = seen.in_view;
            match now {
                Some(now) => {
                    let in_view = if seen.in_view {
                        now.strength >= LOST_BELOW
                    } else {
                        now.strength >= SEEN_FROM
                    };
                    if in_view {
                        if !seen.in_view {
                            seen.since = tick;
                        }
                        seen.down_since = if now.target.down {
                            seen.down_since.or(Some(tick))
                        } else {
                            None
                        };
                        seen.at = now.target.feet;
                        seen.seen = tick;
                    }
                    seen.in_view = in_view;
                    seen.strength = if in_view { now.strength } else { 0.0 };
                }
                None => {
                    seen.in_view = false;
                    seen.strength = 0.0;
                }
            }
        }
        // Newcomers take the room there is: the player first, then the
        // strongest first.
        newcomers.sort_unstable_by(|a, b| match (a, b) {
            (Some(a), Some(b)) => (b.strength.total_cmp(&a.strength)).then(a.nth.cmp(&b.nth)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        let mut taken = [false; MOST_SIGHTED];
        let mut let_go: [Option<Sighting<W>>; MOST_SIGHTED] = [None; MOST_SIGHTED];
        for (is_player, newcomer) in (player.iter().map(|one| (true, one)))
            .chain(newcomers.iter().flatten().map(|one| (false, one)))
        {
            let Some(slot) = self.room_for(is_player, newcomer.strength, tick, &taken) else {
                continue;
            };
            taken[slot] = true;
            if let_go[slot].is_none() {
                let_go[slot] = self.sighted[slot];
            }
            self.sighted[slot] = Some(Sighting {
                who: newcomer.target.who,
                strength: newcomer.strength,
                in_view: true,
                down_since: newcomer.target.down.then_some(tick),
                at: newcomer.target.feet,
                seen: tick,
                since: tick,
            });
        }
        // Then what changed is told, place by place.
        for slot in 0..MOST_SIGHTED {
            if let Some(gone) = let_go[slot]
                && was_in_view[slot]
            {
                tell(Sensed::Lost {
                    who: gone.who,
                    at: gone.at,
                });
            }
            let Some(seen) = self.sighted[slot] else {
                continue;
            };
            if taken[slot] || (seen.in_view && !was_in_view[slot]) {
                tell(Sensed::Saw(seen.who));
            } else if !seen.in_view && was_in_view[slot] {
                tell(Sensed::Lost {
                    who: seen.who,
                    at: seen.at,
                });
            }
        }
    }

    /// Where `who` is kept, if they are.
    fn slot_of(&self, who: W) -> Option<usize> {
        (self.sighted.iter()).position(|kept| matches!(kept, Some(seen) if seen.who == who))
    }

    /// The place for a newcomer seen `strength` strongly on `tick`, if
    /// there is one: an empty one; else that of the one longest lost from
    /// view more than [`KEEP_LOST`] ago; else, for the player, that of the
    /// one longest lost and then of the faintest in view; else that of the
    /// faintest in view, if the newcomer is [`CLEARLY`] stronger. Never the
    /// player's, nor one `taken` by a newcomer this look.
    fn room_for(
        &self,
        is_player: bool,
        strength: f32,
        tick: u32,
        taken: &[bool; MOST_SIGHTED],
    ) -> Option<usize> {
        let free = (0..MOST_SIGHTED).filter(|&slot| !taken[slot]);
        if let Some(empty) = free.clone().find(|&slot| self.sighted[slot].is_none()) {
            return Some(empty);
        }
        let held = free.filter_map(|slot| {
            (self.sighted[slot])
                .filter(|seen| Some(seen.who) != self.player)
                .map(|seen| (slot, seen))
        });
        let longest_lost = |lately: bool| {
            (held.clone())
                .filter(|(_, seen)| !seen.in_view && seen.lately_lost(tick) == lately)
                .max_by_key(|(_, seen)| tick.wrapping_sub(seen.seen))
                .map(|(slot, _)| slot)
        };
        if let Some(slot) = longest_lost(false) {
            return Some(slot);
        }
        if is_player && let Some(slot) = longest_lost(true) {
            return Some(slot);
        }
        let (slot, faintest) = (held.clone())
            .filter(|(_, seen)| seen.in_view)
            .min_by(|(_, a), (_, b)| a.strength.total_cmp(&b.strength))?;
        (is_player || strength > faintest.strength + CLEARLY).then_some(slot)
    }

    /// Listens once, on `tick`, to `sounds`: the noises made in the world
    /// that tick. One heard is kept (see [`MOST_HEARD`]) and, unless it is a
    /// sound of the world, told to `tell` by its place in the list. A ray at
    /// `level` for each noise within reach says whether a wall muffles it;
    /// each is counted in `rays`.
    pub fn listen(
        &mut self,
        me: &Looker<W>,
        level: &Level,
        tick: u32,
        sounds: &[Sound<W, N>],
        rays: &mut u32,
        mut tell: impl FnMut(Sensed<W>),
    ) {
        for (nth, sound) in sounds.iter().enumerate() {
            if sound.by == Some(me.who) {
                continue;
            }
            let mut strength = arriving(sound, me.eyes, self.keen.hearing);
            if strength < HEARD_FROM {
                continue;
            }
            let from = [sound.at[0], sound.at[1] + NOISE_LIFT, sound.at[2]];
            let walled = blocked(level, from, me.eyes, rays);
            if walled {
                strength *= THROUGH_WALL;
            }
            if strength < HEARD_FROM {
                continue;
            }
            self.keep_heard(Heard {
                at: sound.at,
                strength: strength.min(1.0),
                loudness: sound.loudness,
                tick,
                by: sound.by,
                walled,
                what: sound.what,
                of_world: sound.of_world,
                weapon: sound.weapon,
            });
            if !sound.of_world {
                tell(Sensed::Heard(nth));
            }
        }
    }

    /// Keeps `heard`: in place of the one kept of the same maker (the
    /// louder of two on one tick), else in an empty place, else in place of
    /// the oldest of those that matter least (the world's, then nobody's,
    /// then a person's), never the player's and never one that matters
    /// more than it. Else it is not kept.
    fn keep_heard(&mut self, heard: Heard<W, N>) {
        let same = |kept: &Heard<W, N>| kept.by == heard.by && kept.of_world == heard.of_world;
        let slot = if let Some(slot) =
            (self.heard.iter()).position(|kept| kept.as_ref().is_some_and(same))
        {
            let louder_kept = (self.heard[slot])
                .is_some_and(|kept| kept.tick == heard.tick && kept.strength >= heard.strength);
            if louder_kept {
                return;
            }
            Some(slot)
        } else if let Some(empty) = self.heard.iter().position(Option::is_none) {
            Some(empty)
        } else {
            let rank = self.rank(&heard);
            (0..MOST_HEARD)
                .filter_map(|slot| self.heard[slot].map(|kept| (slot, kept)))
                .filter(|(_, kept)| {
                    self.rank(kept) <= rank && kept.by.is_none_or(|by| Some(by) != self.player)
                })
                .min_by_key(|&(slot, ref kept)| (self.rank(kept), self.order[slot]))
                .map(|(slot, _)| slot)
        };
        if let Some(slot) = slot {
            self.heard[slot] = Some(heard);
            self.order[slot] = self.next_order;
            self.next_order = self.next_order.wrapping_add(1);
        }
    }

    /// How much a noise heard matters for keeping: the world's least, then
    /// one nobody made, then a person's, then the player's.
    fn rank(&self, heard: &Heard<W, N>) -> u8 {
        match heard.by {
            _ if heard.of_world => 0,
            None => 1,
            Some(by) if Some(by) == self.player => 3,
            Some(_) => 2,
        }
    }
}

/// Puts `one` among `best`, the strongest few weighed this look, if it is
/// stronger than the faintest of them (the earlier in the list on a tie).
fn offer<W: Copy>(best: &mut [Option<Weighed<W>>; MOST_SIGHTED], one: Weighed<W>) {
    if let Some(empty) = best.iter_mut().find(|kept| kept.is_none()) {
        *empty = Some(one);
        return;
    }
    let faintest = (best.iter_mut().flatten())
        .min_by(|a, b| (a.strength.total_cmp(&b.strength)).then(b.nth.cmp(&a.nth)));
    if let Some(faintest) = faintest
        && one.strength > faintest.strength
    {
        *faintest = one;
    }
}

/// The standing bodies in front of `me` within its reach that may hide who
/// is behind them, the nearest [`MOST_HIDERS`]: a list of a fixed size.
fn hiders<W: Copy + PartialEq>(
    me: &Looker<W>,
    keen: &Keenness,
    seeable: &[Seeable<W>],
) -> [Option<Hider<W>>; MOST_HIDERS] {
    let mut found: [Option<Hider<W>>; MOST_HIDERS] = [None; MOST_HIDERS];
    let reach = keen.sight;
    if reach.is_nan() || reach <= 0.0 {
        return found;
    }
    let ahead = [-libm::sinf(me.facing), -libm::cosf(me.facing)];
    for one in seeable {
        if one.who == me.who || one.down {
            continue;
        }
        let to = [one.feet[0] - me.eyes[0], one.feet[2] - me.eyes[2]];
        let away = to[0] * to[0] + to[1] * to[1];
        if away.is_nan() || away > reach * reach {
            continue;
        }
        // Behind the eyes it hides nothing they look at, unless they see
        // all round.
        if keen.width < 360.0 && to[0] * ahead[0] + to[1] * ahead[1] < -BODY_HIDES_WITHIN {
            continue;
        }
        let hider = Hider {
            who: one.who,
            feet: one.feet,
            height: one.height,
            away,
        };
        if let Some(empty) = found.iter_mut().find(|kept| kept.is_none()) {
            *empty = Some(hider);
        } else if let Some(furthest) =
            (found.iter_mut().flatten()).max_by(|a, b| a.away.total_cmp(&b.away))
            && hider.away < furthest.away
        {
            *furthest = hider;
        }
    }
    found
}

/// Whether one of `hiders` other than `target` stands across the line from
/// `eyes` to `point`.
fn hidden_by_body<'a, W: Copy + PartialEq + 'a>(
    eyes: [f32; 3],
    point: [f32; 3],
    target: W,
    hiders: impl Iterator<Item = &'a Hider<W>>,
) -> bool {
    let way = [point[0] - eyes[0], point[1] - eyes[1], point[2] - eyes[2]];
    let flat = way[0] * way[0] + way[2] * way[2];
    if flat.is_nan() || flat <= 1e-6 {
        return false;
    }
    hiders.filter(|hider| hider.who != target).any(|hider| {
        // Where along the line it passes nearest the body's middle, across
        // the ground.
        let to = [hider.feet[0] - eyes[0], hider.feet[2] - eyes[2]];
        let along = (to[0] * way[0] + to[1] * way[2]) / flat;
        if !(0.0..0.98).contains(&along) {
            return false;
        }
        let off = [to[0] - way[0] * along, to[1] - way[2] * along];
        let height = eyes[1] + way[1] * along - hider.feet[1];
        off[0] * off[0] + off[1] * off[1] < BODY_HIDES_WITHIN * BODY_HIDES_WITHIN
            && (0.0..=hider.height).contains(&height)
    })
}

/// How much of `target` shows from `eyes` past the level and `hiders`,
/// from 0 to 1: a point a body hides costs no ray.
fn showing_past<'a, W: Copy + PartialEq + 'a>(
    level: &Level,
    eyes: [f32; 3],
    target: &Seeable<W>,
    hiders: impl Iterator<Item = &'a Hider<W>> + Clone,
    rays: &mut u32,
) -> f32 {
    target
        .points()
        .iter()
        .zip(POINT_SHARES)
        .filter(|(point, _)| {
            !hidden_by_body(eyes, **point, target.who, hiders.clone())
                && !blocked(level, eyes, **point, rays)
        })
        .map(|(_, share)| share)
        .sum()
}

/// How loud `sound` reaches ears at `ears` that hear `hearing` well, from
/// 0 to 1, with nothing in the way: as loud as it was made inside its
/// `near`, half as loud each time the distance doubles past it, fading to
/// nothing over the last tenth of its reach, as the player hears it. Good
/// hearing is the same as the noise being nearer.
#[must_use]
pub fn arriving<W, N>(sound: &Sound<W, N>, ears: [f32; 3], hearing: f32) -> f32 {
    if !(hearing > 0.0 && sound.loudness > 0.0) {
        return 0.0;
    }
    let near = if sound.near.is_finite() {
        sound.near.max(0.01)
    } else {
        0.01
    };
    let far = if sound.far.is_nan() {
        near
    } else {
        sound.far.max(near)
    };
    let away = distance(sound.at, ears) / hearing;
    if !away.is_finite() || away >= far {
        return 0.0;
    }
    let mut gain = near / away.max(near);
    let fade_from = far * 0.9;
    if away > fade_from {
        gain *= (far - away) / (far - fade_from);
    }
    (sound.loudness * gain).clamp(0.0, 1.0)
}

/// How strongly eyes as keen as `keen` at `me` would see `target` with
/// nothing in the way and in full light, from 0 to 1: 0 past the reach,
/// outside the cone or too far above or below (eyes that see all round
/// see straight up and down too). Weaker toward the edge of
/// the cone, with distance and with `haze`, and for one who crouches,
/// keeps still or lies there.
#[must_use]
pub fn seen_openly<W>(me: &Looker<W>, keen: &Keenness, target: &Seeable<W>, haze: f32) -> f32 {
    let reach = keen.sight;
    if !(reach > 0.0 && keen.width > 0.0) {
        return 0.0;
    }
    let chest = target.points()[1];
    let to = [
        chest[0] - me.eyes[0],
        chest[1] - me.eyes[1],
        chest[2] - me.eyes[2],
    ];
    // The cheapest test first: out of reach.
    let squared = to[0] * to[0] + to[1] * to[1] + to[2] * to[2];
    if squared.is_nan() || squared > reach * reach {
        return 0.0;
    }
    let away = libm::sqrtf(squared);
    let flat = libm::hypotf(to[0], to[2]);
    // Straight ahead, along the facing: -Z turned left by it.
    let ahead = [-libm::sinf(me.facing), -libm::cosf(me.facing)];
    let along = to[0] * ahead[0] + to[2] * ahead[1];
    let across = to[0] * ahead[1] - to[2] * ahead[0];
    let aside = libm::fabsf(libm::atan2f(across, along));
    let half = keen.width.min(360.0).to_radians() / 2.0;
    let edge = if keen.width >= 360.0 {
        1.0
    } else if away < 0.3 {
        // On top of the eyes: no side to be on.
        1.0
    } else if aside > half {
        return 0.0;
    } else {
        1.0 - 0.5 * aside / half
    };
    if keen.width < 360.0 && libm::fabsf(libm::atan2f(to[1], flat)) > LOOKS_UP_AND_DOWN && away >= 0.3
    {
        return 0.0;
    }
    // Clear to a quarter of the reach, then fainter, to 0.3 at the reach.
    let far = ((away / reach - 0.25) / 0.75).clamp(0.0, 1.0);
    let nearness = 1.0 - 0.7 * far;
    let air = libm::expf(-haze.max(0.0) * away);
    let posture = if target.down {
        0.7
    } else if target.crouched {
        0.6
    } else {
        1.0
    };
    let moving = 0.75 + 0.25 * (target.speed / 4.5).clamp(0.0, 1.0);
    (edge * nearness * air * posture * moving).clamp(0.0, 1.0)
}

/// How much of `target` shows from `eyes`, from 0 to 1: a ray at each of
/// its head, chest and knees, each counted in `rays`.
#[must_use]
pub fn showing<W>(level: &Level, eyes: [f32; 3], target: &Seeable<W>, rays: &mut u32) -> f32 {
    target
        .points()
        .iter()
        .zip(POINT_SHARES)
        .filter(|(point, _)| !blocked(level, eyes, **point, rays))
        .map(|(_, share)| share)
        .sum()
}

/// Whether the level hides `to` from `from`.
fn blocked(level: &Level, from: [f32; 3], to: [f32; 3], rays: &mut u32) -> bool {
    let way = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let long = distance(from, to);
    if long.is_nan() || long <= 0.05 {
        return false;
    }
    *rays = rays.saturating_add(1);
    // A hair short of the far end, so what stands on a floor is not hidden
    // by the floor it stands on.
    level
        .raycast(from, way, long - 0.05)
        .is_some_and(|hit| hit.distance < long - 0.05)
}

/// How much light helps eyes see someone `away` metres off where it is
/// `light` light (0 dark, 1 day), for eyes that see `night_sight` well in
/// the dark: 1 in full light, and anyone within a few metres is made out
/// whatever the dark.
#[must_use]
pub fn light_helps(light: f32, night_sight: f32, away: f32) -> f32 {
    let light = if light.is_finite() {
        light.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let night_sight = if night_sight.is_finite() {
        night_sight.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let seen = light + (1.0 - light) * night_sight;
    let close = (1.0 - away / CLOSE_IN_DARK).clamp(0.0, 1.0);
    seen + (1.0 - seen) * close
}

/// How light the open air is at `hour` (0 to 24) of the day, from
/// [`NIGHT`] to 1, under clouds covering `cloud_cover` (0 to 1) of the
/// sky. With no hour (a game with no day and night) it is always day.
#[must_use]
pub fn daylight(hour: Option<f64>, cloud_cover: f32) -> f32 {
    let Some(hour) = hour.filter(|hour| hour.is_finite()) else {
        return 1.0;
    };
    // How high the sun stands: up at six, highest at noon, down at six.
    let sun = libm::sin(std::f64::consts::PI * (hour - 6.0) / 12.0) as f32;
    let day = smoothstep(-0.1, 0.2, sun);
    let clouds = if cloud_cover.is_finite() {
        cloud_cover.clamp(0.0, 1.0)
    } else {
        0.0
    };
    NIGHT + (1.0 - NIGHT) * day * (1.0 - 0.4 * clouds)
}

/// How light a place is under `open` daylight with `lux` of lamplight
/// falling on it, from 0 to 1.
#[must_use]
pub fn lit(open: f32, lux: f32) -> f32 {
    let lamps = if lux.is_finite() && lux > 0.0 {
        1.0 - libm::expf(-lux / 20.0)
    } else {
        0.0
    };
    let open = open.clamp(0.0, 1.0);
    1.0 - (1.0 - open) * (1.0 - lamps)
}

/// The light, in lux, a lamp of `candela` at `lamp` throws on `at`: none
/// past its `reach` (0 for no end), and none outside its beam, if it has
/// one (`beam`: the way its middle points, one long, and the cosine of the
/// angle to its edge). Walls are not looked for: a lamp lights a place
/// whatever stands between.
#[must_use]
pub fn lamp_lux(
    at: [f32; 3],
    lamp: [f32; 3],
    candela: f32,
    reach: f32,
    beam: Option<([f32; 3], f32)>,
) -> f32 {
    let to = [at[0] - lamp[0], at[1] - lamp[1], at[2] - lamp[2]];
    let squared = to[0] * to[0] + to[1] * to[1] + to[2] * to[2];
    if reach > 0.0 && squared > reach * reach {
        return 0.0;
    }
    if let Some((pointing, edge)) = beam {
        let away = libm::sqrtf(squared);
        if away > 1e-4 {
            let along = (to[0] * pointing[0] + to[1] * pointing[1] + to[2] * pointing[2]) / away;
            if along < edge {
                return 0.0;
            }
        }
    }
    let lux = candela / squared.max(0.25);
    if lux.is_finite() { lux.max(0.0) } else { 0.0 }
}

/// How much of what is behind each metre of air is hidden in `fog` (0, the
/// haze of a clear day, to 1, a fog that hides what is forty metres off)
/// with `rain` falling (0 to 1): the same thickness the sky is drawn with.
#[must_use]
pub fn haze(fog: f32, rain: f32) -> f32 {
    let fog = if fog.is_finite() {
        fog.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let rain = if rain.is_finite() {
        rain.clamp(0.0, 1.0)
    } else {
        0.0
    };
    0.0008 * libm::powf(125.0, fog) + 0.01 * rain
}

fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    libm::sqrtf(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
}

#[cfg(test)]
mod tests;
