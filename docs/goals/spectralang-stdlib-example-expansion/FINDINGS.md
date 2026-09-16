# Standard-library example expansion findings

## STD-001: multi-layer `std.serve` registration rejected by semantic arity

- **Status:** fixed and verified
- **Discovered by:** `examples/stdlib/14-serve-lifecycle.spectra`
- **Observed evidence:** `spectralang check --json examples/stdlib/14-serve-lifecycle.spectra` reported that `std.serve.server_register_model_linear` expected 4 arguments but received 7.
- **Cause:** the runtime host accepts `server + 3*n` arguments for a chain of dense layers, while the compiler's `std.serve` export table described only `server, weights, biases, activation`.
- **Impact:** valid multi-layer linear serving models could not pass semantic analysis, despite the runtime implementation supporting them.
- **Correction:** semantic calls now expand the one-layer minimum signature for valid `1 + 3*n` call sites, including direct named imports; the generated stdlib catalog exposes the variadic integer tail.
- **Regression:** the paired example and validation fixture assert a two-layer model, guardrails, cancellation, monitoring, and drift behavior.
- **Verification:** both files pass `spectralang check --json` and `spectralang run`; the example uses both the qualified module alias and the direct named-import path for the serving registration.

## STD-002: core stdlib validator rejected the serving fixture's tensor dependency

- **Status:** fixed and verified
- **Observed evidence:** `run_tests.ps1 -Phase stdlib_core_bug_hunt` rejected `328_stdlib_serve_full.spectra` because it imported `std.tensor`.
- **Cause:** the validator treated `std.tensor` as forbidden even though the official `std.serve` fixture registers a real model through tensor handles.
- **Correction:** the exclusion list now blocks only `std.api` and `std.ml`; `std.tensor` remains available to the serving baseline.
- **Verification:** the focused core stdlib gate passes after the correction.

## STD-003: conversion fallback fixture treated `NaN` as invalid input

- **Status:** fixture corrected and verified; no runtime defect
- **Discovered by:** `tests/validation/453_stdlib_convert_config.spectra`
- **Observed evidence:** the fixture expected `string_to_float_or("NaN", default)` to return `default`, but the execution reached the failure branch.
- **Cause:** the runtime delegates floating-point parsing to the language's `f64` parser, for which `NaN` is valid input; the fallback is reserved for parse errors.
- **Correction:** the fallback assertion now uses an actually malformed token and separately verifies the documented numeric result with `std.math.is_nan_f`.
- **Verification:** the paired conversion example and fixture pass `check --json` and JIT execution.

## STD-004: tensor-backed dataset splits rejected integer tensors

- **Status:** fixed and verified
- **Discovered by:** `examples/stdlib/28-ml-dataset-module.spectra`
- **Observed evidence:** `dataset_from_tensors` accepted `tensor.arange` handles, but `dataset_train_split` failed at runtime with the host call `spectra.std.ml.dataset_train_split`.
- **Cause:** dataset subset, transform, and dataloader paths used the float-only reader even though the public tensor-backed constructor accepted existing integer tensor handles.
- **Impact:** integer-backed datasets could be created but could not be split or loaded through the normal `std.ml` pipeline.
- **Correction:** the shared dataset numeric reader now widens both integer and float tensor storage to `f64`; subsets, transforms, and batches use that reader and materialize the existing float dataset representation.
- **Regression:** the paired example and validation fixture use integer feature/label tensors, train/test splits, dataloader batches, and module registration.
- **Verification:** both files pass `spectralang check --json` and JIT execution after the runtime correction.

## STD-005: checked numeric functions were registered but not executable through `std.numeric`

- **Status:** fixed and verified
- **Discovered by:** `examples/stdlib/34-numeric-checked.spectra`
- **Observed evidence:** the first semantic pass did not expose the checked integer and arithmetic functions; after that export was added, lowering reported `numeric` as unresolved. Once lowering was wired, `checked_u8(255)` failed because the backend passed narrow unsigned values with sign extension.
- **Cause:** the semantic builtin contract omitted the checked exact-width functions, the generated numeric lowering dispatcher treated them as compiler-only operations, and both host-call ABI paths sign-extended all narrow integer arguments.
- **Impact:** valid checked casts and arithmetic could not be called directly from the public standard library, and unsigned values with the high bit set were corrupted before reaching the runtime.
- **Correction:** the semantic contract now exports all checked integer conversions and arithmetic signatures; the catalog-driven lowering generator emits their exact return descriptors; and the backend zero-extends narrow arguments for unsigned numeric host functions in individual and batched calls.
- **Regression:** the paired example covers signed and unsigned conversions, checked arithmetic, float narrowing, and the `u8` boundary value `255`.
- **Verification:** `python scripts/generate_lowering_tables.py --check`, `cargo build -p spectra-cli`, and both `spectralang check --json` and `spectralang run` pass for the example and validation fixture.

## STD-006: `tensor.dot_t` rejected integer tensors despite accepting generic tensor handles

- **Status:** fixed and verified
- **Discovered by:** `examples/stdlib/35-tensor-algebra.spectra`
- **Observed evidence:** `tensor.dot(values, values)` accepted an integer tensor from `tensor.arange`, but the tensor-valued equivalent failed at runtime.
- **Cause:** `std_tensor_dot_t` checked for `TensorDType::Float` on both inputs even though its semantic contract accepts tensor handles and its result is explicitly a float scalar tensor.
- **Impact:** integer tensors could use scalar `dot` but not the tensor-valued reduction needed by autograd-compatible APIs.
- **Correction:** `dot_t` now accepts matching one-dimensional integer or float tensors, converts both data paths to `f64`, and keeps autograd enabled only for float inputs.
- **Regression:** the example and validation fixture call `dot_t` with an integer `arange` tensor and verify the resulting scalar.
- **Verification:** `cargo build -p spectra-cli`, and both `spectralang check --json` and `spectralang run` pass for the example and validation fixture.

## STD-007: `distributed_worker_step` was present in the public contract but absent from the runtime

- **Status:** fixed and verified
- **Discovered by:** `examples/stdlib/41-ml-distributed-checkpoint.spectra`
- **Observed evidence:** semantic analysis and lowering accepted the function, but runtime execution could not resolve its host call.
- **Cause:** the catalog and compiler contract listed `std.ml.distributed_worker_step`, while the runtime had no binding constant, implementation, or registration entry.
- **Impact:** the documented session-based distributed-training API could create sessions and inspect workers, but could not record a worker step through the public language surface.
- **Correction:** the runtime now validates worker/sample identifiers and finite loss values, records step/sample/loss state, returns the worker step count, and is registered under the catalog binding.
- **Regression:** the example and validation fixture record a step for every worker, advance the global step, save and resume a checkpoint, and inspect the summary.
- **Verification:** `cargo build -p spectra-cli`, and both `spectralang check --json` and `spectralang run` pass for the example and validation fixture.
