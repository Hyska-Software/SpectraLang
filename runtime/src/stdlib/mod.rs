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

#[path = "stdlib_bindings.rs"] mod stdlib_bindings;
pub use stdlib_bindings::*;

#[path = "registration.rs"] mod registration;
pub use registration::*;

#[path = "math_io.rs"] mod math_io;
pub(crate) use math_io::*;

#[path = "list_host.rs"] mod list_host;
pub(crate) use list_host::*;

#[path = "list_registry.rs"] mod list_registry;
pub(crate) use list_registry::*;

#[path = "string_map_fast.rs"] mod string_map_fast;
pub use string_map_fast::*;

#[path = "ml_tensor_fast.rs"] mod ml_tensor_fast;
pub use ml_tensor_fast::*;

#[path = "list_methods.rs"] mod list_methods;

#[path = "tensor_types_registry.rs"] mod tensor_types_registry;
pub use tensor_types_registry::*;

#[path = "tensor_helpers_kernels.rs"] mod tensor_helpers_kernels;
pub use tensor_helpers_kernels::*;

#[path = "tensor_constructors_access.rs"] mod tensor_constructors_access;
pub(crate) use tensor_constructors_access::*;

#[path = "tensor_ops_reductions.rs"] mod tensor_ops_reductions;
pub(crate) use tensor_ops_reductions::*;

#[path = "tensor_matmul.rs"] mod tensor_matmul;
pub(crate) use tensor_matmul::*;

#[path = "tensor_autograd_gpu.rs"] mod tensor_autograd_gpu;
pub(crate) use tensor_autograd_gpu::*;

#[path = "tensor_autograd.rs"] mod tensor_autograd;
pub(crate) use tensor_autograd::*;

#[path = "ml_registry_utils.rs"] mod ml_registry_utils;
pub(crate) use ml_registry_utils::*;

#[path = "ml_artifacts_modules.rs"] mod ml_artifacts_modules;
pub(crate) use ml_artifacts_modules::*;

#[path = "ml_training_losses.rs"] mod ml_training_losses;
pub(crate) use ml_training_losses::*;

#[path = "ml_optimizers.rs"] mod ml_optimizers;
pub(crate) use ml_optimizers::*;

#[path = "ml_datasets_artifacts.rs"] mod ml_datasets_artifacts;
pub(crate) use ml_datasets_artifacts::*;

#[path = "ml_onnx.rs"] mod ml_onnx;
pub(crate) use ml_onnx::*;

#[path = "ml_dataset_access.rs"] mod ml_dataset_access;
pub(crate) use ml_dataset_access::*;

#[path = "ml_experiments_distributed.rs"] mod ml_experiments_distributed;
pub(crate) use ml_experiments_distributed::*;

#[path = "ml_distributed_tcp.rs"] mod ml_distributed_tcp;
pub(crate) use ml_distributed_tcp::*;

#[path = "ml_onnx_bindings.rs"] mod ml_onnx_bindings;
pub(crate) use ml_onnx_bindings::*;

#[path = "ml_tokenization_retrieval.rs"] mod ml_tokenization_retrieval;
pub(crate) use ml_tokenization_retrieval::*;

#[path = "ml_generation.rs"] mod ml_generation;
pub(crate) use ml_generation::*;

#[path = "ml_tokenizer_training.rs"] mod ml_tokenizer_training;
pub(crate) use ml_tokenizer_training::*;

#[path = "ml_metrics.rs"] mod ml_metrics;
pub(crate) use ml_metrics::*;
#[path = "ml_text_embedding_model.rs"] mod ml_text_embedding_model;
pub(crate) use ml_text_embedding_model::*;

#[path = "tensor_runtime_extras.rs"] mod tensor_runtime_extras;
pub(crate) use tensor_runtime_extras::*;

#[path = "collections_extras.rs"] mod collections_extras;
pub(crate) use collections_extras::*;

#[path = "collections_higher_order.rs"] mod collections_higher_order;
pub(crate) use collections_higher_order::*;

#[path = "string_registration_helpers.rs"] mod string_registration_helpers;
pub use string_registration_helpers::*;

#[path = "string_host.rs"] mod string_host;
pub(crate) use string_host::*;

#[path = "string_builder.rs"] mod string_builder;
pub(crate) use string_builder::*;

#[path = "string_extras.rs"] mod string_extras;
pub(crate) use string_extras::*;

#[path = "convert_host.rs"] mod convert_host;
pub(crate) use convert_host::*;

#[path = "convert_random.rs"] mod convert_random;
pub(crate) use convert_random::*;

#[path = "fs.rs"] mod fs;
pub(crate) use fs::*;

#[path = "env.rs"] mod env;
pub(crate) use env::*;

#[path = "char.rs"] mod char;
pub(crate) use char::*;

#[path = "time.rs"] mod time;
pub(crate) use time::*;

#[path = "range.rs"] mod range;
pub(crate) use range::*;

#[path = "string_new.rs"] mod string_new;
pub(crate) use string_new::*;

#[path = "math_new.rs"] mod math_new;
pub(crate) use math_new::*;

#[path = "io_new.rs"] mod io_new;
pub(crate) use io_new::*;

#[path = "set_iterator.rs"] mod set_iterator;
pub(crate) use set_iterator::*;

#[path = "map.rs"] mod map;
pub(crate) use map::*;

#[path = "async_types.rs"] mod async_types;
pub(crate) use async_types::*;

#[path = "async_registry.rs"] mod async_registry;
pub(crate) use async_registry::*;

#[path = "async_registry_support.rs"] mod async_registry_support;
pub(crate) use async_registry_support::*;

#[path = "async_task_stream.rs"] mod async_task_stream;
pub use async_task_stream::*;

#[path = "async_network_reactor.rs"] mod async_network_reactor;
pub(crate) use async_network_reactor::*;

#[path = "async_fast_paths.rs"] mod async_fast_paths;
pub use async_fast_paths::*;

#[path = "concurrent_core.rs"] mod concurrent_core;
pub(crate) use concurrent_core::*;

#[path = "concurrent_api.rs"] mod concurrent_api;
pub(crate) use concurrent_api::*;

#[path = "serve_core.rs"] mod serve_core;
pub(crate) use serve_core::*;

#[path = "serve_api.rs"] mod serve_api;
pub(crate) use serve_api::*;

// ── ServeHttp ──
#[path = "serve_http.rs"] mod serve_http;
pub(crate) use serve_http::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
