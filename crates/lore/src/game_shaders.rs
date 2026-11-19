//! The game's own shaders, from the script to the renderer and back.
//!
//! The script says one revision for its whole list of shaders; the renderer
//! wants one for each shader, raised only when that shader's own text
//! changes, so that adding one effect does not build every surface shader
//! again. This keeps the list between them: it gives each shader its own
//! revision, follows the values the script sets every frame without asking
//! for any memory, and holds what the renderer said of each shader.

use std::sync::atomic::{AtomicU64, Ordering};

use lore_render::{
    GameShader, MOST_FLUID_SHADERS, MOST_SCREEN_EFFECTS, MOST_SURFACE_SHADERS, ShaderReport,
};
use lore_script::{ScriptHost, ShaderUse};
use lore_shader::{ParamValue, ShaderError};

/// A shader as the last frame left it.
#[derive(Debug, Clone)]
struct Kept {
    name: String,
    file: String,
    text: String,
    /// The material of a surface shader, or the name of the kind of fluid
    /// a fluid shader is for; `None` for a screen effect.
    material: Option<String>,
    /// The kind of fluid a fluid shader is for, by number.
    fluid: Option<u8>,
    values: Vec<(String, ParamValue)>,
    /// Raised only when `text` or `file` changes. Never reused.
    revision: u64,
    /// What is wrong with the file, as the script side says it.
    problem: Option<ShaderError>,
    /// What the renderer said was wrong with this revision.
    failed: Option<ShaderError>,
}

/// The revision of a report about the graphics driver as a whole, which no
/// shader has: kept revisions start at 1.
const DRIVER_FAULT: u64 = 0;

impl Kept {
    fn is(&self, name: &str, (material, fluid): (Option<&str>, Option<u8>)) -> bool {
        self.name == name && self.material.as_deref() == material && self.fluid == fluid
    }
}

/// The material a shader is used on, if it is a surface shader, or the
/// fluid's name and its kind, if it is a fluid shader.
fn material_of(used_as: &ShaderUse) -> (Option<&str>, Option<u8>) {
    match used_as {
        ShaderUse::Screen => (None, None),
        ShaderUse::Surface { material } => (Some(material), None),
        ShaderUse::Fluid { fluid, kind } => (Some(fluid), Some(*kind)),
    }
}

/// Which game a list follows. Every script's revisions count from the
/// start, so a list lent to another game (the editor plays the level again)
/// must not take the new script's revision for one it has already read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Source(u64);

impl Source {
    /// One no other game has.
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// The shaders a game has turned on, as the renderer is handed them.
#[derive(Debug, Default)]
pub(crate) struct ShaderList {
    /// In the order the script turned them on.
    kept: Vec<Kept>,
    /// The game, and its script's revision of its list, that `kept` follows.
    seen: Option<(Source, u64)>,
    /// The revision the next changed shader gets. Starts at 1.
    next: u64,
    /// What the graphics driver could not make, which stops every shader.
    driver: Option<ShaderError>,
}

impl ShaderList {
    /// Follows the shaders of the script `host` of the game `source`: what
    /// is new is added and what is gone is dropped when the script's
    /// revision changed, and otherwise only the values are copied, which
    /// asks for no memory.
    pub(crate) fn follow(&mut self, source: Source, host: &ScriptHost) {
        let seen = Some((source, host.shader_revision()));
        if self.seen == seen && self.copy_values(host) {
            return;
        }
        self.rebuild(host);
        self.seen = seen;
    }

    /// Copies the values of every shader. Says `false`, having changed
    /// nothing else, when the list is not the one the script has.
    fn copy_values(&mut self, host: &ScriptHost) -> bool {
        let mut at = 0;
        let mut same = true;
        host.each_shader(|said| {
            let Some(kept) = self.kept.get_mut(at) else {
                same = false;
                return;
            };
            at += 1;
            if !kept.is(&said.name, material_of(&said.used_as)) {
                same = false;
            } else if kept.values.len() == said.values.len()
                && kept
                    .values
                    .iter()
                    .zip(&said.values)
                    .all(|(have, new)| have.0 == new.0)
            {
                for (have, new) in kept.values.iter_mut().zip(&said.values) {
                    have.1 = new.1;
                }
            } else {
                kept.values.clone_from(&said.values);
            }
        });
        same && at == self.kept.len()
    }

