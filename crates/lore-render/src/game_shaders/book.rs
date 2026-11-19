//! What the game wants drawn against what is drawn, with no graphics card.
//!
//! The game gives its whole list of shaders every frame (or on change). The
//! book matches it to what it already knows, says which texts must be built
//! (`asks`), takes what was built (`done`) and keeps the last version that
//! worked for each entry.

use std::mem;

use lore_shader::{Hook, ParamBlock, ParamValue, ShaderError};

use super::worker::Done;
use super::{GameShader, MOST_SCREEN_EFFECTS, MOST_SURFACE_SHADERS, ShaderReport, ShaderUse};

/// One text to build.
pub(crate) struct Job {
    /// The entry it is for.
    pub(crate) id: u64,
    /// The revision of the text.
    pub(crate) revision: u64,
    /// What the shader is used for.
    pub(crate) hook: Hook,
    /// The file as errors show it.
    pub(crate) file: String,
    /// The WGSL.
    pub(crate) text: String,
}

/// A version that was built and is drawn.
pub(crate) struct Live<T> {
    /// What was made from it.
    pub(crate) built: T,
    /// The block that carries its values.
    pub(crate) block: ParamBlock,
    /// The revision of the text it was made from.
    pub(crate) revision: u64,
}

/// One shader the game wants.
pub(crate) struct Entry<T> {
    id: u64,
    hook: Hook,
    name: String,
    file: String,
    material: Option<String>,
    values: Vec<(String, ParamValue)>,
    /// The revision the game wants, and its text until it is asked for.
    wanted: u64,
    text: String,
    /// The revision last given to the worker.
    asked: Option<u64>,
    live: Option<Live<T>>,
    /// Over the limit: said so once, and never asked for or drawn.
    refused: bool,
    /// Where in the game's list it was last time, and which call saw it.
    position: usize,
    seen_in: u64,
}

impl<T> Entry<T> {
    /// The game's name for it.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The level's material a surface shader is for.
    pub(crate) fn material(&self) -> Option<&str> {
        self.material.as_deref()
    }

    /// The entry's id, which is never used for another.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// The version that is drawn, if there is one.
    pub(crate) fn live(&self) -> Option<&Live<T>> {
        self.live.as_ref()
    }

    /// The values the game set, for `ParamBlock::write`.
    pub(crate) fn values(&self) -> impl Iterator<Item = (&str, ParamValue)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), *value))
    }

    /// Whether this is the game's shader `shader`.
    fn is(&self, hook: Hook, name: &str, material: Option<&str>) -> bool {
        self.hook == hook && self.name == name && self.material.as_deref() == material
    }
}

/// The list of shaders the game wants, in the game's order.
pub(crate) struct Book<T> {
    entries: Vec<Entry<T>>,
    next_id: u64,
    /// How many times `want` was called, to tell which entries it saw.
    calls: u64,
    reports: Vec<ShaderReport>,
    /// Whether which entries are live, or their order, changed.
    changed: bool,
}

