# std.api.json Derive

`#[derive(Serialize, Deserialize)]` generates the JSON-facing methods for
Spectra structs and enums that participate in the API surface.

## Generated Surface

For `#[derive(Serialize)]`:

- `value.to_json() -> string`

For `#[derive(Deserialize)]`:

- `Type::from_json(json: string) -> Type`
- `Type::json_error_field(json: string) -> string`

The generated surface is specified in terms of `std.api.json.*`: serialization
walks the derived fields and encodes each value through the JSON codec hosts
(`std.api.json.quote_string`, `std.api.json.quote_char`,
`std.api.json.encode_number`, string concatenation), and
deserialization parses with `std.api.json.parse` and extracts each field through
`std.api.json.decode_field` plus field-level validation. `json_error_field` reports
the first violating path through `std.api.json.typed_error_field`.

Supported field types are `int`, `float`, `bool`, `string`, `char`, scalar
`List<...>` fields, nested derived structs, and unit-only enum fields.
`#[json(optional)]` remains limited to scalar fields. Anything else (arrays,
maps, tuples, exact-width numerics, `optional` on aggregate or enum fields,
recursive types, and data-carrying enum variants) is rejected at compile time
with a typed lowering error naming the struct and field. Unit-only enum fields
use their variant wire names, including `#[json(rename = "...")]`, for both
encoding and decoding.

`Type::json_schema()` emits standard JSON Schema `enum` values for unit-only
enums, in declaration order and using their wire names. `json_error_field`
uses the same values, so an unknown enum string is rejected before a derived
tool wrapper calls user code.

Runtime failures (malformed documents, missing required fields, wrong field
types, and unknown enum values) print
`spectra.api.json decode error at '<path>'` to stderr and abort with
`runtime error: host call 'spectra.api.json.decode_field_by_key' failed`
(exit 101) for direct `from_json` calls. Generated `#[agent_tool]` wrappers run
`json_error_field` first and return the typed tool failure to the caller
instead of aborting the process.

## Field Options

Struct fields support:

- `#[json(rename = "wire_name")]`
- `#[json(optional)]`

Renamed fields use the wire name in JSON diagnostics. Optional fields may be
omitted or set to `null`.

Enum variants support:

- `#[json(rename = "wire_variant")]`

`optional` is rejected on enum variants.

## Literal Validation

When `Type::from_json("...")` or `Type::json_error_field("...")` receives a
JSON string literal, the semantic pass validates it against the derived schema.
Diagnostics use stable JSON derive codes:

- `EJSON001`: invalid JSON syntax
- `EJSON002`: root is not an object for a derived struct
- `EJSON003`: missing required field
- `EJSON004`: wrong field type

The diagnostic context points to the failing field, including any `rename`
mapping.

## Validation

R-2209 is validated by:

- `cargo test -p spectra-compiler --offline`
- `cargo test -p spectra-midend --offline`
- `cargo test -p spectra-api --offline`
- `spectralang compile tests/validation/133_json_derive_surface.spectra`
- `spectralang check tests/errors/json_derive_missing_field.spectra`
- `spectralang check tests/errors/json_derive_wrong_type.spectra`
- `spectralang check tests/errors/json_derive_duplicate_rename.spectra`
- `spectralang check tests/errors/json_derive_invalid_attribute.spectra`
- `spectralang run tests/validation/425_agent_option_enum_tool.spectra`
- `scripts/validate_r2209_json_derive.py`
