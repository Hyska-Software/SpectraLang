# Language Feature Maturity Policy

Updated: 2026-08-19
Roadmap item: `R-106`, `R-118`, `R-2102`, `R-2103`, `R-2104`, `R-2105`, `R-2106`, `R-2107`, `R-2108`

This file is the source of truth for language maturity labels. Documentation, examples, and CLI behavior must match this policy exactly.

## Maturity Levels

- `stable`: enabled by default, documented as part of the normal language contract, and covered by the positive test suite
- `beta`: enabled by default and usable, but still expected to evolve in ergonomics or performance
- `experimental`: available only behind an explicit feature gate when active
- `deferred`: documented only as roadmap/future work, not as usable language syntax

## Current Feature Matrix

### Stable

- modules and multi-file project discovery
- imports:
  - `import module.path`
  - `import module.path as alias`
  - `from module.path import name`
  - `public from module.path import name`
- visibility: `public`, `internal`
- `func` declarations and methods with `returns`
- records, enums, traits, impl blocks
- generics in the currently validated surface
- `dyn Trait` in the currently validated surface
- primitives, tuples, function types
- canonical `int`, `float`, `bool`, `char`, and `string` primitives
- exact-width scalar integers and floats (`i8` through `usize`, `f32`/`f64`)
  with checked casts, explicit wrapping operations, JIT/AOT storage, and the
  certified C ABI boundary
- top-level `const` evaluation for primitive literal/arithmetic/logical expressions
- module-level mutable `static` globals with constant initialization, typed
  visibility/imports, mutation, and local/cross-module JIT/AOT equivalence
- control flow:
  - `if`, `else if`, `else`
  - `if not`
  - `if let`
  - `while`
  - `while let`
  - `for ... in ...`
  - `loop`
  - `do-while` (`do { ... } while ...`)
  - `switch`
  - `match`
  - `return`, `break`, `continue`
- tuple, struct, enum, and OR-patterns in the validated pattern surface
- closures/lambdas with by-value captures in the currently validated surface
- qualified stdlib calls such as `std.io.println(...)`
- `std.tensor` production baseline runtime API for tensor handles, safe views, shape metadata, elementwise ops, reductions, transforms, 2D matmul, and batched matmul
- `std.tensor` production baseline reverse-mode autodiff for float tensor handles, scalar tensor losses, gradient accumulation, and inference/no-grad mode
- Phase 14 tensor language core baseline:
  - `Tensor<dtype, rankN, dimN|dynamic_dim, layout, device>` annotations for compiler-visible tensor metadata
  - explicitly typed rank1/rank2 float tensor literals
  - stable JSON diagnostic codes `E1401` through `E1405` for tensor rank/dtype/shape/layout/device mismatches
  - operation-aware static shape checks for elementwise tensor ops, `tensor.matmul`, `tensor.reshape`, and `ml.linear`
  - `diff { ... }` differentiable block syntax lowering to `std.tensor.backward`, with `E1406` for unsupported qualified stdlib operations
- `std.tensor` production baseline device placement contract for CPU handles (`device`, `device_available`, `to_device`, `cpu`, `sync`, `stats_device_transfers`)
- optional `std.tensor` `wgpu` accelerator backend behind Cargo feature `gpu`
- `std.tensor` mixed-precision quantization metadata/API for f32, f16, and bf16 float tensor handles
- `std.ml` production baseline runtime API for modules, layers, losses, optimizers, LR scheduling, tensor-backed datasets, and dataloaders
- Phase 8 interop baseline:
  - Python bridge through `python/spectra_bridge.py`
  - Rust helper crate `spectra-interop`
  - stable C ABI header surface
  - NumPy `.npy` v1.0 little-endian f64 one-dimensional tensor exchange
