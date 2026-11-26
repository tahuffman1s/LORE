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
    /// an enemy. 0 forgives none. A blow that kills is never forgiven.
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
/// How long a grudge is held, in seconds, until the game says: five
/// minutes.
pub const DEFAULT_REMEMBERS: f32 = 300.0;
/// Blows from a friend further apart than this, in seconds, are counted
/// afresh: two stray arrows an hour apart are two accidents.
pub const FORGIVING_SECONDS: f32 = 30.0;
/// The most sides a game can have: more than a world needs.
pub const MOST_SIDES: usize = 63;
/// The most grudges one person holds at once. A new one pushes out the
/// one that would end first.
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
    },
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
}

impl Grudge {
    const EMPTY: Self = Self {
        against: NOBODY,
        hits: 0,
        last_hit: 0,
        until: 0,
        heard: 0,
    };

    fn held(&self, now: u32) -> bool {
        self.against != NOBODY && self.until > now
    }
}

/// One person: their side and what they hold against whom.
#[derive(Debug, Clone, PartialEq)]
struct Person {
    who: u32,
    side: Side,
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
        next.tempers = std::iter::once(Temper::default())
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

    /// How a side's people take what is done to them.
    #[must_use]
    pub fn temper(&self, side: Side) -> Temper {
        self.tempers.get(side.index()).copied().unwrap_or_default()
    }

    fn person(&self, who: u32) -> Option<&Person> {
        let at = self.people.binary_search_by_key(&who, |p| p.who).ok()?;
        self.people.get(at)
    }