    /// Reads the whole list again, giving a new revision to every shader
    /// whose text or file changed and keeping the others'.
    fn rebuild(&mut self, host: &ScriptHost) {
        let old = std::mem::take(&mut self.kept);
        let mut kept = Vec::with_capacity(old.len() + 1);
        host.each_shader(|said| {
            let material = material_of(&said.used_as);
            let before = old
                .iter()
                .find(|before| before.is(&said.name, material))
                .filter(|before| before.text == said.text && before.file == said.file);
            let (revision, failed) = match before {
                Some(before) => (before.revision, before.failed.clone()),
                None => {
                    // A changed shader keeps its old failure on screen until
                    // the renderer has said something about the new text.
                    let failed = old
                        .iter()
                        .find(|before| before.is(&said.name, material))
                        .and_then(|before| before.failed.clone());
                    self.next = self.next.max(1);
                    let revision = self.next;
                    self.next += 1;
                    (revision, failed)
                }
            };
            kept.push(Kept {
                name: said.name.clone(),
                file: said.file.clone(),
                text: said.text.clone(),
                material: material.0.map(str::to_owned),
                fluid: material.1,
                values: said.values.clone(),
                revision,
                problem: said.problem.clone(),
                failed,
            });
        });
        self.kept = kept;
    }

    #[cfg_attr(
        not(feature = "editor"),
        allow(dead_code, reason = "only the editor stops a game it was playing")
    )]
    /// No shaders: a game that is no longer playing draws none.
    pub(crate) fn clear(&mut self) {
        self.kept.clear();
        self.seen = None;
        self.driver = None;
    }

    /// Lends the renderer the list. It is built on the stack.
    pub(crate) fn hand_over<R>(&self, give: impl FnOnce(&[GameShader<'_>]) -> R) -> R {
        const NONE: GameShader<'static> = GameShader {
            name: "",
            file: "",
            text: "",
            used_as: lore_render::ShaderUse::Screen,
            values: &[],
            revision: 0,
        };
        let mut list = [NONE; MOST_SCREEN_EFFECTS + MOST_SURFACE_SHADERS + MOST_FLUID_SHADERS];
        let (mut screens, mut surfaces, mut fluids, mut count) = (0, 0, 0, 0);
        for kept in &self.kept {
            // The script refuses more than the most of each kind, so
            // nothing is left out here in practice; the check keeps the
            // array from overflowing all the same.
            let (used_as, taken, most) = match (&kept.material, kept.fluid) {
                (None, _) => (
                    lore_render::ShaderUse::Screen,
                    &mut screens,
                    MOST_SCREEN_EFFECTS,
                ),
                (Some(fluid), Some(kind)) => (
                    lore_render::ShaderUse::Fluid { kind, fluid },
                    &mut fluids,
                    MOST_FLUID_SHADERS,
                ),
                (Some(material), None) => (
                    lore_render::ShaderUse::Surface { material },
                    &mut surfaces,
                    MOST_SURFACE_SHADERS,
                ),
            };
            if *taken == most {
                continue;
            }
            *taken += 1;
            list[count] = GameShader {
                name: &kept.name,
                file: &kept.file,
                text: &kept.text,
                used_as,
                values: &kept.values,
                revision: kept.revision,
            };
            count += 1;
        }
        give(&list[..count])
    }

    /// Takes in what the renderer said. A report counts only for the
    /// shader it names, at the revision it names; any other is about text
    /// that has been replaced since, or a shader that is gone.
    pub(crate) fn hear(&mut self, reports: Vec<ShaderReport>) {
        for report in reports {
            match &report.outcome {
                // Something was built, so the driver works again.
                Ok(()) => self.driver = None,
                // No shader has revision 0: the driver failed, and no game
                // shader is drawn.
                Err(error) if report.revision == DRIVER_FAULT => {
                    log::error!("{error}");
                    self.driver = Some(error.clone());
                    continue;
                }
                Err(_) => {}
            }
            let Some(kept) = self.kept.iter_mut().find(|kept| {
                kept.name == report.name
                    && kept.material.as_deref() == report.material.as_deref()
                    && kept.revision == report.revision
            }) else {
                continue;
            };
            match report.outcome {
                Ok(()) => kept.failed = None,
                Err(error) => {
                    log::error!("{error}");
                    kept.failed = Some(error);
                }
            }
        }
    }

    /// What is wrong with the shaders, in order: the driver's failure if
    /// there was one, then for each, what the script
    /// side found in its file, or else what the renderer could not build.
    pub(crate) fn problems(&self) -> impl Iterator<Item = &ShaderError> {
        self.driver.iter().chain(
            self.kept
                .iter()
                .filter_map(|kept| kept.problem.as_ref().or(kept.failed.as_ref())),
        )
    }
}

#[cfg(test)]
mod tests {
    use lore_input::ActionMap;

    use super::*;

