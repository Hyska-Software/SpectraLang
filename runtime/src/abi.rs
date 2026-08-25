//! Stable internal ABI catalog shared by the runtime and code generators.
//!
//! This module deliberately describes only the toolchain-facing runtime ABI.
//! It does not add a Spectra-language surface and it does not implement host
//! dispatch. The generic registry and its panic/error semantics remain in
//! [`crate::ffi`].
#![doc(hidden)]

use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU64};

/// Scalar types used by the Cranelift-facing runtime ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[doc(hidden)]
pub enum AbiScalar {
    I32,
    I64,
    F64,
}

/// A portable description of one native runtime import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct AbiSignature {
    pub params: &'static [AbiScalar],
    pub returns: &'static [AbiScalar],
}

const EMPTY: &[AbiScalar] = &[];
const I32: &[AbiScalar] = &[AbiScalar::I32];
const I64: &[AbiScalar] = &[AbiScalar::I64];
const I64_I64: &[AbiScalar] = &[AbiScalar::I64, AbiScalar::I64];
const I64_I64_I64: &[AbiScalar] = &[AbiScalar::I64, AbiScalar::I64, AbiScalar::I64];
const HOST_INVOKE_PARAMS: &[AbiScalar] = &[
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
];
const HOST_INVOKE_CACHED_PARAMS: &[AbiScalar] = &[
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
];
const SIX_I64: &[AbiScalar] = &[
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
    AbiScalar::I64,
];
const I64_F64: &[AbiScalar] = &[AbiScalar::I64, AbiScalar::F64];

/// Runtime imports declared by JIT and AOT code generation.
///
/// The order is stable and is used by the backend's compact indexed binding
/// table. Additions must be appended to preserve existing indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
#[doc(hidden)]
pub enum RuntimeImport {
    ManualAlloc,
    ManualFree,
    ManualFrameEnter,
    ManualFrameExit,
    ManualEscape,
    HostInvoke,
    HostInvokeBatch,
    ConcurrentSpawn,
    ConcurrentJoin,
    ConcurrentSpawnBatch,
    ConcurrentJoinBatchSum,
    ConcurrentSpawnJoin,
    ConcurrentReset,
    BuilderNew,
    BuilderPush,
    BuilderLen,
    BuilderFinish,
    BuilderFree,
    MapSet,
    MapGet,
    MapContains,
    MlLinear,
    MlMseLoss,
    TensorBackward,
    TensorAutodiffApply,
    TensorGradHandle,
    MlSgdStep,
    TensorFullF,
    StringLen,
    StringCharAt,
    MapNew,
    MapRemove,
    MapLen,
    MapClear,
    MapFree,
    ChannelNew,
    ChannelSend,
    ChannelRecv,
    ChannelClose,
    ChannelLen,
    /// Generic host dispatch using a module-owned registry cache slot.
    HostInvokeCached,
    /// Generic host batch dispatch using cache slots embedded in descriptors.
    HostInvokeCachedBatch,
    /// Fatal runtime error reporter used by Div/Rem zero checks and failed
    /// host calls. Prints `runtime error: <message>` and exits with 101.
    SpectraPanic,
}

impl RuntimeImport {
    pub const COUNT: usize = 43;

