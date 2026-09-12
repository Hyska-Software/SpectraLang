# GraphQL dynamic schema and execution

R-2423 tracks GraphQL support. The adapter is implemented in
`packages/spectra-api/src/graphql.rs` with the Spectra host adapter in
`packages/spectra-api/src/graphql_host.rs` and host registrations in
`host_calls.rs` (`spectra.api.graphql.*`).

Parsing, validation, execution, and introspection live in `async-graphql
7.2.1` (`dynamic-schema` feature). The adapter owns only the runtime-safe
callback boundary and the bounded subscription transport.

## Dynamic schema builder

`GraphqlSchemaBuilder` starts from `schema_new` and accumulates query,
mutation, subscription, and registered types (`Scalar`, `Object`,
`InputObject`, `Enum`, `Interface`, `Union` via `register_*`) before
`schema_finish` produces an owned, cheap-to-clone `GraphqlSchema`
(`schema_sdl` exposes the SDL; `schema_drop` discards either side).

- Constant fields: `schema_field_json` (root `0` = query, `1` = mutation; name,
  type reference such as `String!` or `[Int]`, JSON-encoded value).
- Computed fields: `schema_field_callback` (same roots; the value is an
  existing closure handle invoked through `invoke_callback` on a bounded
  worker executor, default 32 permits, tunable with `schema_set_workers`).
- Subscription fields: `schema_subscription_json` (fixed event list) and
  `schema_subscription_callback` (push-backed, see below).
- Type references are validated (`graphql_type` / `parse_graphql_type_ref`):
  empty names, whitespace, overlong names (> 256 bytes), and malformed
  wrappers are rejected with `INVALID_ARGUMENT`.
- Schema construction errors come from async-graphql's strict registry checks
  and surface as `INVALID_ARGUMENT` from `schema_finish`.

## Execution

- `execute` (query string + variables JSON), `execute_named` (plus operation
  name), and `execute_http` (standard GET/POST JSON-over-HTTP shape: method,
  target, content type, body) all return a response handle.
- Responses expose `response_json` (full `data` + `errors` payload),
  `response_data_json`, `response_errors_json`, `response_status` (HTTP status
  for the HTTP shape, `200` otherwise), `response_is_ok` (true when `errors`
  is absent or empty), and `response_drop`.
- Request-scoped values travel in `GraphqlContext`; resolver callbacks receive
  the field name, arguments, and context as an encoded JSON request and return
  a JSON-encoded value string (null becomes a GraphQL null result).

## Opt-in guards with permissive defaults

`GraphqlSchemaBuilder` carries `max_depth`, `max_complexity`, and
`introspection`, applied in `finish` through async-graphql's `limit_depth` /
`limit_complexity` / `disable_introspection`. The defaults preserve today's
behavior: unlimited depth, unlimited complexity, introspection on.

- `schema_set_max_depth`: positive integer caps nesting depth (counted the
  async-graphql way: every nested field level adds one); `0` restores
  unlimited. Over-depth queries are rejected at execution.
- `schema_set_max_complexity`: positive integer caps query complexity; `0`
  restores unlimited.
- `schema_set_introspection`: `0` disables `__schema` / `__type` queries; any
  other value keeps the default enabled behavior.

`schema_set_subscription_capacity` (non-zero, else `INVALID_ARGUMENT`) bounds
the subscription channel independently of these guards.

## Push subscriptions

Subscriptions use the existing `subscribe` / `subscription_next` /
`subscription_pending` / `subscription_capacity` /
`subscription_is_cancelled` / `subscription_cancel` / `subscription_drop`
ABI — no new host symbols. `subscribe` opens a `GraphqlSubscription` on the
schema; each `subscription_next` polls the bounded stream and returns either
a response handle or `0` when the stream is closed. Dropping the handle
cancels the producer.

Callback-backed subscriptions are channel-fed pulls
(`subscription_from_callback`): every poll whose buffer is empty re-invokes
the host callback for the next batch (a JSON array string) and appends it to
an `mpsc` channel. A callback that returns an empty array closes the stream,
so two consecutive polls can deliver two distinct event batches and `cancel`
ends the stream. A non-array return or unrepresentable value is a stream
error, not a silent close.

## Boundary

The adapter does not implement its own parser, validator, or introspection
engine; those are async-graphql's. It does not promise persisted queries,
query caching, federation, or a `.graphql` code generator. Depth, complexity,
and introspection guards are opt-in per schema and off by default.

## Host surface

All `spectra.api.graphql.*` hosts (`host_calls.rs`, 29 total):
`spectra.api.graphql.schema_new`, `spectra.api.graphql.schema_set_workers`,
`spectra.api.graphql.schema_set_subscription_capacity`,
`spectra.api.graphql.schema_set_max_depth`,
`spectra.api.graphql.schema_set_max_complexity`,
`spectra.api.graphql.schema_set_introspection`,
`spectra.api.graphql.schema_field_json`,
`spectra.api.graphql.schema_field_callback`,
`spectra.api.graphql.schema_subscription_json`,
`spectra.api.graphql.schema_subscription_callback`,
`spectra.api.graphql.schema_finish`, `spectra.api.graphql.schema_drop`,
`spectra.api.graphql.schema_sdl`, `spectra.api.graphql.execute`,
`spectra.api.graphql.execute_named`, `spectra.api.graphql.execute_http`,
`spectra.api.graphql.response_json`, `spectra.api.graphql.response_status`,
`spectra.api.graphql.response_is_ok`,
`spectra.api.graphql.response_errors_json`,
`spectra.api.graphql.response_data_json`, `spectra.api.graphql.response_drop`,
`spectra.api.graphql.subscribe`, `spectra.api.graphql.subscription_next`,
`spectra.api.graphql.subscription_pending`,
`spectra.api.graphql.subscription_capacity`,
`spectra.api.graphql.subscription_is_cancelled`,
`spectra.api.graphql.subscription_cancel`,
`spectra.api.graphql.subscription_drop`.
