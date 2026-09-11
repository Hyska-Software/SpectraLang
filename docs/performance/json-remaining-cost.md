# JSON remaining cost: what is left and what to do next

Status after two perf rounds (Sept 2026): `09-json-bench` wall
small (100 objs) ~55ms, big (600 objs) ~66ms; exec-only ~17–22µs/obj
vs Go ~19µs/obj (chart reports `0.11ms vs 0.02ms/obj`, 5.7x, using the
conservative big-wall/600 methodology). Baseline before any fix:
4090ms big, 6.8ms/obj, 368x gap. No language feature was removed or
altered; `tests/validation/363_json_derive_exact_bytes.spectra` pins the
`to_json` bytes.

## Where the time goes now (per 3-field roundtrip)

Fixed cost dominates wall: ~53–57ms of every run is JIT compile +
process startup (unchanged by all JSON work; only AOT or a compile
cache moves it). The ~8–11ms of real JSON work over 600 objects is:

| Cost | Calls/obj | Notes |
|---|---|---|
| `encode_struct` | 1 | one buffer, one trailing alloc; optimal shape already |
| `parse` | 1 | serde parse + `JsonValue`→serde conversion + store insert (global `Mutex`) + full parent clone on every later access |
| root `decode_field` | 1 | object-mode validation, returns same handle |
| `decode_field_by_key` | 3 | one global lock + one child clone each |
| `value_free` (root) | 1 | |
| generic dispatch | ×7 | per call: 2 `manual_alloc` + 2 `manual_free` (global `Mutex` + `HashMap` + frame track each), NUL scan of every string arg, zeroed buffers |

So decode is now the entire remaining problem: ~6 calls, ~5 global
locks, ~5 whole-`Value` clones per object, plus dispatch overhead.

## Ranked candidates

1. **`json_decode_struct` (mirror of `encode_struct`)** — biggest
   remaining item. One call `(doc, kinds, name_1, ptr_1, ...)` that
   parses once and writes scalars straight through the field pointers
   (pointers already travel as i64; lowering knows every offset).
   Strings still need one `alloc_spectra_string` each; nested structs
   recurse first and pass pointers, exactly like `raw` on encode.
   Eliminates the `JsonStore`, all per-field locks/clones/handles, and
   5 of 6 decode calls. Estimated: decode ~6 calls → 1, exec-only
   roughly halved again. No new ABI concepts beyond what
   `encode_struct` already established; error paths (`missing required
   field`, wrong type, object-mode validation) must stay identical —
   extend the 363-style pin test to decode error bytes before starting.
2. **String-arg ABI** — every host call NUL-scans each string arg and
   `allocate_manual_bytes` zeroes before copying. `(ptr, len)` args
   plus a thread-local scratch result buffer remove ~4 `Mutex`+`HashMap`
   round-trips per call. Helps every host call in the language
   (hashmap bench included), but touches all hosts: higher blast radius
   than item 1.
3. **`FastHostCall` for the survivors** — direct Rust calls without
   arg/result buffers for `parse`/`decode_struct`/`encode_struct`.
   Worth doing only after item 1 (while 7 generic calls remain, dispatch
   is still measurable; after item 1 only 2 calls remain and this buys
   less).
4. **Schema interning** — `kinds` is re-split (`;`) on every call.
   Cache the parsed `Vec<Kind>` keyed by the literal pointer in a small
   `HashMap`. Micro; do it inside item 1's implementation, not separately.
5. **Bulk APIs** — `encode_structs`/`decode_structs` over arrays to
   amortize even the 1–2 remaining calls for bulk workloads. Only if a
   real bulk workload (not the bench) needs it.
6. **Wall fixed cost** — AOT binary or compile cache. This is ~80% of
   small-run wall and dwarfs everything above for scripts; it is a
   toolchain project, not a JSON project.

## Non-goals / traps

- Do **not** remove `[profile.test.package.*] opt-level = 0` from the
  root `Cargo.toml`. The dev-profile `opt-level = 2` override leaks
  into test builds and **hangs** the runtime concurrency tests (80ms
  worker-sleep timing asserts); verified by bisect (clean tree passes
  in 0.23s, override hangs >20min, shadow restores green, binary stays
  fast). Any future profile tuning must re-run the full
  `spectra-runtime` suite, not just the fast crates.
- Do **not** reintroduce per-field host calls on encode for any reason
  (readability, debugging): the 363 byte-pin test exists precisely to
  let future refactors prove they did not change output.
- Re-measure with: `spectralang run
  examples/complete/09-json-bench/src/{main,big}.spectra` (median of
  5+, interleaved A/B when the box is loaded), then `python
  scripts/bench_complete_charts.py` (rebuilds PDF pages 9–10, 1–8
  untouched) and the `examples/complete/README.md` tables.

## History

- Round 1: O(N²) frame-free scan → indexed frames; `value_free`
  emission (store leak); `value_get`+`decode_field` → single
  `decode_field_by_key`; concat chains → string builder. 4090ms → 279ms.
- Round 2: dev-profile `opt-level = 2` for the 5 hot crates (279ms →
  ~91ms); whole-struct `encode_struct` in one call (~91ms → ~66ms);
  363 byte-pin test. 279ms → 66ms.


## Known correctness issue (pre-existing, not caused by perf work)

`s.len()` over `for-in` loop variables of string arrays returns wrong
values (`tests/validation/array_iteration_sum.spectra` fails check #5:
`letters != 14`). Direct indexing (`words[1]`) and direct literals are
correct; only the loop-variable path is wrong, intermittently
(observed lens `12,12`, `6,7`, `5,5,5` for `5,4,5`).

- Fails 8/8 on `9e79062` (pre-round-1), 8/8 on `be83cf8` (round-1),
  10/10 on current tree: fully pre-existing, all build configs.
- Full-corpus run comparison current-vs-baseline: identical 52-file
  nonzero sets, i.e. zero new runtime failures from perf work.
- Suspected area (hypothesis, not proven): `for-in` string loop-var
  materialization / backend stack codegen reading past the string
  (first iteration correct, later garbage, ASLR-sensitive). The
  canonical corpus gate is compile-only, which is why it never caught
  this; any fix must add a run-gate for `array_iteration_sum`.