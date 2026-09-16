# Findings and Corrections

This file records implementation defects and library-contract findings
discovered while integrating the five projects. Each entry includes a
reproduction, owning contract, correction, and validation evidence.

## Cross-module symbol collision

- Reproduction: two imported user modules exported a function named `render`
  with different signatures. The old compiler lowered both calls into one
  native symbol namespace, so the generated declarations could not preserve
  which module owned each call.
- Owning contract: user-module functions must remain distinct by module in JIT
  and relocatable AOT code, while a bare call with two possible imports must be
  rejected as ambiguous.
- Correction: semantic import metadata now carries canonical `module::name`
  symbols; the midend preserves them through calls and return-type inference;
  JIT and AOT native declarations are module-qualified. Bare ambiguous calls
  now receive a semantic diagnostic, while qualified calls remain valid.
- Evidence: `tests/projects/valid/cross_module_duplicate_symbols` passes
  check, JIT, and AOT with `int` and `string` functions sharing the name
  `render`; `tests/projects/invalid/cross_module_ambiguous_symbol` fails at the
  semantic phase with the qualified-call guidance.

## Synthetic agent registration symbols

- Reproduction: after ordinary AOT functions were module-qualified, project 17
  failed to link because the imported `__spectra_agent_register_tools_*`
  function was qualified a second time even though its generated name already
  contains the declaring module.
- Owning contract: generated agent registration functions are project-unique
  source symbols shared by the defining object and importing objects.
- Correction: AOT symbol mapping now preserves that synthetic registration
  naming convention while still module-qualifying ordinary user functions.
- Evidence: project 17 passes check, JIT, AOT compilation, and execution after
  the correction; all five complete projects pass the same final matrix.

## Exclusive range semantics

- Reproduction: `for value in 1..5` evaluated only values 1 through 4, while the
  first implementation expected an inclusive range and produced 10 instead of
  15.
- Owning contract: the range operator uses an exclusive upper bound.
- Correction: the evaluator now uses an explicit `while value <= 5` loop, which
  makes the intended inclusive computation visible in the source.
- Evidence: the complete project prints `result=30` in both JIT and AOT runs;
  the interpreter doubles the evaluated 15.

## Binary artifacts are not text files

- Reproduction: after `ml.artifact_save`, passing the `.spar` path to
  `std.fs.fs_read` returned an error even though `artifact_validate` succeeded.
- Owning contract: `fs_read` is a UTF-8 text reader; ML artifacts are binary
  containers and must be checked with `artifact_validate` or reopened with
  `artifact_load`.
- Correction: checkpoint verification now uses the artifact APIs and cleanup
  explicitly removes the experiment manifest created by `experiment_start`.
- Evidence: project 15 completes its JIT run with exit code 0 and leaves no
  `target/complete-ml-training-platform` directory.

## Validation constraints require declared fields

- Reproduction: the official request-validation example applied `regex` to
  `email` immediately after adding only `username` and `age`. The constraint
  builder returned an invalid handle, and the later `validate_json` host call
  failed at runtime.
- Owning contract: `std.api.validation.regex` can constrain only a field already
  present in the schema.
- Classification: this is a library contract violation, not a library
  implementation defect. The host correctly returns an invalid schema handle
  and records `UnknownField`; the source examples were applying a constraint
  before declaring the field.
- Correction: both the official example and project 16 now add the required
  `email` field before applying its regex constraint.
- Evidence: the corrected fixture and project 16 are checked through the normal
  CLI path; the project proceeds past validation into SQLite, routing, and
  server lifecycle stages.

## Reserved module names must not be used as user modules

- Reproduction: project 14 declared `module join` in `join.spectra`; importing
  that file reported that module `join` did not exist even though the source
  file was present.
- Owning contract: `join` is part of the language's reserved syntax surface and
  is not a safe user-module identifier.
- Correction: the module was renamed to `join_ops`, and all imports/calls were
  updated.
- Evidence: project 14 passes semantic check and JIT/AOT execution.

## Module names can be shadowed by local bindings during lowering

