# Spectra complete example projects

24 complete `.spectra` projects. Projects 01–17 are self-validating and print an
`ok` line; `18-spectragit` is a local version-control CLI, `19-spectraboard` is
a local task-board CLI, `20-spectraledger` is a local personal-finance CLI,
`21-spectrahabit` is a local habit tracker, and `22-spectravision` is an
end-to-end convolutional image classifier; `23-spectrapulse` adds differentiable temporal attention for sensor forecasting; `24-spectraroute` adds tensor-backed route planning. Projects 18–24 have their own
integration harnesses.

Run any project (from the repo root):

```
spectralang run examples/complete/<project>
```

Run the spectragit self-test through JIT with:

```powershell
spectralang run examples/complete/18-spectragit -- self-test
```

Run the SpectraBoard self-test through JIT with:

```powershell
spectralang run examples/complete/19-spectraboard -- self-test
```

Run the SpectraLedger self-test through JIT with:

```powershell
spectralang run examples/complete/20-spectraledger -- self-test
```

Run the SpectraHabit self-test through JIT with:

```powershell
spectralang run examples/complete/21-spectrahabit -- self-test
```

Run the SpectraVision training and evaluation pipeline through JIT with:

```powershell
spectralang run examples/complete/22-spectravision
```

## Functional projects

| # | Project | What it covers |
|---|---------|----------------|
| 01 | `01-todo-cli` | Task app: create/complete/pending filter, JSON persistence roundtrip (`TODO ok pending=3 total=5`) |
| 02 | `02-rest-crud` | User CRUD model + 5 HTTP routes with handler dispatch and server lifecycle (`CRUD ok routes=5 users_final=1`) |
| 03 | `03-csv-analytics` | CSV parse (`split_by`), aggregates, descending sort, top scorer (`CSV ok rows=7 sum=605 top=fin`) |
| 04 | `04-site-generator` | Markdown subset to HTML for 3 pages plus index (`SITE ok pages=3`) |
| 05 | `05-lru-cache` | Capacity-3 LRU over `std.collections` map + recency array (`LRU ok hits=1 evicts=2`) |
| 06 | `06-fanout-pipeline` | 4 spawned workers summing 1..1000 (`PIPELINE ok total=500500 mode=spawn`) |
| 11 | `11-ops-workbench` | Full operations workbench: CSV ingestion, JSON domain roundtrip, collection planning, concurrent workers, tensor/ML quality gates, checkpoint artifact, filesystem report and deterministic scheduling (`OPS WORKBENCH ok events=6`) |
| 12 | `12-ml-ai-workbench` | End-to-end local ML/AI lifecycle: CSV/JSONL ingestion, autodiff training, checkpoint reload, tokenizer/RAG retrieval, guarded serving, monitoring, experiment manifest, report, and cleanup (`ML AI WORKBENCH ok`) |
| 13 | `13-core-language-studio` | Core-language product slice: source materialization, lexing, parsing, AST walking, binding, inference, diagnostics, optimization, evaluation, packaging, cache, scheduling, report and cleanup (`CORE LANGUAGE STUDIO ok`) |
| 14 | `14-data-platform` | Data platform slice: CSV/JSONL ingestion, schema, normalization, filtering, aggregation, join, partitioning, batching, catalog/index/query/cache, quality, lineage, report and cleanup (`DATA PLATFORM ok`) |
| 15 | `15-ml-training-platform` | Training platform: dataset readers, feature transforms, autodiff/Adam training, checkpoint artifacts, tokenizer/RAG index, guarded serving, monitoring, experiment reproducibility, report and cleanup (`ML TRAINING PLATFORM ok`) |
| 16 | `16-api-service-platform` | API service: domain JSON, SQLite repository/migrations, query validation, routing, handlers, middleware, security policy, events, OpenAPI, server lifecycle, report and cleanup (`API SERVICE PLATFORM ok`) |
| 17 | `17-agent-operations-center` | Agent operations: governed tools, capability policy, memory, journal, approval, budget, taint, compensation/rollback, MCP surface, telemetry, and durable replay (`AGENT OPERATIONS CENTER ok`) |
| 18 | `18-spectragit` | Local text version control CLI in SpectraLang: blobs/trees/commits, branches, line diff, restore, timelines, snapshots, inspection, and deterministic self-tests |
| 19 | `19-spectraboard` | Independent task-board CLI with nested modules, task lifecycle, filtering, UTF-8 persistence, atomic writes, backup recovery and AOT integration |
| 20 | `20-spectraledger` | Personal-finance CLI with nested modules, income and expense tracking, exact cent arithmetic, monthly category budgets, reports, UTF-8 persistence, backup recovery and AOT integration |
| 21 | `21-spectrahabit` | Habit-tracking CLI with nested modules, UTC calendar validation, dated check-ins, daily streaks, weekly goals and reports, UTF-8 persistence, backup recovery and JIT/AOT integration |
| 22 | `22-spectravision` | CNN for 6×6 grayscale images: CSV datasets, splits, minibatches, conv2d, ReLU, max-pool, dropout, autodiff, AdamW, classification metrics, checkpoint reload and JIT/AOT integration |
| 23 | `23-spectrapulse` | Previsão multivariada de sensores: duas cabeças de atenção temporal, LayerNorm/GELU, MLP residual treinado com autodiff e AdamW, métricas de regressão e integração JIT/AOT |
| 24 | `24-spectraroute` | Planejador de transporte em múltiplos módulos: grafo de tensores, Dijkstra, fechamento de conexões, reroteamento e JIT/AOT |

