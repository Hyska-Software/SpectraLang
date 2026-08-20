# Spectra Runtime Standard Library

The Spectra runtime ships a minimal host-driven standard library implemented as registered host
functions. The functions are grouped by namespace and can be installed by calling
`spectra_runtime::register_standard_library()` (or invoking `spectra_rt_std_register` once it is
gated through the CLI).

The maturity contract is split by surface: scalar core helpers and the
certified exact-width numeric ABI are stable, while typed collection accessors
and `Option`/`Result` transformations remain beta until their complete
cross-target evidence is available. The public collection names that can miss (`list_get`,
`list_pop`, `list_pop_front`, `list_remove_at`, `map_get`, and `map_remove`)
return tagged `Option<T>` values. Legacy sentinel-returning calls are retained
only under the explicit `std.compat.collections` namespace.
The public `std.env.env_get` and `std.env.env_arg` calls use the same
absence-safe `Option<string>` contract; empty-string environment values remain
distinguishable from missing values. Legacy empty-string behavior is retained
only under `std.compat.env`.
The public `std.fs` calls now return tagged `Result<T, Error>` values; the
historical string/boolean sentinel adapter is retained only under
`std.compat.fs`.

All host calls use the shared [`SpectraHostCallContext`](host-call-conventions.md) contract and the
status codes defined in `runtime::ffi` (`HOST_STATUS_*`). Arguments and results are encoded as
64-bit values (`SpectraHostValue`).

## Exact-width numeric ABI

R-2901 adds explicit exact-width scalar representations. Host-call slots remain
canonical 64-bit values, while the compiler/backend materialize `i8`/`i16`/
`i32`/`i64` and `f32`/`f64` values with their declared width. Signed values are
sign-extended and unsigned values must be zero-extended at the ABI boundary.
The checked narrowing, AOT/interop, and C ABI validation gates for this scalar
matrix pass in R-2901. Scalar forms outside this matrix remain deferred.

## math namespace

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.math.abs` | Absolute value for signed integers. | `x` | `abs(x)` |
| `spectra.std.math.min` | Returns the smaller of two integers. | `lhs`, `rhs` | `min(lhs, rhs)` |
| `spectra.std.math.max` | Returns the larger of two integers. | `lhs`, `rhs` | `max(lhs, rhs)` |

## io namespace

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.io.print` | Prints all arguments as integers separated by spaces and terminates with a newline. | variadic | argument count written to `results[0]` when available |
| `spectra.std.io.flush` | Flushes the process stdout stream. | *(none)* | `0` when `results` is provided |

## collections namespace

