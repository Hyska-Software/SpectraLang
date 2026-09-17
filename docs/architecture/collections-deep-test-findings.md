# Collections Deep-Test Findings (Fixed)

Status: **resolved** — five defects found by a Map/Stack/Queue test campaign,
fixed, and covered by regressions.

Found and fixed: 2026-09-17 · CLI 0.3.4 (nightly).

Method: 22 exploratory `.spectra` probes exercised Map, Stack, Queue, and
Iterator beyond the existing suite (computed/unicode/long string keys, stress,
nested handles, loop control, aggregate arrays, generic annotations). Every
defect below reproduced from a probe, was fixed at its source, and now has a
checked-in regression.

## 1. Map string-key iteration order was not reproducible

- Symptom: iterating a `Map<string, _>` produced a different key order on
  every run. Six identical JIT runs gave six distinct orders; AOT runs varied
  too. Int-keyed maps were stable (sorted ascending).
- Root cause: `MapRegistry::keys_snapshot`/`values_snapshot` sorted
  `SpectraHostValue` raw payloads. For string keys that raw value is the
  heap pointer of the packed string, so ordering followed allocation
  addresses.
- Fix: `CollectionKey` now implements `Ord` over key *values* (text for
  strings, `i64` for scalars), and both snapshots sort by it
  (`runtime/src/stdlib/map.rs`, `runtime/src/stdlib/stdlib_bindings.rs`).
- Regression: `tests/validation/507_stdlib_map_string_key_order.spectra`
  (literal, computed, and unicode keys, values order, snapshot isolation).

## 2. Generic annotations rejected in record fields and array elements

- Symptom: `record Inventory { items: Stack<int> }` and
  `let slots: [Stack<int>] = []` failed with a syntax error, while the same
  annotation worked in parameters, returns, and let bindings. Primitive
  fields without commas parsed fine.
- Root cause: `looks_like_type_args_in_annotation` only accepted `{`, `=`,
  a comma, `)`, `returns`, `[`, or `>` after the closing `>`. A record's last
  field is followed by `}` and a comma-less next field by an identifier; an
  array element type is followed by `]`.
- Fix: `compiler/src/parser/type_annotation.rs` accepts `}`, `]`, `#`,
  identifiers, and the `public`/`internal` field modifiers as well.
- Regression: `tests/validation/509_generic_type_annotation_positions.spectra`
  and `compiler/tests/syntax_readability.rs::generic_annotations_parse_in_record_fields_and_array_elements`.

## 3. Array element stride for aggregate elements

- Symptom: `let slots: [Stack<int>] = [a, b]` made `slots[0]` and `slots[1]`
  alias `b`; `stack_free` then double-freed one handle. Records were worse:
  the stride (`layout` size) was larger than the allocation
  (`stored_size * length`), so element stores ran past the array.
- Root cause: `GetElementPtr` lowering used `type_size_bytes` (standalone
  allocation size) as the element stride. The layout module documents array
  elements as `stored_size`: aggregates are embedded as their 8-byte pointer.
  A field-less opaque handle struct (`Stack_int`) has layout size 0, so every
  slot pointed at the same address.
- Fix: `backend/src/codegen_instruction_memory.rs` uses
  `layout::stored_size(element_type)` for the stride, matching the array
  allocation size.
- Regression: `tests/validation/508_array_of_aggregate_elements.spectra`
  (handles, records, tuples, canary array).

## 4. Per-iteration aggregate construction shared one stack slot

- Symptom: `records[i] = Ponto { x: i, y: i * 10 }` inside a loop left every
  element reading the last construction (`(2,20)` for three iterations).
- Root cause: `collect_stack_allocas` promoted any struct alloca to one fixed
  Cranelift stack slot. The slot is reused on each loop iteration, and the
  array stores its pointer, so all elements aliased it. Only non-alloca
  stores and call/return escapes demoted an alloca.
- Fix: `backend/src/codegen_alloca.rs` computes the CFG blocks on cycles
  (iterative Tarjan) and demotes any stack alloca whose defining block is on
  a cycle when a derived pointer is stored, so each execution owns fresh
  storage on the manual heap.
- Regression: `backend/src/codegen_tests.rs` (`cyclic_aggregate_construction_is_not_stack_promoted`,
  `straight_line_aggregate_construction_stays_stack_promoted`) and the
  `[Ponto]` loop case in test 508.

## 5. Monomorphization dropped compound type arguments

- Symptom: `func size_of<T>(items: Stack<T>) returns int` called with a
  `Stack<int>` failed with `error[internal]: Function 'size_of_unknown'
  contains an unresolved IR type`; `func first_of<T>(items: [T])` failed the
  same way. Bare `T` parameters worked.
- Root cause: `substitute_type_in_annotation` replaced a type parameter with
  `ir_type_to_ast_name(concrete)`, a flat name: `Stack<int>` became bare
  `Stack` and `[int]` became `unknown`. `infer_argument_types` also returned
  `Unknown` for compound argument expressions (field access, calls, ...), and
  `MonomorphizationRequest::type_to_string` mangled them as `unknown`.
- Fix: `ir_type_to_annotation` (`midend/src/lowering_impl_types.rs`) now
  round-trips every `IRType` variant (generics keep arguments, arrays/tuples/
  functions recurse); `substitute_type_in_annotation` replaces the whole
  annotation with it; `infer_argument_types` delegates to the general
  expression inference; `type_to_string` renders compound types for mangled
  names (`midend/src/lowering.rs`).
- Regression: `tests/validation/509_generic_type_annotation_positions.spectra`
  plus the existing generic lowering tests.

## Validation

- `cargo test -p spectra-backend` — 59 passed.
- `cargo test -p spectra-compiler` — 94 passed.
- `cargo test -p spectra-midend` — 78 passed.
- `cargo test -p spectra-runtime` — 224 passed.
- `cargo test -p spectra-cli` — 90 passed.
- `tests/validation` compile sweep — 518/518.
- `run_tests.ps1` — 908 expected results, 908 passed, 0 failed (100%).
- The four new validation tests run under the JIT and as AOT executables.
