//! The surface table: which of a game's live surface shaders draws each of
//! the level's materials.
//!
//! A material is matched by its exact name, and the first shader in the
//! game's list wins. The table is made again only when what is live changed
//! or a level was set, and the mesh pass reads it each frame through
//! [`SurfaceDraws`]: no allocation and no search by name while drawing.

use ash::vk;
use lore_shader::{ShaderError, closest};

use super::book::Book;
use super::works::Built;
use super::{MOST_SURFACE_SHADERS, ShaderReport};
use crate::mesh::Meshes;

/// How many names of a level's materials an error lists.
const MOST_NAMES: usize = 12;

/// What a part of the level whose material has no shader is given in the
/// lookup.
const NONE: u8 = u8::MAX;

/// One live surface shader as the mesh pass draws it.
#[derive(Clone, Copy)]
pub(crate) struct Variant {
    /// For solid, cut-out and see-through surfaces, in that order.
    pub(crate) pipelines: [vk::Pipeline; 3],
    /// Its values and the clock, for the frame being recorded.
    pub(crate) set: vk::DescriptorSet,
}

/// What the mesh pass needs to draw the surface shaders: each live shader
/// and which of them draws each material. Kept between frames, so drawing
/// allocates nothing.
pub(crate) struct SurfaceDraws {
    /// The layout set 3 is bound with.
    pub(crate) layout: vk::PipelineLayout,
    variants: [Variant; MOST_SURFACE_SHADERS],
    /// The book's entry that each variant is.
    owners: [u64; MOST_SURFACE_SHADERS],
    count: usize,
    /// For each of the level's materials, and one more for matte white, the
    /// variant that draws it, or `NONE`.
    by_material: Vec<u8>,
}

impl SurfaceDraws {
    /// The variant that draws the material numbered `material` (as
    /// [`Meshes`] numbers them), and its number. `None` when its variant
    /// has no set to draw with.
    pub(crate) fn of(&self, material: u32) -> Option<(u8, &Variant)> {
        let number = *self.by_material.get(material as usize)?;
        let variant = self
            .variants
            .get(usize::from(number))
            .filter(|variant| number != NONE && variant.set != vk::DescriptorSet::null())?;
        Some((number, variant))
    }
}

/// What the table decided for a list of shaders, in the game's order.
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan {
    /// For each material of the level (and one more for matte white), the
    /// shader that draws it.
    by_material: Vec<u8>,
    /// Shaders whose material the level lacks.
    missing: Vec<usize>,
    /// Shaders that lost every material they name to an earlier shader, and
    /// that earlier shader.
    taken: Vec<(usize, usize)>,
}

/// Matches each of `wanted` (a material name each) to the materials named
/// `names`. At most `NONE` shaders.
fn plan(wanted: &[&str], names: &[Option<String>]) -> Plan {
    let mut plan = Plan {
        by_material: vec![NONE; names.len() + 1],
        ..Plan::default()
    };
    for (shader, material) in wanted.iter().enumerate() {
        let Ok(number) = u8::try_from(shader) else {
            break;
        };
        let mut found = false;
        let mut owner = None;
        let mut drawn = false;
        for (slot, name) in plan.by_material.iter_mut().zip(names) {
            if name.as_deref() != Some(*material) {
                continue;
            }
            found = true;
            if *slot == NONE {
                *slot = number;
                drawn = true;
            } else {
                owner.get_or_insert(usize::from(*slot));
            }
        }
        match (found, drawn, owner) {
            (false, ..) => plan.missing.push(shader),
            (true, false, Some(first)) => plan.taken.push((shader, first)),
            _ => {}
        }
    }
    plan
}

/// The names of the level's materials, once each and in order, as an error
/// lists them: at most `MOST_NAMES`, then how many more.
fn list_names(names: &[Option<String>]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for name in names.iter().flatten() {
        if !seen.contains(&name.as_str()) {
            seen.push(name);
        }
    }
    if seen.is_empty() {
        return "None of the level's materials has a name.".to_owned();
    }
    let shown = seen.len().min(MOST_NAMES);
    let mut text = format!("The level's materials are: {}", seen[..shown].join(", "));
    if seen.len() > shown {
        text.push_str(&format!(", and {} more", seen.len() - shown));
    }
    text.push('.');
    text
}

/// What is said of a shader whose material the level does not have.
fn missing_error(file: &str, material: &str, names: &[Option<String>]) -> ShaderError {
    let near = closest(material, names.iter().flatten().map(String::as_str));
    ShaderError {
        file: file.to_owned(),
        line: None,
        column: None,
        message: format!("The level has no material named `{material}`."),
        source_line: None,
        hint: Some(match near {
            Some(near) => format!("Did you mean `{near}`?"),
            None => list_names(names),
        }),
        note: Some("Nothing is changed until a level with that material is loaded.".to_owned()),
        lore_fault: false,
    }
}

