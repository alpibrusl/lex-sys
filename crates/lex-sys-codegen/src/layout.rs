//! What a type costs in memory (`docs/layout.md`), for `lex-sys layout`.

use crate::*;

/// What one type costs in memory (`docs/layout.md` §4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    /// The scalarised leaf count `defined-behaviour.md` §5 defines.
    pub leaves: u32,
    /// What it costs today: leaves × 8, except `byte`.
    pub size: u32,
    /// What it would cost with each leaf at its natural width, fields
    /// reordered widest-first and the whole aligned to its widest member
    /// — which is what `docs/layout.md` §2 is about not having built.
    pub packed: u32,
    /// The distance between elements of a `[T]`, which is the number a
    /// traversal actually pays.
    pub stride: u32,
}

/// Measure a type, for the report rather than for the backend
/// (`docs/layout.md` §4).
///
/// The backend is not consulted twice here: `size` and `stride` are
/// computed from the same `leaves` the emitter uses, so the report cannot
/// drift from what is emitted without the emitter changing — which is
/// what `the_layout_report_says_what_a_type_costs` checks against a
/// running program rather than against this comment.
pub fn layout_of(ty: &Type, program: &Program, triple: &Triple) -> Layout {
    let pointer = match triple.pointer_width() {
        Ok(target_lexicon::PointerWidth::U32) => types::I32,
        _ => types::I64,
    };
    let kinds = leaves(ty, program, pointer);
    let leaf_count = kinds.len() as u32;
    let size = leaf_count * RETURN_SLOT_STRIDE as u32;

    // Each leaf at its natural width, then the whole rounded up to the
    // widest one — the layout rule every C compiler uses, applied to the
    // leaves rather than to the fields, because leaves are what this
    // language has. Reordering is free in the sum, so the packed size is
    // the same whatever order the fields were declared in.
    let widest = kinds.iter().map(|k| k.bytes()).max().unwrap_or(1);
    let used: u32 = kinds.iter().map(|k| k.bytes()).sum();
    let packed = if widest == 0 { 0 } else { used.div_ceil(widest) * widest };

    Layout { leaves: leaf_count, size, packed, stride: stride_of(ty, program, pointer) }
}

/// The distance between elements of a `[T]`, matching `Body::stride`.
pub(crate) fn stride_of(element: &Type, program: &Program, pointer: types::Type) -> u32 {
    match element {
        // `strings.md` §3: the one size in the language that is not a
        // multiple of 8, so that a string is something C could read.
        Type::Byte => 1,
        other => leaf_count(other, program, pointer) * RETURN_SLOT_STRIDE as u32,
    }
}
