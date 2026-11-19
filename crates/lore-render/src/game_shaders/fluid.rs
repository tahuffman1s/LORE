//! The fluid table: which of a game's live fluid shaders draws each kind of
//! fluid.
//!
//! A kind takes one shader, and the first in the game's list wins. The
//! table is filled again at the start of each frame, from what is live, and
//! read by the mesh pass (a pool of that kind) and the fluid pass (drops of
//! it) through [`FluidDraws`]: no allocation and no search while drawing.

use ash::vk;

use super::book::Book;
use super::works::Built;
use crate::fluid::MOST_FLUID_KINDS;

/// One live fluid shader as the passes draw it.
#[derive(Clone, Copy)]
pub(crate) struct FluidVariant {
    /// Draws a pool of the kind.
    pub(crate) body: vk::Pipeline,
    /// Adds up what each drop of the kind in flight takes and scatters.
    pub(crate) amount: vk::Pipeline,
    /// Adds up the film that drops of the kind lying on surfaces make.
    pub(crate) film: vk::Pipeline,
    /// Its values and the clock, for the frame being recorded.
    pub(crate) set: vk::DescriptorSet,
}

/// What the passes need to draw the fluid shaders: for each kind of fluid,
/// the live shader that draws it, if one does.
pub(crate) struct FluidDraws {
    /// The layout set 4 is bound with: the mesh pipeline's sets, the
    /// fluid's own, and then a shader's values.
    pub(crate) layout: vk::PipelineLayout,
    by_kind: [Option<FluidVariant>; MOST_FLUID_KINDS],
    any: bool,
}

impl FluidDraws {
    pub(super) fn new(layout: vk::PipelineLayout) -> Self {
        Self {
            layout,
            by_kind: [None; MOST_FLUID_KINDS],
            any: false,
        }
    }

    /// The shader that draws kind `kind`, if one does.
    pub(crate) fn of(&self, kind: u8) -> Option<&FluidVariant> {
        self.by_kind.get(usize::from(kind))?.as_ref()
    }

    /// Whether any kind has a shader that draws.
    pub(super) fn any(&self) -> bool {
        self.any
    }

    /// Fills the table from what is live in `book`, for the frame that
    /// uses the sets numbered `slot`.
    pub(super) fn update(&mut self, book: &Book<Built>, slot: usize) {
        self.by_kind = [None; MOST_FLUID_KINDS];
        self.any = false;
        for entry in book.fluids() {
            let Some(shader) = entry.live().and_then(|live| live.built.fluid()) else {
                continue;
            };
            let (Some(place), Some(set)) = (
                self.by_kind.get_mut(usize::from(entry.kind())),
                shader.sets.get(slot).copied(),
            ) else {
                continue;
            };
            // The first in the game's list has the kind.
            if place.is_none() {
                *place = Some(FluidVariant {
                    body: shader.body,
                    amount: shader.amount,
                    film: shader.film,
                    set,
                });
                self.any = true;
            }
        }
    }
}
