# SpectraLang Production AI Implementation Plan

## Purpose

This document defines a detailed implementation plan to evolve SpectraLang from its current alpha/beta compiler state into a production-capable programming language and platform for AI, machine learning, and numerical computing workloads.

The plan is organized as:

- phases
- workstreams
- concrete tasks
- dependencies
- acceptance criteria

The goal is not just to add syntax, but to deliver a usable production stack:

- language
- compiler
- runtime
- numerical libraries
- tensor/autodiff engine
- acceleration backends
- tooling
- packaging
- deployment

---

## Current Baseline

At the time of the original plan, the repository already had:

- working lexer, parser, AST, semantic analysis, lowering, backend, runtime plumbing, CLI
- multi-file module resolution
- basic standard library surface
- traits, generics, enums, structs, pattern matching
- formatter and lint support
- regression suite passing in the current expected scope

The first production AI roadmap baseline is now tracked as complete in
`roadmap/roadmap.toml` through Phase 13. That baseline includes:

- compiler productionization and regression infrastructure
- scientific type-system extensions
- tensor runtime, kernels, allocator, RNG, and metrics baseline
- autodiff runtime baseline
- ML module/layer/loss/optimizer/dataloader baseline
- optional accelerator baseline
- interop/package/tooling/security/serving/documentation baseline
- closure values with deterministic by-value captures

---

# Phase 0: Program Setup and Technical Governance

## Goal

Create the delivery structure required to execute the rest of the roadmap without architectural drift.

## 0.1 Architecture Governance

### Tasks

- Create `docs/adr/` and adopt ADRs for all major decisions.
- Define ownership boundaries for:
  - frontend
  - semantic/type system
  - midend/IR
  - backend/codegen
  - runtime
  - tensor engine
  - autodiff
  - package manager
  - tooling
- Define compatibility policy:
  - language surface stability level
  - stdlib stability level
  - CLI compatibility guarantees
  - IR compatibility guarantees
- Define feature maturity levels:
  - experimental
  - beta
  - stable
  - deprecated
- Define performance budget categories:
  - compile time
  - runtime throughput
  - memory overhead
  - startup time

### Acceptance Criteria

- `docs/adr/` exists with at least 5 initial ADRs.
- Ownership map exists and names each subsystem.
- Stability and compatibility policy documented.
- Feature maturity taxonomy documented and referenced by language docs.

## 0.2 Roadmap Tracking Infrastructure

### Tasks

- Create machine-readable roadmap tracker:
  - `roadmap/roadmap.toml` or `roadmap/roadmap.yaml`
- Define status fields:
  - not_started
  - in_progress
  - blocked
  - complete
- Define metadata for each roadmap item:
  - id
  - owner
  - phase
  - dependencies
  - acceptance criteria
  - risk level
- Add a script to generate Markdown progress reports from roadmap metadata.

### Acceptance Criteria

- Roadmap data file exists and validates.
- Report generator exists and produces a readable summary.
- At least 20 initial items are represented in the tracker.

---

# Phase 1: Productionize the Existing Compiler Core

## Goal

Raise compiler reliability from experimental to production-grade infrastructure.

## 1.1 Frontend Completeness Audit

### Tasks

- Audit lexer coverage against documented grammar.
- Audit parser coverage against all documented syntax.
- Audit semantic coverage against all AST node kinds.
- Audit lowering coverage against all AST node kinds.
- Audit backend coverage against all IR instruction kinds.
- Produce a gap matrix:
  - documented but unsupported
  - supported but undocumented
  - parsed but not lowered
  - lowered but not validated

### Acceptance Criteria

- A single audit document exists mapping syntax to implementation status.
- Every AST node has an explicit semantic and lowering status.
- No undocumented parser entry points remain unexplained.

## 1.2 Compiler Test Pyramid

Current state: complete for the current roadmap/backlog production baseline.
Stage-local tests already cover compiler, midend, backend, and CLI crates; this
phase adds canonical AST, diagnostic, and IR snapshots plus cargo-fuzz targets
for parser, semantic analysis, the no-op compilation pipeline, and lowering.
Continuous fuzz execution in external CI remains a future hardening extension,
not an implicit completion claim for the current local baseline.

### Tasks

- Add unit tests for:
  - lexer
  - parser
  - semantic analyzer
  - lowering
  - backend codegen
- Add snapshot tests for:
  - AST
  - diagnostics
  - lowered IR
- Add regression tests for all previously fixed bugs.
- Add randomized parser fuzz tests.
- Add semantic fuzz/property tests for:
  - scope handling
  - import resolution
  - trait resolution
  - pattern exhaustiveness
- Add cross-platform CI jobs for Windows/Linux/macOS.

### Acceptance Criteria

- Every compiler crate has stage-specific unit tests.
- Fuzz targets exist for parser, semantic analysis, pipeline, and lowering.
- Every bug fixed after this point must add a regression test.
- The regression policy is documented and validated by the main test runner.

### Current Implementation

- `compiler/tests/snapshot_tests.rs`
- `compiler/tests/snapshots/parser_ast.snap`
- `compiler/tests/snapshots/semantic_diagnostic.snap`
- `midend/tests/ir_snapshot_tests.rs`
- `midend/tests/snapshots/lowering_ir.snap`
- `fuzz/fuzz_targets/parser.rs`
- `fuzz/fuzz_targets/semantic.rs`
- `fuzz/fuzz_targets/pipeline.rs`
- `fuzz/fuzz_targets/lowering.rs`
- `docs/testing-regression-policy.md`
- `scripts/validate_test_pyramid.py`
- `run_tests.ps1` includes the R-104 structural gate.

## 1.3 Diagnostics Quality

Current state: complete for the current Phase 1 diagnostics baseline. Stable
code families and high-frequency diagnostic codes are documented, JSON
diagnostics are emitted for compile/check/lint, SARIF 2.1.0 diagnostics are
emitted for compile/check/lint, and the main test runner validates generated
JSON/SARIF reports. Midend/backend diagnostics still fall back to phase-level
codes when no stable subcode exists.

### Tasks

- Standardize diagnostic codes across:
  - syntax
  - semantic
  - lowering
  - backend
  - runtime host-call failures
- Add structured diagnostics output:
  - plain text
  - JSON
  - SARIF
- Improve fix-it hints for common cases:
  - missing imports
  - wrong arity
  - trait bound errors
  - non-exhaustive match
  - visibility violations
  - invalid return paths
- Add secondary spans where useful.
- Add notes for inferred types in mismatch errors.

### Acceptance Criteria

- Stable diagnostic-code families and high-frequency codes are documented.
- JSON diagnostics are consumable by editor tooling.
- SARIF diagnostics are consumable by external analysis tooling.
- At least 20 common diagnostics have actionable fix hints.

### Current Implementation

- `docs/diagnostics/error-code-reference.md`
- `spectralang compile --json <path>`
- `spectralang check --json <path>`
- `spectralang lint --json <path>`
- `spectralang compile --sarif <path>`
- `spectralang check --sarif <path>`
- `spectralang lint --sarif <path>`
- `scripts/validate_diagnostics_standardization.py`
- `run_tests.ps1` includes the R-105 generated-report validation gate.

## 1.4 Language Surface Stabilization

Current state: complete for the current Phase 1 maturity-policy baseline and
the human-readable syntax migration. The language maturity policy documents
stable, beta, experimental, and deferred features. The canonical surface uses
`module`, `from ... import`, `func ... returns`, `record`, `public`, `else if`,
`if not`, word-form logical operators, and line-based statement termination;
the CLI reports no active experimental syntax gates through
`--list-experimental`; and the main runner validates docs/source/CLI agreement.

### Tasks

- Mark current stable subset explicitly.
- Freeze syntax for:
  - module/imports
  - `module`, `from ... import`, and line-based statement termination
  - `func ... returns`, `record`, and `public`
  - traits/generics
  - closures
  - `if let` / `while let`
  - `else if`, `if not`, `and`, `or`, and `not`
  - enums and patterns
- Keep promoted core control flow stable:
  - `switch`
  - `do-while`
  - `loop`
- Future experimental syntax must add a documented gate, CLI list entry, and
  parser diagnostic before use.
- Remove or document partially implemented syntax.

### Acceptance Criteria

- Language reference clearly labels stable vs experimental syntax.
- No feature remains in undocumented limbo.
- CLI feature gates match documented policy exactly.
- `tests/validation/120_stable_promoted_control_flow.spectra` runs through the
  normal CLI JIT path without feature flags.

### Current Implementation

- `docs/language-feature-maturity.md`
- `spectralang --list-experimental` reports no active experimental syntax gates.
- `scripts/validate_feature_maturity.py`
- `run_tests.ps1` includes the R-106 feature maturity gate.
- `R-118` completes the production promotion of `switch`, `do-while`, and
  `loop`; the readability migration standardizes the conditional form as
  `if not`.

---

# Phase 2: Type System and Language Features Needed for Scientific Computing

## Goal

Extend the language from general-purpose typed scripting/compiled semantics into a language suitable for numerical and ML workloads.

## 2.1 Numeric Type Expansion

### Tasks

- Add primitive numeric types:
  - `i8`, `i16`, `i32`, `i64`
  - `u8`, `u16`, `u32`, `u64`
  - `f16`, `bf16`, `f32`, `f64`
  - optional: `i128`, `u128`
- Define exact casting rules.
- Define arithmetic promotion rules.
- Define overflow behavior:
  - debug
  - release
  - checked APIs
- Add literal suffix support if desired.
- Add backend support for each primitive.

### Acceptance Criteria

- Numeric type matrix is documented and implemented end-to-end.
- Backend correctly lowers all supported primitive arithmetic types.
- Semantics reject ambiguous or lossy conversions unless explicit.

## 2.2 Const Evaluation

### Tasks

- Add compile-time constant evaluation for:
  - numeric expressions
  - shape expressions
  - tuple/array length expressions
  - simple generic const-like parameters if adopted
- Allow consts in:
  - tensor shapes
  - loop bounds
  - static initialization
- Add diagnostics for non-const expressions used in const contexts.

### Acceptance Criteria

- Const-eval works for declared compile-time numeric expressions.
- Shape-related compile-time expressions are usable in tests and docs.

## 2.3 Richer Pattern and Destructuring Support

Current state: complete for the current Phase 2 pattern baseline. Tuple,
struct, enum, and OR-patterns are parsed, semantically validated, lowered, and
covered by positive validation programs plus a negative non-exhaustive enum
match test.

### Tasks

- Add `let` destructuring:
  - tuple destructuring
  - struct destructuring
  - enum destructuring
- Add OR-patterns in `match`.
- Add pattern guards if desired.
- Add destructuring assignment only if semantics remain clean.

### Acceptance Criteria

- Destructuring is fully parsed, validated, and lowered.
- Exhaustiveness checker handles the new pattern forms.

### Current Implementation

- `tests/validation/31_tuple_variant_destructuring.spectra`
- `tests/validation/60_pattern_control_surface.spectra`
- `tests/validation/63_destructuring_and_or_patterns.spectra`
- `tests/errors/non_exhaustive_enum_match.spectra`
- `scripts/validate_pattern_ergonomics.py`
- `run_tests.ps1` includes the R-203 pattern ergonomics gate.

## 2.4 Closures and Function Values Completion

### Tasks

- Finish lowering of closure values and invocation.
- Define capture model:
  - by value
- Add closure environment representation.
- Add diagnostics for invalid captures.
- Benchmark closure overhead.

### Acceptance Criteria

- Closures work in compile, check, and run modes.
- Captures are deterministic and documented.
- Function values can be passed, returned, stored, and invoked safely.

### Current Implementation

- Function values lower to runtime closure handles with layout `[code_ptr, capture_0, ...]`.
- Captures are by value in deterministic first-use order.
- Closure functions receive the environment handle as a hidden first parameter.
- Direct assignment to a captured variable inside a closure is rejected; mutable/reference captures remain a future language extension.
- `tests/validation/79_closure_captures.spectra` and `tests/errors/closure_capture_mutation.spectra` validate the production contract.

---

# Phase 3: Tensor and NDArray Core

## Goal

Make tensors a first-class, high-performance abstraction in the language ecosystem.

## 3.1 Tensor Type Design

Current state: complete for the current production baseline. ADR [0001](adr/0001-tensor-runtime-contract.md) accepts `std.tensor` as the Phase 3 tensor contract: public `Tensor` metadata plus opaque runtime handles carrying dtype, shape, strides, layout, CPU host device, and safe view semantics. Generic `Tensor<T, Shape>` syntax and static shape forms remain future type-system work, not hidden Phase 3 completion gates.

### Tasks

- Design `Tensor` API:
  - accepted Phase 3 API: `std.tensor` handle-based API with exported `Tensor` metadata
  - future API: `Tensor<T>` and optional rank/shape-aware syntax after type-system support exists
- Define core metadata:
  - shape
  - strides
  - dtype
  - device
  - layout
  - contiguity
- Decide ownership model:
  - owning tensor
  - borrowed view
  - slice/view
- Decide mutability model for views.

### Acceptance Criteria

- Tensor data model is documented with memory and ownership semantics.
- The public tensor API compiles through the whole pipeline.
- Phase 3 tensor type/API design is approved by ADR.

## 3.2 Tensor Runtime Representation

Current state: complete for the current production baseline. CPU host tensors use runtime headers with dtype, shape, strides, layout, shared storage, and base offset. Reshape, contiguous flatten, transpose, permute, and slice create safe views where possible; mutation uses copy-on-write when storage is shared.

### Tasks

- Implement runtime tensor header structure.
- Implement storage backends:
  - completed Phase 3 backend: CPU host storage
  - future backends: pinned host memory and device memory abstraction in accelerator/device phases
- Add shape and stride validation.
- Add zero-copy views where possible.
- Add copy-on-write policy for shared tensor storage.

### Acceptance Criteria