- Reproduction: project 15 used `let evaluation = evaluation.mltrain_evaluate(...)`.
  Semantic checking returned success, but the midend reported an unresolved
  identifier and the backend later hit a Cranelift `FunctionBuilderContext`
  assertion.
- Owning contract: a module-qualified call must remain distinguishable from a
  local binding with the same name throughout lowering.
- Correction: the semantic analyzer now gives a lexical local binding
  precedence over a module namespace in both type inference and method-call
  validation. The midend already refuses to lower the shadowed value as a
  module call, so the old semantic/backend mismatch is gone. The project value
  remains named `eval_text` to keep the application source unambiguous.
- Evidence: `tests/errors/module_namespace_shadowed.spectra` now fails during
  semantic checking with E017 instead of reaching a lowering/backend assertion;
  project 15 passes check, JIT, and AOT with real training and serving output.

## Compensation declarations require tool visibility

- Reproduction: `compensate(run, "aoc_persist_note", payload)` in project 17
  failed semantic validation until the module containing the `#[agent_tool]`
  declaration was imported into the compensation module.
- Owning contract: literal tool names in `compensate` are resolved against
  agent-tool declarations visible in the current compilation unit.
- Correction: `compensation.spectra` imports `tools`, preserving the real
  declaration and compiler check.
- Evidence: project 17 records compensation and rollback events and passes JIT
  and AOT replay verification.

## Rollback is a successful terminal agent state

- Reproduction: project 17 initially treated `agent_end` status `rolled_back`
  as a failed run after a successful compensation.
- Owning contract: a run that completed its requested rollback reports
  `rolled_back`, with compensation and tool-call counts preserved.
- Correction: service validation accepts the explicit `rolled_back` terminal
  status while still requiring the expected journaled tool count.
- Evidence: project 17 prints `journal=8 events=8 replay=verified` and exits 0
  in both JIT and AOT.

## Public re-exports must retain their defining module

- Reproduction: `bug_hunt_v2_291_public_reexport` imported `answer` through a
  public `facade` re-export. Semantic checking succeeded, but code generation
  looked for `facade::answer` even though the implementation lived in `base`.
- Owning contract: a public re-export changes visibility and the public name,
  not the module that owns the generated function symbol.
- Correction: exported function metadata now carries the defining qualified
  symbol through re-exports; imported call metadata uses that symbol for JIT
  and AOT lowering.
- Evidence: the project passes semantic check, JIT execution, AOT compilation,
  and the produced executable.

## Dynamic trait vtables must use qualified implementation symbols

- Reproduction: `oop_cross_module_dispatch` compiled the trait implementation
  but failed while building the dynamic vtable because it requested the local
  `Account_value` symbol instead of the implementation in `traits`.
- Owning contract: vtable entries use the same cross-module symbol resolution
  as ordinary imported calls.
- Correction: dynamic coercion resolves each `Type_method` entry through the
  semantic user-function symbol map before taking its function address.
- Evidence: the project passes semantic check, JIT execution, AOT compilation,
  and the produced executable.

## Validation gates must follow current artifact and host-call contracts

- Reproduction: the report contained stale assumptions: R-1801 required a
  `conv.onnx` artifact that the public fixture intentionally does not export,
  R-1803 parsed a legacy JSON index although the example writes a `.spar`
  artifact container, R-2104 referenced a missing FreeBSD workflow, and
  R-3210 decoded UTF-8 CLI output with the Windows code page. R-2202 planning
  text also lagged the registry's current 558 host calls.
- Owning contract: validators and planning evidence must assert the current
  runtime/library format and platform integration rather than a superseded
  representation.
- Correction: the validators now check the exported ONNX set and the v1
  artifact container/HNSW metadata actually produced by the examples; the
  required FreeBSD workflow was restored; R-3210 explicitly decodes UTF-8;
  and the R-2202 planning counts are synchronized to 558.
- Evidence: R-1801, R-1803, R-2104, R-2202, R-3210, and the dependent R-3221
  certification gate all pass.
