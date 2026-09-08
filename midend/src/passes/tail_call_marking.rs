//! Tail-call marking (Onda 3).
//!
//! Detects direct **self**-recursion in tail position and flags it on the IR
//! so the backend can emit Cranelift's native `return_call` instead of a
//! call followed by a return. Cross-function tail calls are intentionally
//! out of scope: they would require changing the calling convention of two
//! functions and re-checking every entry path into them.
//!
//! A `Call` is marked when it is the last instruction of its block, calls the
//! enclosing function by name, and the block terminator is a `Return` of the
//! call's result with nothing in between. Anything that would make the tail
//! fusion unsafe at the backend level disables marking for the whole function:
//!
//! - host calls / manual allocations / dynamic fat pointers: these activate the
//!   backend's manual allocation frame; skipping `frame_exit` via a tail jump
//!   would leak per-iteration state;
//! - async instructions: async bodies run under reactor-owned trampolines;
//! - indirect calls: keep conservative.
//!
//! The pattern check itself is per-block, so a function can mix safe tail
//! blocks with arbitrary other code.

use crate::ir::{Function, InstructionKind, Module, Terminator};

/// Mark every qualifying self-tail-call in `module`. Returns how many calls
/// were marked.
pub fn mark_tail_self_recursion(module: &mut Module) -> usize {
    let mut marked = 0;
    for func in &mut module.functions {
        if function_is_tail_fusion_candidate(func) {
            let name = func.name.clone();
            for block in &mut func.blocks {
                let return_value_id = match &block.terminator {
                    Some(Terminator::Return { value: Some(value) }) => value.id,
                    _ => continue,
                };
                // The call must be the last instruction of the block: no drops,
                // escapes or other side effects may sit between call and return.
                let Some(last) = block.instructions.last_mut() else {
                    continue;
                };
                if let InstructionKind::Call {
                    result: Some(result),
                    function: callee,
                    is_tail,
                    ..
                } = &mut last.kind
                {
                    if !*is_tail && *callee == name && result.id == return_value_id {
                        *is_tail = true;
                        marked += 1;
                    }
                }
            }
        }
    }
    marked
}

/// A function qualifies for tail-fusion analysis only when none of its
/// instructions require backend machinery that a tail jump would skip.
fn function_is_tail_fusion_candidate(func: &Function) -> bool {
    if func.name == "main" || func.suspension_barrier {
        return false;
    }
    func.blocks.iter().all(|block| {
        block.instructions.iter().all(|instruction| {
            !matches!(
                &instruction.kind,
                InstructionKind::HostCall { .. }
                    | InstructionKind::ManualAlloc { .. }
                    | InstructionKind::EscapeManualAlloc { .. }
                    | InstructionKind::Alloca { .. }
                    | InstructionKind::MakeDynFatPtr { .. }
                    | InstructionKind::AsyncReady { .. }
                    | InstructionKind::CallIndirect { .. }
            )
        })
    })
}
