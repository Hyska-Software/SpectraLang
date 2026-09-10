# Spectra complete example projects

10 complete, self-validating `.spectra` projects. Each prints an `ok` line
and exits `0`; any broken invariant exits with a distinct non-zero code.

Run any project (from the repo root):

```
spectralang run examples/complete/<project>
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
| 09 json | 100.7ms | 12.3ms | 9.4ms | 8.2x |
| 10 matmul | 50.2ms | 13.0ms | 11.8ms | 3.9x |

Sem o custo fixo (página 10 do PDF, `10-complete-benchmarks-exec-only.png`):
cargas 10x (`src/big.spectra`, `go/big.go`, `rust/big.rs` nos mesmos
projetos; checksums 190000 / 1000000 / 600+17700 / 1117388800) e diferencial
`(wall_big - wall_small) / delta` cancelando compilação JIT + startup:

| Bench | Gap exec vs Go | Leitura |
|-------|----------------|---------|
| 07 sieve | 1.7x (1.11us vs 0.67us/iter) | 14.0x no wall era custo fixo |
| 08 hashmap | 15.2x (0.95ms vs 0.06ms/round) | map via host call: custo real |
| 09 json | 16.2x (0.31ms vs 0.02ms/obj) | host calls + 1 alloc final por roundtrip, sem termo quadrático |
| 10 matmul | 0.6x (12.6us vs 20.5us/round) | Spectra mais rápido no loop puro |

## Porting notes (learned while building these)

- JSON `to_json`/`from_json` calls must live in the record's defining module.
- `std.collections` map/list handles cannot cross module boundaries; keep the
  handle in one module and pass plain data.
- Functions taking array parameters must return a value (no bare `return`).
- Element-wise host-call loops (e.g. list-per-cell sieves, 10k JSON
  roundtrips) are orders of magnitude slower than array indexing; size
  workloads accordingly.