    pub const ALL: &'static [Self] = &[
        Self::ManualAlloc,
        Self::ManualFree,
        Self::ManualFrameEnter,
        Self::ManualFrameExit,
        Self::ManualEscape,
        Self::HostInvoke,
        Self::HostInvokeBatch,
        Self::ConcurrentSpawn,
        Self::ConcurrentJoin,
        Self::ConcurrentSpawnBatch,
        Self::ConcurrentJoinBatchSum,
        Self::ConcurrentSpawnJoin,
        Self::ConcurrentReset,
        Self::BuilderNew,
        Self::BuilderPush,
        Self::BuilderLen,
        Self::BuilderFinish,
        Self::BuilderFree,
        Self::MapSet,
        Self::MapGet,
        Self::MapContains,
        Self::MlLinear,
        Self::MlMseLoss,
        Self::TensorBackward,
        Self::TensorAutodiffApply,
        Self::TensorGradHandle,
        Self::MlSgdStep,
        Self::TensorFullF,
        Self::StringLen,
        Self::StringCharAt,
        Self::MapNew,
        Self::MapRemove,
        Self::MapLen,
        Self::MapClear,
        Self::MapFree,
        Self::ChannelNew,
        Self::ChannelSend,
        Self::ChannelRecv,
        Self::ChannelClose,
        Self::ChannelLen,
        Self::HostInvokeCached,
        Self::HostInvokeCachedBatch,
        Self::SpectraPanic,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::ManualAlloc => "spectra_rt_manual_alloc",
            Self::ManualFree => "spectra_rt_manual_free",
            Self::ManualFrameEnter => "spectra_rt_manual_frame_enter",
            Self::ManualFrameExit => "spectra_rt_manual_frame_exit",
            Self::ManualEscape => "spectra_rt_manual_escape",
            Self::HostInvoke => "spectra_rt_host_invoke",
            Self::HostInvokeBatch => "spectra_rt_host_invoke_batch",
            Self::ConcurrentSpawn => "spectra_rt_concurrent_spawn_fast",
            Self::ConcurrentJoin => "spectra_rt_concurrent_join_fast",
            Self::ConcurrentSpawnBatch => "spectra_rt_concurrent_spawn_batch_fast",
            Self::ConcurrentJoinBatchSum => "spectra_rt_concurrent_join_batch_sum_fast",
            Self::ConcurrentSpawnJoin => "spectra_rt_concurrent_spawn_join_fast",
            Self::ConcurrentReset => "spectra_rt_concurrent_reset_fast",
            Self::BuilderNew => "spectra_rt_builder_new",
            Self::BuilderPush => "spectra_rt_builder_push",
            Self::BuilderLen => "spectra_rt_builder_len",
            Self::BuilderFinish => "spectra_rt_builder_finish",
            Self::BuilderFree => "spectra_rt_builder_free",
            Self::MapSet => "spectra_rt_map_set_fast",
            Self::MapGet => "spectra_rt_map_get_fast",
            Self::MapContains => "spectra_rt_map_contains_fast",
            Self::MlLinear => "spectra_rt_ml_linear_fast",
            Self::MlMseLoss => "spectra_rt_ml_mse_loss_fast",
            Self::TensorBackward => "spectra_rt_tensor_backward_fast",
            Self::TensorAutodiffApply => "spectra_rt_tensor_autodiff_apply_fast",
            Self::TensorGradHandle => "spectra_rt_tensor_grad_handle_fast",
            Self::MlSgdStep => "spectra_rt_ml_sgd_step_fast",
            Self::TensorFullF => "spectra_rt_tensor_full_f_fast",
            Self::StringLen => "spectra_rt_string_len_fast",
            Self::StringCharAt => "spectra_rt_string_char_at_fast",
            Self::MapNew => "spectra_rt_map_new_fast",
            Self::MapRemove => "spectra_rt_map_remove_fast",
            Self::MapLen => "spectra_rt_map_len_fast",
            Self::MapClear => "spectra_rt_map_clear_fast",
            Self::MapFree => "spectra_rt_map_free_fast",
            Self::ChannelNew => "spectra_rt_channel_new_fast",
            Self::ChannelSend => "spectra_rt_channel_send_fast",
            Self::ChannelRecv => "spectra_rt_channel_recv_fast",
            Self::ChannelClose => "spectra_rt_channel_close_fast",
            Self::ChannelLen => "spectra_rt_channel_len_fast",
            Self::HostInvokeCached => "spectra_rt_host_invoke_cached",
            Self::HostInvokeCachedBatch => "spectra_rt_host_invoke_cached_batch",
            Self::SpectraPanic => "spectra_rt_panic",
        }
    }

    pub const fn signature(self) -> AbiSignature {
        let (params, returns) = match self {
            Self::ManualAlloc => (I64, I64),
            Self::ManualFree => (I64, EMPTY),
            Self::ManualFrameEnter => (EMPTY, I64),
            Self::ManualFrameExit => (I64, EMPTY),
            Self::ManualEscape => (I64_I64, EMPTY),
            Self::HostInvoke => (HOST_INVOKE_PARAMS, I32),
            Self::HostInvokeBatch => (I64_I64, I32),
            Self::HostInvokeCached => (HOST_INVOKE_CACHED_PARAMS, I32),
            Self::HostInvokeCachedBatch => (I64_I64, I32),
            Self::ConcurrentSpawn => (I64, I64),
            Self::ConcurrentJoin => (I64, I64),
            Self::ConcurrentSpawnBatch => (I64_I64, I64),
            Self::ConcurrentJoinBatchSum => (I64, I64),
            Self::ConcurrentSpawnJoin => (I64, I64),
            Self::ConcurrentReset => (EMPTY, I64),
            Self::BuilderNew => (I64, I64),
            Self::BuilderPush => (I64_I64, EMPTY),
            Self::BuilderLen => (I64, I64),
            Self::BuilderFinish => (I64, I64),
            Self::BuilderFree => (I64, EMPTY),
            Self::MapSet => (I64_I64_I64, I32),
            Self::MapGet => (I64_I64, I64),
            Self::MapContains => (I64_I64, I64),
            Self::MlLinear => (I64_I64_I64, I64),
            Self::MlMseLoss => (I64_I64, I64),
            Self::TensorBackward => (I64, I32),
            Self::TensorAutodiffApply => (SIX_I64, I32),
            Self::TensorGradHandle => (I64, I64),
            Self::MlSgdStep => (I64_F64, I32),
            Self::TensorFullF => (I64_F64, I64),
            Self::StringLen => (I64, I64),
            Self::StringCharAt => (I64_I64, I64),
            Self::MapNew => (EMPTY, I64),
            Self::MapRemove => (I64_I64, I64),
            Self::MapLen => (I64, I64),
            Self::MapClear => (I64, EMPTY),
            Self::MapFree => (I64, EMPTY),
            Self::ChannelNew => (EMPTY, I64),
            Self::ChannelSend => (I64_I64, I32),
            Self::ChannelRecv => (I64, I64),
            Self::ChannelClose => (I64, I32),
            Self::ChannelLen => (I64, I64),
            Self::SpectraPanic => (I64, EMPTY),
        };
        AbiSignature { params, returns }
    }

    /// Returns the native function address used when registering JIT symbols.
    #[doc(hidden)]
    pub fn address(self) -> *const u8 {
        use crate::ffi;

        match self {
            Self::ManualAlloc => ffi::spectra_rt_manual_alloc as *const u8,
            Self::ManualFree => ffi::spectra_rt_manual_free as *const u8,
            Self::ManualFrameEnter => ffi::spectra_rt_manual_frame_enter as *const u8,
            Self::ManualFrameExit => ffi::spectra_rt_manual_frame_exit as *const u8,
            Self::ManualEscape => ffi::spectra_rt_manual_escape as *const u8,
            Self::HostInvoke => ffi::spectra_rt_host_invoke as *const u8,
            Self::HostInvokeBatch => ffi::spectra_rt_host_invoke_batch as *const u8,
            Self::ConcurrentSpawn => ffi::spectra_rt_concurrent_spawn_fast as *const u8,
            Self::ConcurrentJoin => ffi::spectra_rt_concurrent_join_fast as *const u8,
            Self::ConcurrentSpawnBatch => ffi::spectra_rt_concurrent_spawn_batch_fast as *const u8,
            Self::ConcurrentJoinBatchSum => {
                ffi::spectra_rt_concurrent_join_batch_sum_fast as *const u8
            }
            Self::ConcurrentSpawnJoin => ffi::spectra_rt_concurrent_spawn_join_fast as *const u8,
            Self::ConcurrentReset => ffi::spectra_rt_concurrent_reset_fast as *const u8,
            Self::BuilderNew => ffi::spectra_rt_builder_new as *const u8,
            Self::BuilderPush => ffi::spectra_rt_builder_push as *const u8,
            Self::BuilderLen => ffi::spectra_rt_builder_len as *const u8,
            Self::BuilderFinish => ffi::spectra_rt_builder_finish as *const u8,
            Self::BuilderFree => ffi::spectra_rt_builder_free as *const u8,
            Self::MapSet => ffi::spectra_rt_map_set_fast as *const u8,
            Self::MapGet => ffi::spectra_rt_map_get_fast as *const u8,
            Self::MapContains => ffi::spectra_rt_map_contains_fast as *const u8,
            Self::MlLinear => ffi::spectra_rt_ml_linear_fast as *const u8,
            Self::MlMseLoss => ffi::spectra_rt_ml_mse_loss_fast as *const u8,
            Self::TensorBackward => ffi::spectra_rt_tensor_backward_fast as *const u8,
            Self::TensorAutodiffApply => ffi::spectra_rt_tensor_autodiff_apply_fast as *const u8,
            Self::TensorGradHandle => ffi::spectra_rt_tensor_grad_handle_fast as *const u8,
            Self::MlSgdStep => ffi::spectra_rt_ml_sgd_step_fast as *const u8,
            Self::TensorFullF => ffi::spectra_rt_tensor_full_f_fast as *const u8,
            Self::StringLen => ffi::spectra_rt_string_len_fast as *const u8,
            Self::StringCharAt => ffi::spectra_rt_string_char_at_fast as *const u8,
            Self::MapNew => ffi::spectra_rt_map_new_fast as *const u8,
            Self::MapRemove => ffi::spectra_rt_map_remove_fast as *const u8,
            Self::MapLen => ffi::spectra_rt_map_len_fast as *const u8,
            Self::MapClear => ffi::spectra_rt_map_clear_fast as *const u8,
            Self::MapFree => ffi::spectra_rt_map_free_fast as *const u8,
            Self::ChannelNew => ffi::spectra_rt_channel_new_fast as *const u8,
            Self::ChannelSend => ffi::spectra_rt_channel_send_fast as *const u8,
            Self::ChannelRecv => ffi::spectra_rt_channel_recv_fast as *const u8,
            Self::ChannelClose => ffi::spectra_rt_channel_close_fast as *const u8,
            Self::ChannelLen => ffi::spectra_rt_channel_len_fast as *const u8,
            Self::HostInvokeCached => ffi::spectra_rt_host_invoke_cached as *const u8,
            Self::HostInvokeCachedBatch => ffi::spectra_rt_host_invoke_cached_batch as *const u8,
            Self::SpectraPanic => crate::panic::spectra_rt_panic as *const u8,
        }
    }
}

