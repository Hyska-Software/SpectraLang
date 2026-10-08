# Standard Library Source Migration Ledger

This ledger records the current implementation form of each compiler-registered
core `std.*` module. `spectra.api` is a separate package and is intentionally
excluded. Function counts are the public function exports reported by
`dump_stdlib_contract`; compatibility-only runtime ABI bindings are not counted.
The typed catalog remains the per-symbol source of truth for current signatures.

## Architecture

- Source-authored modules live under `stdlib/src/`; a relative path maps to a
  canonical `std.*` name (`algorithms.spectra` maps to `std.algorithms`, and
  hyphens in path segments map to underscores).
- `compiler/build.rs` embeds each source, its stable diagnostic path, its
  development source path, and a deterministic module-name-sorted index. The
  build script emits `cargo:rerun-if-changed` for the source tree.
- The source snapshot and compiler are built and shipped as one toolchain
  artifact. There is no separately installed std source bundle to mismatch or
  override, so source/compiler compatibility is fixed at build time.
- The bundle exposes a deterministic FNV-1a 64-bit content identity through
  `embedded_stdlib_bundle_id()`. This identifies the source snapshot embedded
  in the compiler; it is an integrity/version label, not a security hash.
  The compiler has no cross-compilation cache today; the identity is exposed
  for cache integrations if such a cache is introduced later.
- The project resolver adds only imported source modules and their transitive
  source-module dependencies. The regular module graph compiles them before
  their importers. Native `std.*` modules continue to resolve through the
  existing semantic/runtime contracts.
- Source functions publish exports through ordinary semantic analysis and
  compile through normal Spectra lowering/code generation. Their
  `ModuleExports::stdlib_path` remains unset, so the host-call lowering path is
  not selected for them.
- Project sources cannot declare names in `std.*` or `spectra.std.*`. Unknown
  standard imports continue to report E033.
- LSP completion, hover, and symbol lookup parse the same embedded index.
  Go-to-definition uses the checkout source path when available; installed
  toolchains materialize the embedded file under a temporary directory keyed
  by the bundle identity so the editor can open it.
- The compiler bundle supplies sources during check/JIT/AOT compilation. An
  emitted AOT program contains compiled code and does not open `.spectra` std
  files at runtime.

## Core Module Inventory

Counts below describe the compiler-visible public function exports in the
current registry. “Migrated” means implemented in repository-owned `.spectra`
and emitted as `implementation = "spectra-source"` in the generated contract.
Every remaining function in an existing module remains on its current
compiler/runtime implementation until that module is migrated explicitly.

| Module | Current form | Public functions | Migrated in Phase 34 | Remaining functions / dependency reason |
|---|---|---:|---:|---|
| `std.algorithms` | Source | 2 | 2 | 0; pure integer algorithms |
| `std.char` | Native | 8 | 0 | 8; Unicode scalar/runtime helpers |
| `std.collections` | Hybrid | 150 | 0 | 150; runtime-managed handles plus compiler lowering |
| `std.concurrent` | Native | 17 | 0 | 17; task, thread, and reactor services |
| `std.convert` | Native | 11 | 0 | 11; conversion and formatting runtime surface |
| `std.env` | Native | 6 | 0 | 6; process environment and arguments |
| `std.error` | Hybrid | 7 | 0 | 7; typed error surface plus runtime status/handles |
| `std.fs` | Native | 10 | 0 | 10; operating-system filesystem access |
| `std.io` | Native | 7 | 0 | 7; process input/output streams |
| `std.math` | Native | 24 | 0 | 24; existing runtime math implementation |
| `std.ml` | Hybrid | 112 | 0 | 112; tensors, model/runtime state, and compiler-native paths |
| `std.numeric` | Hybrid | 65 | 0 | 65; exact-width compiler semantics and runtime adapters |
| `std.option` | Hybrid | 5 | 0 | 5; language type semantics plus tagged runtime values |
| `std.random` | Native | 4 | 0 | 4; runtime random-number state |
| `std.range` | Hybrid | 8 | 0 | 8; language range lowering and iterator adapters |
| `std.result` | Hybrid | 7 | 0 | 7; language result semantics plus runtime error propagation |
| `std.serve` | Native | 31 | 0 | 31; local serving and request state |
| `std.string` | Native | 27 | 0 | 27; string allocation and text runtime helpers |
| `std.tensor` | Hybrid | 111 | 0 | 111; storage, CPU/GPU kernels, autodiff, and compiler lowering |
| `std.time` | Native | 23 | 0 | 23; clocks and platform time services |

`std.algorithms` is the first source module. It has no imports or native
dependencies and adds no parallel Rust implementation:

| Public API | Contract |
|---|---|
| `gcd_nonnegative(a: int, b: int) -> int` | Both inputs are nonnegative; `(0, 0)` returns `0`. |
| `is_prime(value: int) -> bool` | Values below `2` return `false`. |

## Incremental Contribution and Migration Gate

For each new source module or migration:

1. Add the `.spectra` module under `stdlib/src/` and declare the path-derived
   `std.*` name. Keep public signatures compatible with any existing API.
2. Implement pure behavior in Spectra. Keep only unavoidable OS, accelerator,
   or runtime primitives behind the existing native contract.
3. Add an import-only `.spectra` consumer that exercises every public function,
   documented edge case, visibility rule, and error behavior.
4. Register the consumer in execution coverage and the stdlib contract probe;
   regenerate `packages/spectra-contract/catalog/stdlib.toml` from the compiler
   contract. Source functions must be labelled `spectra-source`, have a
   compiled-Spectra ABI, and carry no host effect or Rust host symbol.
5. Run semantic/check coverage, JIT and AOT parity, and the R-3007 contract
   audit. For an existing migration, remove a duplicate builtin declaration
   only after the source exports and execution evidence pass.
6. Update this ledger and the Phase 34 status in the strategic plan, backlog,
   and `roadmap/roadmap.toml` together. Leave unmigrated modules explicitly
   classified and keep `spectra.api` outside this process.

The source contract auditor compares the `.spectra` module declaration with its
path, detects duplicate source/native exports, detects source entries missing
from the generated catalog, and checks public source exports against their
probe and documentation contract. Source-file presence by itself is not
migration evidence.