impl<T> Default for Book<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Book<T> {
    /// A book with nothing in it.
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 0,
            calls: 0,
            reports: Vec::new(),
            changed: false,
        }
    }

    /// Takes the game's whole list. An entry is the same one as last time
    /// when its kind, name and material are the same (duplicates are matched
    /// in order). A changed text is kept to be asked for; changed values are
    /// copied in place. What is no longer in the list is removed: its id goes
    /// to `forget` and what was built for it to `retire`.
    ///
    /// Allocates only for a new entry, a changed text or a changed set of
    /// value names.
    pub(crate) fn want(
        &mut self,
        shaders: &[GameShader<'_>],
        mut forget: impl FnMut(u64),
        mut retire: impl FnMut(T),
    ) {
        self.calls += 1;
        let calls = self.calls;
        let (mut screens, mut surfaces) = (0, 0);
        for (position, shader) in shaders.iter().enumerate() {
            let (hook, material) = match shader.used_as {
                ShaderUse::Screen => (Hook::Screen, None),
                ShaderUse::Surface { material } => (Hook::Surface, Some(material)),
            };
            let at =
                match self.entries.iter().position(|entry| {
                    entry.seen_in != calls && entry.is(hook, shader.name, material)
                }) {
                    Some(at) => at,
                    None => {
                        self.next_id += 1;
                        self.entries.push(Entry {
                            id: self.next_id,
                            hook,
                            name: shader.name.to_owned(),
                            file: shader.file.to_owned(),
                            material: material.map(str::to_owned),
                            values: Vec::new(),
                            wanted: shader.revision,
                            text: shader.text.to_owned(),
                            asked: None,
                            live: None,
                            refused: false,
                            position,
                            seen_in: calls,
                        });
                        self.entries.len() - 1
                    }
                };
            let entry = &mut self.entries[at];
            entry.seen_in = calls;
            entry.position = position;
            if entry.file != shader.file {
                shader.file.clone_into(&mut entry.file);
            }
            if entry.wanted != shader.revision {
                entry.wanted = shader.revision;
                shader.text.clone_into(&mut entry.text);
            }
            set_values(&mut entry.values, shader.values);

            let (count, most, what) = match hook {
                Hook::Screen => (&mut screens, MOST_SCREEN_EFFECTS, "screen effects"),
                Hook::Surface => (&mut surfaces, MOST_SURFACE_SHADERS, "surface shaders"),
            };
            *count += 1;
            let over = *count > most;
            if over && !entry.refused {
                self.reports.push(ShaderReport {
                    name: entry.name.clone(),
                    file: entry.file.clone(),
                    material: entry.material.clone(),
                    revision: entry.wanted,
                    outcome: Err(over_the_limit(&entry.file, most, what)),
                });
            }
            entry.refused = over;
        }

        // What the game did not give this time is gone.
        let mut lost_live = false;
        self.entries.retain_mut(|entry| {
            if entry.seen_in == calls {
                return true;
            }
            forget(entry.id);
            if let Some(live) = entry.live.take() {
                lost_live = true;
                retire(live.built);
            }
            false
        });
        // The game's order. Positions are all different, so the order is settled.
        if !self.entries.is_sorted_by_key(|entry| entry.position) {
            self.entries.sort_unstable_by_key(|entry| entry.position);
            self.changed = true;
        }
        self.changed |= lost_live;
    }

    /// Calls `ask` with a job for each entry whose wanted revision has not
    /// been asked for yet. The text is moved into the job.
    pub(crate) fn asks(&mut self, mut ask: impl FnMut(Job)) {
        for entry in &mut self.entries {
            if entry.refused || entry.asked == Some(entry.wanted) {
                continue;
            }
            entry.asked = Some(entry.wanted);
            ask(Job {
                id: entry.id,
                revision: entry.wanted,
                hook: entry.hook,
                file: entry.file.clone(),
                text: mem::take(&mut entry.text),
            });
        }
    }

    /// Takes what the worker made. A result for an entry that is gone, or
    /// for a revision that is no longer the wanted one, is never drawn: what
    /// was built goes to `retire`.
    pub(crate) fn done(&mut self, done: Done<T>, mut retire: impl FnMut(T)) {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == done.id && !entry.refused && entry.wanted == done.revision);
        let Some(entry) = entry else {
            if let Ok((built, _)) = done.outcome {
                retire(built);
            }
            return;
        };
        let outcome = match done.outcome {
            Ok((built, block)) => {
                let live = Live {
                    built,
                    block,
                    revision: done.revision,
                };
                if let Some(old) = entry.live.replace(live) {
                    retire(old.built);
                }
                self.changed = true;
                Ok(())
            }
            Err(error) => {
                let note = match (entry.live.is_some(), entry.hook) {
                    (true, _) => "The last version that worked is still drawn.",
                    (false, Hook::Screen) => "Nothing is drawn for it until it works.",
                    (false, Hook::Surface) => "The level's own material is drawn until it works.",
                };
                Err(error.with_note(note))
            }
        };
        self.reports.push(ShaderReport {
            name: entry.name.clone(),
            file: entry.file.clone(),
            material: entry.material.clone(),
            revision: done.revision,
            outcome,
        });
    }

    /// The screen effects that can be drawn, in the game's order.
    pub(crate) fn screens(&self) -> impl Iterator<Item = &Entry<T>> {
        self.live_of(Hook::Screen)
    }

    /// The surface shaders that can be drawn, in the game's order.
    pub(crate) fn surfaces(&self) -> impl Iterator<Item = &Entry<T>> {
        self.live_of(Hook::Surface)
    }

    fn live_of(&self, hook: Hook) -> impl Iterator<Item = &Entry<T>> {
        self.entries
            .iter()
            .filter(move |entry| entry.hook == hook && entry.live.is_some())
    }

    /// Whether which entries are live, their order, or what they are built
    /// from changed since this was last asked. (An entry's material is part
    /// of which entry it is, so it cannot change alone.)
    pub(crate) fn changed(&mut self) -> bool {
        mem::take(&mut self.changed)
    }

    /// Everything said since the last call.
    pub(crate) fn take_reports(&mut self) -> Vec<ShaderReport> {
        mem::take(&mut self.reports)
    }

    /// Whether the game wants nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Copies `new` into `values`. When the names are the same, in the same
