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
    ConcurrentJoin,
    ConcurrentSpawnBatch,
    ConcurrentJoinBatchSum,
    ConcurrentReset,
    BuilderNew,
    BuilderPush,
    BuilderLen,
    BuilderFinish,
    BuilderFree,
    MapSet,
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
    /// Real-concurrency spawn of a JIT closure onto the worker pool.
    ConcurrentSpawnFn,
    CoroutineFrameAlloc,
    CoroutineFrameStore,
    CoroutineFrameLoad,
    /// Durable per-frame storage for a promoted-local alloca.
    CoroutineLocalPtr,
    CoroutineStateLoad,
    CoroutineStateStore,
    CoroutineCreate,
    CoroutinePollChild,
    CoroutinePollResult,
    CoroutineSubscribe,
    CoroutineWake,
    CoroutineSuspend,
    CoroutineComplete,
    CoroutineError,
    CoroutineCancelled,
    CoroutinePollReturn,
    /// Fatal capability-denial reporter used by the generic host-call lowering
    /// when the dispatcher returns `HOST_STATUS_DENIED`. Prints
    /// `capability denied: <message>` and exits with 101.
    HostDenied,
    ListNew,
    ListPush,
    ListLen,
    ListGet,
    ListGetOption,
    ListSet,
    ListContains,
    ListClear,
    ListFree,
    ListFreeAll,
    ListPop,
    ListPopFront,
    ListPopOption,
    ListPopFrontOption,
    ListInsertAt,
    ListRemoveAt,
    ListRemoveAtOption,
    ListIndexOf,
    ListSort,
    MapGet,
    MapGetOption,
    MapRemove,
    MapRemoveOption,
    MapIsEmpty,
    MapFreeAll,
    StackNew,
    StackPush,
    StackPop,
    StackPeek,
    StackLen,
    StackIsEmpty,
    StackClear,
    StackFree,
    StackFreeAll,
    QueueNew,
    QueueEnqueue,
    QueueDequeue,
    QueuePeek,
    QueueLen,
    QueueIsEmpty,
    QueueClear,
    QueueFree,
    QueueFreeAll,
    IteratorNext,
    IteratorNextUnchecked,
    IteratorRemaining,
    IteratorFree,
    ListIter,
    SetIter,
    MapIter,
    MapValuesIter,
    StackIter,
    QueueIter,
    /// Internal scalar-only collection calls. These are appended so the
    /// existing ABI indexes remain stable.
    MapSetScalar,
    MapContainsScalar,
    MapGetScalar,
    MapRemoveScalar,
}

impl RuntimeImport {
    pub const COUNT: usize = 114;

