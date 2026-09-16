# Spectra ML/AI Workbench Implementation Plan

**Intent:** Build a real, runnable, multi-file Spectra application that uses the
standard tensor, ML, retrieval, serving, experiment, filesystem, and concurrency
surfaces together in one reproducible local workflow.

**Current Behavior:** The repository has complete examples for individual
capabilities and `examples/complete/11-ops-workbench` combines general-purpose
stdlib utilities, but no single application owns a complete ML/AI lifecycle from
dataset preparation through training, evaluation, retrieval, serving, monitoring,
and reproducibility.

**Expected Outcome:** Add `examples/complete/12-ml-ai-workbench` with more than
25 real `.spectra` source files. It will ingest a checked-in local dataset,
train a differentiable linear model through `std.tensor`/`std.ml`, persist and
reload its weights, evaluate the reloaded model, build a tokenizer/RAG/vector
search path, register the model in `std.serve`, enforce guardrails, export
monitoring, track an experiment, and emit a validated report. Add an independent
multi-file verification project under `tests/projects/valid/`.

**Target-Perspective Output:** A user runs one project command and sees a stable
`ML AI WORKBENCH ok` marker plus dataset, training, evaluation, retrieval,
serving, and reproducibility summaries. The user can inspect the generated
report and verify that temporary artifacts are removed after the run.

**Truth Owner:** Public behavior is owned by the checked-in Spectra sources and
their `spectra.toml` manifests. API signatures and runtime behavior are owned by
the existing `std.tensor`, `std.ml`, `std.serve`, `std.fs`, `std.concurrent`,
`std.env`, and `std.time` contracts; no new fake provider or mock model is added.

**Contract Boundary:** The application crosses from user modules into public
stdlib host calls using tensor/dataset/model handles represented by the existing
language contracts. Cross-module application functions use prefixed public names
and plain scalar/string/handle values so the project exercises the real module
linker without introducing a second data representation.

**Cutover:** The new workbench is the canonical complete AI/ML application
example. The existing focused projects `p9` through `p14` and
`11-ops-workbench` remain as narrow regression and stdlib smoke fixtures; they
are not deleted or treated as a competing production path.

**Displaced Path:** None is deleted. The old examples remain focused contracts;
the new workbench displaces only the absence of an integrated AI/ML lifecycle.
If the new project cannot prove a stage end to end, that stage is not described
as complete and the application does not claim production readiness.

**Value Density:** Every application module is wired into the executable path
and owns one observable behavior. The project deliberately favors real public
stdlib calls and deterministic local artifacts over invented model outputs.

**Acceptance Evidence:**

- `spectralang fmt --check`, `check --json`, and `run` pass for the complete app
  and the independent verification project.
- The app prints the stable success marker and non-zero exits identify failed
  lifecycle gates.
- The training path creates gradients, updates parameters, writes a validated
  artifact, reloads it, and evaluates the reloaded tensors.
- The tokenizer/RAG path encodes/decodes text, chunks context, queries a
  persisted vector index, builds a prompt, and reports token-overlap evaluation.
- The serving path performs guarded inference and exports distribution, drift,
  audit, and monitoring evidence.
- The experiment path creates comparable manifests and a reproducibility command.
- JIT and AOT execution both pass, `cargo build -p spectra-cli` passes, and the
  focused compiler/midend tests pass if implementation defects are found.
- Temporary application roots are absent after successful execution; manifests
  parse as TOML; all `.spectra` files have final newlines and `git diff --check`
  is clean.

**Evidence Lane:** Vertical slices: dataset/trace marker first, then actual
training/checkpoint, then retrieval, then serving/experiment/report. Each slice
gets an app run and fixture verification before the next slice is added. Any
compiler or stdlib defect is reproduced in the smallest real project, recorded
in `docs/goals/spectralang-stdlib-example-expansion/FINDINGS.md`, corrected in
the owning layer, and reverified.

**Kill Criteria:** Stop and redesign a module if it only returns a hard-coded
success value, if a stage cannot consume the previous stage's real handle/data,
if cleanup leaves artifacts, if a host API is not present in the public contract,
or if the same generated symbol/path is shared by concurrently runnable
projects. Do not paper over a failing runtime stage with a mock or an unchecked
placeholder.

**Non-goals:** No network provider, external database, cloud deployment, or
optional ONNX model download is required for the deterministic baseline. Optional
ONNX-specific capabilities may be documented as a separate gate but cannot be
used to claim baseline success. No roadmap item is changed by this plan.

**Risk if wrong:** A large-looking collection of files could still be a set of
disconnected demos. The integration proof therefore requires real handle flow,
artifact round-trips, serving observations, and an independent fixture rather
than file-count evidence alone.

**Architecture Slice:**

Source project: `examples/complete/12-ml-ai-workbench/`