/// What is said of a shader whose materials an earlier shader has.
fn taken_error(file: &str, material: &str, first: &str) -> ShaderError {
    ShaderError {
        file: file.to_owned(),
        line: None,
        column: None,
        message: format!(
            "Material `{material}` already has the surface shader `{first}`; a material takes one."
        ),
        source_line: None,
        hint: Some("Remove one of them, or give them different materials.".to_owned()),
        note: Some(format!("The shader `{first}` is drawn on it.")),
        lore_fault: false,
    }
}

/// Which surface shader draws which material of the current level.
pub(super) struct SurfaceTable {
    draws: SurfaceDraws,
    /// Whether any live shader draws any material.
    drawn: bool,
    /// Whether the table must be made again.
    stale: bool,
    /// The problems told about for this level, as (entry, revision,
    /// problem): each is told once, and again when a new version of the
    /// shader is drawn. When one goes away, the shader is told to work.
    told: Vec<(u64, u64, Problem)>,
    /// Whether every problem still there is told again at the next rebuild.
    resay: bool,
}

/// What can keep a surface shader from drawing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Problem {
    /// The level has no material of its name.
    Missing,
    /// An earlier shader has its material.
    Taken,
}

impl SurfaceTable {
    /// An empty table, for shaders made with `layout`.
    pub(super) fn new(layout: vk::PipelineLayout) -> Self {
        Self {
            draws: SurfaceDraws {
                layout,
                variants: [Variant {
                    pipelines: [vk::Pipeline::null(); 3],
                    set: vk::DescriptorSet::null(),
                }; MOST_SURFACE_SHADERS],
                owners: [0; MOST_SURFACE_SHADERS],
                count: 0,
                by_material: Vec::new(),
            },
            drawn: false,
            stale: true,
            told: Vec::new(),
            resay: false,
        }
    }

    /// A level was set (or the card went idle): the table is made again,
    /// and what is wrong with it is said again.
    pub(super) fn scene_changed(&mut self) {
        self.stale = true;
        self.resay = true;
    }

    /// Makes the table again if what is live or the level changed, and
    /// points each variant at its set for frame slot `slot`. Reports what
    /// cannot be drawn, once each. With no level there is no table.
    pub(super) fn update(&mut self, book: &mut Book<Built>, meshes: Option<&Meshes>, slot: usize) {
        if book.changed() {
            self.stale = true;
        }
        if self.stale {
            self.stale = false;
            self.rebuild(book, meshes);
        }
        // Each variant is given the set of its own entry, or none, which
        // leaves its material drawn as the level's own.
        let draws = &mut self.draws;
        for (variant, owner) in draws.variants[..draws.count].iter_mut().zip(draws.owners) {
            variant.set = book
                .surfaces()
                .find(|entry| entry.id() == owner)
                .and_then(|entry| entry.live())
                .and_then(|live| live.built.surface())
                .and_then(|shader| shader.sets.get(slot))
                .copied()
                .unwrap_or_default();
        }
    }

    fn rebuild(&mut self, book: &mut Book<Built>, meshes: Option<&Meshes>) {
        let draws = &mut self.draws;
        draws.count = 0;
        draws.by_material.clear();
        self.drawn = false;
        let Some(meshes) = meshes else {
            return;
        };
        let names = meshes.material_names();

        let mut wanted: Vec<Wanted> = Vec::new();
        for entry in book.surfaces().take(MOST_SURFACE_SHADERS) {
            let (Some(live), Some(material)) = (entry.live(), entry.material()) else {
                continue;
            };
            let Some(shader) = live.built.surface() else {
                continue;
            };
            let at = draws.count;
            draws.variants[at].pipelines = shader.pipelines;
            draws.owners[at] = entry.id();
            draws.count += 1;
            wanted.push(Wanted {
                id: entry.id(),
                name: entry.name().to_owned(),
                file: entry.file().to_owned(),
                material: material.to_owned(),
                revision: live.revision,
            });
        }
        let materials: Vec<&str> = wanted.iter().map(|w| w.material.as_str()).collect();
        let plan = plan(&materials, names);
        self.drawn = plan.by_material.iter().any(|&number| number != NONE);
        draws.by_material = plan.by_material;

        // What is wrong now, and what was said about it before.
        let mut found: Vec<(u64, u64, Problem)> = Vec::new();
        let mut said = Vec::new();
        let again = std::mem::take(&mut self.resay);
        for &shader in &plan.missing {
            let w = &wanted[shader];
            let key = (w.id, w.revision, Problem::Missing);
            if again || !self.told.contains(&key) {
                said.push(w.says(Err(missing_error(&w.file, &w.material, names))));
            }
            found.push(key);
        }
        for &(shader, first) in &plan.taken {
            let (w, other) = (&wanted[shader], &wanted[first]);
            let key = (w.id, w.revision, Problem::Taken);
            if again || !self.told.contains(&key) {
                said.push(w.says(Err(taken_error(&w.file, &w.material, &other.name))));
            }
            found.push(key);
        }
        // A problem told before whose cause is gone: the shader draws now.
        for &(id, revision, _) in &self.told {
            let gone = !found.iter().any(|&(i, r, _)| (i, r) == (id, revision));
            let drawn = wanted.iter().find(|w| (w.id, w.revision) == (id, revision));
            if let (true, Some(w)) = (gone, drawn) {
                let told = said.iter().any(|r| r.name == w.name && r.outcome.is_ok());
                if !told {
                    said.push(w.says(Ok(())));
                }
            }
        }
        self.told = found;
        for report in said {
            book.report(report);
        }
    }

