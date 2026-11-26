//! Who likes whom: the game's sides, how each feels about the others, and
//! what each person holds against someone for what they did.
//!
//! A game names its sides in one table. A feeling said one way holds both
//! ways unless the other side says its own: a beginner who writes "Town
//! counts Outlaws as enemies" expects the Outlaws to fight back. Someone
//! on no side is neutral to everyone, and everyone to them, except to a
//! side that named "everyone". Two people on one side are friends unless
//! the side says it is its own enemy.
//!
//! On top of its side, each person holds grudges: someone who hurt them,
//! or whom they saw hurt a friend, or whom a friend told them of, is their
//! enemy for a while ([`Temper::remembers`]) or until they forgive. A
//! friend's blows are forgiven up to a point ([`Temper::forgives`]): an
//! arrow that strays is not a war.
//!
//! Everything here is plain data, the same from the same calls: sides are
//! small numbers given in the order of their names, the feelings a small
//! square table, each person's grudges a few slots of fixed size. Nothing
//! is set aside per person per tick. It is part of the game's story state:
//! a save keeps a [`Sides`] whole.

use std::fmt;

/// A side, as a small number. [`Side::NONE`] is no side at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Side(u8);

impl Side {
    /// No side: neutral to everyone, unless a side named "everyone".
    pub const NONE: Self = Self(0);

    fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// How one person or side feels about another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Feeling {
    /// Someone to fight, or to run from.
    Enemy,
    /// Someone to leave alone.
    #[default]
    Neutral,
    /// Someone to help and not to hurt.
    Friend,
}

impl Feeling {
    /// The word a script reads and writes: "enemy", "neutral" or "friend".
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Enemy => "enemy",
            Self::Neutral => "neutral",
            Self::Friend => "friend",
        }
    }

    /// The feeling a script's word means, capitals ignored.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        let word = word.trim();
        [Self::Enemy, Self::Neutral, Self::Friend]
            .into_iter()
            .find(|feeling| feeling.word().eq_ignore_ascii_case(word))
    }
}

/// How a side's people take what is done to them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Temper {
    /// How many blows from a friend are forgiven, within
    /// [`FORGIVING_SECONDS`] of each other, before the next one makes them
    /// an enemy. 0 forgives none; [`FORGIVES_ALL`] forgives every blow from
    /// a friend. A blow that kills is never forgiven.
    pub forgives: u8,
    /// How long a grudge is held, in seconds, unless forgiven sooner.
    /// Infinite holds it until forgiven.
    pub remembers: f32,
}

impl Default for Temper {
    fn default() -> Self {
        Self {
            forgives: DEFAULT_FORGIVES,
            remembers: DEFAULT_REMEMBERS,
        }
    }
}

/// How many blows from a friend a side forgives, until the game says.
pub const DEFAULT_FORGIVES: u8 = 2;
/// [`Temper::forgives`] that forgives every blow from a friend but one
/// that kills: friendly fire is never held against anyone.
pub const FORGIVES_ALL: u8 = u8::MAX;
/// How long a grudge is held, in seconds, until the game says: five
/// minutes.
pub const DEFAULT_REMEMBERS: f32 = 300.0;
/// Blows from a friend further apart than this, in seconds, are counted
/// afresh: two stray arrows an hour apart are two accidents.
pub const FORGIVING_SECONDS: f32 = 30.0;
/// The most sides a game can have: more than a world needs.
pub const MOST_SIDES: usize = 63;
/// The most grudges one person holds at once. When all are taken, a new
/// one takes the place of the least important (see [`Sides::suffered`]).
pub const GRUDGES: usize = 4;
/// The longest name of a side, in characters.
pub const LONGEST_NAME: usize = 32;

/// Who a side's feeling is toward, as a game writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toward {
    /// Every other side, and everyone on no side.
    Everyone,
    /// The side of this name.
    Side(String),
}

/// One side as a game writes it in its table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SideSaid {
    /// What it is called.
    pub name: String,
    /// Who it counts as friends.
    pub friends: Vec<Toward>,
    /// Who it counts as enemies.
    pub enemies: Vec<Toward>,
    /// Who it says it leaves alone, which it would otherwise feel as the
    /// others feel about it.
    pub neutral: Vec<Toward>,
    /// How its people take what is done to them.
    pub temper: Temper,
}

/// What is wrong with a table of sides, in words a person can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidesError {
    /// A name with nothing in it, or too long.
    BadName(String),
    /// The same name twice, perhaps in other capitals.
    Twice(String),
    /// More sides than [`MOST_SIDES`].
    TooMany(usize),
    /// A side named in a feeling that the table does not have.
    Unknown {
        /// The side whose feeling names it.
        by: String,
        /// The name that is not a side.
        name: String,
        /// The sides there are.
        sides: Vec<String>,
    },
    /// A side said two feelings about one other side.
    Torn {
        /// The side that said both.
        by: String,
        /// The side it said them about.
        about: String,
    },
}

impl fmt::Display for SidesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName(name) => write!(
                f,
                "\"{name}\" cannot be a side's name: a name is 1 to {LONGEST_NAME} characters, such as \"Town\""
            ),
            Self::Twice(name) => write!(f, "the side \"{name}\" is named twice"),
            Self::TooMany(count) => write!(
                f,
                "it names {count} sides, and a game has at most {MOST_SIDES}"
            ),
            Self::Unknown { by, name, sides } => write!(
                f,
                "\"{by}\" names \"{name}\", which is not one of the sides; {}",
                listed(sides)
            ),
            Self::Torn { by, about } => write!(
                f,
                "\"{by}\" lists \"{about}\" twice (in both friends and enemies, say); keep it once, in one list"
            ),
        }
    }
}

impl std::error::Error for SidesError {}

/// The sides there are, for a sentence that says a name is not one.
#[must_use]
pub fn listed(sides: &[String]) -> String {
    let quoted: Vec<String> = sides.iter().map(|side| format!("\"{side}\"")).collect();
    match quoted.split_last() {
        None => {
            "the game has named no sides yet: name them first with game.sides { ... }".to_owned()
        }
        Some((only, [])) => format!("the only side is {only}"),
        Some((last, rest)) => format!("the sides are {} and {last}", rest.join(", ")),
    }
}

/// Something one person did to another, as someone saw it done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Act {
    /// `by` hurt `victim`, and the victim held it against them.
    Hurt {
        /// Who was hurt.
        victim: u32,
        /// Who hurt them.
        by: u32,
        /// Whether the blow took the last of the victim's health.
        killed: bool,
    },
}