/// Per-host-name cache storage embedded by generated JIT/AOT code.
///
/// The type is an internal toolchain ABI. The runtime owns its layout and
/// publishes the function pointer before publishing the generation, so a
/// reader that observes a matching generation also observes the pointer for
/// that generation. A zero pointer represents a cached `NOT_FOUND` result.
#[repr(C)]
#[doc(hidden)]
pub struct SpectraHostCallCache {
    pub(crate) generation: AtomicU64,
    pub(crate) function: AtomicPtr<()>,
}

impl SpectraHostCallCache {
    /// Creates an empty cache slot. Generated code keeps the slot alive for
    /// the entire lifetime of the compiled module.
    #[doc(hidden)]
    pub const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            function: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

impl Default for SpectraHostCallCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Host calls with an explicit lowering path. Generic host calls are not
/// enumerated because they remain dynamically registered by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
#[doc(hidden)]
pub enum FastHostCall {
    ConcurrentReset,
    StringLen,
    StringCharAt,
    ConcurrentSpawnJoin,
    ConcurrentSpawn,
    ConcurrentSpawnBatch,
    ConcurrentJoinBatchSum,
    ConcurrentJoin,
    BuilderNew,
    BuilderPush,
    BuilderLen,
    BuilderFinish,
    BuilderFree,
    MapSet,
    MapGet,
    MapContains,
    MapNew,
    MapRemove,
    MapLen,
    MapClear,
    MapFree,
    ChannelNew,
    ChannelSend,
    ChannelRecv,
    ChannelClose,
    ChannelLen,
    MlLinear,
    MlMseLoss,
    TensorBackward,
    MlSgdStep,
    TensorFullF,
}

impl FastHostCall {
    pub const COUNT: usize = 31;