    fn person_mut(&mut self, who: u32) -> &mut Person {
        let at = match self.people.binary_search_by_key(&who, |p| p.who) {
            Ok(at) => at,
            Err(at) => {
                self.people.insert(
                    at,
                    Person {
                        who,
                        side: Side::NONE,
                        grudges: [Grudge::EMPTY; GRUDGES],
                    },
                );
                at
            }
        };
        &mut self.people[at]
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
            self.person_mut(who).side = side;
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

    /// Forgets `who`, who has left the world: their side and grudges.
    pub fn forget(&mut self, who: u32) {
        self.people.retain(|person| person.who != who);
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

    /// The slot of `who`'s grudges for `against`: theirs if they have one,
    /// else an empty or finished one, else the one that ends first.
    fn slot(grudges: &[Grudge; GRUDGES], against: u32, now: u32, window: u32) -> usize {
        let live =
            |g: &Grudge| g.held(now) || (g.hits > 0 && now.saturating_sub(g.last_hit) <= window);
        (grudges.iter().position(|g| g.against == against))
            .or_else(|| grudges.iter().position(|g| g.against == NOBODY || !live(g)))
            .unwrap_or_else(|| {
                (0..GRUDGES)
                    .min_by_key(|&i| (grudges[i].until, i))
                    .unwrap_or(0)
            })
    }

    /// Takes up a grudge of `who` against `against` that lasts until
    /// `until`, heard now, unless one held already lasts as long.
    fn hold(&mut self, who: u32, against: u32, until: u32) {
        if who == against {
            return;
        }
        let (now, window) = (self.now, self.ticks(FORGIVING_SECONDS));
        let grudges = &mut self.person_mut(who).grudges;
        let at = Self::slot(grudges, against, now, window);
        let grudge = &mut grudges[at];
        if grudge.against == against && grudge.held(now) && grudge.until >= until {
            return;
        }
        if grudge.against != against && grudge.held(now) && grudge.until >= until {
            // Every slot holds a grudge that outlasts this one.
            return;
        }
        *grudge = Grudge {
            against,
            hits: 0,
            last_hit: now,
            until,
            heard: now,
        };
    }

    /// `victim` was hurt by `by` (and `killed`, if it took the last of
    /// their health). Unless `by` is a friend whose blow is forgiven, the
    /// victim holds it against them: gives whether they do, which is when
    /// anyone who saw it minds ([`perceive`](Self::perceive)).
    pub fn suffered(&mut self, victim: u32, by: u32, killed: bool) -> bool {
        if victim == by {
            return false;
        }
        let side = self.side_of(victim);
        let temper = self.temper(side);
        let friends = self.between(side, self.side_of(by)) == Feeling::Friend;
        let already = self.holds_grudge(victim, by);
        let until = self.now.saturating_add(self.ticks(temper.remembers));
        if !friends || killed || already || temper.forgives == 0 {
            self.hold(victim, by, until);
            return true;
        }
        let (now, window) = (self.now, self.ticks(FORGIVING_SECONDS));
        let grudges = &mut self.person_mut(victim).grudges;
        let at = Self::slot(grudges, by, now, window);
        let grudge = &mut grudges[at];
        if grudge.against != by && grudge.held(now) {
            // Every slot holds a grudge against someone else: the blow is
            // forgiven uncounted rather than one of those forgotten.
            return false;
        }
        let counted = if grudge.against == by && now.saturating_sub(grudge.last_hit) <= window {
            grudge.hits
        } else {
            0
        };
        if counted < temper.forgives {
            *grudge = Grudge {
                against: by,
                hits: counted + 1,
                last_hit: now,
                until: 0,
                heard: now,
            };
            return false;
        }
        self.hold(victim, by, until);
        true
    }

    /// `witness` perceived `act`: if it was done to a friend of theirs,
    /// they hold it against whoever did it, as their side remembers. The
    /// world calls it for each person who saw something done.
    pub fn perceive(&mut self, witness: u32, act: Act) {
        match act {
            Act::Hurt { victim, by } => {
                if witness == victim
                    || witness == by
                    || self.feels(witness, victim) != Feeling::Friend
                {
                    return;
                }
                let temper = self.temper(self.side_of(witness));
                let until = self.now.saturating_add(self.ticks(temper.remembers));
                self.hold(witness, by, until);
            }
        }
    }

    /// Each of `near` (who, and where they stand, in the order of their
    /// ids) tells everyone of their own side within `within` metres of
    /// them of the grudges they hold, as long as they last. What one heard
    /// this tick waits for the next telling, so word goes from person to
    /// person. People on no side tell nobody.
    pub fn tell_near(&mut self, near: &[(u32, [f32; 3])], within: f32) {
        let now = self.now;
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
                        self.hold(hearer, grudge.against, grudge.until);
                    }
                }
            }
        }
    }

    /// `who` forgives `against`: the grudge and the blows counted go.
    pub fn forgive(&mut self, who: u32, against: u32) {
        let Ok(at) = self.people.binary_search_by_key(&who, |p| p.who) else {
            return;
        };
        for grudge in &mut self.people[at].grudges {
            if grudge.against == against {
                *grudge = Grudge::EMPTY;
            }
        }
    }

    /// Everyone on `side` forgives `against`.
    pub fn forgive_all(&mut self, side: Side, against: u32) {
        for person in self.people.iter_mut().filter(|p| p.side == side) {
            for grudge in &mut person.grudges {
                if grudge.against == against {
                    *grudge = Grudge::EMPTY;
                }
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
        assert!(sides.suffered(villager, stranger, false));
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
        assert!(!sides.suffered(villager, guard, false));
        assert!(!sides.suffered(villager, guard, false));
        assert_eq!(sides.feels(villager, guard), Feeling::Friend);
        assert!(sides.suffered(villager, guard, false), "the third counts");
        assert!(sides.is_enemy(villager, guard));
        // Blows far apart are counted afresh.
        sides.forgive(villager, guard);
        for tick in [100, 100 + 31 * 60, 100 + 62 * 60] {
            sides.set_time(tick, 1.0 / 60.0);
            assert!(!sides.suffered(villager, guard, false));
        }
        // A killing blow is never forgiven.
        assert!(sides.suffered(villager, guard, true));
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
        assert!(sides.suffered(1, 2, false));
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
            },
        );
        assert!(sides.is_enemy(guard, PLAYER));
        sides.forgive(guard, PLAYER);
        sides.perceive(
            guard,
            Act::Hurt {
                victim: outlaw,
                by: PLAYER,
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
        assert!(sides.suffered(a, PLAYER, false));
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
            sides.suffered(1, against, false);
        }
        assert!(!sides.is_enemy(1, 10), "the oldest went");
        assert!((11..10 + GRUDGES as u32 + 1).all(|who| sides.is_enemy(1, who)));
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