- Phase 9 package baseline:
  - `spectralang package lock/build/check/run/test/bench/doc/add/update`
  - deterministic `spectra.lock`
  - multi-package workspace builds
  - exact semver version validation
  - local path dependencies
  - local filesystem registry publish/install with checksum validation
  - Git-backed package dependencies with catalog discovery, one-command add, commit-pinned lockfile entries, SHA-256 payload checksums, and normal package imports
- Phase 10 tooling baseline:
  - LSP hover, go-to-definition, references, rename, completion, diagnostics, formatting, inlay hints, quick fixes, and semantic tokens
  - `spectralang bench` with JSON timing reports
  - source-aware `error[runtime]` diagnostics for non-zero program exits
- Phase 11 concurrency and serving baseline:
  - `std.concurrent` task handles, deterministic join, non-blocking FIFO channels, counters, stats/reset, and parallel pipeline sum
  - `std.serve` local in-process server handles, warmup, request batching, cancellation, timeout state, resident model lookup, result lookup, and deterministic toy benchmark
- Phase 12 security and operations baseline:
  - release manifests, SHA-256 checksums, signed release evidence, provenance, and CycloneDX-compatible SBOM
  - CI dependency scanning with `cargo audit` and high-severity `npm audit`
  - defined stress/soak runner with timeout, optional RSS limit, and JSON report
  - runtime debug invariant checks for host registry/manual allocation state
- Phase 13 documentation and adoption baseline:
  - `docs/book/` production adoption book for language basics, numerics, tensors, autodiff, model authoring, deployment/export, stdlib/runtime/packages, and benchmark/comparison workflow
  - six end-to-end AI reference examples under `examples/ai/`
  - automated book/example discoverability validation through `scripts/validate_ai_book.py`
  - AI example execution integrated into `run_tests.ps1`
- Phase 15 numerical performance benchmark baseline:
  - release-mode benchmark suite for tensor creation, unary ops, reductions, matmul, convolution, autodiff, optimizer steps, and data loading
  - checked-in performance thresholds in `docs/performance/r1501-benchmark-baseline.json`
  - CI-style validation through `scripts/validate_r1501_bench.py` and `run_tests.ps1`
  - runtime tensor memory-planner reports with allocation sites, release steps, active/peak bytes, and reuse-rate metrics
  - `tests/validation/83_tensor_memory_planner.spectra` validates repeated training-loop reuse and bounded memory behavior
  - portable numerical correctness artifacts for RNG, reductions, matmul, convolution, and optimizer kernels through `scripts/validate_r1503_correctness.py`
  - documented `1e-9` absolute/relative float tolerance policy enforced by the R-1503 gate
- Phase 16 tensor graph IR baseline:
  - `spectra_midend::TensorGraph` extracts lowered tensor host calls into validated graph nodes with operator, metadata, dependencies, and stable dumps
  - `cargo test -p spectra-midend --test tensor_graph_tests` and `run_tests.ps1` cover snapshot, cycle, shape mismatch, and device mismatch behavior
  - `TensorGraph::optimize` supports deterministic elementwise and reduction-adjacent fusion with optimized/unoptimized graph comparison and stable snapshots
  - `std.tensor` exposes production GPU capability diagnostics through `device_status`, `stats_gpu_kernel_ops`, `stats_cpu_fallbacks`, `stats_device_transfers`, and `kernel_strategy`
  - optional WGPU execution covers float transfer, elementwise ops, reductions, `matmul`, `std.ml.conv2d`, and autodiff-required forward kernels while preserving CPU fallback
  - `scripts/validate_r1603_gpu_backend.py` and `tests/validation/91_tensor_phase16_gpu_backend.spectra` validate the R-1603 backend contract