    pub const ALL: &'static [Self] = &[
        Self::ConcurrentReset,
        Self::StringLen,
        Self::StringCharAt,
        Self::ConcurrentSpawnJoin,
        Self::ConcurrentSpawn,
        Self::ConcurrentSpawnBatch,
        Self::ConcurrentJoinBatchSum,
        Self::ConcurrentJoin,
        Self::BuilderNew,
        Self::BuilderPush,
        Self::BuilderLen,
        Self::BuilderFinish,
        Self::BuilderFree,
        Self::MapSet,
        Self::MapGet,
        Self::MapContains,
        Self::MapNew,
        Self::MapRemove,
        Self::MapLen,
        Self::MapClear,
        Self::MapFree,
        Self::ChannelNew,
        Self::ChannelSend,
        Self::ChannelRecv,
        Self::ChannelClose,
        Self::ChannelLen,
        Self::MlLinear,
        Self::MlMseLoss,
        Self::TensorBackward,
        Self::MlSgdStep,
        Self::TensorFullF,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn host_name(self) -> &'static str {
        match self {
            Self::ConcurrentReset => "spectra.std.concurrent.reset",
            Self::StringLen => "spectra.std.string.len",
            Self::StringCharAt => "spectra.std.string.char_at",
            Self::ConcurrentSpawnJoin => "spectra.std.concurrent.task_spawn_join",
            Self::ConcurrentSpawn => "spectra.std.concurrent.task_spawn",
            Self::ConcurrentSpawnBatch => "spectra.std.concurrent.task_spawn_batch",
            Self::ConcurrentJoinBatchSum => "spectra.std.concurrent.task_join_batch_sum",
            Self::ConcurrentJoin => "spectra.std.concurrent.task_join",
            Self::BuilderNew => "spectra.std.string.builder_new",
            Self::BuilderPush => "spectra.std.string.builder_push",
            Self::BuilderLen => "spectra.std.string.builder_len",
            Self::BuilderFinish => "spectra.std.string.builder_finish",
            Self::BuilderFree => "spectra.std.string.builder_free",
            Self::MapSet => "spectra.std.collections.map_set",
            Self::MapGet => spectra_contract::STD_COMPAT_COLLECTIONS_MAP_GET_BINDING,
            Self::MapContains => "spectra.std.collections.map_contains",
            Self::MapNew => "spectra.std.collections.map_new",
            Self::MapRemove => spectra_contract::STD_COMPAT_COLLECTIONS_MAP_REMOVE_BINDING,
            Self::MapLen => "spectra.std.collections.map_len",
            Self::MapClear => "spectra.std.collections.map_clear",
            Self::MapFree => "spectra.std.collections.map_free",
            Self::ChannelNew => "spectra.std.concurrent.channel_new",
            Self::ChannelSend => "spectra.std.concurrent.channel_send",
            Self::ChannelRecv => "spectra.std.concurrent.channel_recv",
            Self::ChannelClose => "spectra.std.concurrent.channel_close",
            Self::ChannelLen => "spectra.std.concurrent.channel_len",
            Self::MlLinear => "spectra.std.ml.linear",
            Self::MlMseLoss => "spectra.std.ml.mse_loss",
            Self::TensorBackward => "spectra.std.tensor.backward",
            Self::MlSgdStep => "spectra.std.ml.sgd_step",
            Self::TensorFullF => "spectra.std.tensor.full_f",
        }
    }

