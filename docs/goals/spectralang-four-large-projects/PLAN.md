# Four Large Spectra Projects Implementation Plan

**Intent:** Maintain four runnable, multi-file applications that exercise
Spectra across core language processing, data-platform operations, ML training,
and API service development.

**Current Behavior:** `examples/complete` contains four domain-shaped products:
the core-language studio and data platform each have 28 modules, while the ML
training platform and API service each have 31. Every project has a connected
entrypoint and an isolated cleanup path.

**Expected Outcome:** Keep all four projects executable, with more than 25 real
`.spectra` files, an isolated workspace, and a stable success marker. Each
project must use the APIs and runtime behavior its domain advertises.

**Target-Perspective Output:** A user can run each project from the repository
root and see a domain-specific `ok` marker, inspect generated reports or
artifacts, and rerun without state leakage.

**Truth Owner:** Project manifests and `src/main.spectra` own user-visible
behavior. Existing standard-library contracts, package APIs, and compiler/runtime
tests own language semantics; no simulated success path is accepted.

**Contract Boundary:** Projects cross module boundaries with typed records where
useful and scalar/string/handle values at standard-library boundaries.
Filesystem state is isolated per project; the API service uses the public
`std.api.*` contract.

**Value Density:** Every source module is imported by the executable graph and
owns a domain transformation, validation, persistence boundary, or operational
concern. File count alone is not acceptance evidence.

**Acceptance Evidence:**

- Each project has more than 25 `.spectra` files under `src/`.
- Each project passes `fmt --check` and `check --json`.
- Each project runs successfully through JIT and AOT, with a stable marker.
- Each project exercises its advertised domain APIs using real handles/data.
- Generated workspace state is cleaned after a successful run.
- Manifests parse as TOML, all sources end with newlines, and `git diff --check`
  is clean.
- `cargo build -p spectra-cli` and focused compiler/midend tests remain green.
- Any real contract or implementation defect found during integration is
  recorded in this goal's findings and corrected before completion.

## Architecture slices

### 13-core-language-studio

A deterministic language-workbench pipeline: source loading, lexical analysis,
AST-like records, binding/type inference, optimization, interpretation,
formatting/linting, package/cache scheduling, and a persisted diagnostic report.
The 28 modules are `main`, `app`, `config`, `source`, `tokens`, `lexer`,
`token_rules`, `parser`, `ast`, `ast_walk`, `binder`, `types`, `infer`,
`diagnostics`, `source_map`, `optimizer`, `evaluator`, `interpreter`,
`formatter`, `linter`, `compiler`, `package`, `cache`, `scheduler`,
`filesystem`, `report`, `validation`, and `cleanup`.

### 14-data-platform

A local data-platform flow: ingest CSV/JSONL, normalize schema, filter and
aggregate rows, join dimensions, partition batches, schedule concurrent work,
cache a materialized view, write a catalog/index, audit metrics, and clean its
workspace. The 28 modules are `main`, `app`, `config`, `paths`, `cleanup`,
`seed_data`, `raw_csv`, `raw_json`, `schema`, `normalize`, `filter`, `aggregate`,
`join_ops`, `partition`, `batch`, `catalog`, `index`, `query`, `cache`, `retry`,
`scheduler`, `metrics`, `report`, `audit`, `quality`, `lineage`, `validation`,
and `filesystem`.

### 15-ml-training-platform

A CPU-deterministic ML platform: dataset ingestion, feature transforms,
deterministic splits and batches, differentiable model training, optimizer
state, checkpoint round-trip, tokenizer/RAG evaluation, serving, experiment
 tracking, reproducibility, metrics and report. The 31 modules are `main`,
`app`, `config`, `paths`, `cleanup`, `seed_data`, `dataset`, `ingest`,
`features`, `split_ops`, `batches`, `model`, `forward`, `loss`, `optimizer`,
`trainer`, `evaluation`, `checkpoint`, `tokenizer`, `chunks`, `index_ops`, `prompt`,
`retrieval`, `serve_registry`, `serve_policy`, `monitor`, `experiment`, `repro`,
`metrics`, `report`, and `validation`.

### 16-api-service-platform

A local API product using the public HTTP types: typed domain records, SQLite
repository/migrations, query/form/JSON validation, routing, sync handlers,
middleware/security headers/CORS/compression/rate limiting, lifecycle, metrics,
and an in-process request suite. The 31 modules are `main`, `app`, `config`,
`paths`, `cleanup`, `seed_data`, `domain`, `models`, `repository`, `migrations`,
`queries`, `auth`, `validation`, `router`, `handlers`, `middleware`, `errors`,
`serialization`, `pagination`, `health`, `metrics`, `audit`, `api_client`,
`lifecycle`, `openapi`, `events`, `security`, `integration`, `report`,
`filesystem`, and `verify`.

## Source and write paths

Each project writes only below its own `target/complete-<project>` directory and
removes generated files before returning success. The four project source trees
remain independent.

## Validation

For each project, run formatting, semantic checking, JIT, and AOT execution.
Validate the normal CLI path for the API request fixture, inspect cleanup, and
record any reproduced contract defect in `FINDINGS.md` before declaring the
project complete.