    /// What the mesh pass draws: `None` when no shader draws any material,
    /// which leaves the pass exactly as it is without shaders.
    pub(super) fn draws(&self) -> Option<&SurfaceDraws> {
        self.drawn.then_some(&self.draws)
    }
}

/// A live surface shader, as the table was last made.
struct Wanted {
    id: u64,
    name: String,
    file: String,
    material: String,
    revision: u64,
}

impl Wanted {
    /// The report that says `outcome` about this shader.
    fn says(&self, outcome: Result<(), ShaderError>) -> ShaderReport {
        ShaderReport {
            name: self.name.clone(),
            file: self.file.clone(),
            material: Some(self.material.clone()),
            revision: self.revision,
            outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ash::vk::Handle;

    fn names(list: &[&str]) -> Vec<Option<String>> {
        list.iter().map(|name| Some((*name).to_owned())).collect()
    }

    #[test]
    fn a_variant_with_no_set_draws_nothing() {
        let layout = vk::PipelineLayout::null();
        let mut table = SurfaceTable::new(layout);
        table.draws.variants[1].set = vk::DescriptorSet::from_raw(7);
        table.draws.by_material = vec![0, 1, NONE];
        assert!(table.draws.of(0).is_none());
        assert!(table.draws.of(1).is_some_and(|(number, _)| number == 1));
        assert!(table.draws.of(2).is_none());
        assert!(table.draws.of(9).is_none());
    }

    #[test]
    fn a_material_is_matched_by_its_exact_name_and_the_first_shader_wins() {
        let found = plan(
            &["Lava", "Stone", "Lava", "lava"],
            &names(&["Lava", "Rock", "Stone"]),
        );
        assert_eq!(found.by_material, [0, NONE, 1, NONE]);
        assert_eq!(found.taken, [(2, 0)]);
        assert_eq!(found.missing, [3]);
    }

    #[test]
    fn every_material_with_the_name_is_drawn_and_unnamed_ones_never() {
        let level = [Some("Lava".to_owned()), None, Some("Lava".to_owned())];
        let found = plan(&["Lava", ""], &level);
        assert_eq!(found.by_material, [0, NONE, 0, NONE]);
        assert_eq!(found.missing, [1]);
    }

    #[test]
    fn a_shader_with_a_free_material_among_taken_ones_is_not_told_off() {
        let found = plan(&["A", "A"], &names(&["A", "A"]));
        assert_eq!(found.by_material, [0, 0, NONE]);
        assert_eq!(found.taken, [(1, 0)]);
    }

    #[test]
    fn the_names_listed_are_few_once_each_and_counted() {
        assert_eq!(
            list_names(&names(&["A", "B", "A", "C"])),
            "The level's materials are: A, B, C."
        );
        let many: Vec<String> = (1..=15).map(|n| format!("M{n}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let text = list_names(&names(&many));
        assert!(text.ends_with("M12, and 3 more."), "{text}");
        assert_eq!(
            list_names(&[None]),
            "None of the level's materials has a name."
        );
    }

    #[test]
    fn a_missing_material_gets_a_guess_or_the_list() {
        let level = names(&["Lava", "Stone"]);
        let near = missing_error("shaders/x.wgsl", "Lavva", &level);
        assert_eq!(near.hint.as_deref(), Some("Did you mean `Lava`?"));
        let far = missing_error("shaders/x.wgsl", "Water", &level);
        assert_eq!(
            far.hint.as_deref(),
            Some("The level's materials are: Lava, Stone.")
        );
    }
}