Spectra exposes typed `List<T>` operations at the source boundary. The runtime
ABI currently transports each list as an opaque generational handle backed by a
runtime-managed vector. The handle representation is an implementation/ABI
detail; stale or invalid handles are rejected through the host status channel.
The legacy rows below therefore describe the host ABI adapter, not the stable
source-level collection signatures. A process-wide cleanup remains available
for shutdown and test isolation.

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.collections.list_new` | Allocates an empty list and returns its handle. | *(none)* | handle |
| `spectra.std.collections.list_push` | Appends an integer to the list referenced by the handle. | `handle`, `value` | new length |
| `spectra.std.collections.list_len` | Returns the current length of the list. | `handle` | length |
| `spectra.std.collections.list_clear` | Removes all elements from the list without releasing the handle. | `handle` | `0` |
| `spectra.std.collections.list_free` | Drops the list allocation associated with the handle. | `handle` | `0` when `results` provided |
| `spectra.std.collections.list_free_all` | Drops every list managed by the runtime. | *(none)* | number of freed lists |
| `spectra.std.collections.list_get` / `list_pop` / `list_pop_front` / `list_remove_at` | Absence-safe reads/removals for the public `std.collections` surface. | list handle, optional index | tagged `Option<T>` payload |
| `spectra.std.compat.collections.list_get` / `list_pop` / `list_pop_front` / `list_remove_at` | Compatibility reads/removals with the historical `-1` sentinel. | list handle, optional index | integer payload |
| `spectra.std.collections.set_new` / `set_insert` / `set_contains` / `set_remove` | Creates and mutates an insertion-ordered typed set. | handle, optional value | handle or bool |
| `spectra.std.collections.set_len` / `set_get` / `set_clear` / `set_free` | Reads, clears, and releases a set. | handle, optional index/value | length, tagged `Option<T>`, or `0` |
| `spectra.std.collections.list_iter` / `set_iter` / `map_iter` | Creates deterministic snapshot iterators for collections. Map iteration yields keys. | collection handle | iterator handle |
| `spectra.std.collections.iterator_next` / `iterator_remaining` / `iterator_free` | Consumes, inspects, and releases an `Iterator<T>`. | iterator handle | tagged `Option<T>`, length, or `0` |
| `spectra.std.range.iter` | Adapts an integer range to the common iterator protocol. | range handle | `Iterator<int>` handle |

The explicit `_option` names remain aliases for the absence-safe operations:
`list_get_option`, `list_pop_option`, `list_pop_front_option`,
`list_remove_at_option`, `map_get_option`, and `map_remove_option`. They allocate
a small tagged payload and report invalid handles through the host status
channel. The compatibility namespace also exposes
`spectra.std.compat.collections.map_get` and `map_remove`, whose missing-value
sentinels are `0`.

## fs namespace

The source-level filesystem contract is typed even though the host ABI still
uses 64-bit slots. `Ok` carries the operation payload and `Err` carries an
`Error` record from `std.error`.

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.fs.fs_read` | Reads a UTF-8 file. | path | `Result<string, Error>` |
| `spectra.std.fs.fs_write` | Replaces file contents and creates missing parents when possible. | path, content | `Result<bool, Error>` |
| `spectra.std.fs.fs_append` | Appends file contents and creates missing parents when possible. | path, content | `Result<bool, Error>` |
| `spectra.std.fs.fs_exists` | Checks metadata existence; a missing path is `Ok(false)`. | path | `Result<bool, Error>` |
| `spectra.std.fs.fs_remove` | Removes a file. | path | `Result<bool, Error>` |
| `spectra.std.compat.fs.*` | Legacy sentinel adapters. | same as above | string/bool sentinel values |

Invalid paths and I/O failures are represented by `ErrorCode` values rather
than an empty string or `false`. The runtime maps `InvalidArgument`, `NotFound`,
`PermissionDenied`, `Io`, `Internal`, and `Unsupported` into the structured
error record.

## error namespace

`std.error` owns the structured error record shared by typed standard-library
operations. Its public accessors are `code`, `message`, `operation`, `context`,
`origin`, and `retryable`; `new` constructs a record from the closed
`ErrorCode` enum. Invalid host arguments still use `HOST_STATUS_INVALID_ARGUMENT`
at the ABI boundary and are not converted into a fabricated success payload.

## env namespace

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.env.env_get` / `env_get_option` | Read an environment variable without conflating absence and an empty value. | key | tagged `Option<string>` |
| `spectra.std.env.env_set` | Set an environment variable. | key, value | bool |
| `spectra.std.env.env_args_count` | Count forwarded process arguments. | none | integer count |
| `spectra.std.env.env_arg` / `env_arg_option` | Read a process argument by index. | index | tagged `Option<string>` |
| `spectra.std.compat.env.env_get` / `env_arg` | Legacy empty-string sentinel adapters. | key or index | string (`""` when absent) |

## time namespace

Spectra exposes wall-clock timestamps plus runtime-managed opaque handles for production time
operations. `Duration`, `Instant`, and `UtcDateTime` are represented as integer handles in the host
ABI; invalid handles return `HOST_STATUS_INVALID_ARGUMENT`.

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.time.time_now_millis` / `time_now_secs` | Unix wall-clock timestamp from `SystemTime`. | none | milliseconds or seconds since Unix epoch |
| `spectra.std.time.sleep_ms` | Backwards-compatible blocking sleep in milliseconds. | `ms` | `0` when a result slot is provided |
| `spectra.std.time.monotonic_millis` / `monotonic_nanos` | Monotonic elapsed time since runtime start. | none | elapsed milliseconds or nanoseconds |
| `spectra.std.time.duration_ms` / `duration_secs` | Create a non-negative duration handle. | milliseconds or seconds | duration handle |
| `spectra.std.time.duration_millis` / `duration_secs_value` | Read a duration handle. | duration handle | milliseconds or whole seconds |
| `spectra.std.time.duration_add` / `duration_sub` | Checked duration arithmetic. | duration handles | duration handle |
| `spectra.std.time.instant_now` / `instant_elapsed_ms` | Capture a monotonic instant and inspect elapsed time. | none or instant handle | instant handle or elapsed milliseconds |
| `spectra.std.time.instant_add` / `instant_has_elapsed` | Create and inspect monotonic deadlines. | instant and duration handles | instant handle or bool |
| `spectra.std.time.sleep` | Blocking sleep for a checked duration; excessive sleeps are rejected. | duration handle | none |
| `spectra.std.time.unix_to_utc` | Convert Unix seconds to UTC using deterministic civil-calendar arithmetic. | seconds | UTC datetime handle |
| `spectra.std.time.utc_year` / `utc_month` / `utc_day` / `utc_hour` / `utc_minute` / `utc_second` | Extract UTC datetime fields. | UTC datetime handle | integer field |

