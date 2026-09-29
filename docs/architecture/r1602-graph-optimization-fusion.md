# R-1602 Graph Optimization and Fusion

Updated: 2026-09-29

Roadmap item: `R-1602 Graph Optimization and Fusion`

## Purpose

R-1602 adds a deterministic optimization layer over the Phase 16 Tensor Graph IR. The optimizer produces a validated graph. The backend now consumes a supported subset of CPU unary fusion during JIT/AOT code generation; other graph optimizations remain analysis results until a matching execution path is implemented.

## Public Midend Contract

- Optimization entry point: `TensorGraph::optimize()`.
- Comparison entry point: `TensorGraph::compare_optimized(&optimized)`.
- Optimization output: `TensorGraphOptimizationResult`.
- Report output: `TensorGraphOptimizationReport`.
- Numerical tolerance policy: `1e-9` absolute and `1e-9` relative, matching R-1503.

## Implemented Optimizations

- Elementwise chain fusion:
  - `relu -> sqrt_f` becomes one `fused_elementwise.relu+sqrt_f` graph node when the chain has a single consumer and observable output metadata is preserved.
  - JIT/AOT code generation executes supported CPU unary chains as one runtime kernel. The current executable set is `neg`, `relu`, `sigmoid_f`, `tanh_f`, `sqrt_f`, and `log_f`; chains are limited to eight operations and must have a provably single-consumer path.
- Reduction-adjacent fusion:
  - `relu -> tanh_f -> sum_t` becomes one `fused_reduction.relu+tanh_f->sum_t` graph node when the elementwise chain feeds the reduction through single-consumer edges.
  - This fused-reduction graph node is not emitted as one backend kernel. In JIT/AOT, the supported unary prefix executes as one kernel and `sum_t` executes as a separate reduction kernel.
- Memory-aware scheduling metadata:
  - The optimization report records `reusable_edges`, which identifies fused input edges that can be scheduled without materializing intermediate tensors.
  - `planned_buffers`, `peak_live_buffers`, and reusable-edge counts are planner estimates/metadata; they do not measure or guarantee reduced runtime allocations by themselves.

## Correctness Contract

The current optimizer is semantics-preserving at graph level:

- it validates the input graph before optimizing;
- it preserves observable output metadata by value ID;
- it emits stable reports with node counts, fused groups, fused elementwise op count, fused reduction count, reusable edges, and tolerance policy;
- `TensorGraph::compare_optimized` checks optimized graph outputs against the original graph.

The graph optimizer and backend execution contract are distinct. The graph can represent a wider set of transformations than the backend executes. Backend legalization accepts only transformations with an implemented lowering; unsupported graph-only transformations retain their ordinary host-call execution path and must not be reported as fused runtime kernels.

## Test Gate

Run:

```powershell
cargo test -p spectra-midend --test tensor_graph_tests
cargo test -p spectra-backend
.\target\debug\spectralang.exe run tests/validation/640_tensor_graph_fused_unary.spectra
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/tensor-graph-fused-unary.exe tests/validation/640_tensor_graph_fused_unary.spectra
.\target\tensor-graph-fused-unary.exe
```

The graph suite includes:

- elementwise chain fusion;
- reduction-adjacent fusion;
- optimized vs unoptimized graph comparison;
- stable optimized graph snapshot;
- existing R-1601 graph validation regressions.

The separate backend and CLI checks prove CPU JIT/AOT execution and gradient
behavior for the supported unary subset.

## Spectra Examples

- `examples/ai/tensor_graph_elementwise_fusion.spectra` demonstrates a `relu -> sqrt_f -> tanh_f` elementwise chain.
- `examples/ai/tensor_graph_reduction_fusion.spectra` demonstrates a `relu -> tanh_f -> sum_t` reduction-adjacent pattern.

Both examples are executable through the Phase 13 AI examples block in `run_tests.ps1`.