    const SCREEN: &str = "//! param strength: f32 = 0.5 range 0..1\n\
        fn screen(in: ScreenIn) -> vec3f { return in.color * params.strength; }";

    fn host(script: &str, files: &[(&str, &str)]) -> ScriptHost {
        let mut sources = vec![("main.luau".to_owned(), script.to_owned())];
        sources.extend(
            files
                .iter()
                .map(|(name, text)| ((*name).to_owned(), (*text).to_owned())),
        );
        let host = ScriptHost::from_sources(sources, ActionMap::standard());
        assert!(host.last_error().is_none(), "{:?}", host.last_error());
        host
    }

    /// Follows `host` as the script of a game of its own, which is how the
    /// list knows to read it all although its revisions count from the start.
    fn follow_another(list: &mut ShaderList, host: &ScriptHost) {
        list.follow(Source::new(), host);
    }

    /// The game the tests' first script belongs to.
    const ONE: Source = Source(0);

    fn revisions(list: &ShaderList) -> Vec<(String, u64)> {
        list.kept
            .iter()
            .map(|kept| (kept.name.clone(), kept.revision))
            .collect()
    }

    fn error(file: &str) -> ShaderError {
        ShaderError {
            file: file.to_owned(),
            line: Some(2),
            column: None,
            message: "no".to_owned(),
            source_line: None,
            hint: None,
            note: None,
            lore_fault: false,
        }
    }

    fn report(name: &str, revision: u64, outcome: Result<(), ShaderError>) -> ShaderReport {
        ShaderReport {
            name: name.to_owned(),
            file: format!("shaders/{name}.wgsl"),
            material: None,
            revision,
            outcome,
        }
    }

    fn handed(list: &ShaderList) -> Vec<(String, u64)> {
        list.hand_over(|shaders| {
            shaders
                .iter()
                .map(|shader| (shader.name.to_owned(), shader.revision))
                .collect()
        })
    }

    #[test]
    fn a_second_shader_leaves_the_first_one_s_revision_alone() {
        let one = host(
            r#"game.shaders.screen("a", {})"#,
            &[("shaders/a.wgsl", SCREEN), ("shaders/b.wgsl", SCREEN)],
        );
        let mut list = ShaderList::default();
        list.follow(ONE, &one);
        assert_eq!(revisions(&list), [("a".to_owned(), 1)]);

        let two = host(
            r#"game.shaders.screen("a", {}) game.shaders.screen("b", {})"#,
            &[("shaders/a.wgsl", SCREEN), ("shaders/b.wgsl", SCREEN)],
        );
        follow_another(&mut list, &two);
        assert_eq!(revisions(&list), [("a".to_owned(), 1), ("b".to_owned(), 2)]);
        assert_eq!(handed(&list), revisions(&list));

        // Changed text gives that shader a new revision and no other.
        let changed = host(
            r#"game.shaders.screen("a", {}) game.shaders.screen("b", {})"#,
            &[
                ("shaders/a.wgsl", SCREEN),
                ("shaders/b.wgsl", &SCREEN.replace("0.5", "0.6")),
            ],
        );
        follow_another(&mut list, &changed);
        assert_eq!(revisions(&list), [("a".to_owned(), 1), ("b".to_owned(), 3)]);
    }

    #[test]
    fn values_follow_without_a_new_revision() {
        let mut host = host(
            "local s = game.shaders.screen(\"a\", { strength = 0.2 })\n\
             game.onTick(function() s:set(\"strength\", 0.9) end)",
            &[("shaders/a.wgsl", SCREEN)],
        );
        let mut list = ShaderList::default();
        list.follow(ONE, &host);
        let before = list.kept[0].values.clone();
        assert_eq!(before, [("strength".to_owned(), ParamValue::F32(0.2))]);

        host.tick(&lore_input::InputFrame::default(), 1.0 / 60.0);
        list.follow(ONE, &host);
        assert_eq!(
            list.kept[0].values,
            [("strength".to_owned(), ParamValue::F32(0.9))]
        );
        assert_eq!(list.kept[0].revision, 1);
        assert_eq!(list.seen, Some((ONE, host.shader_revision())));
    }

