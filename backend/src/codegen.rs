// Code generation using Cranelift JIT
// Translates Spectra IR to native machine code

use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use spectra_midend::ir::{
    BasicBlock as IRBasicBlock, Constant, Function as IRFunction, Global, Instruction,
    InstructionKind, Module as IRModule, Terminator, Type as IRType, Value as IRValue,
};
use spectra_midend::{TensorDevice, TensorGraph, TensorGraphLoweringReport};
use std::collections::{HashMap, HashSet};

use crate::error::{BackendCodegenError, BackendResult};
use crate::hostcall_abi::{
    declare_runtime_bindings, intern_jit_host_call_site, register_jit_runtime_symbols,
    HostCallLoweringContext, HostCallSiteRecord, RuntimeBindings,
};
use spectra_runtime::abi::{
    classify_host_call, resolve_host_call, FastHostCall, HostCallClass, RuntimeImport,
    SpectraHostCallCache,
};

/// Dense SSA value lookup used by both JIT and AOT lowering.
///
/// IR values are assigned monotonically by `IRFunction::next_value_id`, so a
/// vector avoids hashing on the hot path while still handling synthetic IR
/// with sparse or late-created ids safely.
#[derive(Debug, Default)]
pub(crate) struct DenseValueMap {
    values: Vec<Option<Value>>,
}

impl DenseValueMap {
    pub(crate) fn with_capacity(next_value_id: usize) -> Self {
        Self {
            values: vec![None; next_value_id],
        }
    }

    pub(crate) fn insert(&mut self, id: usize, value: Value) {
        if id >= self.values.len() {
            self.values.resize_with(id + 1, || None);
        }
        self.values[id] = Some(value);
    }

    pub(crate) fn get(&self, id: usize) -> Option<Value> {
        self.values.get(id).copied().flatten()
    }
}

/// Compile-time accounting for the conservative R-3105 hostcall planner.
///
/// The counters describe generated sites, not runtime invocations. They are
/// intentionally kept in the backend so benchmark evidence can prove that a
/// candidate actually emitted batches without changing the language or IR
/// surface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostCallBatchStats {
    pub batched_sites: usize,
    pub batched_hostcalls: usize,
    pub fallback_hostcalls: usize,
    pub argument_arena_bytes: usize,
    pub result_arena_bytes: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct HostNameRecord {
    pub(crate) ptr: u64,
    pub(crate) len: usize,
    /// When `Some`, the name resides in a Cranelift data section (AOT mode).
    /// When `None`, `ptr` is a compile-time heap pointer valid in JIT mode.
    pub(crate) data_id: Option<DataId>,
}

/// Storage record for a string literal (R-3126).
///
/// Resolves a `ConstString` IR value to a stable pointer. In JIT mode the
/// bytes are allocated on the heap (in `string_literal_storage`) and `ptr`
/// is the heap address. In AOT mode the bytes live in a `.rodata` data
/// section and `data_id` is the Cranelift handle. Either way, the bytes
/// are stored null-terminated, one byte per `i64` slot, and `len_with_null`
/// is the total slot count (including the trailing null terminator).
#[derive(Clone, Copy)]
pub(crate) struct StringLiteralRecord {
    pub(crate) ptr: u64,
    pub(crate) len_with_null: i64,
    pub(crate) data_id: Option<DataId>,
}

/// Resolves a string literal to a stable pointer + length (R-3126).
///
/// In JIT mode (when the entry is not already interned) this allocates a
/// null-terminated byte buffer on the heap and stores it in
/// `string_literal_storage` so the pointer outlives any JIT function that
/// references it. The buffer is laid out as one byte per `i64` slot
/// (matching the existing `IRType::Array{Int, N+1}` representation used
/// by `emit_stack_string_char_at_inline` which indexes with `*8`).
/// In AOT mode the entry is pre-populated by
/// [`AotCodeGenerator::pre_intern_string_literals`] with a `data_id`, so
/// the heap fallback never fires.
pub(crate) fn intern_string_literal(
    string_literal_data: &mut HashMap<String, StringLiteralRecord>,
    string_literal_storage: &mut Vec<Box<[i64]>>,
    value: &str,
) -> StringLiteralRecord {
    if let Some(record) = string_literal_data.get(value) {
        return *record;
    }

    let mut slots: Vec<i64> = value.as_bytes().iter().map(|&b| b as i64).collect();
    slots.push(0);
    let boxed: Box<[i64]> = slots.into_boxed_slice();
    let ptr = boxed.as_ptr() as u64;
    let len_with_null = boxed.len() as i64;
    string_literal_storage.push(boxed);

    let record = StringLiteralRecord {
        ptr,
        len_with_null,
        data_id: None,
    };
    string_literal_data.insert(value.to_string(), record);
    record
}

