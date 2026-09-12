// Static call and field-access index over SIR modules.
//
// R-3203 (impact analysis): answer "what breaks if I change this symbol" from
// the call graph instead of text search. `direct_calls` is shared with the
// inlining pass so the pass and the index can never disagree about who calls
// whom.

use crate::ir::{Function, InstructionKind, Module, Type};
use crate::layout;
use std::collections::{HashMap, HashSet};

/// Direct call graph in the same orientation the inlining pass consumes it:
/// caller -> callees. Only `Call` instructions are followed; indirect and
/// host calls are not direct calls and never appear here. Every module
/// function gets an entry, even when it calls nothing, which is exactly what
/// the inlining pass expects when it checks a function for self-recursion.
pub fn direct_calls(module: &Module) -> HashMap<String, HashSet<String>> {
    let mut calls = HashMap::new();
    for function in &module.functions {
        let entry = calls
            .entry(function.name.clone())
            .or_insert_with(HashSet::new);
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let InstructionKind::Call { function, .. } = &instruction.kind {
                    entry.insert(function.clone());
                }
            }
        }
    }
    calls
}

/// Reverse index for impact analysis.
///
/// Built once from an immutable module and then queried by symbol. The index
/// is deliberately conservative: anything that cannot be resolved statically
/// is reported through [`ImpactIndex::unresolved_dynamic`] instead of being
/// guessed at.
pub struct ImpactIndex {
    callers: HashMap<String, HashSet<String>>,
    field_users: HashMap<String, HashSet<String>>,
    dynamic: Vec<String>,
}

impl ImpactIndex {
    /// Build the index from a module. Functions with indirect calls are
    /// collected into the dynamic bucket rather than attributed to a target.
    pub fn build(module: &Module) -> Self {
        let callers = reverse_callers(&direct_calls(module));

        let mut return_types: HashMap<String, Type> = HashMap::new();
        for function in &module.functions {
            return_types.insert(function.name.clone(), function.return_type.clone());
        }
        for function in &module.external_functions {
            return_types.insert(function.name.clone(), function.return_type.clone());
        }

        let mut field_users: HashMap<String, HashSet<String>> = HashMap::new();
        let mut dynamic: Vec<String> = Vec::new();
        for function in &module.functions {
            for symbol in field_accesses(function, &return_types) {
                field_users
                    .entry(symbol)
                    .or_insert_with(HashSet::new)
                    .insert(function.name.clone());
            }
            if has_indirect_call(function) {
                dynamic.push(function.name.clone());
            }
        }
        dynamic.sort();
        dynamic.dedup();

        Self {
            callers,
            field_users,
            dynamic,
        }
    }

    /// Sorted, deduplicated direct callers of `symbol`. An unknown symbol
    /// yields an empty vector.
    pub fn callers_of(&self, symbol: &str) -> Vec<String> {
        sorted(self.callers.get(symbol))
    }

    /// Sorted, deduplicated functions that read, write or construct the
    /// `Type.field` symbol. An unknown symbol yields an empty vector.
    pub fn field_users_of(&self, symbol: &str) -> Vec<String> {
        sorted(self.field_users.get(symbol))
    }

    /// Functions containing indirect calls that cannot be resolved
    /// statically (closures, `dyn` dispatch). Sorted and deduplicated.
    pub fn unresolved_dynamic(&self) -> Vec<String> {
        self.dynamic.clone()
    }
}

/// Invert the caller -> callees graph into callee -> callers.
///
/// The inlining pass consumes the forward orientation, so impact analysis
/// reverses it here instead of maintaining a second extraction path.
fn reverse_callers(graph: &HashMap<String, HashSet<String>>) -> HashMap<String, HashSet<String>> {
    let mut reversed: HashMap<String, HashSet<String>> = HashMap::new();
    for (caller, callees) in graph {
        for callee in callees {
            reversed
                .entry(callee.clone())
                .or_insert_with(HashSet::new)
                .insert(caller.clone());
        }
    }
    reversed
}

fn sorted(set: Option<&HashSet<String>>) -> Vec<String> {
    let mut values: Vec<String> = set.map(|s| s.iter().cloned().collect()).unwrap_or_default();
    values.sort();
    values.dedup();
    values
}

