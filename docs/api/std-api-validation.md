# `std.api.validation`

`std.api.validation` validates request payloads against an immutable,
handle-backed schema and exposes failures as RFC 7807 Problem Details.

## Schema

Start with `schema()` and add wire-name fields with `field(schema, name,
type, required)`. The type codes are `1` string, `2` integer, `3` boolean, and
`4` float. Each builder returns a new schema handle, so a schema can be shared
without mutating an existing route contract.

The constraint builders are:

- `min_length` and `max_length` for string fields;
- `range` for integer and float fields;
- `regex` for string fields, using the Rust `regex` engine.

Invalid schema operations are reported by `error_code()` and
`error_message()`. The error codes are `1` invalid schema, `2` unknown field,
and `3` invalid constraint.

## JSON and form validation

`validate_json(schema, body)` expects a JSON object and matches fields by their
wire names. `validate_form(schema, form)` validates values from a parsed
`std.api.form.Form`; integer, float, and boolean values are parsed from their
form representation. Both functions return a `ValidationResult` containing
all field issues rather than stopping at the first failure.

Use `result_ok`, `result_count`, `result_field`, `result_code`, and
`result_message` to inspect a result. The stable issue codes include
`required`, `type`, `min_length`, `max_length`, `min`, `max`, `regex`, and
`duplicate`.

## RFC 7807 response

`result_problem_json(result)` returns a JSON Problem Details document with
`type`, `title`, `status`, `detail`, and an `errors` array. Each error contains
`field`, `code`, and `message`. Invalid results use HTTP `422` and
`application/problem+json`; valid results produce a `204` response.

The public language regression is
`tests/validation/339_api_validation.spectra`, and native constraint and
response-shape tests are in `packages/spectra-api/src/validation.rs`.