| File | Responsibility and public boundary |
|---|---|
| `src/main.spectra` | CLI entry and final success marker |
| `src/app.spectra` | End-to-end lifecycle orchestration |
| `src/config.spectra` | Environment/argument configuration and seed |
| `src/paths.spectra` | Isolated deterministic artifact paths |
| `src/cleanup.spectra` | Idempotent filesystem cleanup |
| `src/run_context.spectra` | Experiment/run identifiers and lifecycle metadata |
| `src/schema.spectra` | Stage schema/marker handoff validation |
| `src/seed_data.spectra` | Local CSV, JSONL, and directory dataset materialization |
| `src/csv_ingest.spectra` | CSV-backed `std.ml` dataset loading |
| `src/jsonl_ingest.spectra` | JSONL/directory/dataframe ingestion checks |
| `src/dataset_ops.spectra` | Dataset transforms and label filtering |
| `src/feature_engineering.spectra` | Feature scaling and tensor shape preparation |
| `src/split_ops.spectra` | Deterministic train/test split handles |
| `src/batch_ops.spectra` | Dataloader and real batch extraction |
| `src/model_init.spectra` | Gradient-enabled parameter initialization |
| `src/model_forward.spectra` | Differentiable dense forward pass |
| `src/model_loss.spectra` | Differentiable MSE loss construction |
| `src/model_train.spectra` | Backward pass, optimizer steps, and step accounting |
| `src/model_eval.spectra` | Reloaded-checkpoint evaluation metrics |
| `src/checkpoint.spectra` | Artifact metadata/tensor save-load-validation |
| `src/tokenizer.spectra` | BPE/WordPiece training and round-trip text checks |
| `src/rag_chunks.spectra` | Context chunking and chunk contract validation |
| `src/rag_index.spectra` | Vector index insert/query/persist/load |
| `src/rag_prompt.spectra` | Prompt assembly from retrieved context |
| `src/rag_eval.spectra` | Retrieval answer and generation metrics |
| `src/serving_registry.spectra` | Named model registration from checkpoint tensors |
| `src/serving_guardrails.spectra` | Input/output policy, fallback, rate, and batch gates |
| `src/serving_monitor.spectra` | Snapshot, distribution, drift, audit, export |
| `src/experiment.spectra` | Experiment config, metrics, artifact/model records |
| `src/reproducibility.spectra` | Manifest comparison and reproduction command |
| `src/concurrency.spectra` | Deterministic parallel shard/checksum work |
| `src/report_metrics.spectra` | Cross-stage machine-readable summary fields |
| `src/report_render.spectra` | Human report rendering and persistence |
| `src/validation.spectra` | Final invariant matrix and cleanup gate |

Verification project: `tests/projects/valid/complete_ml_ai_workbench/` will have
its own manifest and independent `.spectra` modules for the same public slices,
using a separate target root so JIT/AOT or parallel validation cannot race with
the example.

**Read path:** `main -> app -> config/paths -> seed_data -> csv_ingest /
jsonl_ingest -> dataset_ops -> feature_engineering -> split_ops -> batch_ops ->
model_init -> model_forward/model_loss/model_train -> checkpoint -> model_eval ->
tokenizer -> rag_* -> serving_* -> experiment/reproducibility -> report ->
validation/cleanup`.

**Write path:** Only `seed_data`, `run_context`, `checkpoint`, `experiment`,
`serving_monitor`, and `report_render` write under the isolated target root.
`cleanup` owns removal and is called both before setup and after validation.

**Integration points:** Existing public stdlib contracts, the compiler's
multi-module lowering/linking path, tensor graph/autodiff, and the standard CLI
project runner. No direct Rust runtime call is introduced by the application.

**Plan Review Gate:** PRE review completed below before implementation.

### PRE review

- **Verdict:** aligned
- **Blockers:** none
- **Major risks accepted:** the baseline uses deterministic CPU ML and RAG/vector
  infrastructure; optional ONNX is not required for the success gate.
- **Smallest next gate:** implement the dataset tracer slice and run both
  projects through `check --json` and JIT execution before adding model code.

## Execution Tasks

1. Create manifests and the 34-file application skeleton plus independent fixture
   manifest. Keep all source comments in English and public names prefixed.
2. Implement the data slice (`seed_data` through `batch_ops`) and its fixture;
   prove CSV/JSONL/directory/dataframe loading and real batch extraction.
3. Implement model training and checkpoint reload; prove gradients, optimizer
   steps, artifact validation, and reloaded evaluation.
4. Implement tokenizer, RAG chunks/index/prompt/evaluation; prove persisted
   vector retrieval and token-level metrics.
5. Implement serving, guardrails, monitoring, experiment manifests, concurrency,
   and reports; prove cleanup and deterministic output.
6. Run formatter, JIT, AOT, focused Rust tests, manifest/source audits, and
   inspect the complete diff. Document and fix real defects before completion.

## Implementation status

The design is implemented in the working tree. The application contains 34
connected `.spectra` modules, and the independent verification project covers
data ingestion, autodiff training, checkpoint round-trip, persisted vector
retrieval, and evaluation reporting. The serving boundary explicitly adapts the
`std.ml.linear` `[in, out]` weight layout to the `std.serve` `[out, in]`
contract; the rationale and evidence are recorded in `FINDINGS.md`.

The final evidence lane passed formatter/checker/JIT/AOT for both projects,
`cargo build -p spectra-cli`, `cargo test -p spectra-midend`, and
`cargo test -p spectra-compiler`. Successful runs remove their isolated target
roots before returning.