fn has_indirect_call(function: &Function) -> bool {
    function.blocks.iter().any(|block| {
        block
            .instructions
            .iter()
            .any(|instruction| matches!(instruction.kind, InstructionKind::CallIndirect { .. }))
    })
}

/// View a type as a struct aggregate, through a single pointer indirection.
fn struct_shape(ty: &Type) -> Option<(&str, &[(String, Type)])> {
    match ty {
        Type::Struct { name, fields } => Some((name.as_str(), fields.as_slice())),
        Type::Pointer(inner) => match inner.as_ref() {
            Type::Struct { name, fields } => Some((name.as_str(), fields.as_slice())),
            _ => None,
        },
        _ => None,
    }
}

/// Resolve the `Type.field` symbol at a padded byte offset.
fn field_symbol(ty: &Type, offset: i64) -> Option<String> {
    if offset < 0 {
        return None;
    }
    let (name, fields) = struct_shape(ty)?;
    let offsets = layout::layout_of(fields.iter().map(|(_, field_ty)| field_ty)).offsets;
    fields
        .iter()
        .enumerate()
        .find(|(index, _)| offsets.get(*index).copied() == Some(offset as usize))
        .map(|(_, (field_name, _))| format!("{name}.{field_name}"))
}

/// Field type reached by a `FieldPtr` with a padded byte offset.
fn field_type_at(ty: &Type, offset: i64) -> Option<Type> {
    if offset < 0 {
        return None;
    }
    let (_, fields) = struct_shape(ty)?;
    let offsets = layout::layout_of(fields.iter().map(|(_, field_ty)| field_ty)).offsets;
    fields
        .iter()
        .enumerate()
        .find(|(index, _)| offsets.get(*index).copied() == Some(offset as usize))
        .map(|(_, (_, field_ty))| field_ty.clone())
}

/// Value ids that carry a known IR type, used to type the base pointer of a
/// `FieldPtr`. Only pointer-producing instructions are tracked; a value whose
/// type cannot be proven is simply absent.
fn collect_value_types(
    function: &Function,
    return_types: &HashMap<String, Type>,
) -> HashMap<usize, Type> {
    let mut types: HashMap<usize, Type> = HashMap::new();
    for param in &function.params {
        types.insert(param.id, param.ty.clone());
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            match &instruction.kind {
                InstructionKind::Alloca { result, ty } => {
                    types.insert(result.id, ty.clone());
                }
                InstructionKind::Copy { result, source } => {
                    if let Some(ty) = types.get(&source.id).cloned() {
                        types.insert(result.id, ty);
                    }
                }
                InstructionKind::Phi { result, incoming } => {
                    let mut incoming_types = incoming
                        .iter()
                        .filter_map(|(value, _)| types.get(&value.id));
                    let resolved = incoming_types.next().cloned().filter(|first| {
                        incoming_types.all(|candidate| candidate == first)
                    });
                    if let Some(ty) = resolved {
                        types.insert(result.id, ty);
                    }
                }
                InstructionKind::Load { result, ty, .. } => {
                    types.insert(result.id, ty.clone());
                }
                InstructionKind::GetElementPtr {
                    result,
                    element_type,
                    ..
                } => {
                    types.insert(result.id, element_type.clone());
                }
                InstructionKind::Call {
                    result: Some(result),
                    function,
                    ..
                } => {
                    if let Some(ty) = return_types.get(function).cloned() {
                        types.insert(result.id, ty);
                    }
                }
                InstructionKind::FieldPtr { result, ptr, offset } => {
                    if let Some(ty) = types.get(&ptr.id).and_then(|ty| field_type_at(ty, *offset)) {
                        types.insert(result.id, ty);
                    }
                }
                _ => {}
            }
        }
    }
    types
}

/// All `Type.field` symbols read, written or constructed by the function.
fn field_accesses(function: &Function, return_types: &HashMap<String, Type>) -> HashSet<String> {
    let types = collect_value_types(function, return_types);
    let mut touched = HashSet::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let InstructionKind::FieldPtr { ptr, offset, .. } = &instruction.kind {
                if let Some(symbol) = types.get(&ptr.id).and_then(|ty| field_symbol(ty, *offset)) {
                    touched.insert(symbol);
                }
            }
        }
    }
    touched
}