- Tensor runtime allocation/deallocation is tested.
- View semantics do not leak or alias unsafely.
- Copy-on-write mutation isolation is tested.

## 3.3 Tensor Operations MVP

Current state: complete for the current production baseline. `std.tensor` covers creation, metadata, reshape, flatten, permute, transpose, slice, concat, stack, elementwise arithmetic, unary kernels, reductions, argmax, dot, 2D matmul, batched matmul, RNG fills, and runtime metrics over integer and float tensors where applicable.

### Tasks

- Implement core ops:
  - `zeros`
  - `ones`
  - `full`
  - `arange`
  - `reshape`
  - `transpose`
  - `permute`
  - `flatten`
  - `slice`
  - `concat`
  - `stack`
- Implement elementwise ops:
  - add/sub/mul/div
  - neg
  - exp/log/sqrt
  - relu/sigmoid/tanh
- Implement reductions:
  - sum
  - mean
  - max
  - min
  - argmax
- Implement matrix ops:
  - matmul
  - batched matmul
  - dot

### Acceptance Criteria

- All core ops have unit tests, shape tests, and numeric correctness tests.
- Shape mismatch diagnostics are deterministic through host status codes.
- Performance baseline exists for CPU execution through the Phase 4 benchmark harness.

## 3.4 Shape System

Current state: complete for the current production baseline. Runtime rank, dimension, slice bounds, reshape size, concat/stack compatibility, matmul compatibility, batched matmul compatibility, transpose, and permute axis checks are enforced consistently with deterministic host status codes.

### Tasks

- Decide whether shape checking is:
  - accepted Phase 3 model: runtime-only validation
  - future model: partially static or rank-static / shape-dynamic hybrid after typed tensor syntax exists
- Add internal shape algebra representation.
- Add diagnostics for:
  - invalid reshape
  - incompatible broadcast in future broadcast ops
  - reduction axis errors in future axis-aware reductions
  - invalid transpose axes

### Acceptance Criteria

- Shape validation is deterministic and well-tested.
- Rank and axis validation are enforced for the current tensor API.

---

# Phase 4: Numerical Runtime and Kernel Layer

## Goal

Build the numerical execution layer required for serious ML workloads.

## 4.1 CPU Kernel Library

Current state: complete for the current production baseline. `std.tensor` has portable CPU kernels, deterministic runtime work metrics, a checked-in release benchmark gate, and an explicit SIMD/BLAS policy for the default Windows-compatible build.

### Tasks

- Implement optimized CPU kernels for tensor primitives.
- Add architecture-aware vectorization strategy:
  - scalar fallback
  - AVX2
  - AVX-512
  - NEON
- Decide when to use handwritten kernels vs external libraries.
- Integrate BLAS/LAPACK where appropriate.
- Add benchmark suite against:
  - NumPy baseline
  - PyTorch CPU baseline

### Acceptance Criteria

- Core kernels match or outperform naive scalar loops in release benchmarks.
- Benchmarks are repeatable in CI/perf runs.
- SIMD/BLAS decisions are implemented or explicitly rejected with benchmark evidence.

## 4.2 Memory and Allocator Strategy for Numerical Workloads

Current state: complete for the current production baseline. A runtime tensor buffer pool, scratch reuse tracking, allocation metrics, and benchmark evidence are implemented.

### Tasks

- Add tensor-aware allocator and buffer pool.
- Reduce heap churn for short-lived tensors.
- Add temporary buffer reuse for matmul/reduction kernels.
- Add alignment guarantees for vectorized code.
- Add memory profiling hooks.

### Acceptance Criteria

- Allocator metrics exist for tensor-heavy programs.
- Repeated tensor ops do not show pathological allocation churn.
- Alignment and scratch-buffer behavior are validated by tests or benchmarks.

## 4.3 Randomness and Statistical Primitives

Current state: complete for the current production baseline. Seeded tensor random fills exist for integer uniform, float uniform, float normal, Bernoulli, and categorical sampling, with deterministic and statistical sanity validation.

### Tasks

- Implement RNG subsystem:
  - seeded deterministic RNG
  - CPU vectorized RNG
- Add distributions:
  - uniform
  - normal
  - Bernoulli
  - categorical
- Add tensor random fills and sampling.

### Acceptance Criteria

- RNG reproducibility is guaranteed by seed.
- Distribution tests pass basic statistical sanity checks.

---

# Phase 5: Autodiff Engine

## Goal

Make the platform suitable for neural network training.

## 5.1 Reverse-Mode Autodiff Core

Current state: complete for the current production baseline. ADR [0002](adr/0002-autodiff-runtime-contract.md) accepts eager reverse-mode autodiff in `std.tensor` for float tensors, scalar tensor losses, gradient accumulation, and default graph release after `backward`.

### Tasks

- Choose eager autodiff vs graph-building vs hybrid.
  - accepted Phase 5 model: eager graph-building reverse mode in the tensor runtime
- Implement computation graph node model.
- Track:
  - value tensor
  - grad tensor
  - parent links
  - backward closure or op descriptor
- Add `requires_grad` semantics.
- Implement `backward()` from scalar loss.

### Acceptance Criteria

- Gradients are correct for scalar and tensor examples.
- Reverse-mode tests pass against analytical gradients.
- `tests/validation/71_tensor_phase5_autodiff.spectra` compiles and runs through the public API.

## 5.2 Gradient Rules

Current state: complete for the current production baseline. Gradient rules are implemented for the supported differentiable `std.tensor` operation set. Broadcast-specific reduction is deferred until broadcasted tensor operations are added to the production tensor API.

### Tasks

- Implement gradient rules for:
  - elementwise arithmetic
  - reductions
  - matmul
  - transpose
  - broadcasted ops when broadcasted tensor ops exist
  - activation functions
- Add tensor-returning scalar loss primitives:
  - `sum_t`
  - `mean_t`
  - `dot_t`
- Add broadcast-aware gradient reduction when broadcasted tensor ops exist.
- Add gradient accumulation semantics.

### Acceptance Criteria

- Finite-difference gradient checks pass for all supported ops.
- Reduction gradient rules are correct.
- Broadcast gradient rules are tracked as future work with broadcasted tensor operations.

## 5.3 Graph Lifetime and Memory Control

Current state: complete for the current production baseline. Graph nodes are released after `backward` by default, `stats_graph_nodes` exposes graph retention, and `set_grad_enabled(false)` disables autograd construction for inference/no-grad blocks.

### Tasks

- Add graph retention policy.
- Add `no_grad` / inference mode.
- Add graph release after backward by default.
- Add graph observability through `stats_graph_nodes`.
- Keep checkpointing as future memory tradeoff work.

### Acceptance Criteria

- Training loops do not leak graph memory across iterations.
- Inference mode avoids autograd overhead.

---

# Phase 6: High-Level ML Framework Layer

## Goal

Provide a developer-facing framework for neural network and ML model authoring.

## 6.1 NN Module System

Current state: complete for the current production baseline. ADR [0003](adr/0003-ml-framework-runtime-contract.md) accepts `std.ml` as a runtime-backed high-level ML layer over `std.tensor`.

### Tasks

- Add `Module` or equivalent abstraction.
- Implement built-in layers:
  - linear
  - conv2d
  - dropout
  - pooling
  - future: embedding, layer norm, batch norm
- Add parameter registration.
- Add mode switching:
  - training
  - eval

### Acceptance Criteria

- A simple MLP and CNN can be defined and trained end-to-end.
- Parameters are discoverable through module handles.
- Serialization remains future package/model tooling work.

## 6.2 Losses and Optimizers

Current state: complete for the current production baseline. `std.ml` includes scalar tensor losses, first-order optimizers, Adam-family optimizers, and exponential LR scheduling.

### Tasks

- Implement losses:
  - MSE
  - cross entropy
  - BCE
  - NLL
- Implement optimizers:
  - SGD
  - momentum SGD
  - Adam
  - AdamW
- Add learning rate scheduling.

### Acceptance Criteria

- Standard toy models converge on simple datasets.
- Optimizer steps are validated numerically.

## 6.3 Data Pipeline APIs

Current state: complete for the current production baseline. Tensor-backed datasets and dataloaders support deterministic minibatches and reproducible shuffling for in-memory training examples.

### Tasks

- Add dataset abstraction.
- Add dataloader:
  - batching
  - shuffling
  - future: parallel prefetch
  - future: pinned-memory transfer path
- Add basic dataset readers:
  - future: CSV
  - future: image folders
  - future: JSONL

### Acceptance Criteria

- End-to-end training example can stream minibatches.
- Data loading supports reproducible shuffling.

---

# Phase 7: Accelerator Backends

## Goal

Support serious training and inference throughput.

## 7.1 Device Abstraction

Current state: complete for the current production baseline. ADR [0004](adr/0004-device-runtime-contract.md) accepts explicit device placement over `std.tensor` handles: `device`, `device_available`, `to_device`, `cpu`, `sync`, and `stats_device_transfers`. CPU (`0`) is available in the default build; `wgpu` (`6`) is available behind the optional `gpu` Cargo feature when a real adapter is detected; CUDA (`1`), ROCm (`2`), Metal (`3`), DirectML (`4`), and Vulkan (`5`) remain reserved.

### Tasks

- Keep CPU (`0`) and optional WGPU (`6`) as the implemented device contract.
- Keep CUDA, ROCm, Metal, DirectML, and Vulkan explicitly reserved until each has a real backend, capability probe, transfer path, kernel coverage, and validation gate.
- Preserve explicit tensor placement APIs and predictable host/device transfer semantics.
- Do not describe reserved device codes as supported accelerators.

### Acceptance Criteria

- Tensor device placement is inspectable and tested.
- Host/device transfer APIs behave predictably.

## 7.2 GPU Kernel Execution

Current state: validated WGPU baseline, still in progress toward production acceleration. The optional `gpu` feature executes real float kernels for elementwise arithmetic, `relu`, `sum_f`, `matmul`, `ml.conv2d`, selected resident training operations, and supported backward paths. Upload, device buffer pooling, residency, typed GPU errors, CPU fallback, and diagnostics are tested. The backend is not yet CUDA/ROCm/Metal/Vulkan, and kernel efficiency remains limited: `sum` uses `workgroup_size(1)`, `matmul` has no tiling, `conv2d` uses nested loops per thread, and some public paths still require host materialization.

Note (2026-07-13): performance-expansion blocks for tiled/parallel kernels, broader device memory planning, GPU mixed precision, graph execution, optimizer kernels, and cross-language GPU speedup were retired from the original R-30xx plan. The validated current baseline separately retains real upload, typed errors, buffer pooling, full residency, and supported backward kernels; these are tracked as complete sub-items. Production GPU speedup is not claimed by the baseline and remains follow-up evidence.

### Remaining implementation steps

1. Extract device execution behind a typed executor/backend boundary; keep WGPU runtime dispatch separate from tensor registry and host-call registration.
2. Add shader/pipeline caching and efficient reduction, tiled matmul, and convolution kernels; measure correctness and throughput without lowering the baseline gate.
3. Extend residency-aware execution so supported chains avoid host round-trips; make fallback decisions observable per operator.
4. Implement compiler-native tensor/device lowering through `R-2904`; host calls remain compatibility backend, not sole tensor representation.
5. Add asynchronous stream/queue semantics only after ordering, synchronization, error propagation, and lifetime contracts are specified and tested.
6. Add new native accelerator backends only with explicit capability, transfer, kernel, and conformance evidence.

### Acceptance Criteria

- Same tensor programs run on CPU and GPU with identical semantics.
- GPU benchmarks record CPU/GPU timings and semantic parity for target workloads.
- Speedup is follow-up evidence until efficient kernels and workload methodology are established; no current baseline claim promises GPU speedup.

## 7.3 Mixed Precision

Current state: host half complete; GPU half not started. `std.tensor.to_precision` supports f64, f32, f16, and bf16 quantization for float tensors, `std.tensor.precision` exposes precision metadata, and `std.ml.unscale_grad` supports host loss-scaling workflows. `tests/validation/76_mixed_precision_training.spectra` validates host convergence. No f16/bf16 WGSL shader, autocast/precision scope, or GPU-side loss-scaling path exists yet.

### Remaining implementation steps

1. Add WGPU feature detection and typed unsupported-feature diagnostics.
2. Add f16 storage/shader variants; define bf16 representation and conversion policy.
3. Add explicit precision scope or autocast policy, then GPU loss scaling and unscale operations.
4. Validate numerical stability and convergence against the existing host reference.

### Acceptance Criteria

- Mixed precision training works on supported devices.
- Numerical stability tests pass for standard training loops.

---

# Phase 8: Interoperability and Ecosystem Entry

## Goal

Make SpectraLang usable inside existing AI ecosystems.

## 8.1 Python Interop

### Tasks

- Define Python binding strategy:
  - embed Python
  - generate Python extension modules
  - FFI bridge through C ABI
- Support conversion between:
  - Spectra tensors
  - NumPy arrays
  - PyTorch tensors where practical
- Add notebook compatibility strategy.

### Acceptance Criteria

- A Spectra program can be called from Python through the CLI/JIT boundary.
- Tensor data can be exchanged with NumPy through the `.npy` baseline format.

### Current Implementation

- `python/spectra_bridge.py` provides the Python bridge.
- `python/demo_phase8.py` validates Python calling Spectra and exchanging tensor data with NumPy.
- The current baseline prioritizes deterministic interoperability over zero-copy embedding. Zero-copy Python extension work remains future scope unless added as a new roadmap item.

## 8.2 C / C++ / Rust FFI

### Tasks

- Define stable C ABI for exported functions.
- Generate headers or bindings.
- Add Rust helper crate for safe interop.
- Add zero-copy tensor interop contracts where possible.

### Acceptance Criteria

- Foreign code can call compiled Spectra modules via stable ABI.
- FFI examples exist in C and Rust.

### Current Implementation

