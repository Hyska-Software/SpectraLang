# ADR 0017: Agent Surface Ownership and Catalog-Driven Generation

Status: Accepted

Date: 2026-09-12

Roadmap item: R-3201

## Context

Phase 32 adds `std.agent`, a native surface with a model gateway, a tool loop,
memory, governance and interoperability. The repository has already paid for
one native package (`spectra.api`, ADR 0011) and has learned where the cost
concentrates: not in the implementation, but in the seam. A single host
function is currently described in four hand-maintained places:

1. the compiler's builtin module/type table (`builtin_std_core.rs`,
   `builtin_modules.rs`, `builtin_contract.rs`);
2. the midend lowering tables (`lowering_std_host_*.rs`);
3. the Rust host-call table and registration (`host_calls.rs`,
   `api_registration.rs`);
4. the documentation and probes (`docs/`, `scripts/stdlib_contract.toml`).

Four copies of one surface is the repository's dominant drift hazard. Adding
`std.agent` on top of that model would multiply it. Separately, a coding agent
consuming this project needs a machine-readable answer to "what does this
project expose?", which does not exist today.

This ADR fixes ownership and generation before the first `std.agent` function
lands.

## Decision

### D1 — `spectra.agent` is an rlib aggregated through the existing registration pair

`std.agent` is backed by a Rust workspace crate `packages/spectra-agent` with
`crate-type = ["rlib"]`. It depends on `spectra-runtime` and is depended on by
`packages/spectra-api`.

The crate is registered through the **existing** pair in
`packages/spectra-api/src/api_registration.rs`:

- `spectra_api_register_host_calls` (`:13-16`) — the single exported
  registration entrypoint; `spectra_api::register()` additionally calls
  `spectra_agent::register()`.
- `spectra_api_host_call_count` (`:18-21`) — the single exported count.

There is no second static library, no new C symbol, and no change to the
linker, `runtime_lib`, the AOT shim or CLI library discovery.

This mirrors how every existing native package aggregates: one staticlib
embeds `spectra-runtime`, so linking a second crate that also defines the
runtime symbols duplicates them, and the AOT shim imports exactly
`spectra_api_register_host_calls` (`backend/src/aot.rs:764-775`). Aggregating
into the existing rlib avoids both problems at the cost of one dependency edge.

### D7 — Surface commands are CLI subcommands with JSON output

`surface`, `impact`, `explain` and `docs` are `spectralang` subcommands that
emit machine-readable JSON and follow the existing exit-code contract
(`tools/spectra-cli/src/lib.rs:77-82`: 0 success, 64 usage, 65 compile
failure, 74 I/O).

The midend and backend do not depend on `serde`, and the module registry is
private to the pipeline. The compiler therefore gains exactly one public
accessor returning the registry and one serializable surface snapshot;
serialization stays in the CLI. This keeps the compiler's dependency set
unchanged.

### D8 — The contract catalog is the single source, migrated in three steps

`packages/spectra-contract` is the single source for the generated midend
lowering tables and the Rust host-call table. The migration is a three-step
sequence, each step gated by a byte-identical diff check:

1. **Extract.** Generate from the current hand-written tables and require the
   output to be byte-identical to them. This proves the generator reproduces
   the current behavior before anything consumes it.
2. **Regenerate.** Switch the generator's source to the catalog and require the
   output to be byte-identical again. This proves the catalog faithfully
   describes the current tables.
3. **Delete.** Delete the manual copies. Generated files carry a do-not-edit
   header naming the generator and a `--check` command. The `--check` mode
   fails on staleness and is wired into tests.

A manual edit to a generated file makes `--check` fail. Until step 3 completes,
the manual tables remain authoritative; after it, the catalog is.

#### Catalog fields to add

The catalog entry (`packages/spectra-contract/src/lib.rs`) gains these fields,
optional in the struct for one release, required for `kind=function` entries
once migration completes:

| Field | Meaning |
| --- | --- |
| `params` | ordered name + type list of the function's parameters |
| `returns` | the surface return type |
| `ir_return` | the IR-level return descriptor used by lowering |
| `returns_value` | whether the lowering writes a result value |
| `rust_symbol` | the Rust `extern "C"` symbol that implements the call |
| `cfg_feature` | the cargo feature gating the entry, if any |
| `sink` | whether the call is a sensitive sink for taint gating (D10) |
| `scope_keys` | scope predicate keys the call supports (D14) |

