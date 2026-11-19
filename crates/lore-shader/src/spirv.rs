//! A narrow look at the SPIR-V this crate writes, before a driver sees it.
//!
//! naga 30 can write a vector constant whose parts have another type than
//! the vector's own: asked to clamp an image read whose coordinates are
//! unsigned, it makes a `vec2<u32>` of ones out of `i32` constants. Vulkan's
//! validation layer refuses such a module and a driver may do anything with
//! it (#434). So what is written is read back once, and such a constant is
//! a fault that is reported, not handed on.

use std::collections::HashMap;

/// The words before the first instruction: magic, version, generator, bound, schema.
const HEADER_WORDS: usize = 5;

const OP_TYPE_VECTOR: u32 = 23;
const OP_CONSTANT_TRUE: u32 = 41;
const OP_CONSTANT_FALSE: u32 = 42;
const OP_CONSTANT: u32 = 43;
const OP_CONSTANT_COMPOSITE: u32 = 44;
const OP_CONSTANT_NULL: u32 = 46;

/// Each instruction of a module: its opcode and the words after the first.
/// Stops at a word count that cannot be right.
fn instructions(words: &[u32]) -> impl Iterator<Item = (u32, &[u32])> {
    let mut rest = words.get(HEADER_WORDS..).unwrap_or_default();
    std::iter::from_fn(move || {
        let first = *rest.first()?;
        let count = usize::try_from(first >> 16).ok()?;
        if count == 0 || count > rest.len() {
            return None;
        }
        let (this, after) = rest.split_at(count);
        rest = after;
        Some((first & 0xffff, this.get(1..).unwrap_or_default()))
    })
}

/// The first vector constant with a part that is not of the vector's own
/// element type, said in words, or `None` when every one is well made.
pub(crate) fn mistyped_constant(words: &[u32]) -> Option<String> {
    // What each vector type is a vector of, and what type each constant has.
    let mut elements: HashMap<u32, u32> = HashMap::new();
    let mut types: HashMap<u32, u32> = HashMap::new();
    for (op, operands) in instructions(words) {
        match (op, operands) {
            (OP_TYPE_VECTOR, [id, element, ..]) => {
                elements.insert(*id, *element);
            }
            (
                OP_CONSTANT
                | OP_CONSTANT_TRUE
                | OP_CONSTANT_FALSE
                | OP_CONSTANT_NULL
                | OP_CONSTANT_COMPOSITE,
                [kind, id, ..],
            ) => {
                types.insert(*id, *kind);
            }
            _ => {}
        }
    }
    instructions(words).find_map(|(op, operands)| {
        let (OP_CONSTANT_COMPOSITE, [kind, id, parts @ ..]) = (op, operands) else {
            return None;
        };
        // Only vectors: a matrix, an array or a struct is made of other things.
        let element = elements.get(kind)?;
        let wrong = parts
            .iter()
            .find(|part| types.get(*part).is_some_and(|found| found != element))?;
        Some(format!(
            "the vector constant %{id} is of type %{kind}, whose parts are of type %{element}, \
             but its part %{wrong} is of another type"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module's header, then the instructions given as (opcode, operands).
    fn module(instructions: &[(u32, &[u32])]) -> Vec<u32> {
        let mut words = vec![0x0723_0203, 0x0001_0300, 0, 100, 0];
        for (op, operands) in instructions {
            let count = u32::try_from(operands.len() + 1).expect("a short instruction");
            words.push((count << 16) | op);
            words.extend_from_slice(operands);
        }
        words
    }

    // %1 = int, %2 = uint, %3 = vec2<uint>, %4 = int 1, %5 = uint 1.
    const TYPES: [(u32, &[u32]); 5] = [
        (21, &[1, 32, 1]),
        (21, &[2, 32, 0]),
        (OP_TYPE_VECTOR, &[3, 2, 2]),
        (OP_CONSTANT, &[1, 4, 1]),
        (OP_CONSTANT, &[2, 5, 1]),
    ];

    #[test]
    fn a_vector_of_its_own_element_type_is_well_made() {
        let mut all = TYPES.to_vec();
        all.push((OP_CONSTANT_COMPOSITE, &[3, 6, 5, 5]));
        assert_eq!(mistyped_constant(&module(&all)), None);
    }

    #[test]
    fn an_unsigned_vector_made_of_signed_ones_is_found() {
        let mut all = TYPES.to_vec();
        all.push((OP_CONSTANT_COMPOSITE, &[3, 6, 4, 4]));
        let found = mistyped_constant(&module(&all)).expect("it is mistyped");
        assert!(found.contains("%6") && found.contains("%4"), "{found}");
    }

    #[test]
    fn words_that_are_not_a_module_are_not_a_fault_and_do_not_panic() {
        assert_eq!(mistyped_constant(&[]), None);
        assert_eq!(mistyped_constant(&[1, 2, 3]), None);
        // A word count that runs past the end, and one of nought.
        assert_eq!(mistyped_constant(&[0, 0, 0, 0, 0, (9 << 16) | 44, 1]), None);
        assert_eq!(mistyped_constant(&[0, 0, 0, 0, 0, 44, 1, 2]), None);
    }
}