- `tools/spectra-interop` provides the stable ABI crate with `cdylib` and `rlib` outputs.
- `tools/spectra-interop/include/spectra_interop.h` defines the C ABI.
- `tools/spectra-interop/examples/rust_ffi_sample.rs` validates the Rust helper path.
- `tools/spectra-interop/examples/c_ffi_sample.c` compiles and runs against the generated release import library with LLVM `clang`.
- `run_tests.ps1` builds the release interop library, compiles the C sample, and runs the resulting executable when a supported C compiler is available.

## 8.3 Model and Data Format Support

### Tasks

- Add import/export for:
  - ONNX
  - `.npy`
  - `.npz`
  - safetensors
  - checkpoints
- Add tokenizer and text dataset integration strategy if NLP is in scope.

### Acceptance Criteria

- At least one external model/data format can round-trip successfully.

### Current Implementation

- NumPy `.npy` v1.0 little-endian f64 arrays are the completed baseline interchange format.
- Round-trip validation exists through Rust unit tests, C ABI in-process tests, Rust sample, and Python/NumPy demo.
- `.npz`, safetensors, checkpoints, and ONNX are not yet implemented and should be tracked as later roadmap work before being claimed as production features.

---

# Phase 9: Package Manager, Registry, and Dependency Resolution

## Goal

Provide a real production ecosystem.

## 9.1 Package Manager

### Tasks

- Define `spectra` package commands:
  - build
  - run
  - check
  - test
  - bench
  - doc
  - add
  - update
  - publish
- Implement dependency resolver.
- Add lockfile support.
- Add semver handling.

### Acceptance Criteria

- A multi-package workspace can be built reproducibly.
- Lockfile guarantees deterministic resolution.

### Current Implementation

- `spectralang package lock/build/check/run/test/bench/doc/add/update` is implemented.
- `spectra.lock` records deterministic package order, package versions, path sources, manifest hashes, and resolved dependency sources.
- Exact semver versions are validated for manifests and dependencies.
- Local path dependencies and workspace members are resolved through `tools/spectra-cli/src/package.rs`.
- Normal project compilation includes package dependency sources when a manifest contains multi-package dependency metadata.
- Git-backed package sources are now supported through catalog or direct Git metadata:
  - `spectralang package add <name>` resolves a configured package catalog and installs the newest matching package version.
  - `spectralang package add <name> --git <url> --tag <tag>` installs directly from a public Git repository.
  - `spectra.lock` version 2 records source kind, Git URL/ref, resolved commit SHA, SHA-256 checksum, manifest hash, and dependency graph metadata.
  - `spectralang package search/info/versions/tree/register/publish-metadata/fetch` covers package discovery, developer registration metadata, dependency graph inspection, and cached fetch validation.
  - `scripts/validate_r914_package_catalog_git.py` validates a deterministic local Git catalog flow with transitive dependencies and normal Spectra imports.

## 9.2 Registry

### Tasks

- Define package registry protocol.
- Implement auth and publishing model.
- Add checksums and provenance.
- Add internal/private registry support.

### Acceptance Criteria

- A package can be published and consumed from a registry.
- Dependency downloads are integrity-checked.

### Current Implementation

- The completed Phase 9 baseline is a local filesystem registry.
- `spectralang package publish --registry <path>` copies a package payload into the registry and writes checksum metadata.
- `spectralang package add <name> --registry <path> --version <version>` verifies the checksum before installing into `.spectra/packages`.
- Git catalog publication requires immutable tag/rev refs, records resolved commit SHA, validates catalog metadata, and rejects conflicting same-version source changes.
- Central hosted registries, authentication, provenance signatures, remote catalog synchronization, host allowlist policy, atomic cache writes, `--locked` enforcement, and full compatibility-gate resolution remain future hardening work tracked by `R-911`, `R-912`, and `R-913`.

---

# Phase 10: Tooling, IDE, and Developer Experience

## Goal

Make the language productive enough for daily engineering.

## 10.1 LSP Completion

### Tasks

- Complete and stabilize LSP support for:
  - hover
  - go to definition
  - references
  - rename
  - completion
  - diagnostics
  - document symbols
  - semantic tokens
- Add language-aware completion for:
  - traits
  - imports
  - modules
  - tensor APIs

### Acceptance Criteria

- VS Code workflow is good enough for real project editing.
- LSP integration test suite exists.

### Current Implementation

- `tools/spectra-lsp` supports hover, go-to-definition, references, rename, completion, diagnostics, document/workspace symbols, formatting, inlay hints, quick fixes, and semantic tokens.
- `prepareRename` and `rename` are implemented with semantic linking where available and bounded lexical fallback for local identifiers.
- `cargo test -p spectra-lsp` covers rename edits for definitions, uses, and identifier boundaries.

## 10.2 Debugger and Runtime Introspection

### Tasks

- Decide debugging strategy:
  - source maps
  - AOT debug sidecars for native debugger workflows
  - JIT introspection strategy
- Add stack traces.
- Add panic/runtime error reporting with frames and locals where possible.

### Acceptance Criteria

- Runtime crashes are diagnosable with source locations.
- AOT artifacts emit a source debug map that can be used with native symbols in gdb/lldb workflows.

### Current Implementation

- `spectralang run` emits an `error[runtime]` diagnostic with the source location and stack frame `0: main()` when a program exits with a non-zero status.
- `spectralang compile --emit-object` and `--emit-exe` write a sibling `.spectra-debug.json` source map containing the artifact path, source path, entrypoint span, exported native symbol, and supported debugger workflow.
- `scripts/validate_debugger_stack_traces.py` validates runtime stack output and AOT debug map emission.
- R-2903 owns native debug emission. Windows AOT debug builds attach compiler-owned CodeView records and request linker-produced PDBs; Unix builds generate DWARF v4 DIEs and line programs through `gimli` and attach them to the native object when a Unix target/toolchain is available. The CLI now receives function/local metadata from the IR and Cranelift value labels are collected from the register allocator; source-text scanning is not an authoritative debug source. The `.spectra-debug.json` file remains supplementary metadata and is never accepted as a replacement for the native artifact. Structural parsing is mandatory in the Phase 29 gate; interactive debugger smoke must execute when a usable debugger is available and is not satisfied by tool discovery alone. The aggregate `run_tests.ps1` records the known allocator-location gap as `INFO:DEFERRED` when structural checks pass, while the direct validator remains fail-closed. R-2903 remains incomplete until allocator locations are consumed by final native records and a Linux lane validates DWARF.

## 10.3 Profiler and Benchmark Tooling

### Tasks

- Add `spectra bench`.
- Add CPU and memory profiling hooks.
- Add tensor/op timing instrumentation.
- Add regression benchmark suite for compiler and runtime.

### Acceptance Criteria

- Benchmark results are repeatable.
- Performance regressions are detectable automatically.

### Current Implementation

- `spectralang bench <paths>` runs compilation with pipeline timing metrics.
- `--bench-json <path>` writes machine-readable module and aggregate timings that can be diffed by CI or scripts.
- `spectralang package bench` runs the same benchmark mode for package workspaces.

---

# Phase 11: Concurrency, Data Loading, and Serving Readiness

## Goal

Enable real ML training/inference system architecture.

## 11.1 Concurrency Model

Current state: complete for the current production baseline. The implemented
model is stdlib-only and exposed through `std.concurrent`; no new concurrency
syntax was added. The runtime provides task handles, deterministic `join`,
non-blocking FIFO channels, counters, reset/stats functions, and a parallel
chunked `pipeline_sum` primitive. The midend recognizes aliased
`std.concurrent` calls as host calls, so examples compile and execute through
the normal CLI pipeline.

### Tasks

- Preserve current stdlib-only task handles, channels, counters, synchronization, and specialized real-thread `pipeline_sum` behavior.
- Do not describe `task_spawn` as general worker execution: it stores an immediate host value and returns a deterministic slot handle.
- Define a separate future workstream for arbitrary-function parallel execution, worker pool/scheduler semantics, data-race rules, and parallel loop syntax if the language adopts them.

### Acceptance Criteria

- Parallel data loading and pipeline stages work correctly.
- Concurrency primitives have deterministic tests.

## 11.2 Inference Serving Foundations

Current state: complete for the current in-process serving baseline. The
implemented API is exposed through `std.serve` and covers local server handles,
warmup, request queueing, batching, cancellation, timeout state, resident model
lookup, result lookup, and deterministic toy benchmarking. This does not claim
HTTP/gRPC or async network transport readiness; those remain future hardening
items outside the completed Phase 11 baseline.

### Tasks

- Add local runtime support needed for model serving.
- Add batching and request queue abstractions.
- Add model warmup and memory residency controls.
- Add cancellation and timeouts.
- Track HTTP/gRPC or async network transport as future hardening outside this
  completed baseline.

### Acceptance Criteria

- A toy inference server can be implemented and benchmarked.

---

# Phase 12: Security, Reliability, and Production Operations

## Goal

Meet the baseline expectations of production infrastructure.

## 12.1 Supply Chain and Build Security

Current state: complete for the current production baseline. Release assets now
have generated SHA-256 checksums, signed manifest evidence, provenance metadata,
and a CycloneDX-compatible SBOM derived from Cargo and npm lockfiles. CI includes
Rust and npm dependency scanning. Production release signing requires the
`SPECTRA_RELEASE_SIGNING_KEY` secret; local validation may use the documented
development key path only for tests.

### Tasks

- Add reproducible builds where possible.
- Sign releases.
- Add SBOM generation.
- Add dependency/license scanning.

### Acceptance Criteria

- Release artifacts have provenance and checksums.
- CI includes dependency security scanning.

## 12.2 Reliability and Crash Safety

Current state: complete for the current defined stress/soak baseline. The
versioned stress runner covers representative parser/compile, runtime/JIT,
tensor/autodiff, concurrency/serving, and package workflows with timeouts,
optional RSS limits, and JSON reports. Long nightly soak windows remain an
operations policy decision rather than a missing fast-regression requirement.
Runtime invariant checks cover host registry and manual allocation state, and
host invocation validates buffers and reports internal error on contained host
panic paths.

### Tasks

- Add runtime invariant checks in debug mode.
- Add panic containment strategy for host interop.
- Add stress tests for:
  - parser
  - allocator
  - JIT
  - tensor engine
  - autodiff graph lifecycle

### Acceptance Criteria

- Stress suites run without crashes or leaks in supported scenarios.

---

# Phase 13: Documentation and Adoption Layer

## Goal

Make the language understandable and adoptable by real teams.

## 13.1 Documentation Set

Current state: complete for the current production adoption baseline. The
checked-in Spectra book under `docs/book/` covers the language, numerics,
tensors, autodiff, model authoring, deployment/export, and the stdlib/runtime
surfaces needed to train and export toy AI models from docs alone.

### Tasks

- Write the Spectra book:
  - language basics
  - numerics
  - tensors
  - autograd
  - model authoring
  - deployment
- Write standard library reference.
- Write runtime and FFI reference.
- Write package manager guide.

### Acceptance Criteria

- A new user can build, train, and export a toy model following the docs alone.

### Current Implementation

- `docs/book/README.md` defines the reading path and verified AI examples.
- `docs/book/01-language-basics.md` through
  `docs/book/08-benchmarks-and-comparisons.md` provide the production adoption
  tutorial set.
- `scripts/validate_ai_book.py` verifies chapter coverage and example
  discoverability.
- `run_tests.ps1` executes the Phase 13 book validation.

## 13.2 AI-Focused Examples and Reference Apps

Current state: complete for the current production adoption baseline. Six
AI-focused `.spectra` programs run end-to-end through the normal CLI and are
wired into the repository validation runner.

### Tasks

- Add examples:
  - linear regression
  - logistic regression
  - MLP on MNIST-like dataset
  - CNN image classifier
  - transformer toy inference
  - data preprocessing pipeline
- Add benchmark and comparison notebooks.

### Acceptance Criteria

- At least 3 end-to-end ML examples run successfully in CI or gated integration environments.

### Current Implementation

- `examples/ai/linear_regression_train_export.spectra`
- `examples/ai/logistic_regression_train_export.spectra`
- `examples/ai/mlp_training_serving.spectra`
- `examples/ai/cnn_image_classifier.spectra`
- `examples/ai/toy_transformer_inference.spectra`
- `examples/ai/data_preprocessing_pipeline.spectra`
- `scripts/ai_examples_benchmark.py` emits machine-readable JSON timing evidence
  for all AI examples.
- `run_tests.ps1` executes all six AI examples as gated Phase 13 checks.

## 13.3 Human-Readable Language Surface

Current state: complete. The report-driven syntax pass selected the changes
that improve readability without expanding the primitive type system: explicit
`func ... returns` declarations, `record`/`public`, `from ... import` names,
`else if`, `if not`, word-form logical operators, `when ... then` match arms,
and line-based statement termination. The migration is intentionally breaking:
legacy spellings such as `fn`, `struct`, `pub`, `elif`, `unless`, `of`, arrow
returns, brace imports, and statement semicolons are rejected by the parser.

### Acceptance Criteria

- canonical syntax parses, type-checks, lowers, and runs through the normal CLI;
- legacy spellings fail with actionable parser diagnostics;
- fixtures, examples, packages, formatter, LSP, and fenced documentation use
  the canonical surface;
- the migration scripts report no remaining source or embedded-fixture work;
- the standalone report remains preserved as the audit input and is not
  overwritten by the implementation.

### Current Implementation

- `scripts/migrate_syntax_surface.py`
- `scripts/migrate_embedded_spectra_in_rust.py`
- `scripts/migrate_embedded_spectra_in_python.py`
- `scripts/migrate_syntax_docs.py`
- `tests/validation/syntax_human_surface.spectra`
- `compiler/tests/syntax_readability.rs`

---

# Cross-Cutting Non-Functional Requirements

## Performance Requirements

### Tasks

- Define baseline performance targets for:
  - parse time
  - semantic analysis time
  - lowering time
  - JIT startup
  - matmul throughput
  - training step throughput
- Track perf over time.