    pub const ALL: &'static [Self] = &[
        Self::ManualAlloc,
        Self::ManualFree,
        Self::ManualFrameEnter,
        Self::ManualFrameExit,
        Self::ManualEscape,
        Self::HostInvoke,
        Self::HostInvokeBatch,
        Self::ConcurrentJoin,
        Self::ConcurrentSpawnBatch,
        Self::ConcurrentJoinBatchSum,
        Self::ConcurrentReset,
        Self::BuilderNew,
        Self::BuilderPush,
        Self::BuilderLen,
        Self::BuilderFinish,
        Self::BuilderFree,
        Self::MapSet,
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
        Self::ConcurrentSpawnFn,
        Self::CoroutineFrameAlloc,
        Self::CoroutineFrameStore,
        Self::CoroutineFrameLoad,
        Self::CoroutineLocalPtr,
        Self::CoroutineStateLoad,
        Self::CoroutineStateStore,
        Self::CoroutineCreate,
        Self::CoroutinePollChild,
        Self::CoroutinePollResult,
        Self::CoroutineSubscribe,
        Self::CoroutineWake,
        Self::CoroutineSuspend,
        Self::CoroutineComplete,
        Self::CoroutineError,
        Self::CoroutineCancelled,
        Self::CoroutinePollReturn,
        Self::HostDenied,
        Self::ListNew,
        Self::ListPush,
        Self::ListLen,
        Self::ListGet,
        Self::ListGetOption,
        Self::ListSet,
        Self::ListContains,
        Self::ListClear,
        Self::ListFree,
        Self::ListFreeAll,
        Self::ListPop,
        Self::ListPopFront,
        Self::ListPopOption,
        Self::ListPopFrontOption,
        Self::ListInsertAt,
        Self::ListRemoveAt,
        Self::ListRemoveAtOption,
        Self::ListIndexOf,
        Self::ListSort,
        Self::MapGet,
        Self::MapGetOption,
        Self::MapRemove,
        Self::MapRemoveOption,
        Self::MapIsEmpty,
        Self::MapFreeAll,
        Self::StackNew,
        Self::StackPush,
        Self::StackPop,
        Self::StackPeek,
        Self::StackLen,
        Self::StackIsEmpty,
        Self::StackClear,
        Self::StackFree,
        Self::StackFreeAll,
        Self::QueueNew,
        Self::QueueEnqueue,
        Self::QueueDequeue,
        Self::QueuePeek,
        Self::QueueLen,
        Self::QueueIsEmpty,
        Self::QueueClear,
        Self::QueueFree,
        Self::QueueFreeAll,
        Self::IteratorNext,
        Self::IteratorNextUnchecked,
        Self::IteratorRemaining,
        Self::IteratorFree,
        Self::ListIter,
        Self::SetIter,
        Self::MapIter,
        Self::MapValuesIter,
        Self::StackIter,
        Self::QueueIter,
        Self::MapSetScalar,
        Self::MapContainsScalar,
        Self::MapGetScalar,
        Self::MapRemoveScalar,
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
            Self::ConcurrentJoin => "spectra_rt_concurrent_join_fast",
            Self::ConcurrentSpawnBatch => "spectra_rt_concurrent_spawn_batch_fast",
            Self::ConcurrentJoinBatchSum => "spectra_rt_concurrent_join_batch_sum_fast",
            Self::ConcurrentSpawnFn => "spectra_rt_concurrent_spawn_fn_fast",
            Self::ConcurrentReset => "spectra_rt_concurrent_reset_fast",
            Self::BuilderNew => "spectra_rt_builder_new",
            Self::BuilderPush => "spectra_rt_builder_push",
            Self::BuilderLen => "spectra_rt_builder_len",
            Self::BuilderFinish => "spectra_rt_builder_finish",
            Self::BuilderFree => "spectra_rt_builder_free",
            Self::MapSet => "spectra_rt_map_set_fast",
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
            Self::CoroutineFrameAlloc => "spectra_rt_coroutine_frame_alloc",
            Self::CoroutineFrameStore => "spectra_rt_coroutine_frame_store",
            Self::CoroutineFrameLoad => "spectra_rt_coroutine_frame_load",
            Self::CoroutineLocalPtr => "spectra_rt_coroutine_local_ptr",
            Self::CoroutineStateLoad => "spectra_rt_coroutine_state_load",
            Self::CoroutineStateStore => "spectra_rt_coroutine_state_store",
            Self::CoroutineCreate => "spectra_rt_coroutine_create",
            Self::CoroutinePollChild => "spectra_rt_coroutine_poll_child",
            Self::CoroutinePollResult => "spectra_rt_coroutine_poll_result",
            Self::CoroutineSubscribe => "spectra_rt_coroutine_subscribe",
            Self::CoroutineWake => "spectra_rt_coroutine_wake",
            Self::CoroutineSuspend => "spectra_rt_coroutine_suspend",
            Self::CoroutineComplete => "spectra_rt_coroutine_complete",
            Self::CoroutineError => "spectra_rt_coroutine_error",
            Self::CoroutineCancelled => "spectra_rt_coroutine_cancelled",
            Self::CoroutinePollReturn => "spectra_rt_coroutine_poll_return",
            Self::HostDenied => "spectra_rt_capability_denied",
            Self::ListNew => "spectra_rt_list_new_fast",
            Self::ListPush => "spectra_rt_list_push_value_fast",
            Self::ListLen => "spectra_rt_list_len_fast",
            Self::ListGet => "spectra_rt_list_get_fast",
            Self::ListGetOption => "spectra_rt_list_get_option_fast",
            Self::ListSet => "spectra_rt_list_set_fast",
            Self::ListContains => "spectra_rt_list_contains_fast",
            Self::ListClear => "spectra_rt_list_clear_fast",
            Self::ListFree => "spectra_rt_list_free_fast",
            Self::ListFreeAll => "spectra_rt_list_free_all_fast",
            Self::ListPop => "spectra_rt_list_pop_fast",
            Self::ListPopFront => "spectra_rt_list_pop_front_fast",
            Self::ListPopOption => "spectra_rt_list_pop_option_fast",
            Self::ListPopFrontOption => "spectra_rt_list_pop_front_option_fast",
            Self::ListInsertAt => "spectra_rt_list_insert_at_fast",
            Self::ListRemoveAt => "spectra_rt_list_remove_at_fast",
            Self::ListRemoveAtOption => "spectra_rt_list_remove_at_option_fast",
            Self::ListIndexOf => "spectra_rt_list_index_of_fast",
            Self::ListSort => "spectra_rt_list_sort_fast",
            Self::MapGet => "spectra_rt_map_get_fast",
            Self::MapGetOption => "spectra_rt_map_get_option_fast",
            Self::MapRemove => "spectra_rt_map_remove_fast",
            Self::MapRemoveOption => "spectra_rt_map_remove_option_fast",
            Self::MapIsEmpty => "spectra_rt_map_is_empty_fast",
            Self::MapFreeAll => "spectra_rt_map_free_all_fast",
            Self::StackNew => "spectra_rt_stack_new_fast",
            Self::StackPush => "spectra_rt_stack_push_fast",
            Self::StackPop => "spectra_rt_stack_pop_fast",
            Self::StackPeek => "spectra_rt_stack_peek_fast",
            Self::StackLen => "spectra_rt_stack_len_fast",
            Self::StackIsEmpty => "spectra_rt_stack_is_empty_fast",
            Self::StackClear => "spectra_rt_stack_clear_fast",
            Self::StackFree => "spectra_rt_stack_free_fast",
            Self::StackFreeAll => "spectra_rt_stack_free_all_fast",
            Self::QueueNew => "spectra_rt_queue_new_fast",
            Self::QueueEnqueue => "spectra_rt_queue_enqueue_fast",
            Self::QueueDequeue => "spectra_rt_queue_dequeue_fast",
            Self::QueuePeek => "spectra_rt_queue_peek_fast",
            Self::QueueLen => "spectra_rt_queue_len_fast",
            Self::QueueIsEmpty => "spectra_rt_queue_is_empty_fast",
            Self::QueueClear => "spectra_rt_queue_clear_fast",
            Self::QueueFree => "spectra_rt_queue_free_fast",
            Self::QueueFreeAll => "spectra_rt_queue_free_all_fast",
            Self::IteratorNext => "spectra_rt_iterator_next_fast",
            Self::IteratorNextUnchecked => "spectra_rt_iterator_next_unchecked_fast",
            Self::IteratorRemaining => "spectra_rt_iterator_remaining_fast",
            Self::IteratorFree => "spectra_rt_iterator_free_fast",
            Self::ListIter => "spectra_rt_list_iter_fast",
            Self::SetIter => "spectra_rt_set_iter_fast",
            Self::MapIter => "spectra_rt_map_iter_fast",
            Self::MapValuesIter => "spectra_rt_map_values_iter_fast",
            Self::StackIter => "spectra_rt_stack_iter_fast",
            Self::QueueIter => "spectra_rt_queue_iter_fast",
            Self::MapSetScalar => "spectra_rt_map_set_scalar_fast",
            Self::MapContainsScalar => "spectra_rt_map_contains_scalar_fast",
            Self::MapGetScalar => "spectra_rt_map_get_scalar_fast",
            Self::MapRemoveScalar => "spectra_rt_map_remove_scalar_fast",
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
            Self::ConcurrentJoin => (I64, I64),
            Self::ConcurrentSpawnBatch => (I64_I64, I64),
            Self::ConcurrentJoinBatchSum => (I64, I64),
            Self::ConcurrentReset => (EMPTY, I64),
            Self::ConcurrentSpawnFn => (I64_I64, I64),
            Self::BuilderNew => (I64, I64),
            Self::BuilderPush => (I64_I64, EMPTY),
            Self::BuilderLen => (I64, I64),
            Self::BuilderFinish => (I64, I64),
            Self::BuilderFree => (I64, EMPTY),
            Self::MapSet => (I64_I64_I64, I32),
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
            Self::MapLen => (I64, I64),
            Self::MapClear => (I64, EMPTY),
            Self::MapFree => (I64, EMPTY),
            Self::ChannelNew => (EMPTY, I64),
            Self::ChannelSend => (I64_I64, I32),
            Self::ChannelRecv => (I64, I64),
            Self::ChannelClose => (I64, I32),
            Self::ChannelLen => (I64, I64),
            Self::SpectraPanic => (I64, EMPTY),
            Self::HostDenied => (I64, EMPTY),
            Self::ListNew => (EMPTY, I64),
            Self::ListPush => (I64_I64, I64),
            Self::ListLen => (I64, I64),
            Self::ListGet => (I64_I64, I64),
            Self::ListGetOption => (I64_I64, I64),
            Self::ListSet => (I64_I64_I64, I32),
            Self::ListContains => (I64_I64, I64),
            Self::ListClear => (I64, I32),
            Self::ListFree => (I64, I32),
            Self::ListFreeAll => (EMPTY, I64),
            Self::ListPop => (I64, I64),
            Self::ListPopFront => (I64, I64),
            Self::ListPopOption => (I64, I64),
            Self::ListPopFrontOption => (I64, I64),
            Self::ListInsertAt => (I64_I64_I64, I32),
            Self::ListRemoveAt => (I64_I64, I64),
            Self::ListRemoveAtOption => (I64_I64, I64),
            Self::ListIndexOf => (I64_I64, I64),
            Self::ListSort => (I64, I32),
            Self::MapGet => (I64_I64, I64),
            Self::MapGetOption => (I64_I64, I64),
            Self::MapRemove => (I64_I64, I64),
            Self::MapRemoveOption => (I64_I64, I64),
            Self::MapIsEmpty => (I64, I64),
            Self::MapFreeAll => (EMPTY, I64),
            Self::StackNew => (EMPTY, I64),
            Self::StackPush => (I64_I64, I32),
            Self::StackPop => (I64, I64),
            Self::StackPeek => (I64, I64),
            Self::StackLen => (I64, I64),
            Self::StackIsEmpty => (I64, I64),
            Self::StackClear => (I64, I32),
            Self::StackFree => (I64, I32),
            Self::StackFreeAll => (EMPTY, I64),
            Self::QueueNew => (EMPTY, I64),
            Self::QueueEnqueue => (I64_I64, I32),
            Self::QueueDequeue => (I64, I64),
            Self::QueuePeek => (I64, I64),
            Self::QueueLen => (I64, I64),
            Self::QueueIsEmpty => (I64, I64),
            Self::QueueClear => (I64, I32),
            Self::QueueFree => (I64, I32),
            Self::QueueFreeAll => (EMPTY, I64),
            Self::IteratorNext => (I64, I64),
            Self::IteratorNextUnchecked => (I64, I64),
            Self::IteratorRemaining => (I64, I64),
            Self::IteratorFree => (I64, I32),
            Self::ListIter => (I64, I64),
            Self::SetIter => (I64, I64),
            Self::MapIter => (I64, I64),
            Self::MapValuesIter => (I64, I64),
            Self::StackIter => (I64, I64),
            Self::QueueIter => (I64, I64),
            Self::MapSetScalar => (I64_I64_I64, I32),
            Self::MapContainsScalar => (I64_I64, I64),
            Self::MapGetScalar => (I64_I64, I64),
            Self::MapRemoveScalar => (I64_I64, I64),
            Self::CoroutineFrameAlloc => (I64, I64),
            Self::CoroutineFrameStore => (I64_I64_I64, I64),
            Self::CoroutineFrameLoad => (I64_I64, I64),
            Self::CoroutineLocalPtr => (I64_I64_I64, I64),
            Self::CoroutineStateLoad => (I64, I64),
            Self::CoroutineStateStore => (I64_I64, I64),
            Self::CoroutineCreate => (I64_I64_I64, I64),
            Self::CoroutinePollChild => (I64, I64),
            Self::CoroutinePollResult => (I64, I64),
            Self::CoroutineSubscribe => (I64_I64, I64),
            Self::CoroutineWake => (I64, I64),
            Self::CoroutineSuspend => (I64_I64, I64),
            Self::CoroutineComplete => (I64_I64, I64),
            Self::CoroutineError => (I64_I64, I64),
            Self::CoroutineCancelled => (I64, I64),
            Self::CoroutinePollReturn => (I64, EMPTY),
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
            Self::ConcurrentJoin => ffi::spectra_rt_concurrent_join_fast as *const u8,
            Self::ConcurrentSpawnBatch => ffi::spectra_rt_concurrent_spawn_batch_fast as *const u8,
            Self::ConcurrentJoinBatchSum => {
                ffi::spectra_rt_concurrent_join_batch_sum_fast as *const u8
            }
            Self::ConcurrentSpawnFn => ffi::spectra_rt_concurrent_spawn_fn_fast as *const u8,
            Self::ConcurrentReset => ffi::spectra_rt_concurrent_reset_fast as *const u8,
            Self::BuilderNew => ffi::spectra_rt_builder_new as *const u8,
            Self::BuilderPush => ffi::spectra_rt_builder_push as *const u8,
            Self::BuilderLen => ffi::spectra_rt_builder_len as *const u8,
            Self::BuilderFinish => ffi::spectra_rt_builder_finish as *const u8,
            Self::BuilderFree => ffi::spectra_rt_builder_free as *const u8,
            Self::MapSet => ffi::spectra_rt_map_set_fast as *const u8,
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
            Self::HostDenied => crate::panic::spectra_rt_capability_denied as *const u8,
            Self::ListNew => crate::ffi::spectra_rt_list_new_fast as *const u8,
            Self::ListPush => crate::ffi::spectra_rt_list_push_value_fast as *const u8,
            Self::ListLen => crate::ffi::spectra_rt_list_len_fast as *const u8,
            Self::ListGet => crate::ffi::spectra_rt_list_get_fast as *const u8,
            Self::ListGetOption => crate::ffi::spectra_rt_list_get_option_fast as *const u8,
            Self::ListSet => crate::ffi::spectra_rt_list_set_fast as *const u8,
            Self::ListContains => crate::ffi::spectra_rt_list_contains_fast as *const u8,
            Self::ListClear => crate::ffi::spectra_rt_list_clear_fast as *const u8,
            Self::ListFree => crate::ffi::spectra_rt_list_free_fast as *const u8,
            Self::ListFreeAll => crate::ffi::spectra_rt_list_free_all_fast as *const u8,
            Self::ListPop => crate::ffi::spectra_rt_list_pop_fast as *const u8,
            Self::ListPopFront => crate::ffi::spectra_rt_list_pop_front_fast as *const u8,
            Self::ListPopOption => crate::ffi::spectra_rt_list_pop_option_fast as *const u8,
            Self::ListPopFrontOption => {
                crate::ffi::spectra_rt_list_pop_front_option_fast as *const u8
            }
            Self::ListInsertAt => crate::ffi::spectra_rt_list_insert_at_fast as *const u8,
            Self::ListRemoveAt => crate::ffi::spectra_rt_list_remove_at_fast as *const u8,
            Self::ListRemoveAtOption => {
                crate::ffi::spectra_rt_list_remove_at_option_fast as *const u8
            }
            Self::ListIndexOf => crate::ffi::spectra_rt_list_index_of_fast as *const u8,
            Self::ListSort => crate::ffi::spectra_rt_list_sort_fast as *const u8,
            Self::MapGet => crate::ffi::spectra_rt_map_get_fast as *const u8,
            Self::MapGetOption => crate::ffi::spectra_rt_map_get_option_fast as *const u8,
            Self::MapRemove => crate::ffi::spectra_rt_map_remove_fast as *const u8,
            Self::MapRemoveOption => crate::ffi::spectra_rt_map_remove_option_fast as *const u8,
            Self::MapIsEmpty => crate::ffi::spectra_rt_map_is_empty_fast as *const u8,
            Self::MapFreeAll => crate::ffi::spectra_rt_map_free_all_fast as *const u8,
            Self::StackNew => crate::ffi::spectra_rt_stack_new_fast as *const u8,
            Self::StackPush => crate::ffi::spectra_rt_stack_push_fast as *const u8,
            Self::StackPop => crate::ffi::spectra_rt_stack_pop_fast as *const u8,
            Self::StackPeek => crate::ffi::spectra_rt_stack_peek_fast as *const u8,
            Self::StackLen => crate::ffi::spectra_rt_stack_len_fast as *const u8,
            Self::StackIsEmpty => crate::ffi::spectra_rt_stack_is_empty_fast as *const u8,
            Self::StackClear => crate::ffi::spectra_rt_stack_clear_fast as *const u8,
            Self::StackFree => crate::ffi::spectra_rt_stack_free_fast as *const u8,
            Self::StackFreeAll => crate::ffi::spectra_rt_stack_free_all_fast as *const u8,
            Self::QueueNew => crate::ffi::spectra_rt_queue_new_fast as *const u8,
            Self::QueueEnqueue => crate::ffi::spectra_rt_queue_enqueue_fast as *const u8,
            Self::QueueDequeue => crate::ffi::spectra_rt_queue_dequeue_fast as *const u8,
            Self::QueuePeek => crate::ffi::spectra_rt_queue_peek_fast as *const u8,
            Self::QueueLen => crate::ffi::spectra_rt_queue_len_fast as *const u8,
            Self::QueueIsEmpty => crate::ffi::spectra_rt_queue_is_empty_fast as *const u8,
            Self::QueueClear => crate::ffi::spectra_rt_queue_clear_fast as *const u8,
            Self::QueueFree => crate::ffi::spectra_rt_queue_free_fast as *const u8,
            Self::QueueFreeAll => crate::ffi::spectra_rt_queue_free_all_fast as *const u8,
            Self::IteratorNext => crate::ffi::spectra_rt_iterator_next_fast as *const u8,
            Self::IteratorNextUnchecked => {
                crate::ffi::spectra_rt_iterator_next_unchecked_fast as *const u8
            }
            Self::IteratorRemaining => crate::ffi::spectra_rt_iterator_remaining_fast as *const u8,
            Self::IteratorFree => crate::ffi::spectra_rt_iterator_free_fast as *const u8,
            Self::ListIter => crate::ffi::spectra_rt_list_iter_fast as *const u8,
            Self::SetIter => crate::ffi::spectra_rt_set_iter_fast as *const u8,
            Self::MapIter => crate::ffi::spectra_rt_map_iter_fast as *const u8,
            Self::MapValuesIter => crate::ffi::spectra_rt_map_values_iter_fast as *const u8,
            Self::StackIter => crate::ffi::spectra_rt_stack_iter_fast as *const u8,
            Self::QueueIter => crate::ffi::spectra_rt_queue_iter_fast as *const u8,
            Self::MapSetScalar => crate::ffi::spectra_rt_map_set_scalar_fast as *const u8,
            Self::MapContainsScalar => {
                crate::ffi::spectra_rt_map_contains_scalar_fast as *const u8
            }
            Self::MapGetScalar => crate::ffi::spectra_rt_map_get_scalar_fast as *const u8,
            Self::MapRemoveScalar => {
                crate::ffi::spectra_rt_map_remove_scalar_fast as *const u8
            }
            Self::CoroutineFrameAlloc => {
                crate::async_abi::spectra_rt_coroutine_frame_alloc as *const u8
            }
            Self::CoroutineFrameStore => {
                crate::async_abi::spectra_rt_coroutine_frame_store as *const u8
            }
            Self::CoroutineFrameLoad => {
                crate::async_abi::spectra_rt_coroutine_frame_load as *const u8
            }
            Self::CoroutineLocalPtr => {
                crate::async_abi::spectra_rt_coroutine_local_ptr as *const u8
            }
            Self::CoroutineStateLoad => {
                crate::async_abi::spectra_rt_coroutine_state_load as *const u8
            }
            Self::CoroutineStateStore => {
                crate::async_abi::spectra_rt_coroutine_state_store as *const u8
            }
            Self::CoroutineCreate => crate::async_abi::spectra_rt_coroutine_create as *const u8,
            Self::CoroutinePollChild => {
                crate::async_abi::spectra_rt_coroutine_poll_child as *const u8
            }
            Self::CoroutinePollResult => {
                crate::async_abi::spectra_rt_coroutine_poll_result as *const u8
            }
            Self::CoroutineSubscribe => {
                crate::async_abi::spectra_rt_coroutine_subscribe as *const u8
            }
            Self::CoroutineWake => crate::async_abi::spectra_rt_coroutine_wake as *const u8,
            Self::CoroutineSuspend => crate::async_abi::spectra_rt_coroutine_suspend as *const u8,
            Self::CoroutineComplete => crate::async_abi::spectra_rt_coroutine_complete as *const u8,
            Self::CoroutineError => crate::async_abi::spectra_rt_coroutine_error as *const u8,
            Self::CoroutineCancelled => {
                crate::async_abi::spectra_rt_coroutine_cancelled as *const u8
            }
            Self::CoroutinePollReturn => {
                crate::async_abi::spectra_rt_coroutine_poll_return as *const u8
            }
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
    ConcurrentSpawnBatch,
    ConcurrentJoinBatchSum,
    ConcurrentJoin,
    BuilderNew,
    BuilderPush,
    BuilderLen,
    BuilderFinish,
    BuilderFree,
    MapSet,
    MapContains,
    MapNew,
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
    ConcurrentSpawnFn,
    ListNew,
    ListPush,
    ListLen,
    ListGet,
    ListGetOption,
    ListSet,
    ListContains,
    ListClear,
    ListFree,
    ListFreeAll,
    ListPop,
    ListPopFront,
    ListPopOption,
    ListPopFrontOption,
    ListInsertAt,
    ListRemoveAt,
    ListRemoveAtOption,
    ListIndexOf,
    ListSort,
    MapGet,
    MapGetOption,
    MapRemove,
    MapRemoveOption,
    MapIsEmpty,
    MapFreeAll,
    StackNew,
    StackPush,
    StackPop,
    StackPeek,
    StackLen,
    StackIsEmpty,
    StackClear,
    StackFree,
    StackFreeAll,
    QueueNew,
    QueueEnqueue,
    QueueDequeue,
    QueuePeek,
    QueueLen,
    QueueIsEmpty,
    QueueClear,
    QueueFree,
    QueueFreeAll,
    IteratorNext,
    IteratorNextUnchecked,
    IteratorRemaining,
    IteratorFree,
    ListIter,
    SetIter,
    MapIter,
    MapValuesIter,
    StackIter,
    QueueIter,
    MapSetScalar,
    MapContainsScalar,
    MapGetScalar,
    MapRemoveScalar,
}
impl FastHostCall {
    pub const COUNT: usize = 85;