/// What came of a blow for the one it hurt ([`Sides::suffered`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// Not held against anyone: a friend's blow that is forgiven, or one's
    /// own.
    Forgiven,
    /// Held: the one hurt counts whoever did it an enemy.
    Held,
    /// Minded, but not kept: every grudge the one hurt can hold is already
    /// for a killing, and only a killing takes the place of one of those.
    NoRoom,
}

impl Taken {
    /// Whether the blow was held against whoever dealt it, kept or not:
    /// when anyone who saw it minds too.
    #[must_use]
    pub fn minded(self) -> bool {
        self != Self::Forgiven
    }
}

/// Who the number `against` a grudge slot holds is when it holds nobody.
const NOBODY: u32 = u32::MAX;

/// What one person holds against another: blows counted toward a
/// grudge, or a grudge held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Grudge {
    against: u32,
    /// Blows from them counted while forgiving.
    hits: u8,
    /// The tick of the last of those blows.
    last_hit: u32,
    /// The tick the grudge ends on; 0 while none is held.
    until: u32,
    /// The tick it was taken up, by being hurt, seeing or being told: it is
    /// passed on only after, so word goes one person at a time.
    heard: u32,
    /// Whether it is for a killing: only another killing pushes it out.
    killing: bool,
}

impl Grudge {
    const EMPTY: Self = Self {
        against: NOBODY,
        hits: 0,
        last_hit: 0,
        until: 0,
        heard: 0,
        killing: false,
    };

    fn held(&self, now: u32) -> bool {
        self.against != NOBODY && self.until > now
    }

    /// Whether it was taken up as a grudge, worn off or not: what
    /// [`Sides::grudges`] counts until it is cleared away.
    fn taken(&self) -> bool {
        self.against != NOBODY && self.until > 0
    }

    /// How much it matters to keep, least first: what a full set of slots
    /// gives up for something new. Nothing at all (empty, worn off, or
    /// blows counted too long ago), then blows from a friend being
    /// counted, then a grudge not for a killing, then one for a killing;
    /// within each, the one that ends soonest, then the one taken up
    /// longest ago. (A grudge against someone who has gone down or left
    /// the world is not here to give up: it ended when they did.)
    fn worth(&self, now: u32, window: u32) -> (u8, u32, u32) {
        let rank = if self.held(now) {
            if self.killing { KILLING } else { GRUDGE }
        } else if self.against != NOBODY
            && self.until == 0
            && self.hits > 0
            && now.saturating_sub(self.last_hit) <= window
        {
            COUNTING
        } else {
            0
        };
        (rank, self.until, self.heard)
    }
}

/// The ranks of [`Grudge::worth`]. Something new may push out what ranks
/// no higher than itself.
const COUNTING: u8 = 1;
const GRUDGE: u8 = 2;
const KILLING: u8 = 3;

/// One person: their side, whether they are down, and what they hold
/// against whom.
#[derive(Debug, Clone, PartialEq)]
struct Person {
    who: u32,
    side: Side,
    /// Down (no health left) as the world last said: nobody takes up a
    /// grudge against them, and those held against them ended as they fell.
    down: bool,
    grudges: [Grudge; GRUDGES],
}

/// A feeling between two sides that changed: for the game to hear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeelingChange {
    /// The side whose feeling it is.
    pub side: Side,
    /// The side it is about.
    pub toward: Side,
    /// How it feels now.
    pub now: Feeling,
    /// How it felt before.
    pub before: Feeling,
}

/// The game's sides and the people on them: everything about who likes
/// whom. See the [module](self).
#[derive(Debug, Clone, PartialEq)]
pub struct Sides {
    /// The sides' names, in the order of their numbers: the first is no
    /// side and has no name.
    names: Vec<String>,
    /// How side `a` feels about side `b`, at `a * count + b`.
    feel: Vec<Feeling>,
    /// Whether side `a` said its own feeling about side `b`, which the
    /// other way round does not change.
    said: Vec<bool>,
    /// How each side's people take what is done to them.
    tempers: Vec<Temper>,
    /// Everyone on a side or holding a grudge, in the order of their ids.
    people: Vec<Person>,
    /// How many grudge slots are taken, worn off or not, across everyone:
    /// while none is, word has nothing to carry.
    held: u32,
    /// The tick it is, counted from 1, and how long a tick is in seconds.
    now: u32,
    tick_seconds: f32,
}

impl Default for Sides {
    fn default() -> Self {
        Self {
            names: vec![String::new()],
            feel: vec![Feeling::Neutral],
            said: vec![false],
            tempers: vec![Temper::default()],
            people: Vec::new(),
            held: 0,
            now: 0,
            tick_seconds: 1.0 / 60.0,
        }
    }
}

impl Sides {
    fn count(&self) -> usize {
        self.names.len()
    }

    fn at(&self, a: Side, b: Side) -> usize {
        a.index() * self.count() + b.index()
    }