### Acceptance Criteria

- Benchmarks exist for all critical paths.
- Regressions are visible before release.

## Memory Requirements

### Tasks

- Add leak detection in tests.
- Add allocator and tensor memory accounting.
- Add graph retention memory metrics.

### Acceptance Criteria

- Memory growth under repeated training iterations is bounded and tested.

## Stability Requirements

### Tasks

- Add nightly stress suites.
- Add randomized AST and semantic tests.
- Add long-running JIT/runtime soak tests.

### Acceptance Criteria

- Repeated long-run stress tests complete without crashes or unbounded leaks.

---

# Recommended Delivery Order

## Near-Term Priority

1. Phase 1: productionize compiler core
2. Phase 2: numeric type system expansion
3. Phase 3: tensor core
4. Phase 4: CPU kernel layer
5. Phase 5: autodiff
6. Phase 6: high-level ML layer

## Mid-Term Priority

7. Phase 7: accelerator backends
8. Phase 8: interop
9. Phase 9: package manager and registry
10. Phase 10: tooling maturity

## Long-Term Priority

11. Phase 11: concurrency and serving
12. Phase 12: security and operations
13. Phase 13: adoption/documentation scale-out

---

# Current Production AI Baseline

The first production AI roadmap baseline is tracked as complete through Phase 13.
That means SpectraLang now has a validated compiler/runtime/tooling/package/docs
foundation for AI-oriented development in the scope covered by the roadmap.

The next development cycle should not reopen completed baseline work. Instead,
it should extend the platform toward a fuller AI/ML ecosystem with compiler-
visible tensors, graph compilation, production accelerator execution, richer
data/model workflows, LLM/RAG primitives, monitoring, and conformance gates.

---

# Next Horizon: Complete AI/ML Development Platform

## Delivery Order

1. Phase 14: AI language core
2. Phase 15: production numerical performance
3. Phase 16: accelerator and graph compilation
4. Phase 17: data and experiment platform
5. Phase 18: model ecosystem and LLM workloads
6. Phase 19: AI operations and evaluation
7. Phase 20: production certification

## Phase 14: AI Language Core

### Goal

Promote AI/ML concepts from stdlib-host-call patterns into first-class language
and compiler constructs.

### Workstreams

- `R-1401 First-Class Tensor Language Constructs`: tensor literals, dtype/device/layout annotations, compiler-visible tensor operation semantics, and compatibility with the current `std.tensor` handle API.
- `R-1402 Shape and DType Type System`: static/dynamic dimensions, rank constraints, dtype/layout/device constraints, and check-time diagnostics for static cases.
- `R-1403 Differentiable Language Blocks`: syntax and semantic rules for differentiable functions/blocks, plus diagnostics for unsupported operations.

### Current Implementation State

Status: R-1401, R-1402, and R-1403 are complete for the current Phase 14 production baseline.

Completed:

- `Tensor<dtype, rankN, dimN|dynamic_dim, layout, device>` is represented in compiler semantic types and midend IR while preserving the runtime handle ABI.
- Explicit `Tensor<float, rank1>` and `Tensor<float, rank2>` literals lower to runtime tensor allocation.
- Rank, dtype, static shape, layout, and device mismatches fail during semantic analysis with stable JSON diagnostic codes `E1401` through `E1405`.
- Static shape checks cover declared tensor compatibility, elementwise tensor operations, `tensor.matmul`, `tensor.reshape`, and `ml.linear`.
- `diff { ... }` is available as the language-level differentiable block expression and lowers to compiler-visible R-3004 reverse steps dispatched directly to runtime kernels; the public `std.tensor.backward` path remains compatibility-only.
- Unsupported qualified stdlib operations inside `diff { ... }` fail with stable diagnostic `E1406`.
- Gradient validation covers direct tensor math, control-flow around differentiable blocks, and `std.ml` layer/loss integration. Interprocedural differentiation of user-defined helper calls remains a future extension outside the current Phase 14 completion gate.

Future extensions outside the Phase 14 completion gate:

- dedicated differentiable function annotations beyond block syntax
- richer interprocedural differentiability proofs for user-defined functions
- symbolic/static scalar shape values beyond `dimN` and `dynamic_dim`

### Acceptance Direction

- Tensor programs should be expressible without ad-hoc host-call style for common cases.
- Static tensor mistakes should fail during `check` where possible.
- Differentiable regions should be explicit, diagnosable, and testable.

## Phase 15: Production Numerical Performance

### Goal

Provide measurable production-grade numerical performance, memory behavior, and
regression tracking.

### Workstreams

- `R-1501 Numerical Performance Benchmark Suite`: release-mode benchmarks for core tensor, autodiff, optimizer, and data-loading paths.
- `R-1502 Memory Planner and Tensor Lifetime Analysis`: tensor lifetime metadata, temporary reuse, peak memory reports, and allocation-site visibility.
- `R-1503 Numerical Correctness and Determinism Certification`: deterministic RNG/numerics modes, float tolerance policy, and cross-platform correctness artifacts.

### Current Implementation State

Status: R-1501, R-1502, and R-1503 are complete for the current Phase 15 production baseline.

Completed:

- `runtime/examples/numerical_performance_bench.rs` provides release-mode JSON benchmarks for tensor creation, unary ops, reductions, matmul, convolution, autodiff, optimizer steps, and data loading.
- `docs/performance/r1501-benchmark-baseline.json` stores checked-in regression thresholds.
- `scripts/validate_r1501_bench.py` compares observed release results against the baseline and is integrated into `run_tests.ps1`.
- `std.tensor.memory_report()` exposes runtime tensor lifetime plans with allocation sites, release steps, active/peak bytes, and reuse-rate metrics.
- `tests/validation/83_tensor_memory_planner.spectra` validates common training-loop reuse without unbounded memory growth.
- `runtime/examples/numerical_correctness_cert.rs` and `scripts/validate_r1503_correctness.py` provide portable correctness artifacts for RNG, reductions, matmul, convolution, and optimizer kernels under the documented `1e-9` absolute/relative tolerance policy.

### Acceptance Direction

- Performance must be measured with checked-in baselines.
- Memory behavior must be bounded and visible.
- Numerical correctness must be reproducible across supported platforms.

## Phase 16: Accelerator and Graph Compilation

### Goal

Compile tensor/model programs to optimized graph and device execution targets.

### Workstreams

- `R-1601 Tensor Graph IR`: graph-level tensor IR with ops, shapes, dtypes, devices, dependencies, validation, and stable dumps.
- `R-1602 Graph Optimization and Fusion`: elementwise fusion, constant/layout propagation, memory-aware scheduling, and optimized/unoptimized comparisons.
- `R-1603 Production GPU Backend`: production accelerator coverage for transfer, matmul, reductions, elementwise ops, convolution, and backward kernels with CPU fallback.

### Current Implementation State

Status: R-1601 and R-1602 are complete. R-1603 has a validated WGPU graph/device execution baseline but remains `in_progress` for production continuation. `R-3080` backward kernels and `R-3052` full residency are complete for their currently supported operation sets; compiler-native device lowering, efficient kernels, GPU mixed precision, and broader accelerator coverage remain open.

Completed:

- `spectra_midend::TensorGraph` extracts graph-level tensor nodes from lowered SSA host calls without changing the backend ABI.
- Graph nodes carry operator, shape, dtype, layout, device, dependency, and source-location metadata.
- `TensorGraph::validate()` catches cycles, invalid dependencies, shape mismatches, and device-placement conflicts.
- `TensorGraph::stable_dump()` is covered by `midend/tests/snapshots/tensor_graph.snap`.
- `run_tests.ps1` includes the R-1601 graph validation gate.
- `TensorGraph::optimize()` performs deterministic elementwise and reduction-adjacent fusion with `TensorGraphOptimizationReport` metrics.
- `TensorGraph::compare_optimized()` compares observable original and optimized graph outputs under the documented `1e-9` tolerance policy.
- `midend/tests/snapshots/tensor_graph_optimized.snap` locks the optimized graph dump format.
- `run_tests.ps1` includes the R-1602 graph optimization gate.
- R-1603 extends the optional `gpu` runtime feature into a validated WGPU baseline for float tensor transfer, elementwise ops, unary `relu`/`neg`, reductions, `matmul`, `std.ml.conv2d`, resident training operations, and supported autodiff kernels.
- CPU fallback remains the default build path and is also used when a WGPU kernel reports failure after dispatch.
- Device capability diagnostics are exposed through `std.tensor.device_status`, `device_available`, `stats_gpu_kernel_ops`, `stats_cpu_fallbacks`, `stats_device_transfers`, and `kernel_strategy`.
- `scripts/validate_r1603_gpu_backend.py` validates the default CPU fallback path and the optional WGPU backend path.
- `tests/validation/91_tensor_phase16_gpu_backend.spectra` validates the public Spectra API with safe skip behavior when WGPU is unavailable.

### Acceptance Direction

- Tensor programs should lower to a validated graph representation.
- Graph optimization should be observable and correctness-preserving.
- GPU support should be equivalent to CPU fallback within documented tolerance for covered operations.
- Production completion requires explicit device lowering, efficient kernels, and evidence for each newly claimed operation; WGPU baseline correctness must not be conflated with native multi-backend or general compiler GPU support.

## Phase 17: Data and Experiment Platform

### Goal

Support realistic datasets, feature pipelines, experiment tracking, and
reproducible training workflows.

### Workstreams

- `R-1701 Dataset and DataFrame Runtime`: CSV, JSONL, NPY, directory datasets, transforms, batching, shuffling, splits, and deterministic seeds.
- `R-1702 Experiment Tracking and Reproducibility`: run manifests, configs, metrics, artifacts, package lockfiles, model outputs, and exact reproduction commands.
- `R-1703 Distributed Training Foundations`: single-machine multi-worker training simulation, checkpoint coordination, resume, and topology documentation.

### Acceptance Direction

- Realistic data pipelines should run without Python glue for supported formats.
- Training results should be reproducible from manifests and lockfiles.
- Distributed behavior should be explicitly scoped and testable.

### Current Implementation State

Status: R-1701, R-1702, and R-1703 are complete for the current production data/experiment baseline.

Completed:

- `std.ml.dataset_from_csv`, `dataset_from_jsonl`, `dataset_from_npy`, and `dataset_from_directory` load supported numerical datasets into tensor-backed dataset handles.
- `std.ml.dataset_map_features`, `dataset_filter_label_min`, `dataset_train_split`, and `dataset_test_split` provide materialized preprocessing and split operations.
- `std.ml.dataframe_from_csv`, `dataframe_rows`, `dataframe_cols`, and `dataframe_column` provide numeric dataframe inspection and column extraction.
- `scripts/validate_r1701_data_runtime.py`, `tests/validation/92_ml_phase17_data_runtime.spectra`, and `examples/ai/tabular_dataset_training.spectra` validate file-backed tabular training without Python glue.
- `std.ml.experiment_start` through `experiment_finish` record structured experiment manifests with configs, metrics, artifacts, seeds, lockfiles, model outputs, and reproduction commands.
- `std.ml.experiment_compare_manifests` compares run evidence for metrics/artifacts/configs/lockfile/model output/seed.
- `scripts/validate_r1702_experiment_tracking.py`, `tests/validation/93_ml_phase17_experiment_tracking.spectra`, and `examples/ai/experiment_tracking_reproducibility.spectra` validate reproducible experiment tracking.
- `std.ml.distributed_session_start` through `distributed_summary` provide deterministic single-machine simulated-worker training coordination, checkpoint save/resume, interrupted-worker tracking, and topology summaries.
- `scripts/validate_r1703_distributed_training.py`, `tests/validation/94_ml_phase17_distributed_training.spectra`, and `examples/ai/distributed_training_checkpoint.spectra` validate distributed training foundations.

## Phase 18: Model Ecosystem and LLM Workloads

### Goal

Support model import/export, transformer workloads, tokenization, embeddings,
and RAG-oriented development.

### Workstreams

- `R-1801 ONNX Import and Export`: supported ONNX subset export/import, shape/dtype validation, and external runtime validation.
- `R-1802 Transformer and LLM Runtime Primitives`: attention, layer norm, embeddings, positional encoding, GELU/SwiGLU, KV cache, and sampling. The validated CPU host baseline is complete; accelerator parity is outside this item and belongs to the active GPU/optimization workstreams.
- `R-1803 Tokenization, Embeddings, and RAG Toolkit`: deterministic tokenization, vector indexes, retrieval, chunking, prompt assembly, and RAG evaluation.

### Acceptance Direction

- Spectra models should interoperate with mainstream model tooling through ONNX.
- Transformer examples should use real runtime primitives.
- RAG examples should be runnable and evaluable end-to-end.

### Current Implementation State

Status: R-1801, R-1802, and R-1803 are complete for the current production model/LLM/RAG baseline.

Completed:

- `std.ml.onnx_export`, `onnx_import_summary`, `onnx_validate`, and `onnx_roundtrip` provide binary ONNX `ModelProto` subset export/import/round-trip.
- Supported ONNX model kinds cover linear, convolutional, activation, normalization, and simple transformer blocks with ranked `float32` shapes.
- `scripts/validate_r1801_onnx_import_export.py`, `tests/validation/95_ml_phase18_onnx_import_export.spectra`, and `examples/ai/onnx_transformer_export.spectra` validate the current ONNX baseline.
- `std.ml.embedding_lookup`, `positional_encoding`, `layer_norm`, `gelu`, `swiglu`, `attention`, `kv_cache_*`, and `logits_sample` provide runtime-backed transformer/LLM primitives over real tensor handles.
- `examples/ai/toy_transformer_inference.spectra` now uses real transformer primitives instead of placeholder tensor arithmetic.
- `scripts/validate_r1802_transformer_primitives.py` and `tests/validation/96_ml_phase18_transformer_primitives.spectra` validate the current transformer primitive baseline.
- `std.ml.tokenizer_wordpiece`, `tokenizer_encode`, `tokenizer_decode`, `text_embed`, `vector_index_*`, and `rag_*` provide deterministic tokenization, embeddings, retrieval, prompt assembly, and RAG evaluation.
- `examples/ai/rag_retrieval_pipeline.spectra` runs retrieval, prompt assembly, model-call boundary, evaluation, and vector-index persistence end-to-end.
- `scripts/validate_r1803_rag_toolkit.py` and `tests/validation/97_ml_phase18_rag_toolkit.spectra` validate the current RAG toolkit baseline.