## Performance comparisons (Spectra vs Go vs Rust)

Each bench ships the same algorithm and workload in three languages with a
shared checksum. Spectra runs via `spectralang run <project>`; Go via
`go run ./go/bench.go`; Rust via `rustc -O rust/bench.rs -o bench_rs && ./bench_rs`.

| Bench | Workload | Checksum |
|-------|----------|----------|
| `07-sieve-bench` | Eratosthenes to 500, 200 rounds | `primes=95 total=19000` |
| `08-hashmap-bench` | 500 inserts + 500 lookups, 200 rounds | `total=100000` |
| `09-json-bench` | 20 JSON encode/decode roundtrips, 5 rounds | `objs=100 sum_ids=950` |
| `10-matmul-bench` | Naive 32x32 matmul (`A=i+j`, `B=i-j`), 20 rounds, order i,j,k | `sum=55869440` |

Median wall times, 5 runs on Windows x64 (Spectra includes JIT compile;
Go/Rust are prebuilt `-O` binaries; small workloads are startup-dominated).
Charts: `.bench-charts/09-complete-benchmarks.png` and page 9 of
`.bench-charts/spectralang-benchmarks.pdf` (regenerate with
`python scripts/bench_complete_charts.py`).

| Bench | Spectra | Go | Rust | vs Go |
|-------|---------|----|------|-------|
| 07 sieve | 166.8ms | 11.9ms | 10.3ms | 14.0x |
| 08 hashmap | 232.4ms | 18.0ms | 13.9ms | 12.9x |
| 09 json | 55.0ms | 12.3ms | 9.4ms | 4.5x |
| 10 matmul | 50.2ms | 13.0ms | 11.8ms | 3.9x |

Sem o custo fixo (página 10 do PDF, `10-complete-benchmarks-exec-only.png`):
cargas 10x (`src/big.spectra`, `go/big.go`, `rust/big.rs` nos mesmos
projetos; checksums 190000 / 1000000 / 600+17700 / 1117388800) e diferencial
`(wall_big - wall_small) / delta` cancelando compilação JIT + startup:

| Bench | Gap exec vs Go | Leitura |
|-------|----------------|---------|
| 07 sieve | 1.7x (1.11us vs 0.67us/iter) | 14.0x no wall era custo fixo |
| 08 hashmap | 15.2x (0.95ms vs 0.06ms/round) | map via host call: custo real |
| 09 json | 5.7x (0.11ms vs 0.02ms/obj) | 1 call de encode + 5 de decode por roundtrip |
| 10 matmul | 0.6x (12.6us vs 20.5us/round) | Spectra mais rápido no loop puro |

## Porting notes (learned while building these)

- JSON `to_json`/`from_json` calls must live in the record's defining module.
- `std.collections` map/list handles cannot cross module boundaries; keep the
  handle in one module and pass plain data.
- Functions taking array parameters must return a value (no bare `return`).
- Element-wise host-call loops (e.g. list-per-cell sieves, 10k JSON
  roundtrips) are orders of magnitude slower than array indexing; size
  workloads accordingly.