    pub const fn runtime_import(self) -> RuntimeImport {
        match self {
            Self::ConcurrentReset => RuntimeImport::ConcurrentReset,
            Self::StringLen => RuntimeImport::StringLen,
            Self::StringCharAt => RuntimeImport::StringCharAt,
            Self::ConcurrentSpawnJoin => RuntimeImport::ConcurrentSpawnJoin,
            Self::ConcurrentSpawn => RuntimeImport::ConcurrentSpawn,
            Self::ConcurrentSpawnBatch => RuntimeImport::ConcurrentSpawnBatch,
            Self::ConcurrentJoinBatchSum => RuntimeImport::ConcurrentJoinBatchSum,
            Self::ConcurrentJoin => RuntimeImport::ConcurrentJoin,
            Self::BuilderNew => RuntimeImport::BuilderNew,
            Self::BuilderPush => RuntimeImport::BuilderPush,
            Self::BuilderLen => RuntimeImport::BuilderLen,
            Self::BuilderFinish => RuntimeImport::BuilderFinish,
            Self::BuilderFree => RuntimeImport::BuilderFree,
            Self::MapSet => RuntimeImport::MapSet,
            Self::MapGet => RuntimeImport::MapGet,
            Self::MapContains => RuntimeImport::MapContains,
            Self::MapNew => RuntimeImport::MapNew,
            Self::MapRemove => RuntimeImport::MapRemove,
            Self::MapLen => RuntimeImport::MapLen,
            Self::MapClear => RuntimeImport::MapClear,
            Self::MapFree => RuntimeImport::MapFree,
            Self::ChannelNew => RuntimeImport::ChannelNew,
            Self::ChannelSend => RuntimeImport::ChannelSend,
            Self::ChannelRecv => RuntimeImport::ChannelRecv,
            Self::ChannelClose => RuntimeImport::ChannelClose,
            Self::ChannelLen => RuntimeImport::ChannelLen,
            Self::MlLinear => RuntimeImport::MlLinear,
            Self::MlMseLoss => RuntimeImport::MlMseLoss,
            Self::TensorBackward => RuntimeImport::TensorBackward,
            Self::MlSgdStep => RuntimeImport::MlSgdStep,
            Self::TensorFullF => RuntimeImport::TensorFullF,
        }
    }