## Phase 19: AI Operations and Evaluation

### Goal

Provide model evaluation, monitoring, safety, drift detection, and deployment
operations.

### Workstreams

- `R-1901 Model Evaluation and Metrics Suite`: classification, regression, ranking, generation, and serving metrics.
- `R-1902 AI Safety and Guardrail Runtime`: policy hooks, output validation, rate limiting, fallback behavior, and audit events.
- `R-1903 Model Monitoring and Drift Detection`: inference metrics, distribution summaries, drift checks, and JSON observability artifacts.

### Acceptance Direction

- Models should have evaluation gates before export or serving.
- Serving should support policy enforcement and auditability.
- Runtime monitoring should produce machine-readable operational evidence.

### Current Implementation State

Status: `R-1901`, `R-1902`, and `R-1903` are complete for the current production
evaluation, safety, monitoring, and drift-detection baseline.

Completed:

- `std.ml.metrics_classification` reports accuracy, precision, recall, F1, and ROC-AUC baseline evidence.
- `std.ml.metrics_regression`, `metrics_ranking`, `metrics_generation`, and `serving_metrics` cover MSE/MAE, ranking quality, generation overlap/perplexity proxy, latency, error rate, and throughput.
- `std.ml.evaluation_report` emits a versioned JSON evaluation report and a human-readable companion report.
- `scripts/validate_r1901_evaluation_metrics.py`, `tests/validation/98_ml_phase19_evaluation_metrics.spectra`, and `examples/ai/model_evaluation_report.spectra` validate the current R-1901 baseline.
- `std.serve` guardrail APIs attach input/output policies, rate limits, safe fallbacks, structured diagnostics, and versioned audit logs to local serving servers.
- `scripts/validate_r1902_ai_safety_guardrails.py`, `tests/validation/99_phase19_ai_safety_guardrails.spectra`, and `examples/ai/safe_serving_guardrails.spectra` validate the current R-1902 baseline.
- `std.serve` monitoring APIs emit model-version request metrics, latency/error/throughput snapshots, input/output distribution summaries, drift checks, and versioned observability exports.
- `scripts/validate_r1903_model_monitoring.py`, `tests/validation/100_phase19_model_monitoring.spectra`, and `examples/ai/model_monitoring_drift_detection.spectra` validate the current R-1903 baseline.

## Phase 20: Production Certification

### Goal

Establish compatibility, benchmark, conformance, and release gates for production
AI users.

### Workstreams

- `R-2001 AI Conformance Suite`: compiler, runtime, tensor, autodiff, graph, interop, package, serving, and docs-example conformance.
- `R-2002 Production Release Channels`: nightly/beta/stable channels, compatibility policy, deprecation warnings, and migration guidance.
- `R-2003 Base Language and std Regression Audit Gate`: explicit compile-only vs runtime-required `.spectra` catalog and pre-API regression gate.
- `R-2004 Pattern Control-Flow Lowering Correctness`: `if let`, `while let`, `match`, enum payload bindings, string literal patterns, and loop/return paths through normal CLI execution.
- `R-2005 Core std/runtime Panic and Host-Status Hardening`: stable host status values and diagnostics for user-triggerable invalid std/runtime inputs.
- `R-2006 Tensor and std Performance Refresh`: fresh release benchmark evidence for materialization, elementwise chains, reductions, matmul, autodiff, and buffer reuse.
- `R-2007 Backend and Codegen Robustness Cleanup`: warning cleanup and typed backend errors for reachable IR/codegen edge cases.
- `R-2008 Language Feature Project Matrix`: matrix mapping basic language and AI Support features to concrete checked-in `.spectra` project validation scenarios, with project paths, entrypoints, required files, exact commands, expected outcomes, and owners.
- `R-2009 Basic Components Integration Projects`: complete checked-in `.spectra` projects for modules, functions, records, traits, generics, closures, control flow, and stdlib composition.
- `R-2010 AI Support Integration Projects`: complete checked-in `.spectra` projects for tensors, autodiff, graph/fusion, data, experiment, ONNX, RAG, serving, evaluation, safety, and monitoring.
- `R-2011 Full Pipeline Project Runner`: project-level runner for the matrix-declared `.spectra` projects via `spectralang run`, `spectralang package check`, and `spectralang package test` with JSON evidence.
- `R-2012 Failure-To-Roadmap Triage Gate`: completed gate where every unfixed integrated `.spectra` project failure must become a roadmap item with owner, phase, dependencies, risk, reproduction command, affected project path, and acceptance criteria.
- `R-2013 Release Candidate Integrated Project Gate`: final gate requiring zero untracked failures across integrated basic-language and AI Support `.spectra` projects.
- `R-2014 Multi-Module Aggregate and Trait Codegen Recovery`: completed
  correction for a valid multi-module `.spectra` package that previously
  failed codegen with `Value 13 not found` while combining cross-module
  records, enum payloads, trait dispatch, `match`, `while let`, `if not`, and
  mutable loop state.

### Acceptance Direction

- Release candidates should require a versioned conformance report.
- Integrated project validation should exercise realistic checked-in multi-file
  `.spectra` package projects, not isolated fixtures or parser-only samples.
- Each integrated project should include `spectra.toml`, `src/*.spectra`,
  package tests or deterministic fixtures where required, exact commands, and
  expected output/report evidence.
- Stable releases should communicate compatibility and deprecation status clearly.
- Certification should fail when required conformance tests or integrated
  project gates fail.
- Any real implementation defect found by integrated validation should be fixed
  with regression coverage or tracked as a new roadmap item before certification.

### Completed So Far

- `R-2001` is complete for the current production baseline.
- `scripts/validate_r2001_ai_conformance.py` runs executable gates for compiler,
  runtime, tensors, autodiff, graph, interop, package, serving, tooling, and
  docs/examples.
- The conformance report is versioned with schema
  `spectralang.ai_conformance_report.v1` and conformance version `R-2001/v1`.
- `run_tests.ps1` includes the `phase20-conformance` release-candidate gate.
- `R-2002` is complete for release-channel metadata and deprecation policy.
- `R-2003` is complete for the base-language/std regression audit gate, with
  compile-only and runtime-required `.spectra` validation separated.
- `R-2004` is complete for pattern control-flow lowering correctness, including
  enum payload bindings, user enum shadowing of builtin generic names, string
  literal pattern matching, and loop control-flow execution through the normal
  CLI path.
- `R-2005` is complete for core std/runtime panic and host-status hardening,
  including invalid host contexts, invalid handles, poisoned runtime locks, and
  normal CLI regression coverage for invalid std paths.
- `R-2006` is complete for tensor/std performance evidence, with a release
  benchmark report covering materialization, elementwise chains, reductions,
  matmul, autodiff, and buffer reuse.
- `R-2007` is complete for backend/codegen robustness, with typed JIT/AOT
  errors, warning cleanup, and valid-source edge control-flow coverage through
  the normal CLI path.
- `R-2008` is complete for the integrated project matrix and checked-in
  validation project set, with
  `docs/architecture/r2008-language-feature-project-matrix.toml`,
  `docs/architecture/r2008-language-feature-project-matrix.md`,
  `scripts/validate_r2008_language_feature_matrix.py`, eight
  `tests/projects/valid/integrated_*` project directories, and the
  `phase20-project-matrix` gate in `run_tests.ps1`.
- `R-2011` is complete for the integrated project runner, with
  `scripts/validate_r2011_integrated_project_runner.py`,
  `docs/architecture/r2011-integrated-project-runner.md`, JSON evidence under
  `target/r2011-integrated-project-runner/report.json`, and the
  `phase20-integrated-project-runner` gate in `run_tests.ps1`.
- `R-2012` is complete for the failure-to-roadmap triage gate, with
  `scripts/validate_r2012_failure_triage.py`,
  `docs/architecture/r2012-failure-to-roadmap-triage.md`, JSON evidence under
  `target/r2012-failure-triage/report.json`, and the
  `phase20-failure-triage` gate in `run_tests.ps1`.
- `R-2014` is complete for the first integrated-project defect found after the
  matrix landed: imported struct-style enum payload metadata is preserved for
  midend lowering, undefined IR operands are rejected before backend codegen,
  and the former known-failure package is now
  `tests/projects/valid/integrated_basic_deep_components`.

### Remaining Integrated Project Certification

`R-2013` completes the post-baseline certification track focused
on complete checked-in `.spectra` projects that combine the basic language
surface with AI Support features. This track does not reopen the completed
`R-2003` through `R-2007` pre-API stabilization evidence or the completed
`R-2008` project matrix, the completed `R-2009`/`R-2010` integrated projects,
the completed `R-2011` runner, or the completed `R-2012` triage gate. It adds
stronger release-candidate proof that real Spectra projects can compose
modules, traits, generics, closures, control flow, stdlib helpers, tensors,
autodiff, graph/fusion, data pipelines, model interop, RAG, serving,
evaluation, safety, and monitoring through the normal CLI and package paths.

Project implementation must use `tests/projects/valid/integrated_*`
directories with `spectra.toml`, `src/main.spectra`, supporting
`src/*.spectra` modules, and package tests or deterministic fixtures whenever
the matrix requires them. The runner must reject missing files, missing package
tests, parser-only substitutions, non-deterministic outputs, and commands that
do not match the R-2008 matrix.

The R-2013 implementation now provides the aggregate fail-closed validator
`scripts/validate_r2013_release_candidate.py`. It regenerates the R-2001,
R-2011, and R-2012 reports in one ordered execution and writes the versioned
release-candidate evidence to `target/r2013-release-candidate/report.json`.
The directed certification passes all eight matrix projects with zero
untracked failures. The repository-wide `run_tests.ps1` also returns zero
after the Phase 31 code-validation runner was separated from the standalone
performance certification. `R-2013` is therefore complete; its current report
is `target/r2013-release-candidate/report.json`.

When execution of this track finds a real compiler, runtime, package, or AI
Support defect, the defect must either be fixed in the same change with
regression coverage or added as a new roadmap/backlog item beyond `R-2008`
through `R-2013`. The first such tracked defect, `R-2014`, has been fixed and
promoted into `tests/projects/valid/integrated_basic_deep_components`.

### Completed Pre-API Stabilization

The Phase 20 pre-API stabilization gate (`R-2003` through `R-2007`) is
complete. This does not reopen completed AI conformance or release-channel
evidence; it records that the base-language runtime gate, pattern lowering
gate, std/runtime hardening gate, performance evidence gate, and
backend/codegen robustness gate are now all validated before API lifecycle work
continues.

---

# API Platform Vision

The previous 20 phases focused on making SpectraLang a production-grade
language and runtime for AI/ML workloads: tensors, autodiff, ONNX, RAG,
ML serving, and observability for models. The completed `R-2003`/`R-2004`/
`R-2005`/`R-2006`/`R-2007` stabilization gate covers the base-language runtime
regression audit, pattern control-flow lowering correctness, core std/runtime
hardening, tensor/std performance evidence, and backend/codegen robustness.
The next horizon then turns SpectraLang into a production-grade language and
runtime for **building the HTTP and event-driven services that surround those
models** — APIs, gateways, workers, webhooks, schedulers, and the operational
layer around them.

This is delivered as a separate package, `spectra.api`, published through
the existing Phase 9 registry. The platform is intentionally **not** part
of `std` because it evolves on a faster cadence, has its own version, and
pulls in heavier optional dependencies (TLS, drivers, observability
exporters).

## Why a separate package, not more `std` modules

- Independent versioning: `spectra.api` follows its own release cadence
  while `std` remains the stable core.
- Heavier optional dependencies: `rustls`, `tokio`-style primitives, and
  database drivers should not bloat every `spectra` build.
- Independent deprecation policy: web standards and APIs change faster
  than the language core, and the migration surface is much smaller when
  it lives in a package.
- Independent distribution: `spectra.api` can be published to the local
  registry and adopted incrementally by teams.

## Why async/await must come first (Phase 21)

The API platform depends on a first-class async/await model with a
platform-specific reactor (`epoll` on Linux, `IOCP` on Windows, `kqueue` on
macOS) and structured concurrency. Without this, the API server cannot
match the latency, throughput, and connection-density characteristics that
production teams expect. Callbacks and `std.concurrent` primitives are
insufficient for sustained 10k+ concurrent connections.

`R-2101` is complete through `docs/adr/0010-async-execution-model.md`. The
accepted model is stackless async lowered to state-machine SSA, driven by a
polling scheduler and platform reactor boundary. The public surface is fixed
around `async func`, `async {}`, `await`, `Task<T>`, and `Stream<T>`; pinning is
internal to runtime-managed task frames. Structured concurrency, cooperative
cancellation, and `Send`/`Sync` validation are required gates for the remaining
Phase 21 implementation items.

## Why HTTP/1.1 first (Phase 22)

HTTP/1.1 still serves the overwhelming majority of public APIs. It is also
where the foundational HTTP/1.1 parsing, routing, and TLS surface must be
correct before HTTP/2 multiplexing, HPACK, and HTTP/3/QUIC can be layered
on top. The Phase 24 workstream covers h2 and h3 explicitly.

## Why first-class drivers for PostgreSQL, SQLite, and Redis (Phase 25)

These three cover the dominant backend combinations that AI/ML services
need: a transactional store (Postgres or SQLite), a fast in-memory store
or message broker (Redis), and migrations. The driver selection keeps the
core surface small while still addressing the dominant production use
cases. MySQL and NoSQL options are listed as P1 and P2 follow-on work.