    /// The sides' names, in the order of their numbers (which is the
    /// order of the names, capitals ignored).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().skip(1).map(String::as_str)
    }

    /// The side called `name`, capitals ignored.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<Side> {
        let name = name.trim();
        let index = (self.names.iter().skip(1)).position(|n| n.eq_ignore_ascii_case(name))?;
        u8::try_from(index + 1).ok().map(Side)
    }

    /// What a side is called: "" for no side.
    #[must_use]
    pub fn name(&self, side: Side) -> &str {
        self.names.get(side.index()).map_or("", String::as_str)
    }

    /// The sides' names, for a message that says a name is not one.
    #[must_use]
    pub fn listed(&self) -> String {
        listed(&self.names().map(str::to_owned).collect::<Vec<_>>())
    }

    /// Says which tick it is, counted from 1, and how long a tick is, in
    /// seconds: what grudges are timed by. The world says it each tick.
    pub fn set_time(&mut self, tick: u32, tick_seconds: f32) {
        self.now = tick;
        if tick_seconds.is_finite() && tick_seconds > 0.0 {
            self.tick_seconds = tick_seconds;
        }
    }

    /// Replaces the table with what `said` writes. People keep their side
    /// by its name; one whose side is no longer named is on none, and is
    /// given back so the game can be told. Nothing changes on an error.
    ///
    /// # Errors
    ///
    /// A name that is empty, too long or given twice, too many sides, a
    /// feeling about a side the table does not name, or two feelings about
    /// one side.
    pub fn declare(&mut self, said: &[SideSaid]) -> Result<Vec<(u32, String)>, SidesError> {
        self.declare_with(said, Temper::default())
    }

    /// [`declare`](Self::declare), with `everyone` the temper of people on
    /// no side (each side's own is in what it said).
    ///
    /// # Errors
    ///
    /// As [`declare`](Self::declare).
    pub fn declare_with(
        &mut self,
        said: &[SideSaid],
        everyone: Temper,
    ) -> Result<Vec<(u32, String)>, SidesError> {
        if said.len() > MOST_SIDES {
            return Err(SidesError::TooMany(said.len()));
        }
        let mut order: Vec<&SideSaid> = said.iter().collect();
        for side in &order {
            let name = side.name.trim();
            if name.is_empty() || name.chars().count() > LONGEST_NAME {
                return Err(SidesError::BadName(name.to_owned()));
            }
        }
        order.sort_by_key(|side| side.name.trim().to_ascii_lowercase());
        for pair in order.windows(2) {
            if pair[0]
                .name
                .trim()
                .eq_ignore_ascii_case(pair[1].name.trim())
            {
                return Err(SidesError::Twice(pair[1].name.trim().to_owned()));
            }
        }
        let mut next = Self {
            names: std::iter::once(String::new())
                .chain(order.iter().map(|side| side.name.trim().to_owned()))
                .collect(),
            people: Vec::new(),
            now: self.now,
            tick_seconds: self.tick_seconds,
            ..Self::default()
        };
        let count = next.count();
        next.feel = vec![Feeling::Neutral; count * count];
        next.said = vec![false; count * count];
        next.tempers = std::iter::once(everyone)
            .chain(order.iter().map(|side| side.temper))
            .collect();
        for index in 1..count {
            next.feel[index * count + index] = Feeling::Friend;
        }
        let sides: Vec<String> = next.names().map(str::to_owned).collect();
        // Each side's own feelings: "everyone" first, then those it names,
        // which win over it.
        for (index, side) in order.iter().enumerate() {
            let me = Side(u8::try_from(index + 1).unwrap_or(0));
            let lists = [
                (Feeling::Friend, &side.friends),
                (Feeling::Enemy, &side.enemies),
                (Feeling::Neutral, &side.neutral),
            ];
            let mut everyone = None;
            for (feeling, list) in lists {
                if list.contains(&Toward::Everyone) {
                    if everyone.is_some() {
                        return Err(SidesError::Torn {
                            by: side.name.trim().to_owned(),
                            about: "everyone".to_owned(),
                        });
                    }
                    everyone = Some(feeling);
                }
            }
            if let Some(feeling) = everyone {
                for other in 0..count {
                    if other != me.index() {
                        let at = me.index() * count + other;
                        next.feel[at] = feeling;
                        next.said[at] = true;
                    }
                }
            }
            let mut named = vec![false; count];
            for (feeling, list) in lists {
                for toward in list {
                    let Toward::Side(name) = toward else { continue };
                    let Some(other) = next.find(name) else {
                        return Err(SidesError::Unknown {
                            by: side.name.trim().to_owned(),
                            name: name.trim().to_owned(),
                            sides,
                        });
                    };
                    if std::mem::replace(&mut named[other.index()], true) {
                        return Err(SidesError::Torn {
                            by: side.name.trim().to_owned(),
                            about: next.name(other).to_owned(),
                        });
                    }
                    let at = next.at(me, other);
                    next.feel[at] = feeling;
                    next.said[at] = true;
                }
            }
        }
        // What one side said holds the other way round, unless that side
        // said its own.
        for a in 0..count {
            for b in 0..count {
                if next.said[a * count + b] && !next.said[b * count + a] {
                    next.feel[b * count + a] = next.feel[a * count + b];
                }
            }
        }
        // People keep their side by name.
        let mut lost = Vec::new();
        let mut people = std::mem::take(&mut self.people);
        for person in &mut people {
            let name = self.name(person.side);
            let side = next.find(name).unwrap_or(Side::NONE);
            if side == Side::NONE && person.side != Side::NONE {
                lost.push((person.who, name.to_owned()));
            }
            person.side = side;
        }
        next.people = people;
        next.held = self.held;
        *self = next;
        Ok(lost)
    }

    /// How side `side` feels about side `about`.
    #[must_use]
    pub fn between(&self, side: Side, about: Side) -> Feeling {
        if side.index() >= self.count() || about.index() >= self.count() {
            return Feeling::Neutral;
        }
        self.feel
            .get(self.at(side, about))
            .copied()
            .unwrap_or_default()
    }

    /// Sets how `side` feels about `about`, and the other way round unless
    /// `about` said its own feeling about `side`. Gives each feeling that
    /// changed, `side`'s first.
    pub fn set_between(
        &mut self,
        side: Side,
        about: Side,
        feeling: Feeling,
    ) -> [Option<FeelingChange>; 2] {
        let count = self.count();
        if side.index() >= count || about.index() >= count {
            return [None, None];
        }
        let mut changed = [None, None];
        let at = self.at(side, about);
        self.said[at] = true;
        let change = |sides: &mut Self, a: Side, b: Side| {
            let at = sides.at(a, b);
            let before = std::mem::replace(&mut sides.feel[at], feeling);
            (before != feeling).then_some(FeelingChange {
                side: a,
                toward: b,
                now: feeling,
                before,
            })
        };
        changed[0] = change(self, side, about);
        if side != about && !self.said[self.at(about, side)] {
            changed[1] = change(self, about, side);
        }
        changed
    }

    /// Sets how `side` feels about `about` and how `about` feels about
    /// `side`, both at once, whatever either said before: peace, or war,
    /// agreed by both. Gives each feeling that changed, `side`'s first.
    pub fn set_both(
        &mut self,
        side: Side,
        about: Side,
        feeling: Feeling,
    ) -> [Option<FeelingChange>; 2] {
        let count = self.count();
        if side.index() >= count || about.index() >= count {
            return [None, None];
        }
        let [first, back] = self.set_between(side, about, feeling);
        if side == about {
            return [first, None];
        }
        // The other way round may have changed already, if `about` had not
        // said its own; now it has.
        let [second, _] = self.set_between(about, side, feeling);
        [first, back.or(second)]
    }

    /// How a side's people take what is done to them.
    #[must_use]
    pub fn temper(&self, side: Side) -> Temper {
        self.tempers.get(side.index()).copied().unwrap_or_default()
    }

    fn person(&self, who: u32) -> Option<&Person> {
        let at = self.people.binary_search_by_key(&who, |p| p.who).ok()?;
        self.people.get(at)
    }

    /// Where `who` is in [`people`](Self::people), added on no side if
    /// they are not there yet.
    fn index_of(&mut self, who: u32) -> usize {
        match self.people.binary_search_by_key(&who, |p| p.who) {
            Ok(at) => at,
            Err(at) => {
                self.people.insert(
                    at,
                    Person {
                        who,
                        side: Side::NONE,
                        down: false,
                        grudges: [Grudge::EMPTY; GRUDGES],
                    },
                );
                at
            }
        }
    }

    /// Puts `grudge` in slot `slot` of person `at`, keeping
    /// [`held`](Self::held) counted.
    fn put(&mut self, at: usize, slot: usize, grudge: Grudge) {
        let old = std::mem::replace(&mut self.people[at].grudges[slot], grudge);
        self.held = (self.held + u32::from(grudge.taken())).saturating_sub(u32::from(old.taken()));
    }

    /// Empties every slot of person `at` that `clear` picks.
    fn clear_where(&mut self, at: usize, clear: impl Fn(&Grudge) -> bool) {
        for slot in 0..GRUDGES {
            if clear(&self.people[at].grudges[slot]) {
                self.put(at, slot, Grudge::EMPTY);
            }
        }
    }

    /// Empties every slot, of anyone, that holds something against
    /// `against`.
    fn clear_against(&mut self, against: u32) {
        if against == NOBODY {
            return;
        }
        for at in 0..self.people.len() {
            self.clear_where(at, |g| g.against == against);
        }
    }

    /// The side `who` is on.
    #[must_use]
    pub fn side_of(&self, who: u32) -> Side {
        self.person(who).map_or(Side::NONE, |p| p.side)
    }

    /// Puts `who` on `side` (or on none). What they hold against anyone
    /// they keep.
    pub fn set_side(&mut self, who: u32, side: Side) {
        if side.index() < self.count() {
            let at = self.index_of(who);
            self.people[at].side = side;
        }
    }

    /// Puts everyone on `side` on `onto` instead: one side joins another.
    /// Gives how many moved.
    pub fn join(&mut self, side: Side, onto: Side) -> usize {
        if side == onto || side == Side::NONE || onto.index() >= self.count() {
            return 0;
        }
        let mut moved = 0;
        for person in self.people.iter_mut().filter(|p| p.side == side) {
            person.side = onto;
            moved += 1;
        }
        moved
    }

    /// Forgets `who`, who has left the world: their side, their grudges,
    /// and every grudge anyone held against them.
    pub fn forget(&mut self, who: u32) {
        if let Ok(at) = self.people.binary_search_by_key(&who, |p| p.who) {
            self.clear_where(at, |_| true);
            self.people.remove(at);
        }
        self.clear_against(who);
    }

    /// Says whether `who` is down (no health left). Going down ends every
    /// grudge held against them, and none is taken up against them while
    /// they stay down; standing again, they start with a clean slate. What
    /// they hold against others they keep. The world says it of each NPC
    /// while anyone holds a grudge, and before what was done in a tick.
    pub fn set_down(&mut self, who: u32, down: bool) {
        let at = match self.people.binary_search_by_key(&who, |p| p.who) {
            Ok(at) => at,
            Err(_) if !down => return,
            Err(_) => self.index_of(who),
        };
        let was = std::mem::replace(&mut self.people[at].down, down);
        if down && !was {
            self.clear_against(who);
        }
    }

    fn is_down(&self, who: u32) -> bool {
        self.person(who).is_some_and(|p| p.down)
    }

    /// How many grudges are held across everyone (some may have worn off
    /// and not yet been cleared away). While it is 0 nobody has anything to
    /// tell, and [`tell_near`](Self::tell_near) costs nothing.
    #[must_use]
    pub fn grudges(&self) -> u32 {
        self.held
    }

    /// How `who` feels about `about`: an enemy if they hold a grudge
    /// against them, else as their sides feel. Everyone is their own
    /// friend.
    #[must_use]
    pub fn feels(&self, who: u32, about: u32) -> Feeling {
        if who == about {
            return Feeling::Friend;
        }
        let me = self.person(who);
        if me.is_some_and(|p| (p.grudges.iter()).any(|g| g.against == about && g.held(self.now))) {
            return Feeling::Enemy;
        }
        self.between(me.map_or(Side::NONE, |p| p.side), self.side_of(about))
    }

    /// Whether `them` is an enemy of `me`: on a side mine counts as
    /// enemies, or someone I hold a grudge against. What a rule's "an
    /// enemy" means.
    #[must_use]
    pub fn is_enemy(&self, me: u32, them: u32) -> bool {
        self.feels(me, them) == Feeling::Enemy
    }

    /// Whether `who` holds a grudge against `against` now.
    #[must_use]
    pub fn holds_grudge(&self, who: u32, against: u32) -> bool {
        self.person(who)
            .is_some_and(|p| (p.grudges.iter()).any(|g| g.against == against && g.held(self.now)))
    }

    /// How many ticks `seconds` is, at least one; infinite is for ever.
    fn ticks(&self, seconds: f32) -> u32 {
        if !seconds.is_finite() {
            return u32::MAX;
        }
        let ticks = (seconds.max(0.0) / self.tick_seconds).ceil();
        // Within u32 after the clamp.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ticks = ticks.clamp(1.0, u32::MAX as f32) as u32;
        ticks
    }

    /// The slot of person `at` for something new against `against` of rank
    /// `rank`: theirs if they have one, else the least worth keeping
    /// ([`Grudge::worth`]) if it ranks no higher than `rank`, else none.
    fn slot(&self, at: usize, against: u32, rank: u8) -> Option<usize> {
        let grudges = &self.people[at].grudges;
        if let Some(slot) = grudges.iter().position(|g| g.against == against) {
            return Some(slot);
        }
        let (now, window) = (self.now, self.ticks(FORGIVING_SECONDS));
        let least = (0..GRUDGES).min_by_key(|&slot| (grudges[slot].worth(now, window), slot))?;
        let worth = grudges[least].worth(now, window).0;
        (worth <= rank).then_some(least)
    }

    /// Takes up a grudge of `who` against `against` that lasts until
    /// `until`, heard now (one already held keeps the later end and a
    /// killing). Gives whether it is held: not against oneself or someone
    /// down, nor when every slot holds a grudge for a killing and this is
    /// not one.
    fn hold(&mut self, who: u32, against: u32, until: u32, killing: bool) -> bool {
        if who == against || against == NOBODY || self.is_down(against) {
            return false;
        }
        let at = self.index_of(who);
        let rank = if killing { KILLING } else { GRUDGE };
        let Some(slot) = self.slot(at, against, rank) else {
            return false;
        };
        let now = self.now;
        let old = self.people[at].grudges[slot];
        let mut grudge = Grudge {
            against,
            hits: 0,
            last_hit: now,
            until,
            heard: now,
            killing,
        };
        if old.against == against && old.held(now) {
            if old.until >= until && (old.killing || !killing) {
                // Held already, as long and as gravely: nothing new to tell.
                return true;
            }
            grudge.until = grudge.until.max(old.until);
            grudge.killing |= old.killing;
        }
        self.put(at, slot, grudge);
        true
    }

    /// `victim` was hurt by `by` (and `killed`, if it took the last of
    /// their health). Unless `by` is a friend whose blow is forgiven, the
    /// victim holds it against them.
    ///
    /// Each person holds at most [`GRUDGES`] grudges. When all are taken, a
    /// new one takes the place of the least important: first one that has
    /// worn off, then blows from a friend still being counted, then a
    /// grudge not for a killing, then one for a killing; among those alike,
    /// the one that ends soonest, then the one taken up longest ago. A
    /// grudge for a killing is pushed out only by another killing, so a
    /// crowd cannot make anyone forget a murder; when every slot holds a
    /// killing, a lesser grudge is minded but not kept ([`Taken::NoRoom`]).
    /// Grudges against someone who went down or left ended then
    /// ([`set_down`](Self::set_down), [`forget`](Self::forget)), so they
    /// fill no slot.
    pub fn suffered(&mut self, victim: u32, by: u32, killed: bool) -> Taken {
        if victim == by {
            return Taken::Forgiven;
        }
        let side = self.side_of(victim);
        let temper = self.temper(side);
        let friends = self.between(side, self.side_of(by)) == Feeling::Friend;
        let already = self.holds_grudge(victim, by);
        let until = self.now.saturating_add(self.ticks(temper.remembers));
        let held = |sides: &mut Self| {
            if sides.hold(victim, by, until, killed) {
                Taken::Held
            } else {
                Taken::NoRoom
            }
        };
        if !friends || killed || already || temper.forgives == 0 {
            return held(self);
        }
        if temper.forgives == FORGIVES_ALL {
            return Taken::Forgiven;
        }
        let (now, window) = (self.now, self.ticks(FORGIVING_SECONDS));
        let at = self.index_of(victim);
        let Some(slot) = self.slot(at, by, COUNTING) else {
            // Every slot holds a grudge, or blows being counted: this blow
            // is forgiven uncounted rather than one of those forgotten.
            return Taken::Forgiven;
        };
        let grudge = self.people[at].grudges[slot];
        let counted = if grudge.against == by && now.saturating_sub(grudge.last_hit) <= window {
            grudge.hits
        } else {
            0
        };
        if counted < temper.forgives {
            self.put(
                at,
                slot,
                Grudge {
                    against: by,
                    hits: counted + 1,
                    last_hit: now,
                    until: 0,
                    heard: now,
                    killing: false,
                },
            );
            return Taken::Forgiven;
        }
        held(self)
    }

    /// `witness` perceived `act`: if it was done to a friend of theirs,
    /// they hold it against whoever did it, as their side remembers. Gives
    /// whether they do. The world calls it for each person who saw
    /// something done.
    pub fn perceive(&mut self, witness: u32, act: Act) -> bool {
        match act {
            Act::Hurt { victim, by, killed } => {
                if witness == victim
                    || witness == by
                    || self.feels(witness, victim) != Feeling::Friend
                {
                    return false;
                }
                let temper = self.temper(self.side_of(witness));
                let until = self.now.saturating_add(self.ticks(temper.remembers));
                self.hold(witness, by, until, killed)
            }
        }
    }

    /// Each of `near` (who, and where they stand, in the order of their
    /// ids) tells everyone of their own side within `within` metres of
    /// them of the grudges they hold, as long as they last. What one heard
    /// this tick waits for the next telling, so word goes from person to
    /// person. People on no side tell nobody. While nobody holds a grudge
    /// ([`grudges`](Self::grudges) is 0) it does nothing at all; else it
    /// first clears away grudges that have worn off.
    pub fn tell_near(&mut self, near: &[(u32, [f32; 3])], within: f32) {
        if self.held == 0 {
            return;
        }
        let now = self.now;
        for at in 0..self.people.len() {
            self.clear_where(at, |g| g.taken() && !g.held(now));
        }
        let reach = within * within;
        for &(teller, at) in near {
            let Some(person) = self.person(teller) else {
                continue;
            };
            let (side, grudges) = (person.side, person.grudges);
            if side == Side::NONE {
                continue;
            }
            for grudge in grudges.iter().filter(|g| g.held(now) && g.heard < now) {
                for &(hearer, there) in near {
                    if hearer == teller || hearer == grudge.against || self.side_of(hearer) != side
                    {
                        continue;
                    }
                    let away: f32 = (0..3)
                        .map(|i| (there[i] - at[i]) * (there[i] - at[i]))
                        .sum();
                    if away <= reach {
                        self.hold(hearer, grudge.against, grudge.until, grudge.killing);
                    }
                }
            }
        }
    }

    /// `who` forgives `against`: the grudge and the blows counted go.
    pub fn forgive(&mut self, who: u32, against: u32) {
        if let Ok(at) = self.people.binary_search_by_key(&who, |p| p.who) {
            self.clear_where(at, |g| g.against == against);
        }
    }

    /// Everyone on `side` forgives `against`.
    pub fn forgive_all(&mut self, side: Side, against: u32) {
        for at in 0..self.people.len() {
            if self.people[at].side == side {
                self.clear_where(at, |g| g.against == against);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: u32 = 0;

    fn side(name: &str) -> SideSaid {
        SideSaid {
            name: name.to_owned(),
            ..SideSaid::default()
        }
    }

    fn named(names: &[&str]) -> Vec<Toward> {
        names
            .iter()
            .map(|n| Toward::Side((*n).to_owned()))
            .collect()
    }

    /// Town counts Guards as friends and Outlaws and Wolves as enemies;
    /// Wolves count everyone as enemies.
    fn town() -> Sides {
        let mut sides = Sides::default();
        sides
            .declare(&[
                SideSaid {
                    friends: named(&["Guards"]),
                    enemies: named(&["Outlaws", "Wolves"]),
                    ..side("Town")
                },
                side("Guards"),
                side("Outlaws"),
                SideSaid {
                    enemies: vec![Toward::Everyone],
                    ..side("Wolves")
                },
            ])
            .map_err(|e| e.to_string())
            .unwrap_or_else(|e| panic!("{e}"));
        sides.set_time(1, 1.0 / 60.0);
        sides
    }

    fn of(sides: &Sides, name: &str) -> Side {
        sides.find(name).unwrap_or_else(|| panic!("no side {name}"))
    }

    #[test]
    fn a_feeling_said_one_way_holds_both_ways() {
        let sides = town();
        let [t, g, o, w] = ["Town", "Guards", "Outlaws", "Wolves"].map(|n| of(&sides, n));
        assert_eq!(sides.between(t, g), Feeling::Friend);
        assert_eq!(sides.between(g, t), Feeling::Friend);
        assert_eq!(sides.between(o, t), Feeling::Enemy);
        assert_eq!(sides.between(g, o), Feeling::Neutral);
        assert_eq!(sides.between(w, g), Feeling::Enemy);
        assert_eq!(sides.between(g, w), Feeling::Enemy);
        // One's own side is a friend; the wolves' "everyone" is others.
        assert_eq!(sides.between(w, w), Feeling::Friend);
        assert_eq!(sides.between(o, o), Feeling::Friend);
    }

    #[test]
    fn a_side_that_says_its_own_feeling_keeps_it() {
        let mut sides = Sides::default();
        sides
            .declare(&[
                SideSaid {
                    enemies: vec![Toward::Everyone],
                    ..side("Wolves")
                },
                SideSaid {
                    neutral: named(&["Wolves"]),
                    ..side("Deer")
                },
            ])
            .unwrap_or_else(|e| panic!("{e}"));
        let (w, d) = (of(&sides, "Wolves"), of(&sides, "Deer"));
        assert_eq!(sides.between(w, d), Feeling::Enemy);
        assert_eq!(sides.between(d, w), Feeling::Neutral);
    }

    #[test]
    fn someone_on_no_side_is_neutral_except_to_everyone_haters() {
        let mut sides = town();
        let (villager, stranger, wolf) = (1, 2, 3);
        sides.set_side(villager, of(&sides, "Town"));
        sides.set_side(wolf, of(&sides, "Wolves"));
        assert_eq!(sides.feels(villager, stranger), Feeling::Neutral);
        assert_eq!(sides.feels(stranger, villager), Feeling::Neutral);
        assert_eq!(sides.feels(stranger, 99), Feeling::Neutral);
        assert_eq!(sides.feels(wolf, stranger), Feeling::Enemy);
        assert_eq!(sides.feels(stranger, wolf), Feeling::Enemy);
        assert_eq!(sides.feels(wolf, wolf), Feeling::Friend);
    }

    #[test]
    fn an_unknown_side_is_an_error_that_lists_the_sides() {
        let mut sides = town();
        let before = sides.clone();
        let error = sides
            .declare(&[
                SideSaid {
                    enemies: named(&["Outlaw"]),
                    ..side("Town")
                },
                side("Outlaws"),
            ])
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert_eq!(
            error,
            "\"Town\" names \"Outlaw\", which is not one of the sides; the sides are \
             \"Outlaws\" and \"Town\""
        );
        assert_eq!(sides, before, "nothing changes on an error");
        assert_eq!(
            Sides::default().declare(&[side("A"), side("a")]),
            Err(SidesError::Twice("a".to_owned()))
        );
        assert!(matches!(
            Sides::default().declare(&[
                SideSaid {
                    friends: named(&["B"]),
                    enemies: named(&["b"]),
                    ..side("A")
                },
                side("B")
            ]),
            Err(SidesError::Torn { .. })
        ));
        assert_eq!(
            listed(&[]),
            "the game has named no sides yet: name them first with game.sides { ... }"
        );
    }

    #[test]
    fn sides_are_numbered_in_the_order_of_their_names() {
        let mut a = Sides::default();
        let mut b = Sides::default();
        a.declare(&[side("Wolves"), side("Town"), side("guards")])
            .unwrap_or_else(|e| panic!("{e}"));
        b.declare(&[side("guards"), side("Wolves"), side("Town")])
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(a, b);
        assert_eq!(a.names().collect::<Vec<_>>(), ["guards", "Town", "Wolves"]);
    }

    #[test]
    fn changes_hold_both_ways_and_say_what_changed() {
        let mut sides = town();
        let (t, o) = (of(&sides, "Town"), of(&sides, "Outlaws"));
        let changed = sides.set_between(t, o, Feeling::Friend);
        assert_eq!(
            changed,
            [
                Some(FeelingChange {
                    side: t,
                    toward: o,
                    now: Feeling::Friend,
                    before: Feeling::Enemy
                }),
                Some(FeelingChange {
                    side: o,
                    toward: t,
                    now: Feeling::Friend,
                    before: Feeling::Enemy
                })
            ]
        );
        assert_eq!(sides.set_between(t, o, Feeling::Friend), [None, None]);
        // Once the outlaws say their own, the town's change is one way.
        sides.set_between(o, t, Feeling::Enemy);
        let changed = sides.set_between(t, o, Feeling::Neutral);
        assert!(changed[0].is_some() && changed[1].is_none());
        assert_eq!(sides.between(o, t), Feeling::Enemy);
    }

    #[test]
    fn a_side_joins_another_and_people_keep_their_side_by_name() {
        let mut sides = town();
        sides.set_side(5, of(&sides, "Outlaws"));
        sides.set_side(6, of(&sides, "Wolves"));
        assert_eq!(sides.join(of(&sides, "Outlaws"), of(&sides, "Town")), 1);
        assert_eq!(sides.side_of(5), of(&sides, "Town"));
        // A new table: the wolves are gone, the town stays.
        let lost = sides
            .declare(&[side("Town"), side("Bandits")])
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(lost, [(6, "Wolves".to_owned())]);
        assert_eq!(sides.name(sides.side_of(5)), "Town");
        assert_eq!(sides.side_of(6), Side::NONE);
    }

    #[test]
    fn whoever_hurts_someone_is_their_enemy_until_it_wears_off_or_is_forgiven() {
        let mut sides = town();
        let (villager, stranger) = (1, 2);
        sides.set_side(villager, of(&sides, "Town"));
        assert_eq!(sides.suffered(villager, stranger, false), Taken::Held);
        assert!(sides.is_enemy(villager, stranger));
        // Their feeling is theirs alone: the stranger feels as before.
        assert_eq!(sides.feels(stranger, villager), Feeling::Neutral);
        // Five minutes on, it has worn off.
        sides.set_time(1 + 300 * 60, 1.0 / 60.0);
        assert!(!sides.is_enemy(villager, stranger));
        sides.suffered(villager, stranger, false);
        sides.forgive(villager, stranger);
        assert!(!sides.is_enemy(villager, stranger));
    }

    #[test]
    fn a_friends_stray_blows_are_forgiven_up_to_a_point() {
        let mut sides = town();
        let (villager, guard) = (1, 2);
        sides.set_side(villager, of(&sides, "Town"));
        sides.set_side(guard, of(&sides, "Guards"));
        assert_eq!(sides.suffered(villager, guard, false), Taken::Forgiven);
        assert_eq!(sides.suffered(villager, guard, false), Taken::Forgiven);
        assert_eq!(sides.feels(villager, guard), Feeling::Friend);
        assert_eq!(
            sides.suffered(villager, guard, false),
            Taken::Held,
            "the third counts"
        );
        assert!(sides.is_enemy(villager, guard));
        // Blows far apart are counted afresh.
        sides.forgive(villager, guard);
        for tick in [100, 100 + 31 * 60, 100 + 62 * 60] {
            sides.set_time(tick, 1.0 / 60.0);
            assert_eq!(sides.suffered(villager, guard, false), Taken::Forgiven);
        }
        // A killing blow is never forgiven.
        assert_eq!(sides.suffered(villager, guard, true), Taken::Held);
    }

    #[test]
    fn forgiving_can_be_turned_off() {
        let mut sides = Sides::default();
        sides
            .declare(&[SideSaid {
                temper: Temper {
                    forgives: 0,
                    remembers: f32::INFINITY,
                },
                ..side("Town")
            }])
            .unwrap_or_else(|e| panic!("{e}"));
        let town = of(&sides, "Town");
        sides.set_side(1, town);
        sides.set_side(2, town);
        assert_eq!(sides.suffered(1, 2, false), Taken::Held);
        sides.set_time(u32::MAX - 1, 1.0 / 60.0);
        assert!(sides.is_enemy(1, 2), "held until forgiven");
    }

    #[test]
    fn a_witness_minds_only_what_is_done_to_a_friend() {
        let mut sides = town();
        let (villager, guard, outlaw) = (1, 2, 3);
        sides.set_side(villager, of(&sides, "Town"));
        sides.set_side(guard, of(&sides, "Guards"));
        sides.set_side(outlaw, of(&sides, "Outlaws"));
        sides.perceive(
            guard,
            Act::Hurt {
                victim: villager,
                by: PLAYER,
                killed: false,
            },
        );
        assert!(sides.is_enemy(guard, PLAYER));
        sides.forgive(guard, PLAYER);
        sides.perceive(
            guard,
            Act::Hurt {
                victim: outlaw,
                by: PLAYER,
                killed: false,
            },
        );
        assert!(!sides.is_enemy(guard, PLAYER));
    }

    #[test]
    fn word_goes_from_person_to_person_one_telling_at_a_time() {
        let mut sides = town();
        let town_side = of(&sides, "Town");
        // Three villagers in a row, two metres apart, and an outlaw who
        // stands among them but is not told.
        let (a, b, c, outlaw) = (1, 2, 3, 4);
        for who in [a, b, c] {
            sides.set_side(who, town_side);
        }
        sides.set_side(outlaw, of(&sides, "Outlaws"));
        let near = [
            (a, [0.0, 0.0, 0.0]),
            (b, [2.0, 0.0, 0.0]),
            (c, [4.0, 0.0, 0.0]),
            (outlaw, [1.0, 0.0, 0.0]),
        ];
        assert_eq!(sides.suffered(a, PLAYER, false), Taken::Held);
        // Heard this tick: not told yet.
        sides.tell_near(&near, 3.0);
        assert!(!sides.is_enemy(b, PLAYER));
        sides.set_time(2, 1.0 / 60.0);
        sides.tell_near(&near, 3.0);
        assert!(sides.is_enemy(b, PLAYER));
        assert!(!sides.is_enemy(c, PLAYER), "one person a telling");
        assert!(!sides.is_enemy(outlaw, PLAYER));
        sides.set_time(3, 1.0 / 60.0);
        sides.tell_near(&near, 3.0);
        assert!(sides.is_enemy(c, PLAYER));
        // What is told ends when the first grudge would.
        sides.set_time(2 + 300 * 60, 1.0 / 60.0);
        assert!(!sides.is_enemy(c, PLAYER));
    }

    #[test]
    fn a_person_holds_a_few_grudges_and_the_first_to_end_goes() {
        let mut sides = town();
        for (tick, against) in (10..).zip(10..10 + GRUDGES as u32 + 1) {
            sides.set_time(tick, 1.0 / 60.0);
            assert_eq!(sides.suffered(1, against, false), Taken::Held);
        }
        assert!(!sides.is_enemy(1, 10), "the oldest went");
        assert!((11..10 + GRUDGES as u32 + 1).all(|who| sides.is_enemy(1, who)));
    }

    #[test]
    fn a_victim_who_holds_for_ever_still_counts_a_new_attacker_an_enemy() {
        let mut sides = Sides::default();
        sides
            .declare(&[SideSaid {
                temper: Temper {
                    remembers: f32::INFINITY,
                    ..Temper::default()
                },
                ..side("Town")
            }])
            .unwrap_or_else(|e| panic!("{e}"));
        sides.set_time(1, 1.0 / 60.0);
        sides.set_side(1, of(&sides, "Town"));
        for attacker in 10..10 + GRUDGES as u32 {
            assert_eq!(sides.suffered(1, attacker, false), Taken::Held);
        }
        // Four held for ever; a fifth still takes a place, truthfully.
        assert_eq!(sides.suffered(1, PLAYER, false), Taken::Held);
        assert!(sides.is_enemy(1, PLAYER));
        assert!(!sides.is_enemy(1, 10), "the one taken up longest ago went");
    }

    #[test]
    fn a_grudge_for_a_killing_is_pushed_out_only_by_another_killing() {
        let mut sides = town();
        let (villager, guard, witness) = (1, 2, 3);
        for who in [villager, witness] {
            sides.set_side(who, of(&sides, "Town"));
        }
        sides.set_side(guard, of(&sides, "Guards"));
        // The player kills a villager in front of a witness...
        assert_eq!(sides.suffered(villager, PLAYER, true), Taken::Held);
        assert!(sides.perceive(
            witness,
            Act::Hurt {
                victim: villager,
                by: PLAYER,
                killed: true
            }
        ));
        // ...who is then hurt by a crowd of others, later and later.
        for (tick, attacker) in (20..).zip(10..10 + 2 * GRUDGES as u32) {
            sides.set_time(tick, 1.0 / 60.0);
            assert_eq!(sides.suffered(witness, attacker, false), Taken::Held);
        }
        assert!(
            sides.is_enemy(witness, PLAYER),
            "a crowd cannot wipe a murder"
        );
        // Four killings fill every place: a lesser grudge is minded, not kept.
        for (tick, killer) in (40..).zip(30..30 + GRUDGES as u32 - 1) {
            sides.set_time(tick, 1.0 / 60.0);
            assert!(sides.perceive(
                witness,
                Act::Hurt {
                    victim: villager,
                    by: killer,
                    killed: true
                }
            ));
        }
        assert_eq!(sides.suffered(witness, 99, false), Taken::NoRoom);
        assert!(Taken::NoRoom.minded());
        assert!(!sides.is_enemy(witness, 99));
        assert!(sides.is_enemy(witness, PLAYER));
        // A fifth killing gives up the oldest killing: the player's.
        sides.set_time(60, 1.0 / 60.0);
        assert_eq!(sides.suffered(witness, 98, true), Taken::Held);
        assert!(!sides.is_enemy(witness, PLAYER));
        assert!(sides.is_enemy(witness, 98));
    }

    #[test]
    fn grudges_against_the_fallen_and_the_gone_end_and_fill_no_place() {
        let mut sides = town();
        let guard = 1;
        sides.set_side(guard, of(&sides, "Guards"));
        let wolves = 10..10 + GRUDGES as u32;
        for wolf in wolves.clone() {
            sides.suffered(guard, wolf, false);
        }
        assert_eq!(sides.grudges(), GRUDGES as u32);
        // Two wolves fall, one is removed from the world.
        sides.set_down(10, true);
        sides.set_down(11, true);
        sides.forget(12);
        assert!(!sides.is_enemy(guard, 10) && !sides.is_enemy(guard, 12));
        assert_eq!(sides.grudges(), 1);
        // A fallen wolf is no grudge to take up, and its own are kept.
        assert_eq!(sides.suffered(guard, 10, false), Taken::NoRoom);
        // Room for three new ones beside the wolf still standing.
        for attacker in 20..23 {
            assert_eq!(sides.suffered(guard, attacker, false), Taken::Held);
        }
        assert!(sides.is_enemy(guard, 13));
        // Standing again, a wolf starts with a clean slate.
        sides.set_down(10, false);
        assert!(!sides.is_enemy(guard, 10));
        // Forgetting someone takes what they held, and what was held
        // against them, out of the count.
        sides.forget(guard);
        assert_eq!(sides.grudges(), 0);
    }

    #[test]
    fn worn_off_grudges_are_cleared_and_a_crowd_with_no_quarrel_tells_nothing() {
        let mut sides = town();
        sides.set_side(1, of(&sides, "Town"));
        sides.set_side(2, of(&sides, "Town"));
        let near = [(1, [0.0; 3]), (2, [1.0, 0.0, 0.0])];
        sides.tell_near(&near, 3.0);
        assert_eq!(sides.grudges(), 0);
        sides.suffered(1, PLAYER, false);
        sides.set_time(2, 1.0 / 60.0);
        sides.tell_near(&near, 3.0);
        assert_eq!(sides.grudges(), 2);
        sides.set_time(10 + 300 * 60, 1.0 / 60.0);
        sides.tell_near(&near, 3.0);
        assert_eq!(sides.grudges(), 0, "worn off and cleared away");
    }

    #[test]
    fn friendly_fire_can_be_forgiven_always() {
        let mut sides = Sides::default();
        sides
            .declare_with(
                &[side("Town")],
                Temper {
                    forgives: FORGIVES_ALL,
                    ..Temper::default()
                },
            )
            .unwrap_or_else(|e| panic!("{e}"));
        let town = of(&sides, "Town");
        sides.set_side(1, town);
        sides.set_side(2, town);
        // The side keeps its own temper; people on no side take everyone's.
        assert_eq!(sides.temper(Side::NONE).forgives, FORGIVES_ALL);
        let mut sides_all = Sides::default();
        sides_all
            .declare(&[SideSaid {
                temper: Temper {
                    forgives: FORGIVES_ALL,
                    ..Temper::default()
                },
                ..side("Town")
            }])
            .unwrap_or_else(|e| panic!("{e}"));
        sides_all.set_side(1, town);
        sides_all.set_side(2, town);
        for _ in 0..300 {
            assert_eq!(sides_all.suffered(1, 2, false), Taken::Forgiven);
        }
        assert!(!sides_all.is_enemy(1, 2));
        assert_eq!(sides_all.suffered(1, 2, true), Taken::Held, "not a killing");
    }

    #[test]
    fn peace_agreed_by_both_holds_both_ways_whatever_either_said() {
        let mut sides = town();
        let (t, o) = (of(&sides, "Town"), of(&sides, "Outlaws"));
        // The town said its own; the outlaws making friends is one way.
        sides.set_between(o, t, Feeling::Friend);
        assert_eq!(sides.between(t, o), Feeling::Enemy);
        let changed = sides.set_both(o, t, Feeling::Friend);
        assert_eq!(changed[0], None, "the outlaws already felt so");
        assert!(changed[1].is_some_and(|c| c.side == t && c.before == Feeling::Enemy));
        assert_eq!(sides.between(t, o), Feeling::Friend);
        assert_eq!(sides.between(o, t), Feeling::Friend);
        // Both say it now: a later one-way change stays one way.
        sides.set_between(t, o, Feeling::Neutral);
        assert_eq!(sides.between(o, t), Feeling::Friend);
    }

    #[test]
    fn a_side_forgives_as_one() {
        let mut sides = town();
        let town_side = of(&sides, "Town");
        for who in [1, 2] {
            sides.set_side(who, town_side);
            sides.suffered(who, PLAYER, false);
        }
        sides.forgive_all(town_side, PLAYER);
        assert!(!sides.is_enemy(1, PLAYER) && !sides.is_enemy(2, PLAYER));
    }

    #[test]
    fn feelings_have_words() {
        for feeling in [Feeling::Enemy, Feeling::Neutral, Feeling::Friend] {
            assert_eq!(Feeling::from_word(feeling.word()), Some(feeling));
        }
        assert_eq!(Feeling::from_word(" Enemy "), Some(Feeling::Enemy));
        assert_eq!(Feeling::from_word("foe"), None);
    }
}