    pub const fn arity(self) -> usize {
        self.runtime_import().signature().params.len()
    }

    pub const fn symbol(self) -> &'static str {
        self.runtime_import().symbol()
    }

    pub const fn result_form(self) -> &'static [AbiScalar] {
        self.runtime_import().signature().returns
    }

    /// Fast ABIs are deliberately excluded from the generic hostcall batch.
    pub const fn batch_eligible(self) -> bool {
        false
    }
}

/// Classification used by the backend before lowering a host call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum HostCallClass {
    Generic,
    Fast(FastHostCall),
}

impl HostCallClass {
    /// Generic hostcalls may enter the backend batch planner; dedicated fast
    /// ABIs already have their own direct lowering and must stay out of it.
    pub const fn batch_eligible(self) -> bool {
        matches!(self, Self::Generic)
    }
}

/// Classifies only the host names that have a dedicated lowering path.
pub fn classify_host_call(name: &str) -> HostCallClass {
    for fast in FastHostCall::ALL {
        if fast.host_name() == name {
            return HostCallClass::Fast(*fast);
        }
    }
    HostCallClass::Generic
}

/// Resolves a host call for lowering while enforcing the fast ABI arity.
///
/// Name-only classification remains useful to the batch planner, which must
/// keep every fast name out of the generic batch even when malformed IR has an
/// incorrect argument count. Lowering uses this arity-aware resolver so that
/// malformed or unknown calls take the existing generic path.
#[doc(hidden)]
pub fn resolve_host_call(name: &str, arity: usize) -> HostCallClass {
    match classify_host_call(name) {
        HostCallClass::Fast(fast) if fast.arity() == arity => HostCallClass::Fast(fast),
        _ => HostCallClass::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn runtime_import_catalog_is_complete_and_unique() {
        assert_eq!(RuntimeImport::ALL.len(), RuntimeImport::COUNT);
        let symbols: HashSet<_> = RuntimeImport::ALL
            .iter()
            .map(|item| item.symbol())
            .collect();
        assert_eq!(symbols.len(), RuntimeImport::COUNT);
        for import in RuntimeImport::ALL {
            assert_ne!(import.address(), std::ptr::null());
            assert_eq!(RuntimeImport::ALL[import.index()], *import);
        }
        assert_eq!(RuntimeImport::HostInvokeCached.signature().params.len(), 7);
        assert_eq!(
            RuntimeImport::HostInvokeCachedBatch
                .signature()
                .params
                .len(),
            2
        );
        assert_eq!(
            std::mem::size_of::<SpectraHostCallCache>(),
            2 * std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::align_of::<SpectraHostCallCache>(),
            std::mem::align_of::<usize>()
        );
    }

    #[test]
    fn fast_host_call_catalog_is_complete_and_unique() {
        assert_eq!(FastHostCall::ALL.len(), FastHostCall::COUNT);
        let names: HashSet<_> = FastHostCall::ALL
            .iter()
            .map(|item| item.host_name())
            .collect();
        assert_eq!(names.len(), FastHostCall::COUNT);
        for fast in FastHostCall::ALL {
            assert_eq!(
                classify_host_call(fast.host_name()),
                HostCallClass::Fast(*fast)
            );
            assert_eq!(FastHostCall::ALL[fast.index()], *fast);
            assert_eq!(fast.arity(), fast.runtime_import().signature().params.len());
            assert_eq!(fast.symbol(), fast.runtime_import().symbol());
            assert_eq!(
                fast.result_form(),
                fast.runtime_import().signature().returns
            );
            assert!(!fast.batch_eligible());
        }
        assert!(HostCallClass::Generic.batch_eligible());
    }

    #[test]
    fn unknown_host_calls_remain_generic() {
        assert_eq!(
            classify_host_call("spectra.api.http.request"),
            HostCallClass::Generic
        );
        assert_eq!(
            classify_host_call("spectra.std.test.dynamic"),
            HostCallClass::Generic
        );
    }

    #[test]
    fn incorrect_fast_arity_resolves_to_generic_lowering() {
        assert_eq!(
            resolve_host_call("spectra.std.compat.collections.map_get", 2),
            HostCallClass::Fast(FastHostCall::MapGet)
        );
        assert_eq!(
            resolve_host_call("spectra.std.compat.collections.map_get", 1),
            HostCallClass::Generic
        );
    }
}