## Phases 21 to 30

The ten phases are summarized below; the detailed items live in
`docs/roadmap-backlog.md` (Phase 21 to Phase 30) and in
`roadmap/roadmap.toml` (items `R-2101` to `R-3004`). `R-2216` is explicitly
blocked on the Phase 20 stabilization items `R-2003` through `R-2007`, and
that Phase 20 prerequisite set is now complete. Phase 29 and Phase 30 exist
to track surfaces that are already visible but still documented or
implemented as alpha, placeholder, sidecar-only, local-only, or simulated
baselines.

### Phase 21 — Async Language Core

- `R-2101` ADR: Async/Await Execution Model (complete; ADR 0010 accepted)
- `R-2102` `async func` and async block in frontend (complete; parser/AST,
  diagnostics, language-service labels, and validation gate landed)
- `R-2103` `await` expression and async lowering (complete; `Task<T>` baseline,
  explicit suspend/resume/ready IR markers, deterministic task host calls, and
  validation gate landed)
- `R-2104` Event loop multiplexer (complete; platform-selected reactor
  boundary, shared task/timer/I/O readiness interface, host-call surface, and
  validation gate landed)
- `R-2105` Cancellation, timeouts, structured concurrency (complete;
  `JoinHandle` value/error status, `CancelHandle`, deterministic
  `with_timeout`, parent/child scopes, cascading cancellation, join
  aggregation, and validation gate landed)
- `R-2106` `Stream<T>` and stream adaptors (complete; runtime-managed stream
  handles, async `next`, backpressure, deterministic finish, cancellation,
  `map`/`filter`/`fold`/`take`/`skip`/`chunks`/`fuse`, and validation gate
  landed)
- `R-2107` Async stdlib: `fs`, `tcp`, `udp`, `channel` (complete; async
  filesystem read/write, nonblocking TCP listener/connect/accept/read/write,
  UDP bind/send/recv, bounded async channels, cancellation coverage, and
  validation gate landed)
- `R-2108` Async trait objects and `dyn Future` (complete; built-in
  `Future`/`Stream` traits, `Box<dyn Future>` / `Box<dyn Stream>` type
  lowering, async vtable dispatch, object-safety diagnostic `E2108`, and
  validation gate landed)
- `R-2109` Async test runtime and macros (complete; function attributes,
  `#[spectra_async_test]` discovery, `block_on(Task<T>) -> T`, package test
  list/filter/JSON reporting, and validation gate landed)
- `R-2110` Async diagnostics and `Send`/`Sync` validation (complete; stable
  `E2101` through `E2120` documentation, non-`Send` across `await`,
  `RefCell` across `await`, spawn-boundary diagnostics, and validation gate
  landed)
- `R-2111` Async benchmarks and profiling (complete; `spectralang bench
  --async`, schema `spectra.r2111.async_benchmark.v1`, checked-in baseline,
  and validation gate landed)
- `R-2112` Formal `Send`/`Sync` trait bounds (complete; `T: Send`,
  `T: Sync`, `dyn Trait + Send/Sync`, auto-trait evidence diagnostics
  `E2104`, and validation gate landed)

### Phase 22 — API Library Foundation (`spectra.api`)

- `R-2201` ADR: API Library Architecture (complete; accepted ADR
  `docs/adr/0011-api-library-architecture.md` fixes `spectra.api`,
  `std.api.*`, `spectra.api.*` host calls, `packages/spectra-api`,
  HTTP/1.1-first delivery, `rustls`, and Phase 21 async dependencies)
- `R-2202` `spectra-api` Rust crate and host call registration (complete;
  `packages/spectra-api` links against `spectra-runtime`, registers 415 public
  `spectra.api.*` host calls through the runtime host-call registry, satisfies
  the runtime's 347-name required namespace, exposes
  `spectra_api_register_host_calls`, and is validated by
  `scripts/validate_r2202_spectra_api_hostcalls.py`)
- `R-2203` `std.api.*` semantic and tooling surface (complete; virtual
  `std.api.*` modules expose the public API function/type table to semantic
  analysis, formatter check, LSP completion, and
  `scripts/validate_r2203_std_api_surface.py`)
- `R-2204` HTTP/1.1 parser (complete; `Http1Parser` streams requests and
  responses, produces structured headers/body chunks, round-trips chunked
  transfer coding, and reports typed parse errors with byte positions)
- `R-2205` HTTP/1.1 server (complete; `mio::Poll`-driven nonblocking accept
  loop,
  per-connection state, response writer, body limits, slowloris/read
  timeouts, cleanup paths, and 10k connection-slot validation landed)
- `R-2206` HTTP/1.1 client (complete; `HttpClient` supports pooled plain
  HTTP connections, GET/POST/PUT/PATCH/DELETE/HEAD, arbitrary bodies,
  redirect method semantics, configurable timeouts, typed
  connection/protocol/timeout errors, plus the registered nonblocking
  `spectra.api.client.request` `Task<Response>` bridge with cancellation)
- `R-2207` TLS via `rustls` (complete; `spectra-api` exposes
  `TlsServerConfig`, `TlsClientConfig`, HTTPS round trips, SNI, configurable
  DER certificate roots/chains, WebPKI client roots, ALPN `http/1.1` plus
  `h2`, typed TLS handshake/certificate errors, and a validated nonblocking
  HTTPS path for the registered `Task<Response>` client bridge)
- `R-2208` `std.api.json` encoder and decoder (complete; native
  `JsonValue`/`JsonNumber` codec handles primitives, null, arrays, maps,
  nested structures, common escapes, RFC 8259 output, typed parse errors with
  byte offsets, and documented `std.api.json.*` host-call compatibility)
- `R-2209` JSON derive: `Serialize` and `Deserialize` (complete;
  `#[derive(Serialize, Deserialize)]` registers `to_json`, `from_json`, and
  `json_error_field`, supports `#[json(optional)]` and
  `#[json(rename = "...")]`, validates JSON string literals against derived
  schemas, and reports field-specific `EJSON003`/`EJSON004` diagnostics)
- `R-2210` `Request`, `Response`, `Header`, `Cookie`, `Method`, `Status` (complete;
  `std.api.http` exposes typed handler request/response handles, stable
  Method/Status constructors, case-insensitive Header/Cookie accessors, native
  validation, midend host-call lowering, and
  `tests/validation/134_http_core_types.spectra`)
- `R-2211` Router: path matching, params, wildcards (complete; trie-backed
  `std.api.routing` handles literals, `{param}`, `*wildcard`, `{id:\d+}`,
  route conflicts with both paths, `RouteMatch` param extraction, 100k route
  lookup benchmark coverage, and `tests/validation/135_api_router_matching.spectra`)
- `R-2212` Query string parser and binding (complete; `std.api.query`
  parses RFC 3986 query strings, preserves repeated keys as arrays, exposes
  `QuerySchema`/`QueryBinding` for typed struct binding, reports malformed
  percent encodings and type mismatches, and is covered by
  `tests/validation/136_api_query_binding.spectra`)
- `R-2213` URL-encoded form binding (complete; `std.api.form` parses
  `application/x-www-form-urlencoded` bodies with percent-decoded UTF-8,
  `+` to space decoding, `[]` arrays, bracket-notation nested field paths,
  schema-driven `FormBinding`, duplicate scalar field errors, and missing
  required field diagnostics)
- `R-2214` Multipart form and file uploads (complete;
  `std.api.multipart` exposes `Multipart`/`MultipartPart`, parses
  `multipart/form-data`, enforces total/part/count limits, spools uploaded
  file parts to disk, supports chunked `file_read` and `file_spool_to`, and
  is covered by `tests/validation/138_api_multipart_uploads.spectra`)
- `R-2215` `api.handler` trait and response return (complete;
  `std.api.handler` exports `IntoResponse`, `Handler`, and `AsyncHandler`,
  supports sync and async handler contracts over `Request -> Response`,
  normalizes text/JSON/bytes/status/error returns, dispatches registered
  handler handles, transports sync/async user callbacks through the runtime
  closure ABI, and is covered by
  `tests/validation/139_api_handler_response_return.spectra` and
  `tests/validation/330_api_handler_callbacks.spectra`)
- `R-2216` Server lifecycle, listen, serve, graceful shutdown (complete;
  `std.api.server` exposes configured listen ports, `serve`, state,
  assigned-port reporting, SIGINT/SIGTERM-compatible shutdown signaling,
  lifecycle stats, graceful drain/cancel policy, synchronous callback routing,
  and mio-polled asynchronous callback tasks; covered by
  `tests/validation/147_api_server_lifecycle.spectra` and
  `tests/validation/330_api_handler_callbacks.spectra`)
- `R-2217` `spectra.api` package published to local registry (complete;
  local registry publish writes checksum and `source_path` metadata,
  `spectralang package add spectra-api` installs the canonical
  `"spectra.api"` dependency, and package build/check/run is validated by
  `scripts/validate_r2217_spectra_api_registry.py`)
- `R-2218` API book chapter: `Hello HTTP` (complete; `docs/book/09-hello-http.md`
  links from the book index and `docs/api/README.md`, documents route
  definition, typed `Response` returns, local `serve`, assigned-port checks,
  and graceful shutdown, and is validated with
  `examples/api/00_hello_http.spectra` by
  `scripts/validate_r2218_hello_http_book.py`)
- `R-2219` API example: REST CRUD (complete;
  `examples/api/01_rest_crud.spectra` exercises public `std.api.*` routing,
  handlers, HTTP responses, JSON derive, path params, query strings, form
  binding, local server lifecycle, and CRUD smoke assertions; validated by
  `scripts/validate_r2219_rest_crud_example.py`)
- `R-2220` API conformance suite v0 (HTTP/1.1) (complete;
  `packages/spectra-api/src/conformance.rs` defines 26 executable must-pass
  cases across HTTP/1.1 parsing/status/header behavior, JSON round-trip/error
  handling, and basic router matching/conflicts; `conformance_v0` emits the
  versioned machine-readable report `target/api-conformance-v0.json`, and
  `scripts/validate_r2220_api_conformance_v0.py` gates Phase 22)

### Phase 23 — Middleware and Security

- `R-2301` middleware chain trait and deterministic ordering (complete;
  `std.api.middleware` exposes sync/async traits, deterministic chain
  composition, short-circuit handling, reverse response hooks, trace
  inspection, `docs/book/10-middleware-chain.md`, and
  `scripts/validate_r2301_middleware_chain.py`)
- `R-2302` CORS middleware (complete; `std.api.cors` exposes immutable
  CORS policies, permissive/restrictive/credentialed configuration,
  preflight and actual-response evaluation, middleware-chain integration,
  `std.api.http.request_with_header`, `docs/api/std-api-cors.md`,
  `tests/validation/149_api_cors_middleware.spectra`, and
  `scripts/validate_r2302_cors_middleware.py`)
- `R-2303` Structured logging and request ID tracing (complete;
  `std.api.middleware.register_logging` assigns a unique request ID before
  request hooks, emits one JSON or text line with request ID, method, path,
  status, and latency, and exposes the rendered records through
  `logging_len`, `logging_line`, and `logging_request_id`; validated by
  `tests/validation/331_api_structured_logging.spectra` and
  `scripts/validate_r2303_structured_logging.py`)
- `R-2304` Rate limiting (complete; `std.api.middleware.register_rate_limit`
  implements token bucket and sliding-window counters with global, route,
  tenant, user, and combined scopes, standard `429` retry headers, and a
  development-only atomic update path; validated by
  `tests/validation/332_api_rate_limiting.spectra` and
  `scripts/validate_r2304_rate_limiting.py`)
- `R-2305` Response compression (complete; `std.api.middleware.register_compression`
  negotiates Brotli, gzip, and deflate with q-values, threshold and HTTP
  exclusions; validated by `tests/validation/336_api_compression.spectra` and
  `scripts/validate_r2305_compression.py`)
- `R-2306` Security headers (complete; `std.api.middleware.register_security_headers`
  applies CSP, Permissions-Policy, X-Frame-Options, X-Content-Type-Options,
  Referrer-Policy, and opt-in HSTS, with longest-prefix per-route overrides;
  validated by `tests/validation/334_api_security_headers.spectra` and
  `scripts/validate_r2306_security_headers.py`)
- `R-2307` API key authentication (complete; `std.api.middleware.register_api_key`
  supports header/query sources, expiry, revocation, structured `401` errors,
  and an internal API-key identity for rate-limit composition; validated by
  `tests/validation/333_api_key_auth.spectra` and
  `scripts/validate_r2307_api_key.py`)
- `R-2308` JWT (complete; `std.api.jwt.sign` and `std.api.jwt.verify` support
  HS256, RS256, and ES256 with exp/nbf/iss/aud/sub/jti validation and
  constant-time cryptographic verification; validated by
  `tests/validation/335_api_jwt.spectra` and
  `scripts/validate_r2308_jwt.py`)
- `R-2309` OAuth2 client (complete; `std.api.oauth` implements
  authorization-code exchange with state and PKCE S256, refresh rotation, and
  revocation; validated by `tests/validation/337_api_oauth.spectra` and
  `scripts/validate_r2309_oauth.py`)
- `R-2310` OAuth2 resource server and introspection
- `R-2311` Sessions (complete; `std.api.session` provides a pluggable
  in-memory/Redis backend contract, opaque cryptographically random IDs,
  bounded TTL/sliding expiration, maximum lifetime, backend lookup validity,
  and immediate revocation; validated by
  `tests/validation/342_api_session.spectra` and
  `scripts/validate_r2311_session.py`; Redis 7 service certification remains
  owned by the independent R-2507 gate)
- `R-2312` Cookie API (complete; `std.api.http` supports typed Path, Domain,
  Max-Age, Secure, HttpOnly, SameSite, multi-value `Set-Cookie`, and
  HMAC-SHA256 signing/constant-time verification with typed error state;
  validated by `tests/validation/338_api_cookie.spectra` and
  `scripts/validate_r2312_cookie.py`)
