# Spectra Tooling

This document describes the Phase 10 tooling baseline.

## LSP

The language server lives in `tools/spectra-lsp`.

Implemented LSP capabilities:

- diagnostics
- hover
- go to definition
- references
- rename
- document highlights
- document symbols
- workspace symbols
- formatting
- completion
- signature help
- inlay hints
- semantic tokens
- quick fixes for selected diagnostics and lints

Rename behavior:

- uses semantic definition links when the compiler exposes them
- falls back to a bounded lexical block rename for local identifiers without definition spans
- validates Spectra identifier syntax and rejects keywords
- avoids identifier substring replacements such as renaming `value` inside `value_extra`

Validation:

```powershell
cargo test -p spectra-lsp
```

## Runtime Diagnostics

`spectralang run` emits a runtime diagnostic when a program exits with a non-zero status:

```text
error[runtime]: program exited with status 7
  --> path/to/file.spectra:3:5
   |
   = stack:
     0: main() at path/to/file.spectra:3:5
   = help: inspect frame 0 and rerun with '--timings' for pipeline context
```

## AOT Debug Maps

`spectralang compile --emit-object` and `spectralang compile --emit-exe` emit a sibling JSON debug map:

```powershell
spectralang compile --emit-object target/app.obj app.spectra
# writes target/app.obj.spectra-debug.json
```

The debug map contains:

- schema version
- native artifact path
- Spectra source path
- `main` source span when present
- exported native symbol for debugger breakpoints
- supported debugger workflow tags such as `gdb` and `lldb`

This is the current production AOT debug strategy. There is no `spectralang debug`
subcommand: use your platform native debugger (MSVC/CDB/WinDbg on Windows, gdb/lldb
on Unix) to break on the exported symbol (`main` for object files, `spectra_user_main` for executable objects) and use the sidecar to resolve the Spectra source span. Native DWARF/PDB source stepping is not claimed by this baseline.
The emitted native sections are honest but partial: line tables contain only
compiler-proven span rows (functions without span data emit an empty table, never
invented lines), and locals without proven types carry the explicit unknown-type
marker (`T_UNKNOWN` in CodeView; omitted `DW_AT_type` in DWARF).

Validation:

```powershell
python scripts\validate_debugger_stack_traces.py --binary target\debug\spectralang.exe
```

## Benchmarking

Use:

```powershell
spectralang bench --bench-json target/bench.json tests/validation/01_basic_syntax.spectra
```

The JSON report includes:

- per-module frontend timings
- lexing/parsing/semantic/backend timings
- lowering/codegen timings
- optimization pass timings
- aggregate totals

Package workspaces can use:

```powershell
spectralang package bench --root .
```

The JSON output is intentionally stable enough for CI scripts to compare totals and detect regressions.