- Phase 17 data runtime baseline:
  - `std.ml` loads CSV, JSONL, NPY, and directory-backed numeric datasets into tensor-backed dataset handles
  - dataset transforms, label filtering, train/test split, seeded dataloaders, and numeric dataframe column extraction are available through stable APIs
  - `scripts/validate_r1701_data_runtime.py` and `tests/validation/92_ml_phase17_data_runtime.spectra` validate file-backed tabular training without Python glue
  - `std.ml` emits experiment manifests with configs, metrics, artifacts, seeds, lockfiles, model outputs, reproduction commands, and manifest comparison
  - `scripts/validate_r1702_experiment_tracking.py` and `tests/validation/93_ml_phase17_experiment_tracking.spectra` validate reproducible experiment tracking
  - `std.ml` supports deterministic single-machine distributed-training simulation with worker progress, coordinated checkpoint JSON, interruption recording, resume, and topology summaries
  - `scripts/validate_r1703_distributed_training.py` and `tests/validation/94_ml_phase17_distributed_training.spectra` validate checkpoint/resume behavior
  - `std.ml` exports/imports a validated binary ONNX `ModelProto` subset for linear, convolutional, activation, normalization, and transformer blocks
  - `scripts/validate_r1801_onnx_import_export.py` and `tests/validation/95_ml_phase18_onnx_import_export.spectra` validate ONNX round-trip behavior
  - `std.ml` implements transformer/LLM primitives for embedding lookup, sinusoidal positional encoding, layer norm, GELU, SwiGLU, scaled dot-product attention, KV cache, and logits sampling
  - `scripts/validate_r1802_transformer_primitives.py` and `tests/validation/96_ml_phase18_transformer_primitives.spectra` validate the transformer primitive baseline
  - `std.ml` implements deterministic WordPiece-style tokenization, hash embeddings, persistent cosine vector indexes, RAG chunking, prompt assembly, and token-overlap F1 evaluation
  - `scripts/validate_r1803_rag_toolkit.py` and `tests/validation/97_ml_phase18_rag_toolkit.spectra` validate the RAG toolkit baseline
  - `std.ml` implements model evaluation metrics for classification, regression, ranking, generation, and serving behavior, plus versioned machine-readable and human-readable evaluation reports
  - `scripts/validate_r1901_evaluation_metrics.py` and `tests/validation/98_ml_phase19_evaluation_metrics.spectra` validate the evaluation metrics baseline
  - `std.serve` implements AI serving guardrails with input/output policy hooks, rate limits, safe fallback results, structured diagnostics, and versioned audit logs
  - `scripts/validate_r1902_ai_safety_guardrails.py` and `tests/validation/99_phase19_ai_safety_guardrails.spectra` validate the guardrail baseline
  - `std.serve` implements serving monitoring snapshots, input/output distribution summaries, drift checks, and versioned JSON observability exports
  - `scripts/validate_r1903_model_monitoring.py` and `tests/validation/100_phase19_model_monitoring.spectra` validate the monitoring baseline
- Phase 20 production certification baseline:
  - `scripts/validate_r2001_ai_conformance.py` certifies compiler, runtime, tensors, autodiff, graph, interop, package, serving, tooling, and docs/example conformance
  - conformance reports use schema `spectralang.ai_conformance_report.v1` and conformance version `R-2001/v1`
  - `run_tests.ps1` includes the `phase20-conformance` release-candidate gate

### Beta

- typed `List<T>`/`Map<K,V>` collections, including higher-order operations;
  legacy sentinel accessors remain available only under `std.compat.collections`
- absence-safe `std.env.env_get`/`env_arg` returning `Option<string>`; legacy
  empty-string environment adapters remain available only under `std.compat.env`
- typed `std.fs` operations returning `Result<T, Error>`; the historical
  string/bool sentinel adapter remains available only under `std.compat.fs`
- `Option<T>`/`Result<T,E>` constructors, predicates, `map`, `map_err`, and
  `unwrap_or`; structured `Error` propagation is implemented for the current
  filesystem slice, while full STD/API propagation and release certification
  remain pending
- mutable/reference closure captures beyond the current by-value capture contract
- async/await execution baseline: `async func`, `async { ... }`, `Task<T>`,
  `await`, deterministic ready/poll/result/cancel host calls, and explicit
  suspend/resume/ready IR markers
