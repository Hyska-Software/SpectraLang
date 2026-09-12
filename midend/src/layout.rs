//! Canonical aggregate layout for structs, tuples, and enum payloads.
//!
//! Field offsets are cumulative with natural alignment (a field is aligned to
//! its stored byte size, capped at 8 bytes) and the total size of an aggregate
//! is aligned to 8 bytes. The lowering and the backend must both use this
//! module so that field access (byte offsets) and allocation sizes agree.
//!
//! Two size notions exist:
//! - [`stored_size`]: bytes a value occupies when embedded as a field or
//!   array element. Aggregates (structs, tuples, arrays, enums, tasks,
//!   tensors, functions, ranges) are represented as pointers and store 8
//!   bytes; `DynTrait` fat pointers store 16 bytes.
//! - [`type_size_bytes`]: bytes allocated for a standalone value of the type
//!   (used by `alloca`), equal to the padded layout total for aggregates.

use crate::ir::{FloatWidth, IntWidth, Type as IRType};

/// Bytes a value of `ty` occupies when stored inside an aggregate.
pub fn stored_size(ty: &IRType) -> usize {
    match ty {
        // Unknown is poison, not a real storage class. Returning zero keeps
        // diagnostics/reporting total; the verifier rejects it before layout
        // data can reach code generation.
        IRType::Unknown => 0,
        IRType::Void => 0,
        IRType::Bool => 1,
        IRType::Char => 4,
        IRType::Int => 8,
        IRType::Float => 8,
        IRType::ExactInt { width, .. } => match width {
            IntWidth::I8 => 1,
            IntWidth::I16 => 2,
            IntWidth::I32 => 4,
            IntWidth::I64 | IntWidth::Isize | IntWidth::Usize => 8,
        },
        IRType::ExactFloat { width } => match width {
            FloatWidth::F32 => 4,
            FloatWidth::F64 => 8,
        },
        IRType::String => 8,
        IRType::Pointer(_) => 8,
        IRType::Task { .. } => 8,
        IRType::Range => 8,
        IRType::Function { .. } => 8,
        IRType::Tensor { .. } => 8,
        IRType::Array { .. }
        | IRType::Tuple { .. }
        | IRType::Struct { .. }
        | IRType::Enum { .. } => {
            8 // pointer to the aggregate
        }
        IRType::Generic { representation, .. } => stored_size(representation),
        IRType::DynTrait { .. } => 16, // fat pointer: data_ptr (8) + vtable_ptr (8)
    }
}

/// Byte size of a standalone value of `ty` (allocation size).
pub fn type_size_bytes(ty: &IRType) -> usize {
    match ty {
        IRType::Array { element_type, size } => stored_size(element_type) * size,
        IRType::Tuple { elements } => layout_of(elements).size,
        IRType::Struct { fields, .. } => layout_of(fields.iter().map(|(_, ty)| ty)).size,
        IRType::Enum { variants, .. } => {
            // Tag (8 bytes) + padded size of the largest variant payload.
            let max_variant_size = variants
                .iter()
                .map(|(_, data_types)| {
                    if let Some(types) = data_types {
                        layout_of(types).size
                    } else {
                        0
                    }
                })
                .max()
                .unwrap_or(0);
            align_to(max_variant_size + 8, 8)
        }
        IRType::Generic { representation, .. } => type_size_bytes(representation),
        _ => stored_size(ty),
    }
}

/// Natural alignment for a stored size: fields align to their own size,
/// capped at 8 bytes (the widest scalar in the current backend).
pub fn alignment_of(size: usize) -> usize {
    match size {
        0 => 1,
        1 => 1,
        2 => 2,
        3..=4 => 4,
        _ => 8,
    }
}

fn align_to(size: usize, alignment: usize) -> usize {
    if size.is_multiple_of(alignment) {
        size
    } else {
        size + (alignment - size % alignment)
    }
}

/// Cumulative offsets and total size for a sequence of element sizes.
pub fn layout_of_sizes(sizes: &[usize]) -> Layout {
    let mut offsets = Vec::with_capacity(sizes.len());
    let mut offset = 0usize;
    for size in sizes {
        let alignment = alignment_of(*size);
        if !offset.is_multiple_of(alignment) {
            offset += alignment - offset % alignment;
        }
        offsets.push(offset);
        offset += *size;
    }
    let size = align_to(offset, 8);
    Layout { offsets, size }
}

/// Cumulative offsets and total size for a sequence of typed elements, using
/// each element's stored size.
pub fn layout_of<'a, I>(elements: I) -> Layout
where
    I: IntoIterator<Item = &'a IRType>,
{
    let sizes: Vec<usize> = elements.into_iter().map(stored_size).collect();
    layout_of_sizes(&sizes)
}

/// Computed aggregate layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Byte offset of each element.
    pub offsets: Vec<usize>,
    /// Total size in bytes (aligned to 8).
    pub size: usize,
}