## range namespace

Spectra ranges are runtime-managed opaque handles. The `start..end` and `start..=end` syntax lowers
to `spectra.std.range.create`, including when the range is stored, passed to a function, compared, or
iterated later. Descending ranges such as `5..2` are valid empty ranges. Invalid handles, negative
indexes, out-of-bounds indexes, invalid inclusive flags, and length overflow return
`HOST_STATUS_INVALID_ARGUMENT`.

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.range.create` | Allocate a range handle. | `start`, `end`, `inclusive` (`0` or `1`) | range handle |
| `spectra.std.range.len` | Return the number of values produced by the range. | range handle | length |
| `spectra.std.range.at` | Return the value at a zero-based range index. | range handle, index | integer value |
| `spectra.std.range.eq` | Compare start, end, and inclusive flag by value. | two range handles | bool |
| `spectra.std.range.start` / `end` | Inspect original bounds. | range handle | bound |
| `spectra.std.range.is_inclusive` | Inspect whether the range came from `..=`. | range handle | bool |

## tensor namespace

Spectra exposes tensor operations through runtime-managed opaque handles. The alpha tensor runtime
stores CPU tensors with dtype (`int` or `float`), shape, strides, layout, shared storage, and a base
offset for safe views. Float values use the same host-call convention as other float stdlib
functions: f64 bits encoded in `SpectraHostValue`.

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.tensor.zeros` | Allocate 1D int tensor filled with zero. | `size` | handle |
| `spectra.std.tensor.ones` | Allocate 1D int tensor filled with one. | `size` | handle |
| `spectra.std.tensor.full` | Allocate 1D int tensor filled with value. | `size`, `value` | handle |
| `spectra.std.tensor.full_f` | Allocate 1D float tensor filled with value. | `size`, `value_bits` | handle |
| `spectra.std.tensor.arange` | Allocate 1D int range tensor. | `start`, `end`, `step` | handle |
| `spectra.std.tensor.zeros2` / `ones2` / `full2` / `full2_f` | Allocate 2D tensors. | `rows`, `cols`, optional `value` | handle |
| `spectra.std.tensor.uniform` / `uniform_f` / `normal_f` / `bernoulli` / `categorical` | Seeded random tensor fills. | `size`, distribution parameters | handle |
| `spectra.std.tensor.len` / `rank` / `dim` / `rows` / `cols` | Query tensor metadata. | `handle`, optional `axis` | integer metadata |
| `spectra.std.tensor.device` / `device_available` / `device_status` | Query tensor placement and capability status. `device_status` returns `0` for available CPU/WGPU and `1` for an implemented but unavailable WGPU backend; reserved or unknown device codes return `HOST_STATUS_INVALID_ARGUMENT`. | `handle` or device code | device code, bool, or status code |
| `spectra.std.tensor.to_device` / `cpu` / `sync` | Transfer to supported devices and synchronize. CPU (`0`) is supported in the default build; `wgpu` (`6`) is supported with `--features gpu` and a detected adapter. | `handle`, optional device code | handle or `0` |
| `spectra.std.tensor.precision` / `to_precision` | Query or quantize float tensor precision. Codes: `0` f64, `1` f32, `2` f16, `3` bf16. | `handle`, optional precision code | precision code or handle |
| `spectra.std.tensor.get` / `get_f` / `get2` / `get2_f` | Read tensor values. | `handle`, index or row/col | scalar |
| `spectra.std.tensor.set` / `set_f` / `set2` / `set2_f` | Mutate tensor values. | `handle`, index/row/col, value | `0` |
| `spectra.std.tensor.reshape` | Return a validated 2D view handle when possible. | `handle`, `rows`, `cols` | handle |
| `spectra.std.tensor.flatten` | Return a 1D view or materialized tensor handle. | `handle` | handle |
| `spectra.std.tensor.permute` / `transpose` | Return axis-swapped view handles. | `handle`, axes or handle | handle |
| `spectra.std.tensor.slice` | Return a 1D shared-storage slice view. | `handle`, `start`, `end` | handle |
| `spectra.std.tensor.concat` / `stack` | Combine compatible tensors. | handles | handle |
| `spectra.std.tensor.add` / `sub` / `mul` / `div` | Elementwise arithmetic with exact shape and dtype match. | `lhs`, `rhs` | handle |
| `spectra.std.tensor.neg` / `relu` / `exp_f` / `log_f` / `sqrt_f` / `sigmoid_f` / `tanh_f` | Unary CPU kernels. | `handle` | handle |
| `spectra.std.tensor.sum` / `sum_f` / `mean_f` / `min` / `max` / `argmax` | Reductions. | `handle` | scalar |
| `spectra.std.tensor.sum_t` / `mean_t` / `dot_t` | Differentiable scalar tensor loss primitives. | handles | handle |
| `spectra.std.tensor.matmul` / `matmul_batched` / `dot` | Matrix/vector kernels with shape validation. | handles | handle or scalar |
| `spectra.std.tensor.seed` | Set deterministic tensor RNG seed. | `seed` | `0` |
| `spectra.std.tensor.requires_grad` / `backward` / `grad` / `zero_grad` | Reverse-mode autodiff controls. | handles, bool flag | handle or `0` |
| `spectra.std.tensor.set_grad_enabled` / `grad_enabled` | Inference/no-grad mode. | bool flag or none | `0` or bool |
| `spectra.std.tensor.stats_graph_nodes` | Live autograd creator node count. | none | integer metric |
| `spectra.std.tensor.stats_*` / `kernel_strategy` / `reset_stats` | Allocation, buffer-pool, scratch, device-transfer, GPU-kernel, CPU-fallback, and kernel work metrics. | none | integer metric or `0` |
| `spectra.std.tensor.free` / `free_all` | Release tensor handles. | `handle` or none | `0` or freed count |

