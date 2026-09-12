# SpectraLang Runtime Memory Strategy

SpectraLang's runtime uses a **single manual allocator** for all runtime-managed values. Every allocation returns an owning handle with deterministic drop semantics and accurate telemetry — there is no tracing garbage collector in the runtime.

## Strategy Overview

- The runtime exposes a single entry point, `ManualMemory`, which owns the manual allocator.
- Manual allocations (`ManualMemory::allocate_manual`) return `ManualBox<T>` handles that release statistics and memory on drop.
- Generated code allocates scratch and host-managed buffers through this allocator (JIT imports `spectra_rt_manual_alloc` / clears via `spectra_rt_manual_clear`).
- Runtime statistics (`MemoryStats`) and configuration (`MemoryConfig`) are centrally tracked so the CLI can surface diagnostics and regression baselines.

## Manual Heap

Manual allocations are targeted at host interop (FFI buffers, pinned memory) and deterministic lifetimes. `ManualMemory::allocate_manual` returns a `ManualBox<T>` that owns the object and updates runtime statistics automatically.

```rust
use spectra_runtime::memory::{ManualMemory, MemoryConfig};

let memory = ManualMemory::with_config(MemoryConfig::default());
let boxed = memory.allocate_manual([0_u8; 16]).expect("within limit");
assert_eq!(memory.stats().manual.bytes, 16);
drop(boxed);
assert_eq!(memory.stats().manual.bytes, 0);
```

- Manual allocations respect a soft limit (`MemoryConfig::manual_soft_limit_bytes`). Exceeding the limit returns an `AllocationError` so hosts can decide whether to fail fast or spill to an external allocator.
- Dropping `ManualBox<T>` or extracting the value with `into_inner()` decrements the live allocation counters, keeping telemetry accurate.

### Compiler Integration

The backend imports `spectra_rt_manual_alloc(size: usize) -> *mut u8` from the runtime. Every IR `alloca` lowers to a call to this function, meaning generated code executes on top of the runtime's manual heap instead of relying on Cranelift stack slots. During JIT execution the CLI clears outstanding allocations via `spectra_rt_manual_clear()` after the program finishes, ensuring allocator telemetry reflects each compilation run without leak accumulation. These hooks prove out the FFI surface and let us validate allocation pressure through `RuntimeState::memory_stats()`.

## Configuration

`MemoryConfig` currently exposes one knob:

| Field | Purpose | Default |
| --- | --- | --- |
| `manual_soft_limit_bytes` | Budget for manually tracked allocations. Set to `0` for "no limit". | `32 MiB` |

The runtime initialisation API allows custom configurations:

```rust
use spectra_runtime::{initialize_with_config, MemoryConfig};

let state = initialize_with_config(MemoryConfig {
    manual_soft_limit_bytes: 128 * 1024 * 1024,
});

println!("Memory stats: {:?}", state.memory_stats());
```

This configuration is stored in `RuntimeState` and can be surfaced by the CLI to match project-level tuning.

## Compiler & Codegen Implications

- Lowering maps Spectra ownership semantics onto the manual allocator; nothing in the runtime enforces a particular policy—those rules live in the compiler's semantic analysis.
- Diagnostics and optimisation passes can rely on `MemoryStats` to assert invariants or surface warnings (e.g., manual heap pressure). These metrics are accessible without mutating the runtime.