- `R-2313` Request validation (complete; `std.api.validation` provides
  immutable handle-backed schemas, required/length/range/regex constraints,
  JSON and Form validation, typed field issues, and RFC 7807
  `application/problem+json` responses; validated by
  `tests/validation/339_api_validation.spectra` and
  `scripts/validate_r2313_validation.py`)
- `R-2314` Unified error handling and exception middleware (complete;
  `std.api.errors.ApiError` maps public errors to deterministic RFC 7807
  responses, `internal_error` logs full details while sanitizing public
  output, and `exception_middleware` recovers sync/async chain failures with
  per-route mappings; validated by `tests/validation/340_api_errors.spectra`
  and `scripts/validate_r2314_errors.py`)
- `R-2315` HTTPS hardening (complete; HSTS preload/includeSubDomains remains
  covered by R-2306, while rustls OCSP stapling and listener-preserving
  certificate rotation are implemented in `spectra-api` and validated by
  `scripts/validate_r2315_https_hardening.py`)
- `R-2316` Threat mitigations (complete; `std.api.security` now provides
  CSRF origin allowlists and default-deny SSRF policies, the HTTP client validates
  resolved addresses before sync/async/TLS sockets, and server body/timeout
  setters preserve early parser rejection and per-connection timeouts; validated
  by `tests/validation/341_api_security.spectra` and
  `scripts/validate_r2316_security.py`)
- `R-2317` API example: authenticated REST API (JWT) (complete; the example
  issues and verifies deterministic HS256 bearer tokens, rejects tampering,
  validates real request bodies, returns unified Problem Details errors, wires
  five CRUD routes and starts/stops a local server; validated by
  `examples/api/02_jwt_auth_crud.spectra` and
  `scripts/validate_r2317_jwt_auth_crud_example.py`)
- `R-2318` API example: middleware composition (complete; the reference
  example composes logging, security headers, CORS, compression, and
  route-scoped rate limiting, proves short-circuit/header behavior and
  longest-prefix configuration, and preserves both `Vary` dimensions after a
  CORS merge fix; validated by
  `examples/api/03_middleware_composition.spectra` and
  `scripts/validate_r2318_middleware_composition_example.py`)

### Phase 24 — Advanced API Features

- `R-2401` WebSocket server (complete; the dedicated `std.api.websocket`
  listener implements the RFC 6455 handshake, strict frame parsing,
  fragmentation, ping/pong, close validation, message limits, and negotiated
  `permessage-deflate`; 18 host calls now include `server_route` for GET-route
  upgrades through the real `std.api.server` loop, with the typed fixture,
  routed integration test, and 10k concurrent-connections soak validated by
  `scripts/validate_r2401_websocket.py`)
- `R-2402` WebSocket client (complete; `WebSocketClient` now implements
  masked `ws://`/`wss://` handshakes, `Sec-WebSocket-Accept` validation, the
  project `rustls`/WebPKI trust path, SSRF policy, per-message deflate, bounded
  reconnect/backoff, and typed async host calls, validated by native
  round-trip/reconnect/TLS tests and
  `tests/validation/344_api_websocket_client.spectra`; external text/binary
  echo interoperability also passes against
  `wss://testserver.host/ws/no-subprotocol/echo` through the dedicated
  validator)
- `R-2403` Server-Sent Events (SSE) (complete; dedicated
  `std.api.sse` transport now performs HTTP/1.1 event-stream handshakes,
  bounded multiline event serialization, automatic heartbeats, `retry` hints,
  and `Last-Event-ID` replay, validated by native tests and
  `tests/validation/345_api_sse.spectra`; `server_response` connects the
  bounded stream to the normal routed `std.api.server` lifecycle)
- `R-2404` HTTP/2 server (complete; the native `h2` transport performs
  stream multiplexing, HPACK header round-trips, bounded flow control, and
  graceful shutdown; rustls advertises `http/1.1` plus `h2` and the listener
  requires negotiated `h2`; validated by native TLS/multiplexing tests and
  `scripts/validate_r2404_http2.py`)
- `R-2405` HTTP/2 client (complete; native `h2` client provides connection
  reuse, bounded flow-controlled request/response bodies, secure rustls + `h2`
  ALPN negotiation, default SSRF blocking, and a server-push callback; local
  multiplexing/HTTPS evidence and the recorded `https://nghttp2.org` external
  round-trip passed)
- `R-2406` HTTP/3 and QUIC (complete as a scope decision; ADR 0014 is now
  superseded-partial: a localhost-validated quinn + h3 transport with 28
  `spectra.api.http3` hosts has landed, while two-peer interop, the
  4-platform matrix, migration coverage, and the perf budget continue under
  the new `R-2422` productionization item; re-evaluate 2026-11-30)
- `R-2407` API versioning (path, header, query)
- `R-2408` Pagination (cursor, offset, RFC 5988 Link header)
- `R-2409` Content negotiation (JSON, XML, MessagePack, CBOR)
- `R-2410` Caching headers (ETag, Last-Modified, Cache-Control, Vary)
- `R-2411` OpenAPI 3.1 generation
- `R-2412` Background jobs and task queue
- `R-2413` Cron and scheduled jobs
- `R-2414` Email send (SMTP and templates)
- `R-2415` Webhooks (signed payloads, retry, dead letter)
- `R-2416` File storage abstraction (S3-compatible)
- `R-2417` Cache layer (LRU in-memory, Redis distributed)
- `R-2418` Configuration management
- `R-2419` gRPC server and client (in_progress; opaque-bytes transport with 4
  cardinalities, TLS option, and typed flag-1 rejection is landed; `.proto`
  codegen to typed services is outstanding)
- `R-2420` WebSocket example: real-time dashboard
- `R-2421` OpenAPI example: serve Swagger UI
- `R-2422` HTTP/3 productionization (not_started; two-peer interop,
  4-platform matrix with migration, Task/Stream mapping, perf budget)
- `R-2423` GraphQL dynamic schema, guards, and subscriptions (in_progress;
  dynamic schema plus opt-in guards and push subscriptions landed)

### Phase 25 — Persistence and Database

The Phase 25 foundation is the reusable async-aware connection pool in
`packages/spectra-db`. R-2501 owns bounded capacity, FIFO acquisition,
timeouts, cancellation, idle reaping, graceful shutdown and pool lifecycle
telemetry. It deliberately does not expose a database API or simulate a
driver; SQLite and the other protocols must consume the pool through their
own real factories. R-2701 database-query tracing remains dependent on the
pool and the first real SQLite driver.

R-2504 is the first real consumer: it provides bundled SQLite connections,
typed prepared statements, transactions, concurrent file-backed reads and a
public `spectra.api.db.sqlite` host-call surface. Query tracing is emitted by
the driver for actual operations; PostgreSQL, Redis and the type-safe query
builder remain separate workstreams. Its v2 gate also certifies the
pool-backed async path, cancellation, non-blocking SQLite lock waits, and
HTTP-parented OTLP query spans.

- `R-2501` Connection pool (async-aware)
- `R-2502` SQL query builder (type-safe)
- `R-2502` is implemented as the shared typed Rust AST/dialect contract in
  `spectra-db`; SQLite is the first validated dialect and drivers execute its
  parameterized output without interpolating user values. The public
  `spectra.api.db.sqlite` raw-statement surface remains compatible until a
  language-level typed query contract is designed.
- `R-2503` Migrations framework
- `R-2503` is implemented first against the certified SQLite driver, with
  paired up/down files, deterministic SHA-256 drift detection, atomic
  application and rollback, and a real `spectralang db` CLI. Future drivers
  consume the Rust migration contract after their own production gates pass.
- `R-2504` SQLite driver (sync and async)
- `R-2505` PostgreSQL driver (async, prepared, COPY)
- `R-2506` MySQL driver
- `R-2507` Redis driver (with pool)
- `R-2508` Minimal ORM: model trait and typed queries
- `R-2509` Transactions (begin, commit, rollback, savepoints)
- `R-2510` Health checks (liveness, readiness, startup)
- `R-2511` Database example: REST + SQLite CRUD
- `R-2512` Database example: REST + PostgreSQL
- `R-2513` Redis example: rate-limit via Redis
- `R-2514` Migration example: multi-version evolution

`R-2514` is complete and certifies the migration workflow on SQLite with three versioned
migrations. The Spectra fixture checks the resulting schema and seed rows,
while the independent CLI validator proves rollback, re-application,
idempotency, checksums, drift rejection, transactional failure and concurrent
runners without introducing a parallel migration API.

`R-2511` is the first integrated database/API proof and is complete after the
independent validator and real TCP harness passed. Its Spectra fixture
validates the language-facing SQLite and server lifecycle, while a real Rust
TCP harness validates dynamic CRUD handlers using the certified SQLite driver,
R-2502 parameterized queries, R-2503 migrations, health, metrics and tracing.
The project does not claim generic Spectra request callbacks from this example;
that capability remains a separate language/API design task.

### Phase 26 — API Tooling and Developer Experience

- `R-2601` `spectralang api new` scaffolder
- `R-2602` Hot reload dev server (`spectralang api dev`)
- `R-2603` API testing framework (`#[api_test]`)
- `R-2604` API mocking and contract tests (Pact)
- `R-2605` `spectralang api doc` (Swagger UI and Redoc)
- `R-2606` Postman, Bruno, and Insomnia export
- `R-2607` Graceful shutdown and signal handling
- `R-2608` Production config profiles (dev, staging, prod)
- `R-2609` API conformance suite v1 (status, headers, errors)
- `R-2610` Book chapter: "Building Production APIs in Spectra"
- `R-2611` LSP: routes, handlers, types
- `R-2612` `spectralang api lint`
- `R-2613` Debugger: breakpoints in handlers
- `R-2614` VS Code plugin updates for `spectra.api`
- `R-2615` Project templates: REST, GraphQL, gRPC, microservice

### Phase 27 — Observability and API Operations

R-2701 is the operational tracing contract for the API and AI workstreams.
Its production path is runtime-owned W3C context, real HTTP server/client
instrumentation, and OTLP/HTTP protobuf export to a collector. Serving and
distributed-training work may depend on this contract. R-2701 is complete for
the currently supported boundaries: bounded export, failure handling,
concurrency isolation, independent payload validation, HTTP client propagation,
filesystem spans, and real SQLite query/transaction spans. Its focused and
early integrated gates are recorded in `target/r2701-tracing/global-gate.json`
and `TEST_RESULTS.txt`. PostgreSQL and Redis are not claimed until their
drivers are implemented.

The exporter lifecycle regression was closed by making shutdown drain the
queue once, clearing active state after export failure, and using a shared
test guard with ephemeral collectors for HTTP tracing isolation.

R-2505 is now the active Phase 25 workstream. Its PostgreSQL driver reuses the
R-2501 pool and R-2502 query contract, exposes real prepared statements and
transactions, and now provides bounded cancellable async tasks, incremental
COPY, typed LISTEN/NOTIFY handles, and driver-owned tracing across the public
Spectra API. A local PostgreSQL 16.14 run now produces a certifying v2
`passed` report proving named capability tests, 100,000-row COPY,
LISTEN/NOTIFY, async cancellation, independent OTLP decoding, tracing, and
HTTP-parent propagation. Production promotion remains reserved for the
checked-in PostgreSQL 16 CI lane reproducing and publishing that report.
Environments without PostgreSQL record `skipped_environment`; they do not
constitute completion evidence, and R-2505 remains `in_progress`.

R-2507 is the parallel Redis 7 workstream. It reuses the R-2501 pool,
provides real async commands and a dedicated pub/sub connection, and supplies
the backend contract required by the distributed cache and rate-limiting
tracks. Redis host calls remain incomplete until the Redis 7 CI lane validates
the fixture and independent report; no local skip is treated as production
evidence.

R-2510 adds the operational health foundation without coupling liveness to
external services. A runtime-owned bounded evaluator publishes atomic
snapshots consumed by `/healthz`, `/readyz`, and `/startupz`; required and
optional checks have distinct readiness semantics, real timeouts and
sanitised diagnostics. SQLite checks are validated against a file-backed
database, while PostgreSQL and Redis remain environment-gated until their
real services are available. R-2703 consumes this registry for deployment
hardening rather than implementing a second health-check system.

R-2703 is the integrated deployment layer for this contract. Its Kubernetes,
Docker and systemd examples reference the real health routes and are checked
by an independent validator. Liveness remains dependency-independent,
readiness controls rollout traffic, and startup gates initialization; systemd
watchdog support remains disabled until a real `sd_notify` implementation is
available.

R-2702 provides the operational metrics contract consumed by the API server.
`HttpServer` shares a thread-safe Rust `MetricsRegistry` and reserves
`/metrics` for deterministic Prometheus text exposition. HTTP request count,
duration, errors, active/accepted connections, and timeouts are recorded from
real server activity; custom counters and histograms use bounded, validated
labels. An independent parser validates the payload produced by a real TCP
server, including concurrent updates and rejection of sensitive or
non-finite values. This first version intentionally has no Spectra host calls;
R-2707 will consume the registry for the integrated OTel/Prometheus example.

R-2707 certifies the integrated observability example without expanding the
language surface: the Spectra fixture configures `std.api.trace`, serves a
real HTTP route, and the external harness scrapes the existing `/metrics`
route. A separate OTLP collector process receives and independently decodes
protobuf, while a separate parser validates Prometheus exposition. The
evidence includes typed attributes, real HTTP spans, request/error metrics,
histogram series, flush, shutdown, process cleanup, and secret absence.

- `R-2701` OpenTelemetry-compatible tracing
- `R-2702` Prometheus-compatible metrics endpoint
- `R-2703` Health, readiness, and startup probes (integrated)
- `R-2704` Request and response audit log (LGPD, GDPR)
- `R-2705` Distributed tracing (W3C Trace Context)
- `R-2706` Per-tenant and per-user rate limiting
- `R-2707` OTel and Prometheus exporters example
- `R-2708` Audit log example with PII redaction