/// order, only the numbers are copied and nothing is allocated.
fn set_values(values: &mut Vec<(String, ParamValue)>, new: &[(String, ParamValue)]) {
    let same_names =
        values.len() == new.len() && values.iter().zip(new).all(|(old, new)| old.0 == new.0);
    if same_names {
        for (old, new) in values.iter_mut().zip(new) {
            old.1 = new.1;
        }
    } else {
        values.clear();
        values.extend(new.iter().cloned());
    }
}

/// The report for a shader that is one too many.
fn over_the_limit(file: &str, most: usize, what: &str) -> ShaderError {
    ShaderError {
        file: file.to_owned(),
        line: None,
        column: None,
        message: format!("At most {most} {what} can be on at once, and this is one too many."),
        source_line: None,
        hint: Some("Remove one you do not need with `:remove()` first.".to_owned()),
        note: Some("It is not drawn.".to_owned()),
        lore_fault: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const SCREEN: ShaderUse<'static> = ShaderUse::Screen;

    fn shader<'a>(
        name: &'a str,
        used_as: ShaderUse<'a>,
        revision: u64,
        values: &'a [(String, ParamValue)],
    ) -> GameShader<'a> {
        GameShader {
            name,
            file: "shaders/x.wgsl",
            text: "text",
            used_as,
            values,
            revision,
        }
    }

    fn made(id: u64, revision: u64, built: u32) -> Done<u32> {
        Done {
            id,
            revision,
            outcome: Ok((built, ParamBlock::new(Vec::new()))),
            compiling: Duration::ZERO,
            building: Duration::ZERO,
        }
    }

    fn failed(id: u64, revision: u64) -> Done<u32> {
        Done {
            id,
            revision,
            outcome: Err(over_the_limit("shaders/x.wgsl", 1, "things")),
            compiling: Duration::ZERO,
            building: Duration::ZERO,
        }
    }

    /// What `want` sent to `forget` and `retire`.
    #[derive(Default)]
    struct Gone {
        forgotten: Vec<u64>,
        retired: Vec<u32>,
    }

    fn want(book: &mut Book<u32>, shaders: &[GameShader<'_>], gone: &mut Gone) {
        let forgotten = &mut gone.forgotten;
        let retired = &mut gone.retired;
        book.want(
            shaders,
            |id| forgotten.push(id),
            |built| retired.push(built),
        );
    }

    fn asked(book: &mut Book<u32>) -> Vec<Job> {
        let mut jobs = Vec::new();
        book.asks(|job| jobs.push(job));
        jobs
    }

    fn live_names(book: &Book<u32>) -> Vec<String> {
        book.screens()
            .chain(book.surfaces())
            .map(|entry| entry.name().to_owned())
            .collect()
    }

    #[test]
    fn a_new_entry_is_asked_for_once() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        want(&mut book, &[shader("heat", SCREEN, 1, &[])], &mut gone);
        let jobs = asked(&mut book);
        assert_eq!(jobs.len(), 1);
        assert_eq!((jobs[0].revision, jobs[0].hook), (1, Hook::Screen));
        assert_eq!(jobs[0].text, "text");
        assert_eq!(jobs[0].file, "shaders/x.wgsl");
        // The same revision again asks for nothing.
        want(&mut book, &[shader("heat", SCREEN, 1, &[])], &mut gone);
        assert!(asked(&mut book).is_empty());
        // A newer one asks again.
        want(&mut book, &[shader("heat", SCREEN, 2, &[])], &mut gone);
        let jobs = asked(&mut book);
        assert_eq!((jobs.len(), jobs[0].revision), (1, 2));
    }

    #[test]
    fn values_change_without_a_job_and_without_reallocating() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        let first = [("strength".to_owned(), ParamValue::F32(0.1))];
        want(&mut book, &[shader("heat", SCREEN, 1, &first)], &mut gone);
        asked(&mut book);
        let before = book.entries[0].values.as_ptr();
        let second = [("strength".to_owned(), ParamValue::F32(0.9))];
        want(&mut book, &[shader("heat", SCREEN, 1, &second)], &mut gone);
        assert!(asked(&mut book).is_empty());
        assert_eq!(book.entries[0].values.as_ptr(), before);
        let values: Vec<_> = book.entries[0].values().collect();
        assert_eq!(values, [("strength", ParamValue::F32(0.9))]);
        // New names are copied over whole.
        let third = [("tint".to_owned(), ParamValue::Vec3([1.0, 0.0, 0.0]))];
        want(&mut book, &[shader("heat", SCREEN, 1, &third)], &mut gone);
        let values: Vec<_> = book.entries[0].values().collect();
        assert_eq!(values, [("tint", ParamValue::Vec3([1.0, 0.0, 0.0]))]);
        assert!(asked(&mut book).is_empty());
    }

    #[test]
    fn removing_an_entry_forgets_it_and_retires_what_was_built() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        want(&mut book, &[shader("heat", SCREEN, 1, &[])], &mut gone);
        asked(&mut book);
        book.done(made(1, 1, 42), |_| {});
        book.changed();
        want(&mut book, &[], &mut gone);
        assert_eq!(gone.forgotten, [1]);
        assert_eq!(gone.retired, [42]);
        assert!(book.is_empty());
        assert!(book.changed());
    }

    #[test]
    fn the_order_follows_the_list() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        let one = shader("one", SCREEN, 1, &[]);
        let two = shader("two", SCREEN, 1, &[]);
        let three = shader("three", SCREEN, 1, &[]);
        want(&mut book, &[one, two, three], &mut gone);
        for job in asked(&mut book) {
            book.done(made(job.id, 1, 0), |_| {});
        }
        assert_eq!(live_names(&book), ["one", "two", "three"]);
        book.changed();
        want(&mut book, &[three, one, two], &mut gone);
        assert_eq!(live_names(&book), ["three", "one", "two"]);
        assert!(book.changed());
        // Ids stay with their names.
        assert_eq!(book.screens().next().map(Entry::id), Some(3));
        want(&mut book, &[three, one, two], &mut gone);
        assert!(!book.changed());
    }

    #[test]
    fn duplicates_are_matched_in_order() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        let list = [shader("a", SCREEN, 1, &[]), shader("a", SCREEN, 1, &[])];
        want(&mut book, &list, &mut gone);
        let ids: Vec<_> = book.entries.iter().map(Entry::id).collect();
        want(&mut book, &list, &mut gone);
        let again: Vec<_> = book.entries.iter().map(Entry::id).collect();
        assert_eq!((ids.len(), ids), (2, again));
    }

    #[test]
    fn surface_shaders_are_told_apart_by_material() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        let lava = shader("glow", ShaderUse::Surface { material: "Lava" }, 1, &[]);
        let ice = shader("glow", ShaderUse::Surface { material: "Ice" }, 1, &[]);
        want(&mut book, &[lava, ice], &mut gone);
        assert_eq!(asked(&mut book).len(), 2);
        want(&mut book, &[ice], &mut gone);
        assert_eq!(gone.forgotten, [1]);
        assert_eq!(book.entries[0].material(), Some("Ice"));
    }

    #[test]
    fn the_ninth_screen_effect_is_refused_once() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        let names: Vec<String> = (0..10).map(|n| format!("effect{n}")).collect();
        let list: Vec<_> = names.iter().map(|n| shader(n, SCREEN, 1, &[])).collect();
        want(&mut book, &list, &mut gone);
        want(&mut book, &list, &mut gone);
        want(&mut book, &list, &mut gone);
        let reports = book.take_reports();
        // The ninth and the tenth are each told about once.
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].name, "effect8");
        let error = reports[0].outcome.as_ref().unwrap_err();
        assert!(error.message.contains('8'), "{error}");
        assert_eq!(asked(&mut book).len(), MOST_SCREEN_EFFECTS);
        // Taking one away lets the next in, and it is asked for then.
        want(&mut book, &list[1..], &mut gone);
        assert!(book.take_reports().is_empty());
        let jobs = asked(&mut book);
        assert_eq!(jobs.len(), 1);
        assert_eq!(book.entries.iter().filter(|e| e.refused).count(), 1);
    }

    #[test]
    fn the_limits_are_counted_for_each_kind() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        let names: Vec<String> = (0..9).map(|n| format!("s{n}")).collect();
        let mut list: Vec<_> = names
            .iter()
            .map(|n| shader(n, ShaderUse::Surface { material: "M" }, 1, &[]))
            .collect();
        list.push(shader("screen", SCREEN, 1, &[]));
        want(&mut book, &list, &mut gone);
        let reports = book.take_reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].name, "s8");
        assert_eq!(reports[0].material.as_deref(), Some("M"));
        assert_eq!(asked(&mut book).len(), MOST_SURFACE_SHADERS + 1);
    }

    #[test]
    fn a_stale_result_is_retired_and_not_installed() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        want(&mut book, &[shader("heat", SCREEN, 1, &[])], &mut gone);
        asked(&mut book);
        want(&mut book, &[shader("heat", SCREEN, 2, &[])], &mut gone);
        let mut retired = Vec::new();
        book.done(made(1, 1, 11), |built| retired.push(built));
        assert_eq!(retired, [11]);
        assert_eq!(book.screens().count(), 0);
        assert!(book.take_reports().is_empty());
        // For an entry that is gone.
        want(&mut book, &[], &mut gone);
        book.done(made(1, 2, 22), |built| retired.push(built));
        assert_eq!(retired, [11, 22]);
        assert!(book.take_reports().is_empty());
        // A stale failure says nothing either.
        want(&mut book, &[shader("heat", SCREEN, 3, &[])], &mut gone);
        book.done(failed(2, 2), |_| {});
        assert!(book.take_reports().is_empty());
    }

    #[test]
    fn a_new_version_replaces_the_live_one_and_retires_it() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        want(&mut book, &[shader("heat", SCREEN, 1, &[])], &mut gone);
        asked(&mut book);
        book.done(made(1, 1, 10), |_| {});
        want(&mut book, &[shader("heat", SCREEN, 2, &[])], &mut gone);
        asked(&mut book);
        let mut retired = Vec::new();
        book.done(made(1, 2, 20), |built| retired.push(built));
        assert_eq!(retired, [10]);
        let live = book.screens().next().and_then(Entry::live).unwrap();
        assert_eq!((live.built, live.revision), (20, 2));
        assert_eq!(live.block.size(), 16);
        let reports = book.take_reports();
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|report| report.outcome.is_ok()));
        assert_eq!(reports[1].revision, 2);
    }

    #[test]
    fn a_failure_says_what_is_drawn_instead() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        let list = [
            shader("heat", SCREEN, 1, &[]),
            shader("lava", ShaderUse::Surface { material: "Lava" }, 1, &[]),
        ];
        want(&mut book, &list, &mut gone);
        asked(&mut book);
        book.done(failed(1, 1), |_| {});
        book.done(failed(2, 1), |_| {});
        let notes: Vec<_> = book
            .take_reports()
            .into_iter()
            .map(|report| report.outcome.unwrap_err().note.unwrap())
            .collect();
        assert_eq!(
            notes,
            [
                "Nothing is drawn for it until it works.",
                "The level's own material is drawn until it works."
            ]
        );
        // With a live version, that is the one that stays.
        book.done(made(1, 1, 5), |_| {});
        book.take_reports();
        want(&mut book, &[list[0]], &mut gone);
        let mut changed = list[0];
        changed.revision = 2;
        want(&mut book, &[changed], &mut gone);
        asked(&mut book);
        book.done(failed(1, 2), |_| {});
        let reports = book.take_reports();
        let error = reports[0].outcome.as_ref().unwrap_err();
        assert_eq!(
            error.note.as_deref(),
            Some("The last version that worked is still drawn.")
        );
        assert_eq!(book.screens().count(), 1);
    }

    #[test]
    fn changed_is_true_once_after_a_change() {
        let mut book = Book::new();
        let mut gone = Gone::default();
        assert!(!book.changed());
        want(&mut book, &[shader("heat", SCREEN, 1, &[])], &mut gone);
        asked(&mut book);
        // Nothing live yet.
        assert!(!book.changed());
        book.done(made(1, 1, 1), |_| {});
        assert!(book.changed());
        assert!(!book.changed());
    }
}