`ir_return` and `returns_value` come from the current lowering tables;
`rust_symbol` from the Rust host-call table; `cfg_feature` from surrounding
`#[cfg]` attributes; `sink`/`scope_keys` from namespace rules with an explicit
override table for exceptions.

### `std.agent` ownership mirrors `std.api`

`std.agent` is a `std` namespace whose owning package is `spectra.agent`,
exactly as `std.api` is owned by `spectra.api` under ADR 0011:

| Layer | `std.api` (ADR 0011) | `std.agent` (this ADR) |
| --- | --- | --- |
| Spectra package | `spectra.api` | `spectra.agent` |
| Import path | `std.api.*` | `std.agent.*` |
| Rust crate | `spectra-api` | `spectra-agent` |
| Crate path | `packages/spectra-api` | `packages/spectra-agent` |
| Package manifest | `packages/spectra-api/spectra.toml` | `packages/spectra-agent/spectra.toml` |
| Spectra bindings | `packages/spectra-api/src/*.spectra` | `packages/spectra-agent/src/*.spectra` |
| Runtime integration | `runtime/src/api/` | `runtime/src/agent/` |
| Host-call prefix | `spectra.api.*` | `spectra.agent.*` |
| Documentation root | `docs/api/` | `docs/agent/` |

The compiler exposes `std.agent.*` because stdlib resolution already treats
`std.*` modules as virtual public modules. Ownership, release cadence and
implementation belong to the package, not to the core language.

### Exactly one new attribute

Phase 32 adds exactly one authored annotation: `#[agent_tool("description")]`.
Name, input schema, effects and required capabilities are derived; the
description is the only authored string. Function attributes have no
validation path today, so the first one is expensive and must be the last. A
second attribute requires revising this ADR first.

## Rationale

**Why aggregate rather than ship a second staticlib.** Each staticlib embeds
`spectra-runtime`; linking two duplicates Rust symbols in the final binary,
and the AOT shim imports one registration entrypoint by name. Aggregating into
the existing rlib means no linker, no `runtime_lib`, no AOT and no CLI
discovery changes. The dependency edge `spectra-agent -> spectra-api` is a
compile-time detail, not a public one.

**Why catalog-first.** Four hand-maintained copies of one surface will drift.
Generation removes three of the four edits and turns drift into a failing
`--check` mode. The migration is staged so each step is provably
non-semantic: byte-identical output before and after each source switch.

**Why serde stays in the CLI.** The compiler is consumed by the midend and
backend and must not acquire serialization dependencies for a tooling feature.
One accessor and one snapshot type is the minimal compiler change.

**Why mirror ADR 0011.** Phase 22 already decided how a native package owns a
`std.*` namespace. Reusing that decision keeps one mental model for package
ownership and avoids inventing a second convention beside the existing one.

## Consequences

- `R-3206` extends the catalog schema with the fields above and populates them.
- `R-3207` generates the seven midend lowering tables and adds the drift guard.
- `R-3208` generates the Rust host-call table, removes the manual count, and
  derives the count from `HOST_CALLS.len()`.
- `R-3209` creates `packages/spectra-agent` as an rlib, adds it as a dependency
  of `spectra-api`, and calls `spectra_agent::register()` from
  `spectra_api::register()`.
- `R-3202`–`R-3205` implement `surface`, `impact`, `explain` and `docs` with
  the accessor and snapshot described here.
- `R-3210` adds the single `#[agent_tool]` attribute and derives everything
  except the description.
- No new linker, `runtime_lib`, AOT or CLI-discovery change is permitted by
  this ADR; a future item that needs one must revise this ADR.
- Generated files are read-only from a contributor's perspective; editing them
  fails `--check`.

## Rejected Alternatives

### A second static library or a new registration symbol

Rejected. It duplicates the embedded `spectra-runtime` symbols at link time
and requires the AOT shim, the linker and CLI library discovery to learn a
second entrypoint.

### Implement `std.agent` as compiler-only fake modules

Rejected by ADR 0011 for `std.api` and rejected here for the same reason: the
namespace must be owned by a versioned, publishable package, not by the core
compiler.

### Keep the hand-written tables and add generators only for new functions

Rejected. It leaves two sources of truth and guarantees eventual drift. The
extract -> regenerate -> diff-empty -> delete sequence exists to migrate the
whole surface without a semantic change.

### Serialize the surface in the compiler and expose serde there

Rejected. The compiler and midend must not depend on `serde` for a CLI
feature.

### A second function attribute

Rejected. Every derived convention reproduces ADR 0011's failure mode in a new
place. One attribute, derived fields, one authored string.