    #[test]
    fn what_the_renderer_says_is_kept_for_its_own_revision_only() {
        let one = host(
            r#"game.shaders.screen("a", {})"#,
            &[("shaders/a.wgsl", SCREEN)],
        );
        let mut list = ShaderList::default();
        list.follow(ONE, &one);
        list.hear(vec![report("a", 1, Err(error("shaders/a.wgsl")))]);
        assert_eq!(list.problems().count(), 1);

        // Stale, or for a shader that is not kept: ignored.
        list.hear(vec![report("a", 0, Ok(())), report("zzz", 1, Ok(()))]);
        assert_eq!(list.problems().count(), 1);
        list.hear(vec![report("zzz", 1, Err(error("shaders/zzz.wgsl")))]);
        assert_eq!(list.problems().count(), 1);

        // The same text again keeps the failure; new text keeps it until
        // the renderer has said something about it.
        list.follow(ONE, &one);
        assert_eq!(list.problems().count(), 1);
        let changed = host(
            r#"game.shaders.screen("a", {})"#,
            &[("shaders/a.wgsl", &SCREEN.replace("0.5", "0.6"))],
        );
        follow_another(&mut list, &changed);
        assert_eq!(list.kept[0].revision, 2);
        assert_eq!(list.problems().count(), 1);
        list.hear(vec![report("a", 1, Ok(()))]);
        assert_eq!(list.problems().count(), 1);
        list.hear(vec![report("a", 2, Ok(()))]);
        assert_eq!(list.problems().count(), 0);
    }

    #[test]
    fn a_problem_with_the_file_comes_before_a_compile_error() {
        let one = host(
            r#"game.shaders.screen("a", {})"#,
            &[("shaders/a.wgsl", SCREEN)],
        );
        let mut list = ShaderList::default();
        list.follow(ONE, &one);
        list.hear(vec![report("a", 1, Err(error("compile")))]);
        list.kept[0].problem = Some(error("script"));
        let files: Vec<&str> = list.problems().map(|error| error.file.as_str()).collect();
        assert_eq!(files, ["script"]);
        list.kept[0].problem = None;
        let files: Vec<&str> = list.problems().map(|error| error.file.as_str()).collect();
        assert_eq!(files, ["compile"]);
    }

    #[test]
    fn a_list_lent_to_another_game_reads_its_shaders_afresh() {
        let script = r#"game.shaders.screen("a", {})"#;
        let first = host(script, &[("shaders/a.wgsl", SCREEN)]);
        let changed = SCREEN.replace("0.5", "0.6");
        let second = host(script, &[("shaders/a.wgsl", &changed)]);
        // Two new scripts with the same shaders are at the same revision.
        assert_eq!(first.shader_revision(), second.shader_revision());
        let mut list = ShaderList::default();
        list.follow(Source::new(), &first);
        list.follow(Source::new(), &second);
        assert_eq!(revisions(&list), [("a".to_owned(), 2)]);
        assert_eq!(list.kept[0].text, changed);
    }

    #[test]
    fn a_driver_failure_is_shown_until_something_is_built_or_the_list_is_cleared() {
        let one = host(
            r#"game.shaders.screen("a", {})"#,
            &[("shaders/a.wgsl", SCREEN)],
        );
        let mut list = ShaderList::default();
        list.follow(ONE, &one);
        let driver = || report("game shaders", 0, Err(error("src/game_shaders/mod.rs")));
        list.hear(vec![driver()]);
        assert_eq!(list.problems().count(), 1);
        assert_eq!(
            list.problems().next().map(|e| e.message.as_str()),
            Some("no")
        );
        // Another report that is not about a shader changes nothing.
        list.hear(vec![report("gone", 7, Err(error("shaders/gone.wgsl")))]);
        assert_eq!(list.problems().count(), 1);
        // Something built.
        list.hear(vec![report("a", 1, Ok(()))]);
        assert_eq!(list.problems().count(), 0);
        // Cleared with the list.
        list.hear(vec![driver()]);
        list.clear();
        assert_eq!(list.problems().count(), 0);
    }

    #[test]
    fn the_script_and_the_renderer_allow_the_same_number_of_shaders() {
        assert_eq!(lore_script::MOST_SCREEN_EFFECTS, MOST_SCREEN_EFFECTS);
        assert_eq!(lore_script::MOST_SURFACE_SHADERS, MOST_SURFACE_SHADERS);
        assert_eq!(lore_script::MOST_FLUID_SHADERS, MOST_FLUID_SHADERS);
    }

    #[test]
    fn clear_leaves_no_shaders() {
        let one = host(
            r#"game.shaders.screen("a", {})"#,
            &[("shaders/a.wgsl", SCREEN)],
        );
        let mut list = ShaderList::default();
        list.follow(ONE, &one);
        list.clear();
        assert!(handed(&list).is_empty());
        assert_eq!(list.problems().count(), 0);
        // Following again brings them back, with a revision never used.
        list.follow(ONE, &one);
        assert_eq!(revisions(&list), [("a".to_owned(), 2)]);
    }
}