- async reactor baseline: platform-selected `epoll` / `IOCP` / `kqueue`
  boundary with shared task wakeup, timer readiness, and I/O readiness events
- async structured concurrency baseline: `JoinHandle` value/error status,
  `CancelHandle`, deterministic `with_timeout`, parent/child task scopes,
  cascading cancellation, structured failure aggregation, and stable join
  ordering host calls
- async stream baseline: runtime-managed `Stream<T>` handles, async `next`,
  finite source streams, backpressure status, deterministic `done`,
  cancellation, and `map`/`filter`/`fold`/`take`/`skip`/`chunks`/`fuse`
  adaptor host calls
- async stdlib baseline: cancelable async filesystem read/write host calls,
  nonblocking TCP listener/connect/accept/read/write, UDP bind/send/recv, and
  bounded async channel send/recv host calls
- async trait object baseline: object-safe async trait methods, built-in
  `Future` and `Stream` trait object signatures, `Box<dyn Future>` /
  `Box<dyn Stream>` lowering to `dyn Trait` fat pointers, and diagnostic
  `E2108` for non-object-safe async trait methods
- async diagnostics baseline: stable `E2101` through `E2120` code range,
  non-`Send` values live across `await`, `RefCell`/interior-mutable values
  across `await`, and `!Send` values crossing spawn-style task boundaries
- formal async auto-trait bounds: `T: Send`, `T: Sync`,
  `dyn Trait + Send`, and `dyn Trait + Send + Sync` are represented in
  parser/AST/semantics/lowering, with `E2104` for missing formal evidence
- async benchmark baseline: `spectralang bench --async` emits schema
  `spectra.r2111.async_benchmark.v1` JSON for 1k, 10k, and 100k concurrent
  async tasks and is checked against
  `docs/performance/r2111-async-benchmark-baseline.json`
- first-class tensor language design beyond the current stdlib handle/autodiff API
- native DWARF/PDB source stepping beyond the current AOT debug-map workflow
- HTTP/gRPC serving, async I/O integration, distributed model residency policy, and external policy-engine integration

These are usable where covered, but still not treated as fully production-hardened language design.

### Experimental

there are currently no active experimental syntax gates. `spectralang --list-experimental` must report an empty set.

CLI compatibility contract:

- `--enable-experimental <feature>` remains accepted as a no-op for older scripts
- new experimental syntax must not be added without documenting the exact feature name here and returning it from `spectralang --list-experimental`
- parser diagnostics for future disabled experimental syntax must emit a feature-gate error with code `P004`

### Deferred

- `class` declarations, inheritance, `override`, `super`, class layout and ABI
- Unicode identifiers
- advanced numeric literal syntax beyond current decimal forms
- scalar exact-width forms outside the certified `i8`–`usize`/`f32`–`f64` matrix
- closure captures with environment objects
- `repeat/until`
- `foreach`
- `goto`
- `yield`
- raw strings and advanced literal modes
- production tensor syntax and static shape types
- native CUDA/ROCm/Metal/DirectML/Vulkan backends beyond the current optional `wgpu` baseline
- `.npz`, safetensors, checkpoints, and ONNX import/export beyond the current `.npy` baseline
- central hosted package registry protocol, authentication, provenance signatures, remote catalog sync, `--locked` enforcement, and semver range solving beyond exact/catalog versions
- async test macros beyond the `R-2103`/`R-2104`/`R-2105`/`R-2106`/
  `R-2107`/`R-2108` async runtime baseline

## Synchronization Rules

When a feature changes maturity:

1. update this file
2. update the user-facing reference docs
3. update examples if their required invocation changes
4. update CLI help or `--list-experimental` if the change affects experimental gating
5. add or adjust tests in `tests/validation`, `tests/errors`, `tests/cli`, or `examples`
