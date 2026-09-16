# Five Large Spectra Projects Implementation Plan

**Intent:** Add five genuinely runnable, multi-file applications that exercise
Spectra from core language constructs through advanced standard-library and
agent capabilities.

**Current Behavior:** `examples/complete` now contains the five additional
domain-shaped products. Their source trees contain 28, 28, 31, 31, and 28
modules respectively, each wired to a runnable entrypoint and isolated cleanup
path.

**Expected Outcome:** Create five new complete projects, each with at least 28
real `.spectra` files, one connected executable path, an isolated workspace,
and a stable success marker. The projects cover compiler-like core processing,
data-platform operations, an ML training platform, an API service, and a
governed AI agent operations center.

**Target-Perspective Output:** A user can run each project from the repository
root and see a domain-specific `ok` marker, inspect the generated reports or
journals, and rerun the command without state leakage.

**Truth Owner:** The project manifests and their `src/main.spectra` entrypoints
own the user-visible behavior. Existing `std.*` contracts, package APIs, and
the compiler/runtime tests own capability semantics; no simulated provider or
fake success path is allowed.

**Contract Boundary:** Each project crosses module boundaries with typed
records where useful and scalar/string/handle values at standard-library host
boundaries. Filesystem state is isolated per project. The API and agent projects
use their existing public `std.api.*` and `std.agent` contracts respectively.

**Cutover:** These five projects become the next complete-example tier. Existing
focused examples remain narrow contracts and are not deleted or silently
replaced.

**Displaced Path:** None is deleted. The new projects fill the missing large
application layer; any duplicate helper stays intentionally scoped to its
project and is not presented as a shared framework.

**Value Density:** Every source module is imported by the executable graph and
owns a domain transformation, validation, persistence boundary, or operational
concern. File count alone is never used as acceptance evidence.

**Acceptance Evidence:**

- Each project has more than 25 `.spectra` files under `src/`.
- Each project passes `fmt --check` and `check --json`.
- Each project runs successfully through JIT and AOT, with a stable marker.
- Each project exercises its advertised domain APIs using real handles/data.
- Generated workspace state is cleaned after a successful run.
- Project manifests parse as TOML, all sources end with newlines, and
  `git diff --check` is clean.
- `cargo build -p spectra-cli` and focused compiler/midend tests remain green.
- Any real contract or implementation defect found during integration is
  documented in the goal findings file and corrected before completion.

**Evidence Lane:** Implement and validate one project at a time, beginning with
the core-language project. For each project: create the manifest and skeleton,
implement a vertical dataflow slice, run checker/JIT, add the remaining
operational modules, then run formatter/JIT/AOT and inspect cleanup. Finish with
the cross-project count and regression audit.

**Kill Criteria:** Stop and redesign a project if modules are disconnected,
only append hard-coded markers, use an unavailable API, share mutable target
paths with another project, or cannot prove its advertised runtime behavior.
Do not hide failures behind unconditional return-zero code.

**Non-goals:** No roadmap status changes, commits, pushes, cloud deployment,
external credentials, or network-only success gate is required. Optional ONNX,
PostgreSQL, Redis, and external model providers remain outside the deterministic
local baseline unless an existing local contract can validate them safely.

**Risk if wrong:** Five large-looking trees could still be five collections of
stubs. The integration proof therefore requires each main path to consume real
outputs from prior modules and to execute the relevant public standard-library
surface in both JIT and AOT.

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

### 17-agent-operations-center

A governed AI operations center using `std.agent`: derived tools, model ask,
capability policy, budget and approval decisions, taint/trust handling,
journal/replay, compensation, memory recall, MCP surface, scheduling,
concurrency, evaluation, report and cleanup. The 28 modules are `main`, `app`,
`config`, `paths`, `cleanup`, `prompt`, `model`, `policy`, `tools`, `planning`,
`memory`, `journal`, `approvals`, `budget`, `taint`, `compensation`, `retry`,
`scheduler`, `mcp`, `protocol`, `evaluation`, `report`, `validation`, `ledger`,
`telemetry`, `fixtures`, `replay`, and `service`.

## Source and write paths

Each project writes only below its own `target/complete-<project>` directory and
removes generated files before returning success. Existing projects were left
intact except for the complete-example index and the focused API validation
fixture corrected after its missing-field contract was reproduced.

## PRE review

- **Mode:** PRE
- **Verdict:** aligned
- **Blockers:** none
- **Major risks accepted:** public API surfaces have stricter shape and
  lifecycle contracts than plain scalar examples; each project will validate
  them incrementally. The agent project will use the existing deterministic
  `mock:` provider only where the checked-in agent contract defines it.
- **Completion gate:** all five trees pass the per-project formatter/check/JIT
  and AOT audit, the official request-validation fixture passes the normal CLI
  path, and the focused Rust build/test gate is green.