## Usage Notes

- All collection handles are process-local and must be treated as opaque identifiers by Spectra
  programs.
- Allocation failures (for example, when the manual heap exceeds its soft limit) produce
  `HOST_STATUS_INTERNAL_ERROR`.
- GPU acceleration is optional. The default build keeps CPU semantics available; with
  `--features gpu`, WGPU float kernels dispatch for supported tensor ops and fall back
  to CPU when a dispatchable kernel reports failure.
- Passing invalid handles or mismatched argument counts yields `HOST_STATUS_INVALID_ARGUMENT` or
  `HOST_STATUS_NOT_FOUND`.
- Tensor shape mismatches return `HOST_STATUS_INVALID_ARGUMENT`; invalid handles return
  `HOST_STATUS_NOT_FOUND`.
- Autodiff is supported for float tensors. Scalar host-returning calls are not graph nodes; use
  `sum_t`, `mean_t`, and `dot_t` when a differentiable scalar tensor loss is required.
- `backward` releases graph creator nodes by default. Disable graph construction in inference
  sections with `set_grad_enabled(false)` and restore it with `set_grad_enabled(true)`.

## `std.ml`

`std.ml` is the Phase 6 high-level ML layer over `std.tensor` handles.

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.ml.module_new` | Create a module handle. | none | module handle |
| `spectra.std.ml.module_add_parameter` / `module_parameter_count` / `module_parameter` | Register and inspect tensor parameters. | module, tensor/index | `0`, count, or tensor handle |
| `spectra.std.ml.module_set_training` / `module_is_training` | Toggle training/eval mode. | module, bool | `0` or bool |
| `spectra.std.ml.linear` | Differentiable dense layer. | input, weight, bias | tensor handle |
| `spectra.std.ml.conv2d` | Differentiable valid 2D convolution over flattened NCHW tensors. | input, kernel, bias, dimensions | tensor handle |
| `spectra.std.ml.dropout` / `max_pool2d` | Inference/training utilities. | tensor and layer params | tensor handle |
| `spectra.std.ml.mse_loss` / `bce_loss` / `cross_entropy_loss` / `nll_loss` | Scalar tensor losses for autodiff. | predictions/logits, targets | loss tensor handle |
| `spectra.std.ml.sgd_step` / `sgd_momentum_step` / `adam_step` / `adamw_step` | In-place optimizer updates from accumulated gradients. | parameter, state tensors, hyperparameters | `0` |
| `spectra.std.ml.exp_lr` | Exponential learning-rate schedule. | base, gamma, step | float bits |
| `spectra.std.ml.unscale_grad` | Divide accumulated parameter gradient by a finite loss scale. | parameter, scale | `0` |
| `spectra.std.ml.dataset_from_tensors` / `dataset_len` | Tensor-backed datasets. | features, labels, length | dataset handle or length |
| `spectra.std.ml.dataset_from_csv` / `dataset_from_jsonl` / `dataset_from_npy` / `dataset_from_directory` | File-backed numerical datasets. CSV uses a label column; JSONL expects `features` and `label`; NPY uses one-dimensional little-endian f64 arrays; directory datasets use `features.csv` and `labels.csv`. | paths and format parameters | dataset handle |
| `spectra.std.ml.dataset_map_features` / `dataset_filter_label_min` / `dataset_train_split` / `dataset_test_split` | Dataset transforms and deterministic train/test splitting. | dataset handle, transform/split parameters | dataset handle |
| `spectra.std.ml.dataloader_new` / `dataloader_batch_*` | Deterministic minibatch access. | dataset/loader, batch index | loader handle, count, or tensor handle |
| `spectra.std.ml.dataframe_from_csv` / `dataframe_rows` / `dataframe_cols` / `dataframe_column` | Numeric dataframe handles and column extraction. | path/frame/column | frame metadata or tensor handle |
| `spectra.std.ml.experiment_start` / `experiment_finish` | Create and finish tracked experiment runs. | name, output directory, seed or experiment handle | experiment handle or `0` |
| `spectra.std.ml.experiment_set_config` / `experiment_log_metric` / `experiment_log_artifact` | Record configs, metrics, and hashed artifacts. | experiment handle plus record data | `0` |
| `spectra.std.ml.experiment_set_lockfile` / `experiment_set_model_output` | Attach reproducibility lockfile and model output artifacts. | experiment handle, path | `0` |
| `spectra.std.ml.experiment_manifest_path` / `experiment_repro_command` / `experiment_compare_manifests` | Query manifest path, reproduction command, and compare equivalent run evidence. | experiment handle or manifest paths | string or integer comparison result |
| `spectra.std.ml.distributed_session_start` / `distributed_worker_step` / `distributed_global_step` | Deterministic single-machine simulated-worker training coordination. | session metadata or worker progress | session handle, worker step, or global step |
| `spectra.std.ml.distributed_checkpoint_save` / `distributed_resume` / `distributed_summary` / `distributed_worker_step_count` | Coordinated checkpoint, resume from checkpoint JSON, and topology/progress inspection. | session handle, checkpoint path, worker id | string, session handle, or integer |
| `spectra.std.ml.onnx_export` / `onnx_import_summary` / `onnx_validate` / `onnx_roundtrip` | Binary ONNX `ModelProto` subset export, import validation, summary, and round-trip for linear, convolutional, activation, normalization, and transformer blocks. | path and model kind | string path/summary or integer validation result |
| `spectra.std.ml.embedding_lookup` / `positional_encoding` / `layer_norm` / `gelu` / `swiglu` / `attention` | Transformer runtime primitives over real tensor handles. | tensor handles and primitive params | tensor handle |
| `spectra.std.ml.kv_cache_new` / `kv_cache_append` / `kv_cache_keys` / `kv_cache_values` / `kv_cache_len` / `logits_sample` | KV cache state and logits sampling for LLM-style inference. | cache/tensor handles and sampling params | cache handle, tensor handle, length, or sampled index |
| `spectra.std.ml.tokenizer_wordpiece` / `tokenizer_encode` / `tokenizer_decode` / `text_embed` | Deterministic WordPiece-style tokenization and hash embeddings. | tokenizer/text/dim | tokenizer handle, tensor handle, or string |
| `spectra.std.ml.vector_index_new` / `vector_index_insert` / `vector_index_query` / `vector_index_persist` / `vector_index_load` / `vector_index_set_metadata` / `vector_index_metrics` | Deterministic HNSW cosine index persisted as a checked R-3003 Artifact Container v1; legacy JSON is rejected. | index/vector/path/metadata params | index handle, versioned result/metrics JSON, path, bool, or count |
| `spectra.std.ml.rag_chunk_text` / `rag_build_prompt` / `rag_evaluate_answer` | Chunking, prompt assembly, and token-overlap F1 evaluation for RAG flows. | text/context/question/answer | JSON, prompt string, or integer permille score |
| `spectra.std.ml.metrics_classification` / `metrics_regression` / `metrics_ranking` / `metrics_generation` / `serving_metrics` | Deterministic model-evaluation metrics for classification, regression, ranking, generation, and serving behavior. | tensor handles, text, request/error counts | JSON metric payload |
| `spectra.std.ml.evaluation_report` | Write a versioned JSON evaluation report plus a human-readable `.txt` companion report. | path, name, metric JSON payloads | report path string |
- Host calls are idempotent where practical; re-registering the standard library simply replaces
  existing bindings with the same implementations.

## `std.serve`

`std.serve` provides the local in-process serving baseline used by AI examples and
runtime validation.

| Host call | Description | Arguments | Results |
|-----------|-------------|-----------|---------|
| `spectra.std.serve.server_new` / `server_warmup` / `server_is_warm` | Create and warm a local serving server. | model integer, server handle | server handle or bool |
| `spectra.std.serve.server_enqueue` / `server_process_batch` / `server_result` / `server_pending` / `server_cancel` | Queue, process, read, inspect, and cancel local inference requests. | server/request/input params | request handle, result, count, or bool |
| `spectra.std.serve.server_set_timeout` / `server_resident_model` / `server_benchmark` | Timeout, model residency, and local batch benchmark utilities. | server and numeric params | bool, model handle, or processed count |
| `spectra.std.serve.server_set_input_policy` / `server_set_output_policy` | Attach inclusive input/output range guardrails to a server. | server, min, max | bool |
| `spectra.std.serve.server_set_rate_limit` / `server_set_fallback` | Attach request limit and safe fallback result. | server, limit/fallback | bool |
| `spectra.std.serve.server_last_diagnostic` / `server_audit_log` | Read structured guardrail diagnostic JSON and versioned audit-log JSON. | server | string |
| `spectra.std.serve.server_set_model_version` / `server_monitoring_snapshot` | Attach model-version metadata and emit request/error/latency/throughput metrics. | server, version | bool or JSON string |
| `spectra.std.serve.server_distribution_summary` / `drift_check` / `export_monitoring` | Emit input/output distribution summaries, compare live traffic to references, and write versioned observability JSON. | server, JSON payloads, path, threshold | JSON string or path |
| `spectra.std.serve.reset` | Clear local serving registry state. | none | `0` |