pub struct CodeGenerator {
    /// Cranelift JIT module
    module: JITModule,
    /// Function builder context
    ctx: codegen::Context,
    /// Builder for creating IR
    builder_context: FunctionBuilderContext,
    /// Mapping from IR function names to Cranelift function IDs
    function_map: HashMap<String, FuncId>,
    /// Mapping from IR global names to writable Cranelift data objects.
    global_data: HashMap<String, DataId>,
    /// Raw addresses for functions finalized by an earlier module.  JIT
    /// `func_addr` references are module-local during lowering; using the
    /// finalized address makes cross-module dyn-vtable entries callable.
    finalized_function_ptrs: HashMap<String, i64>,
    /// Runtime imports declared from the runtime-owned ABI catalog.
    runtime_bindings: RuntimeBindings,
    /// Dedup table for string literals (R-3126). Each unique `ConstString`
    /// value resolves to one entry; see [`intern_string_literal`].
    string_literal_data: HashMap<String, StringLiteralRecord>,
    /// Owned storage for JIT-mode string literal buffers (R-3126).
    /// Each `ConstString` IR instruction resolves to a stable pointer
    /// into one of these buffers. The buffers must outlive any JIT
    /// function that references them. Layout is one byte per `i64`
    /// slot (matches `IRType::Array{Int, N+1}` and the `*8` indexing
    /// in `emit_stack_string_char_at_inline`).
    string_literal_storage: Vec<Box<[i64]>>,
    host_call_sites: HashMap<String, HostCallSiteRecord>,
    host_name_storage: Vec<Box<[u8]>>,
    // Box keeps cache addresses stable while the storage Vec grows.
    #[allow(clippy::vec_box)]
    host_call_cache_storage: Vec<Box<SpectraHostCallCache>>,
    hostcall_batch_stats: HostCallBatchStats,
}

/// Describes a PHI node so that the backend can emit Cranelift block parameters.
#[derive(Debug, Clone)]
pub(crate) struct PhiDescriptor {
    pub result_id: usize,
    pub incoming: HashMap<usize, usize>, // predecessor_block_id -> incoming_value_id
}

pub(crate) fn validate_tensor_ir(ir_module: &IRModule) -> BackendResult<TensorGraphLoweringReport> {
    let graph = TensorGraph::from_ir_module(ir_module);
    let backend = if graph.functions.iter().any(|function| {
        function
            .nodes
            .iter()
            .any(|node| node.output.device == TensorDevice::Wgpu)
    }) {
        TensorDevice::Wgpu
    } else {
        TensorDevice::Cpu
    };
    graph
        .lower_for_backend(backend)
        .map(|result| result.report)
        .map_err(|errors| {
            let details = errors
                .iter()
                .map(|error| {
                    format!(
                        "{} function='{}' node={:?}: {}",
                        error.kind.diagnostic_code(),
                        error.function,
                        error.node,
                        error.message
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            BackendCodegenError::tensor_ir(format!("Tensor IR legalization failed: {details}"))
        })
}

/// Collect the Cranelift block arguments that should be passed for the PHIs
/// in `target_block` when jumping from `current_block`.
fn get_phi_args(
    target_block: usize,
    current_block: usize,
    phi_map: &HashMap<usize, Vec<PhiDescriptor>>,
    value_map: &DenseValueMap,
) -> BackendResult<Vec<cranelift_codegen::ir::BlockArg>> {
    let mut args = Vec::new();
    if let Some(phis) = phi_map.get(&target_block) {
        for phi in phis {
            let incoming_id = phi.incoming.get(&current_block).ok_or_else(|| {
                BackendCodegenError::missing_phi_incoming(current_block, target_block)
            })?;
            let val = value_map
                .get(*incoming_id)
                .ok_or_else(|| BackendCodegenError::missing_value(*incoming_id))?;
            args.push(val.into());
        }
    }
    Ok(args)
}

include!("codegen_core.rs");
include!("codegen_alloca.rs");
include!("codegen_hostcalls.rs");
include!("codegen_block.rs");
include!("codegen_instruction_core.rs");
include!("codegen_instruction_arithmetic.rs");
include!("codegen_instruction_memory.rs");
include!("codegen_instruction_calls.rs");
include!("codegen_instruction_host.rs");
include!("codegen_instruction_async.rs");
include!("codegen_instruction_indirect.rs");
include!("codegen_instruction_cast.rs");
include!("codegen_instruction_dyn.rs");
include!("codegen_instruction_values.rs");
include!("codegen_strings.rs");
include!("codegen_default.rs");
include!("codegen_tests.rs");
