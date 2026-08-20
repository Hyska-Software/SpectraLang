use crate::ffi::{
    register_host_function, SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INTERNAL_ERROR,
    HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_NOT_FOUND, HOST_STATUS_SUCCESS,
};
use crate::handles::{HandleId, HandleKind, HandleTable};
use crate::initialize;
use crate::memory::ManualBox;
use crate::reactor::{self, Interest, ReactorEvent};
use crate::tracing::{self, SpanKind, SpanStatus};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::slice;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, AtomicUsize, Ordering},
    mpsc, Arc, Condvar, Mutex, MutexGuard, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant as StdInstant, SystemTime, UNIX_EPOCH};

mod error;
mod option_result;

#[cfg(test)]
use crate::ffi::{clear_host_functions, lookup_host_function};
#[cfg(test)]
use std::ptr;

include!("stdlib_bindings.rs");

include!("registration.rs");

include!("math_io.rs");

include!("list_host.rs");

include!("list_registry.rs");

include!("string_map_fast.rs");

include!("ml_tensor_fast.rs");

include!("list_methods.rs");

include!("tensor_types_registry.rs");

include!("tensor_helpers_kernels.rs");

include!("tensor_constructors_access.rs");

include!("tensor_ops_reductions.rs");

include!("tensor_matmul.rs");

include!("tensor_autograd_gpu.rs");

include!("tensor_autograd.rs");

include!("ml_registry_utils.rs");

include!("ml_artifacts_modules.rs");

include!("ml_training_losses.rs");

include!("ml_optimizers.rs");

include!("ml_datasets_artifacts.rs");

include!("ml_onnx.rs");

include!("ml_dataset_access.rs");

include!("ml_experiments_distributed.rs");

include!("ml_onnx_bindings.rs");

include!("ml_tokenization_retrieval.rs");

include!("ml_metrics.rs");

include!("tensor_runtime_extras.rs");

include!("collections_extras.rs");

include!("collections_higher_order.rs");

include!("string_registration_helpers.rs");

include!("string_host.rs");

include!("string_builder.rs");

include!("string_extras.rs");

include!("convert_host.rs");

include!("convert_random.rs");

include!("fs.rs");

include!("env.rs");

include!("char.rs");

include!("time.rs");

include!("range.rs");

include!("string_new.rs");

include!("math_new.rs");

include!("io_new.rs");

include!("set_iterator.rs");

include!("map.rs");

include!("async_types.rs");

include!("async_registry.rs");

include!("async_registry_support.rs");

include!("async_task_stream.rs");

include!("async_network_reactor.rs");

include!("async_fast_paths.rs");

include!("concurrent_core.rs");

include!("concurrent_api.rs");

include!("serve_core.rs");

include!("serve_api.rs");

#[cfg(test)]
mod tests {
    include!("tests.rs");
}