### Phase 28 — API Conformance and Release

- `R-2801` API conformance suite v1 (final)
- `R-2802` Interop tests against Express, FastAPI, and Actix
- `R-2803` Documentation site for `spectra.api`
- `R-2804` API example gallery (REST, GraphQL, gRPC, WebSocket, SSE)
- `R-2805` Production hardening: load, soak, chaos
- `R-2806` `spectra.api` v1.0 registry release
- `R-2807` Migration guide: from ad-hoc `std` web to `spectra.api`

### Phase 29 — Production Reality Gap Closure

This phase closes visible production gaps found during the standard-library
and runtime audit. It is for existing surfaces that are not fake enough to
delete, but not real enough to certify.

- `R-2901` Exact-width numeric runtime semantics
- `R-2902` Range and iterator production semantics (complete; range syntax now lowers to typed runtime handles, stored/passed ranges iterate through `spectra.std.range.len`/`at`, and the validator is gated)
- `R-2903` Native debug info emission
- `R-2904` First-class tensor IR and device lowering

R-2901 is complete: exact-width semantic/IR/backend storage, checked dynamic
narrowing, checked arithmetic, explicit wrapping helpers, AOT object emission,
and C ABI evidence are covered by the executable validator and normal CLI
fixtures.

### Phase 30 — Production ML Systems Gap Closure

This phase converts local or simulated ML-system baselines into real
production paths. It follows the API/runtime observability work where
networking, lifecycle, and operations are prerequisites.

The phase also owns the Production Standard Library and Artifact Runtime
workstream. Public stdlib APIs must not be marketed as production when their
implementation is a local simulation, hash-based approximation, alias-only
ABI, narrow sidecar format, or host-call surface without normal CLI evidence.
The workstream requires shared versioned artifact contracts, integrity
validation, executable capability classification, and target-perspective
evidence for process and network boundaries.

R-3007 now provides the executable evidence layer for this workstream. The
versioned `scripts/stdlib_contract.toml` manifest reconciles semantic modules,
runtime/API registrations, lowering sources, documentation, fixtures, and
normal CLI probes. Its JSON report records production, baseline, simulation,
unsupported, and incomplete claims separately, exposes source divergences, and
fails closed for unclassified symbols or contradictory production claims. The
initial probe is `tests/validation/185_stdlib_contract_audit.spectra`; a failed
report is evidence for the responsible production task and does not promote
the underlying implementation.
The completed audit implementation uses typed source inventories rather than
matching every textual `std.*` occurrence. It ran eleven namespace or external
conformance probes, covered 651 discovered symbols, and recorded 58 tracked
follow-ups without promoting the remaining ML, serving, tensor-device, or
distributed baselines to production.

- `R-3001` Networked ML serving runtime (in_progress; embedded localhost
  listener with real dense/ONNX dispatch landed, fixture 355)
- `R-3002` Distributed training real transport (in_progress; OS-thread and
  TCP-loopback ALLREDUCE landed, fixture 356)
- `R-3003` Production model artifact formats
- `R-3004` Compiler-native autodiff lowering
- `R-3005` Production tokenization and embedding backends
- `R-3006` Persistent production vector index
- `R-3007` Stdlib production contract and capability audit

The workstream is coupled to `R-2901` exact-width numeric ABI semantics and
`R-2904` first-class tensor/device lowering. Existing CPU transformer and LLM
primitives are complete under `R-1802`; accelerator parity is not silently
claimed by that item. NPY/ONNX support, single-process distributed workers,
hash embeddings, and in-process serving remain explicit baselines until their
production tasks pass their evidence gates. R-3006 is the production
exception for vector search.

R-2904 is complete. The compiler materializes the existing tensor graph as a
typed legalization boundary with deterministic fusion and memory-planning
evidence. JIT and AOT invoke the same validation before code generation, CPU
and WGPU use the same device-aware graph contract, and host calls remain only
as an explicit compatibility execution backend. R-3004 is now in progress:
it consumes this contract to materialize a versioned compiler-native reverse
graph with registered gradient rules, saved-forward values, seeds, and explicit
accumulation. R-3004 is complete for its supported operation set: `diff` now
materializes explicit `AutodiffStep` instructions and dispatches reverse
kernels directly; the legacy runtime `tensor.backward` path is retained only
as an explicit public compatibility API. The independent report proves CPU,
JIT, AOT object emission, negative diagnostics, and public backward
compatibility; WGPU remains environment-dependent.

R-3003 is the first implementation in this workstream. Its Spectra Artifact
Container v1 is the shared contract for checkpoint and multi-array persistence:
validated binary framing, canonical manifest, explicit tensor representation,
per-array/global SHA-256 integrity, compatibility metadata, and atomic writes.
The contract is consumed through `std.ml` handles and proven by a normal CLI
round trip plus an independent parser and corruption suite. R-3005 and R-3006
remain downstream consumers; they must not reintroduce JSON sidecars.

R-3005 consumes this contract through additive production loaders. Versioned
WordPiece vocabularies are validated from artifact metadata, real embedding
weights are loaded from rank-2 tensor arrays, and encode/decode plus lookup
are proven through the normal CLI. The previous inline tokenizer and hash
embedding remain explicit compatibility baselines rather than hidden fallback
paths. R-3006 replaces the former linear in-memory vector search and JSON
sidecar with deterministic HNSW search persisted in the same Artifact Container
v1, including model metadata, padded graph links, checksums, atomic writes,
reload validation, and query/latency evidence. The legacy JSON index is
rejected rather than silently migrated.
The R-3005 gate now proves valid vocabulary and embedding artifacts,
deterministic special-token handling, reference lookup values, and rejection
of twelve malformed or incompatible fixtures through the normal CLI workflow.
The R-3006 gate proves valid and corrupt index containers, deterministic
round-trip results, HNSW metadata, and measured query/insert counters through
the normal CLI workflow. R-3006 is complete: its report passed, the full
`run_tests.ps1` suite passed all 373 decisive tests with zero failures, and
the persistent HNSW implementation is now the certified vector-index path.

### Phase 31 — Benchmark Evidence Hardening

`R-3101` and `R-3130` are complete. Reports
must identify profile, binary, revision, host, timestamp, and sample policy;
measurements above the 10% standard-deviation threshold are inconclusive;
confirmed drift remains a failure; and baseline updates require repeated stable
runs with review evidence. Standalone performance certification uses five
independent attempts, three warmups, twenty timed samples, and up to two
confirmations. Repository code validation uses one execution per runtime and
scenario with no statistical certification. This split keeps `run_tests.ps1`
fast while preserving the full performance contract.

The semantic mismatch in `async-echo` was corrected with a real fan-out/fan-in
contract: ten executable task units are registered before joining, the runtime
uses a persistent worker pool, and diagnostics prove a maximum of ten pending
tasks. The earlier `R-3131` reports (`1.025752` and `1.048312`) are preserved
as historical evidence from revision `bd48a6b`, but current reports at the
later HEAD contradicted that parity window. `R-3131` is therefore reopened by
`R-3133`, while `R-3130` and the dependent `R-2013` remain complete. The
historical Spectra baseline remains unchanged.

The Phase 31 work now distinguishes two independent evidence products. R-3103
certifies functional/performance repeatability and an executable optimization
plan from current release benchmarks plus O0/O3 IR snapshots. Its hypotheses
are explicitly classified as `benchmark_and_ir_hypothesis`: they guide the
next implementation items but do not claim that a measured gap is a causal
hotspot. R-3102 remains `in_progress` as the later Linux `perf`/FlameGraph
attribution track; its environmental blocker does not prevent R-3103 from
closing once the benchmark and IR gates pass. The baseline is immutable, and
the five-independent-attempt policy is retained for standalone certification.

`R-3133` is the current-revision reconciliation gate for this contract. It
measures the exact `task_spawn_batch`/`task_join_batch_sum` path through
`batch-reset-only`, `batch-spawn-only`, `batch-join-only`, `batch-full`, and
`batch-full-no-reset`, while preserving the legacy diagnostic variants. Its
versioned schema records process-inclusive medians, p95, dispersion, revision,
commands, and internal locks/scheduler/execution/task/batch counters. The
classification is deterministic (`compiler_backend_lowering`,
`runtime_batch_path`, `benchmark_process_startup`, `external_noise`, or
`benchmark_contract`); benchmark timing is hypothesis evidence, never causal
profiler attribution. A runtime/backend finding may receive only a narrow fix
with a regression fixture. Startup, contract, and noise findings remain
follow-up work without baseline or silent 5% contract changes. `R-3104` stays
not started until `R-3103` has passed its benchmark+IR gates, including a
conclusive `tensor-create` result.

R-1603 GPU validation follows the same evidence rule. Its CPU and WGPU tests
run in separate serialized commands with per-step timeouts and captured output,
while adapter absence remains an explicit supported skip condition.

R-3132 adds the next backend/runtime optimization under this evidence rule:
`task_spawn` followed by a single-use immediate `task_join` is fused only when
the IR proves that the handle cannot be observed or escape. The fused path has
one registry lock, preserves task accounting, creates no observable slot, and
has generic-host and JIT/AOT Fast ABI implementations. The historical
33,865,050 ns baseline is intentionally unchanged. The measured 38.9 ms result
was accepted for R-3132 without changing the baseline. R-3130 subsequently
replaced the semantically unequal fixture with real fan-out/fan-in evidence;
that gate and the dependent R-2013 certification now pass independently.

## Architectural Principles for the API Platform

1. **Async by default, sync where it makes sense.** Handlers can be
   `async func` or synchronous; the runtime and the reactor drive both
   through the same task scheduler.
2. **Typed HTTP.** `Request`, `Response`, `Method`, `Status`, `Header`,
   and `Cookie` are first-class types, not raw strings. The `api.Error`
   type maps to HTTP status codes and bodies deterministically.
3. **JSON as the lingua franca, with content negotiation.** `serde`-style
   derives work over JSON, with content negotiation for XML, MessagePack,
   and CBOR.
4. **Middleware is a typed chain, not a function soup.** Middleware
   composes deterministically: request order top-down, response order
   bottom-up, with a documented lifecycle.
5. **Authentication is a first-class middleware.** JWT, API keys,
   sessions, and OAuth2 are all middleware that yields an authenticated
   request.
6. **Validation is RFC 7807 by default.** Failed validation returns a
   `Problem Details` body with the offending fields and stable codes.
7. **TLS is on by default, with explicit opt-out.** HTTPS hardening
   (HSTS, OCSP stapling) is a documented, configurable default, not a
   separate effort.
8. **Databases are async, pooled, and explicit.** No connection
   string magic; the pool is configured and the query builder is type
   safe.
9. **Observability is not an afterthought.** OpenTelemetry traces and
   Prometheus metrics are first-class, with W3C Trace Context
   propagation across the HTTP client and the database drivers.
10. **Hot reload and graceful shutdown are non-negotiable.** The dev
    server reloads in milliseconds, the production server drains in
    seconds.

## Workstream Dependencies

```
R-2101 → R-2102 → R-2103 → R-2104 → R-2105 → R-2106 → R-2107
                                              ↓
                              R-2201 → R-2202 → R-2204 → R-2205
                                                          ↓
                                          R-2211 → R-2301 → R-2304
                                                          ↓
                                          R-2411 → R-2412 → R-2501 → R-2505
                                                                          ↓
                                                              R-2701 → R-2801 → R-2806

R-2013/R-2015 → R-2901/R-2902/R-2903/R-2904
R-2216/R-2401/R-2419/R-2701 → R-3001
R-1703/R-2107/R-2701 → R-3002
R-801/R-1702/R-1801 → R-3003
R-501/R-2904 → R-3004
```

## New Owner Groups

| Owner | Scope |
|---|---|
| `web` | HTTP server/client, routing, middleware, WebSocket, SSE |
| `db` | Drivers, query builder, migrations, ORM, connection pool |

These owner groups are added to the same ownership table defined in
`AGENTS.md` and used by `roadmap/roadmap.toml` so that the
cross-cutting review policy applies to the API platform work the same
way it applies to the AI/ML work.

## Risk Register

| Risk | Phase | Mitigation |
|---|---|---|
| Async/await design changes after Phase 22 starts | 21 | Land `R-2101` ADR first; freeze the surface before starting the parser |
| TLS library choice (`rustls` vs `openssl`) | 22 | Use `rustls`; record rationale in `R-2201` ADR |
| Connection pool semantics differ across drivers | 25 | Define the pool trait first; drivers implement it |
| OpenAPI generator scope creep | 24 | Lock the supported subset in the ADR before generating |
| Production hardening (R-2805) depends on real deployments | 28 | Run on a representative staging workload before cutting v1.0 |
| Existing alpha/placeholder surfaces are mistaken for production-ready features | 29 | Keep R-2901 to R-2904 visible and `not_started` until code, docs, and executable gates prove the real behavior |
| Local or simulated ML systems are marketed as distributed or networked production systems | 30 | Require network-process fixtures, observability, artifact validation, and failure tests before completion |
| `Send`/`Sync` ergonomics | 21 | Document the rules in the language reference and enforce them with stable diagnostics |

## Cross-Reference

- Strategic direction: this chapter.
- Executable backlog: `docs/roadmap-backlog.md`, Phase 21 to Phase 30.
- Machine-readable tracker: `roadmap/roadmap.toml`, post-baseline integrated
  validation items `R-2008` to `R-2015`, API platform items `R-2101` to
  `R-2807`, and production gap-closure items `R-2901` to `R-3004`.
- Feature maturity classification: `docs/language-feature-maturity.md`
  (to be updated when each phase begins implementation).
- Conformance: `R-2801` is the final API conformance gate; release
  candidates for `spectra.api` v1.0 cannot be certified while any
  required category fails.