    pub const ALL: &'static [Self] = &[
        Self::ConcurrentReset,
        Self::StringLen,
        Self::StringCharAt,
        Self::ConcurrentSpawnBatch,
        Self::ConcurrentJoinBatchSum,
        Self::ConcurrentJoin,
        Self::BuilderNew,
        Self::BuilderPush,
        Self::BuilderLen,
        Self::BuilderFinish,
        Self::BuilderFree,
        Self::MapSet,
        Self::MapContains,
        Self::MapNew,
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
        Self::ConcurrentSpawnFn,
        Self::ListNew,
        Self::ListPush,
        Self::ListLen,
        Self::ListGet,
        Self::ListGetOption,
        Self::ListSet,
        Self::ListContains,
        Self::ListClear,
        Self::ListFree,
        Self::ListFreeAll,
        Self::ListPop,
        Self::ListPopFront,
        Self::ListPopOption,
        Self::ListPopFrontOption,
        Self::ListInsertAt,
        Self::ListRemoveAt,
        Self::ListRemoveAtOption,
        Self::ListIndexOf,
        Self::ListSort,
        Self::MapGet,
        Self::MapGetOption,
        Self::MapRemove,
        Self::MapRemoveOption,
        Self::MapIsEmpty,
        Self::MapFreeAll,
        Self::StackNew,
        Self::StackPush,
        Self::StackPop,
        Self::StackPeek,
        Self::StackLen,
        Self::StackIsEmpty,
        Self::StackClear,
        Self::StackFree,
        Self::StackFreeAll,
        Self::QueueNew,
        Self::QueueEnqueue,
        Self::QueueDequeue,
        Self::QueuePeek,
        Self::QueueLen,
        Self::QueueIsEmpty,
        Self::QueueClear,
        Self::QueueFree,
        Self::QueueFreeAll,
        Self::IteratorNext,
        Self::IteratorNextUnchecked,
        Self::IteratorRemaining,
        Self::IteratorFree,
        Self::ListIter,
        Self::SetIter,
        Self::MapIter,
        Self::MapValuesIter,
        Self::StackIter,
        Self::QueueIter,
        Self::MapSetScalar,
        Self::MapContainsScalar,
        Self::MapGetScalar,
        Self::MapRemoveScalar,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn host_name(self) -> &'static str {
        match self {
            Self::ConcurrentReset => "spectra.std.concurrent.reset",
            Self::StringLen => "spectra.std.string.len",
            Self::StringCharAt => "spectra.std.string.char_at",
            Self::ConcurrentSpawnBatch => "spectra.std.concurrent.task_spawn_batch",
            Self::ConcurrentJoinBatchSum => "spectra.std.concurrent.task_join_batch_sum",
            Self::ConcurrentJoin => "spectra.std.concurrent.task_join",
            Self::BuilderNew => "spectra.std.string.builder_new",
            Self::BuilderPush => "spectra.std.string.builder_push",
            Self::BuilderLen => "spectra.std.string.builder_len",
            Self::BuilderFinish => "spectra.std.string.builder_finish",
            Self::BuilderFree => "spectra.std.string.builder_free",
            Self::MapSet => "spectra.std.collections.map_set",
            Self::MapContains => "spectra.std.collections.map_contains",
            Self::MapNew => "spectra.std.collections.map_new",
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
            Self::ConcurrentSpawnFn => "spectra.std.concurrent.task_spawn_fn",
            Self::ListNew => "spectra.std.collections.list_new",
            Self::ListPush => "spectra.std.collections.list_push",
            Self::ListLen => "spectra.std.collections.list_len",
            Self::ListGet => "spectra.std.collections.list_get",
            Self::ListGetOption => "spectra.std.collections.list_get_option",
            Self::ListSet => "spectra.std.collections.list_set",
            Self::ListContains => "spectra.std.collections.list_contains",
            Self::ListClear => "spectra.std.collections.list_clear",
            Self::ListFree => "spectra.std.collections.list_free",
            Self::ListFreeAll => "spectra.std.collections.list_free_all",
            Self::ListPop => "spectra.std.collections.list_pop",
            Self::ListPopFront => "spectra.std.collections.list_pop_front",
            Self::ListPopOption => "spectra.std.collections.list_pop_option",
            Self::ListPopFrontOption => "spectra.std.collections.list_pop_front_option",
            Self::ListInsertAt => "spectra.std.collections.list_insert_at",
            Self::ListRemoveAt => "spectra.std.collections.list_remove_at",
            Self::ListRemoveAtOption => "spectra.std.collections.list_remove_at_option",
            Self::ListIndexOf => "spectra.std.collections.list_index_of",
            Self::ListSort => "spectra.std.collections.list_sort",
            Self::MapGet => "spectra.std.collections.map_get",
            Self::MapGetOption => "spectra.std.collections.map_get_option",
            Self::MapRemove => "spectra.std.collections.map_remove",
            Self::MapRemoveOption => "spectra.std.collections.map_remove_option",
            Self::MapIsEmpty => "spectra.std.collections.map_is_empty",
            Self::MapFreeAll => "spectra.std.collections.map_free_all",
            Self::StackNew => "spectra.std.collections.stack_new",
            Self::StackPush => "spectra.std.collections.stack_push",
            Self::StackPop => "spectra.std.collections.stack_pop",
            Self::StackPeek => "spectra.std.collections.stack_peek",
            Self::StackLen => "spectra.std.collections.stack_len",
            Self::StackIsEmpty => "spectra.std.collections.stack_is_empty",
            Self::StackClear => "spectra.std.collections.stack_clear",
            Self::StackFree => "spectra.std.collections.stack_free",
            Self::StackFreeAll => "spectra.std.collections.stack_free_all",
            Self::QueueNew => "spectra.std.collections.queue_new",
            Self::QueueEnqueue => "spectra.std.collections.queue_enqueue",
            Self::QueueDequeue => "spectra.std.collections.queue_dequeue",
            Self::QueuePeek => "spectra.std.collections.queue_peek",
            Self::QueueLen => "spectra.std.collections.queue_len",
            Self::QueueIsEmpty => "spectra.std.collections.queue_is_empty",
            Self::QueueClear => "spectra.std.collections.queue_clear",
            Self::QueueFree => "spectra.std.collections.queue_free",
            Self::QueueFreeAll => "spectra.std.collections.queue_free_all",
            Self::IteratorNext => "spectra.std.collections.iterator_next",
            Self::IteratorNextUnchecked => "spectra.std.collections.iterator_next_unchecked",
            Self::IteratorRemaining => "spectra.std.collections.iterator_remaining",
            Self::IteratorFree => "spectra.std.collections.iterator_free",
            Self::ListIter => "spectra.std.collections.list_iter",
            Self::SetIter => "spectra.std.collections.set_iter",
            Self::MapIter => "spectra.std.collections.map_iter",
            Self::MapValuesIter => "spectra.std.collections.map_values_iter",
            Self::StackIter => "spectra.std.collections.stack_iter",
            Self::QueueIter => "spectra.std.collections.queue_iter",
            Self::MapSetScalar => "spectra.compiler.collections.map_set_scalar",
            Self::MapContainsScalar => "spectra.compiler.collections.map_contains_scalar",
            Self::MapGetScalar => "spectra.compiler.collections.map_get_scalar",
            Self::MapRemoveScalar => "spectra.compiler.collections.map_remove_scalar",
        }
    }

    pub const fn runtime_import(self) -> RuntimeImport {
        match self {
            Self::ConcurrentReset => RuntimeImport::ConcurrentReset,
            Self::StringLen => RuntimeImport::StringLen,
            Self::StringCharAt => RuntimeImport::StringCharAt,
            Self::ConcurrentSpawnBatch => RuntimeImport::ConcurrentSpawnBatch,
            Self::ConcurrentJoinBatchSum => RuntimeImport::ConcurrentJoinBatchSum,
            Self::ConcurrentJoin => RuntimeImport::ConcurrentJoin,
            Self::BuilderNew => RuntimeImport::BuilderNew,
            Self::BuilderPush => RuntimeImport::BuilderPush,
            Self::BuilderLen => RuntimeImport::BuilderLen,
            Self::BuilderFinish => RuntimeImport::BuilderFinish,
            Self::BuilderFree => RuntimeImport::BuilderFree,
            Self::MapSet => RuntimeImport::MapSet,
            Self::MapContains => RuntimeImport::MapContains,
            Self::MapNew => RuntimeImport::MapNew,
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
            Self::ConcurrentSpawnFn => RuntimeImport::ConcurrentSpawnFn,
            Self::ListNew => RuntimeImport::ListNew,
            Self::ListPush => RuntimeImport::ListPush,
            Self::ListLen => RuntimeImport::ListLen,
            Self::ListGet => RuntimeImport::ListGet,
            Self::ListGetOption => RuntimeImport::ListGetOption,
            Self::ListSet => RuntimeImport::ListSet,
            Self::ListContains => RuntimeImport::ListContains,
            Self::ListClear => RuntimeImport::ListClear,
            Self::ListFree => RuntimeImport::ListFree,
            Self::ListFreeAll => RuntimeImport::ListFreeAll,
            Self::ListPop => RuntimeImport::ListPop,
            Self::ListPopFront => RuntimeImport::ListPopFront,
            Self::ListPopOption => RuntimeImport::ListPopOption,
            Self::ListPopFrontOption => RuntimeImport::ListPopFrontOption,
            Self::ListInsertAt => RuntimeImport::ListInsertAt,
            Self::ListRemoveAt => RuntimeImport::ListRemoveAt,
            Self::ListRemoveAtOption => RuntimeImport::ListRemoveAtOption,
            Self::ListIndexOf => RuntimeImport::ListIndexOf,
            Self::ListSort => RuntimeImport::ListSort,
            Self::MapGet => RuntimeImport::MapGet,
            Self::MapGetOption => RuntimeImport::MapGetOption,
            Self::MapRemove => RuntimeImport::MapRemove,
            Self::MapRemoveOption => RuntimeImport::MapRemoveOption,
            Self::MapIsEmpty => RuntimeImport::MapIsEmpty,
            Self::MapFreeAll => RuntimeImport::MapFreeAll,
            Self::StackNew => RuntimeImport::StackNew,
            Self::StackPush => RuntimeImport::StackPush,
            Self::StackPop => RuntimeImport::StackPop,
            Self::StackPeek => RuntimeImport::StackPeek,
            Self::StackLen => RuntimeImport::StackLen,
            Self::StackIsEmpty => RuntimeImport::StackIsEmpty,
            Self::StackClear => RuntimeImport::StackClear,
            Self::StackFree => RuntimeImport::StackFree,
            Self::StackFreeAll => RuntimeImport::StackFreeAll,
            Self::QueueNew => RuntimeImport::QueueNew,
            Self::QueueEnqueue => RuntimeImport::QueueEnqueue,
            Self::QueueDequeue => RuntimeImport::QueueDequeue,
            Self::QueuePeek => RuntimeImport::QueuePeek,
            Self::QueueLen => RuntimeImport::QueueLen,
            Self::QueueIsEmpty => RuntimeImport::QueueIsEmpty,
            Self::QueueClear => RuntimeImport::QueueClear,
            Self::QueueFree => RuntimeImport::QueueFree,
            Self::QueueFreeAll => RuntimeImport::QueueFreeAll,
            Self::IteratorNext => RuntimeImport::IteratorNext,
            Self::IteratorNextUnchecked => RuntimeImport::IteratorNextUnchecked,
            Self::IteratorRemaining => RuntimeImport::IteratorRemaining,
            Self::IteratorFree => RuntimeImport::IteratorFree,
            Self::ListIter => RuntimeImport::ListIter,
            Self::SetIter => RuntimeImport::SetIter,
            Self::MapIter => RuntimeImport::MapIter,
            Self::MapValuesIter => RuntimeImport::MapValuesIter,
            Self::StackIter => RuntimeImport::StackIter,
            Self::QueueIter => RuntimeImport::QueueIter,
            Self::MapSetScalar => RuntimeImport::MapSetScalar,
            Self::MapContainsScalar => RuntimeImport::MapContainsScalar,
            Self::MapGetScalar => RuntimeImport::MapGetScalar,
            Self::MapRemoveScalar => RuntimeImport::MapRemoveScalar,
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
            resolve_host_call("spectra.std.collections.map_set", 3),
            HostCallClass::Fast(FastHostCall::MapSet)
        );
        assert_eq!(
            resolve_host_call("spectra.std.collections.map_set", 1),
            HostCallClass::Generic
        );
        assert_eq!(
            resolve_host_call("spectra.std.collections.map_get", 2),
            HostCallClass::Fast(FastHostCall::MapGet)
        );
    }

    /// Host-call namespace prefixes whose calls touch the outside world and
    /// therefore require the generic dispatch path, where capability policy is
    /// enforced and denial can be reported (ADR 0016, decisions D2/D4).
    ///
    /// The set is derived from the host functions registered by
    /// `runtime/src/stdlib/registration.rs` and
    /// `runtime/src/stdlib/stdlib_bindings.rs`. `spectra.agent.*` is included
    /// preemptively: the namespace is allocated by ADR 0017 and its calls are
    /// effect-bearing by construction.
    const EFFECT_HOST_NAMESPACES: &[&str] = &[
        "spectra.agent.",
        "spectra.api.",
        "spectra.async.",
        "spectra.std.env.",
        "spectra.std.fs.",
        "spectra.std.io.",
        "spectra.std.random.",
        "spectra.std.serve.",
        "spectra.std.time.",
    ];

    /// Pins invariant I1: no host call that touches the outside world is in
    /// the fast path. The fast path is in-process compute and has no denial
    /// channel, so an effect-bearing call there would be an unenforceable
    /// bypass (ADR 0016).
    #[test]
    fn fast_host_call_effect_namespace() {
        assert_eq!(FastHostCall::ALL.len(), FastHostCall::COUNT);
        for fast in FastHostCall::ALL {
            // Exhaustive classification. There is deliberately no wildcard
            // arm: adding a `FastHostCall` variant breaks compilation here
            // until the new call is consciously classified as effect-free.
            match fast {
                FastHostCall::ConcurrentReset
                | FastHostCall::StringLen
                | FastHostCall::StringCharAt
                | FastHostCall::ConcurrentSpawnBatch
                | FastHostCall::ConcurrentJoinBatchSum
                | FastHostCall::ConcurrentJoin
                | FastHostCall::BuilderNew
                | FastHostCall::BuilderPush
                | FastHostCall::BuilderLen
                | FastHostCall::BuilderFinish
                | FastHostCall::BuilderFree
                | FastHostCall::MapSet
                | FastHostCall::MapContains
                | FastHostCall::MapNew
                | FastHostCall::MapLen
                | FastHostCall::MapClear
                | FastHostCall::MapFree
                | FastHostCall::ChannelNew
                | FastHostCall::ChannelSend
                | FastHostCall::ChannelRecv
                | FastHostCall::ChannelClose
                | FastHostCall::ChannelLen
                | FastHostCall::MlLinear
                | FastHostCall::MlMseLoss
                | FastHostCall::TensorBackward
                | FastHostCall::MlSgdStep
                | FastHostCall::TensorFullF
                | FastHostCall::ConcurrentSpawnFn
                | FastHostCall::ListNew
                | FastHostCall::ListPush
                | FastHostCall::ListLen
                | FastHostCall::ListGet
                | FastHostCall::ListGetOption
                | FastHostCall::ListSet
                | FastHostCall::ListContains
                | FastHostCall::ListClear
                | FastHostCall::ListFree
                | FastHostCall::ListFreeAll
                | FastHostCall::ListPop
                | FastHostCall::ListPopFront
                | FastHostCall::ListPopOption
                | FastHostCall::ListPopFrontOption
                | FastHostCall::ListInsertAt
                | FastHostCall::ListRemoveAt
                | FastHostCall::ListRemoveAtOption
                | FastHostCall::ListIndexOf
                | FastHostCall::ListSort
                | FastHostCall::MapGet
                | FastHostCall::MapGetOption
                | FastHostCall::MapRemove
                | FastHostCall::MapRemoveOption
                | FastHostCall::MapIsEmpty
                | FastHostCall::MapFreeAll
                | FastHostCall::StackNew
                | FastHostCall::StackPush
                | FastHostCall::StackPop
                | FastHostCall::StackPeek
                | FastHostCall::StackLen
                | FastHostCall::StackIsEmpty
                | FastHostCall::StackClear
                | FastHostCall::StackFree
                | FastHostCall::StackFreeAll
                | FastHostCall::QueueNew
                | FastHostCall::QueueEnqueue
                | FastHostCall::QueueDequeue
                | FastHostCall::QueuePeek
                | FastHostCall::QueueLen
                | FastHostCall::QueueIsEmpty
                | FastHostCall::QueueClear
                | FastHostCall::QueueFree
                | FastHostCall::QueueFreeAll
                | FastHostCall::IteratorNext
                | FastHostCall::IteratorNextUnchecked
                | FastHostCall::IteratorRemaining
                | FastHostCall::IteratorFree
                | FastHostCall::ListIter
                | FastHostCall::SetIter
                | FastHostCall::MapIter
                | FastHostCall::MapValuesIter
                | FastHostCall::StackIter
                | FastHostCall::QueueIter
                | FastHostCall::MapSetScalar
                | FastHostCall::MapContainsScalar
                | FastHostCall::MapGetScalar
                | FastHostCall::MapRemoveScalar => {}
            }

            let name = fast.host_name();
            for prefix in EFFECT_HOST_NAMESPACES {
                assert!(
                    !name.starts_with(prefix),
                    "fast host call `{name}` falls under effect namespace `{prefix}`; \
                     effect-bearing calls must use the generic dispatch path"
                );
            }
        }
    }
}
