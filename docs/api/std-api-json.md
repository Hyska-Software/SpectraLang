# std.api.json

`std.api.json` is the Phase 22 JSON codec surface for Spectra API programs.
The native implementation lives in `packages/spectra-api/src/json.rs`.

## Values

The codec supports the RFC 8259 JSON data model:

- `null`
- booleans
- finite numbers
- strings with standard JSON escapes and Unicode escapes
- arrays
- objects/maps with string keys
- nested combinations of the values above

## Decoder

The decoder parses a complete JSON document and returns a typed value.
Invalid input reports:

- `kind`: syntax, unexpected EOF, data, or I/O classification
- `offset`: byte offset in the original document
- `line` and `column`
- `message`: parser detail from the underlying RFC 8259 parser

The host-call compatibility surface keeps:

- `spectra.api.json.validate`
- `spectra.api.json.kind`

Both host calls use the same full decoder as the Rust API. They do not use
brace balancing or partial classification.

### Handle-based access surface

`spectra.api.json.parse` parses a document and stores the value in a global
handle table, returning an opaque `int` handle (`0` signals a parse error).
The language surface `std.api.json.parse` mirrors it and is likewise typed as
returning `int`; handles must be released with `value_free`.

- `spectra.api.json.parse(text) -> handle`
- `spectra.api.json.value_kind(value) -> int` — kind code: `0` invalid,
  `1` null, `2` bool, `3` number, `4` string, `5` array, `6` object
- `spectra.api.json.value_len(value) -> int` — array length or object size
  (`0` otherwise)
- `spectra.api.json.value_get(value, key) -> handle` — object member
  (`0` when absent or not an object)
- `spectra.api.json.value_at(value, index) -> handle` — array element
  (`0` when out of range or not an array)
- `spectra.api.json.value_text(value) -> string` — string value
  (`""` otherwise)
- `spectra.api.json.value_number_bits(value) -> i64` — IEEE-754 bit pattern
  of a number (`0` when not numeric)
- `spectra.api.json.value_bool(value) -> int` — `1`/`0`/`-1`
- `spectra.api.json.value_free(value) -> unit`
- `spectra.api.json.stringify(value) -> string` — compact RFC 8259 output


## Encoder

The encoder serializes supported values to RFC 8259 JSON. It rejects non-finite
numbers such as `NaN` and infinity, and it validates stored number
representations before writing output.

Objects are represented with deterministic key ordering in the native API.

## Validation

R-2208 is validated by:

- `cargo test -p spectra-api json --offline`
- `scripts/validate_r2208_json_codec.py`
