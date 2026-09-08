# Real HTTP/3, gRPC, GraphQL, and Suspendable Coroutines

**Intent:** Replace the four remaining capability gaps with real protocol/runtime implementations: HTTP/3 over QUIC, interoperable gRPC over HTTP/2, executable GraphQL schemas, and lazy stackless suspension for `async`/`await`.

**Current Behavior:**
- `spectra-api` has real HTTP/1.1, HTTP/2, TLS, WebSocket, SSE, and JSON, but no HTTP/3/QUIC module or dependency.
- There is no gRPC framing, metadata/trailer/status implementation, protobuf message transport, or public gRPC binding.
- There is no GraphQL parser, schema validator, executor, introspection, resolver, or subscription binding. Existing JSON/router/OpenAPI code is not a GraphQL implementation.
- `async func`, `async {}`, and `await` parse and type-check, but async bodies execute eagerly. `await` lowers to blocking `task.wait` + `task.result`; the IR has no suspend/resume operation or persistent coroutine frame.

**Expected Outcome:**
1. A real HTTP/3 server and client use QUIC/TLS 1.3 and the `h3` protocol implementation, support independent request streams, incremental DATA, trailers, flow control, graceful GOAWAY/drain, cancellation, and localhost interoperability tests.
2. A real gRPC transport supports the HTTP/2 wire contract: `POST /package.Service/Method`, `application/grpc(+proto)`, 5-byte message envelopes, metadata, `grpc-timeout`, status trailers, percent-encoded messages, cancellation/reset, unary, client-streaming, server-streaming, and bidi-streaming. The public language API transports opaque protobuf byte messages without pretending to provide runtime reflection.
3. A real GraphQL dynamic schema supports query, mutation, subscription, variables/defaults, aliases, directives, fragments, validation, standard introspection, resolver callbacks, GraphQL error paths/locations, JSON response serialization, and HTTP integration. Subscription transport uses an explicit stream API; it is not silently reduced to a one-shot query.
4. Async functions, async blocks, and async closures are lazy constructors. Each call returns a generational `Task<T>` backed by a private heap frame. Polling executes ordinary compiled state-machine blocks, stores values live across each await, returns `Pending` without blocking, resumes from the recorded state after a child wake, and drops initialized frame fields exactly once on cancellation/error/completion. Async closures must infer `Task<T>` rather than the current plain `Fn` result.

**Target-Perspective Output:** A Spectra user can write and run `.spectra` fixtures that start an HTTP/3 endpoint, issue a real QUIC request, execute a gRPC call against an independent HTTP/2 peer, execute GraphQL queries/mutations/subscriptions with resolvers and introspection, and prove that a pending async function does not execute before polling and resumes after a manually controlled child task completes.

**Truth Owner:**
- `packages/spectra-api`: HTTP/3, gRPC, GraphQL wire/protocol behavior and host handles.
- `compiler` + `midend` + `backend`: async state-machine construction, lowering, verification, code generation, and debug metadata.
- `runtime`: task-frame ownership, poll/wake/cancel lifecycle, ABI-safe callback dispatch, and reactor integration.
- `packages/spectra-contract` plus compiler/midend binding tables: public contract synchronization.

**Contract Boundary:** Protocol bytes use bounded, generational runtime-owned handles: `Http3Buffer`, `GrpcMessage`, and `GrpcStream` are represented as `Type::Int` in the current language ABI but are validated in distinct handle domains; `message_from_base64`/`message_to_base64` are the binary-safe public boundary. GraphQL requests/responses remain UTF-8 JSON strings because GraphQL itself is UTF-8 JSON. GraphQL subscriptions use a distinct generational `GraphqlSubscription` handle represented as `Type::Int`, with typed `next`, `pending`, `cancel`, and bounded-capacity/backpressure operations; it is never represented as a one-shot response or an unbounded vector. Raw frame pointers never cross the Spectra host ABI.

The coroutine callback ABI is internal to the runtime/backend, not a user-visible host call: `unsafe extern "C" fn(*mut c_void, i64, *mut AsyncPollContext) -> i32`, where the first argument is a runtime-owned frame pointer, the second is the generational task handle, and the third is a runtime-owned poll context. Status values are `0=Pending`, `1=Ready`, `2=Failed`, `3=Cancelled`; typed result/error data stays in the unified task record. Runtime polling marks a record `Polling`, releases the task-registry lock before invoking generated code, and reacquires it with a generation check. This prevents generated poll code from deadlocking when it invokes child tasks or host calls. The matching private drop ABI is `unsafe extern "C" fn(*mut c_void, i64, i32)`.

**Cutover:**
- Add the real protocol modules and replace the HTTP/3 decision-only path with working `http3` APIs; keep unsupported QUIC features explicit rather than advertising them.
- Add gRPC and GraphQL namespaces to all binding/catalog layers; remove any decision-only or absent-feature wording from the public capability contract.
- Supersede ADR 0015 with a new coroutine ADR. Replace eager `AsyncReady` construction for async functions/blocks with ramp/frame/poll/drop generation. Keep `block_on` as a compatibility executor over the unified poll contract, not as the implementation of `await`.
- Migrate the existing scalar/background `AsyncTask` records and all producers/consumers (`ready`, `poll`, `wait`, `result`, `join`, `cancel`, scopes, timeouts, streams) to one task-record owner with an optional coroutine frame. Do not introduce a second task-handle domain.
- Update `validate_r2103_async_lowering.py`, `validate_r2109_async_test_runtime.py`, all async IR snapshots, and ADR references in the same cutover.

**Displaced Path:**
- HTTP/3 `R-2406` decision/deferred-only evidence.
- Missing gRPC/GraphQL namespaces and any placeholder/501-only claims for their public surfaces.
- `lowering_expr_tail.rs` await path that calls blocking `task.wait`; eager `wrap_async_return_value` for async bodies.
- Runtime task records that only store a scalar result and have no child waiter/waker/frame ownership.

**HTTP/3 release entry criteria:** The implementation must update `docs/adr/0014-http3-quic-decision.md` from deferred to superseded/implemented only after two independent HTTP/3 peers, QUIC connection migration coverage, flow-control/cancellation/shutdown tests, and cross-target compile/test evidence for Linux epoll, Windows IOCP, macOS/BSD kqueue are recorded. A localhost-only test is useful development evidence but is not the release gate. The implementation must also document its resource/performance budget against HTTP/2 and preserve TLS, SSRF, timeout, and observability contracts.

**Integration Constraints:** Protocol cores may be developed in isolated files only. Changes to `Cargo.toml`, `lib.rs`, host registries, compiler public builtins, midend host descriptors, router adapters, and shared HTTP/2 types are serialized under one integration owner. The coroutine migration must use the existing generational task handle table as its single owner: legacy ready/background records become records with `frame: None`, generated coroutines use `frame: Some(...)`, and every existing wait/result/join/cancel/scope/timeout consumer is migrated before the old scalar-only record is removed. GraphQL resolver callbacks run on a dedicated worker executor and receive a request-scoped `Arc` context; no GraphQL resolver is invoked synchronously on the HTTP/2 or HTTP/3 event-loop thread.

**Runtime value ownership:** A frame slot is an internal `AsyncFrameSlot { value: SpectraHostValue, initialized: bool, drop_glue: SlotDropFn }`; strings, aggregates, tensors, and protocol handles are released through their registered drop glue. The result path uses an owned `AsyncResultStorage` plus typed result/drop metadata, so pointer-backed values never outlive the frame and no `i64` is reinterpreted as an unowned pointer. The internal poll ABI is `unsafe extern "C" fn(*mut c_void, i64, *mut AsyncPollContext) -> i32`; the drop ABI is `unsafe extern "C" fn(*mut c_void, i64, i32)`, where both function pointers stay private to the runtime/backend and the context exposes only runtime-owned result/error setters.

**Value Density:** Protocol correctness and coroutine lifecycle correctness are the highest-risk boundaries. Implement transport cores first, then language adapters, then end-to-end fixtures. Do not add generated-code promises, protocol versions, or scheduler behavior that is not tested on the actual target surface.

**Acceptance Evidence:**
- HTTP/3: `cargo test -p spectra-api --features http3` (or the final selected feature contract), a real localhost QUIC server/client round-trip, incremental request/response body plus trailers, flow-control/backpressure test, cancellation and graceful shutdown test, and a `.spectra` host fixture.
- gRPC: Rust integration tests against an independent `h2`/tonic peer for all four RPC cardinalities, metadata and status trailer assertions, percent-encoded `grpc-message` round-trip plus malformed-percent rejection, timeout/reset test, malformed-envelope rejection, binary payload round-trip through `GrpcMessage`, and a `.spectra` unary plus streaming fixture.
- GraphQL: dynamic schema tests for query/mutation/subscription, variables/directives/fragments, introspection, validation failures with paths/locations, resolver callback invocation on a worker executor, HTTP POST/GET behavior, subscription stream delivery, bounded backpressure, cancellation on disconnect, and a `.spectra` fixture.
- Coroutines: compiler/midend IR snapshot containing ramp/poll/drop states, JIT and AOT execution of a pending-child fixture, no-eager-side-effect assertion, multiple-await/branch/loop tests, cancellation/drop counter tests, Send/Sync rejection tests, task-registry reentrancy tests, and race-free concurrent polling tests.
- Whole project: `cargo build --workspace`, `cargo test --workspace`, feature checks for HTTP/3, ONNX, and GPU combinations, protocol fixtures, coroutine fixtures, and the project validation harness.

**Evidence Lane:** Direct observable behavior first (wire bytes, status/trailers, task state transitions and side effects), then crate tests and IR snapshots, then workspace/full validation. External live services are not required: use deterministic localhost QUIC and HTTP/2 peers, but assert actual sockets/transport state rather than in-process dispatch shortcuts.

**Kill Criteria:**
- No protocol module may report success without validating its wire framing and transport state.
- No `await` implementation may call a blocking wait, spin loop, or execute the async body on constructor entry.
- No frame may use a `ManualAlloc` whose lifetime ends at the caller's manual frame; frame ownership must be tied to the task record.
- No state-machine pass may inline, DCE, reorder, or drop a value across a suspension boundary without an explicit test.
- No duplicate protocol truth may exist between a new implementation and an old fake/decision-only path; redirect or remove the old path in the same change.

**Non-goals:** HTTP/3 server push, WebSocket-over-HTTP/3 extended CONNECT, gRPC reflection/code generation, GraphQL federation/defer/@stream unless the selected crate supports them without weakening the core contract, and preemptive stackful fibers. These remain explicit unsupported capabilities, not simulated success paths.

**Risk if wrong:** A protocol adapter that only serializes happy-path bytes can appear complete while failing interoperability, stream cancellation, trailers, or flow control. A coroutine transform that misses liveness/drop state can corrupt values, leak handles, double-drop resources, or resume a completed task. Every acceptance gate therefore exercises the boundary from the target user's perspective.
## Architecture Slice

### Files to create
- `packages/spectra-api/src/http3.rs` and focused HTTP/3 tests.
- `packages/spectra-api/src/grpc.rs` and focused gRPC tests/peer helpers.
- `packages/spectra-api/src/graphql.rs` and focused GraphQL tests.
- `packages/spectra-api/src/bindings/http3.spectra`, `grpc.spectra`, `graphql.spectra` where the package binding convention requires them.
- Runtime coroutine frame/registry module, e.g. `runtime/src/async_frame.rs`.
- Midend async state-machine lowering/verification module(s), e.g. `midend/src/async_lowering.rs`.
- `docs/adr/0016-stackless-coroutine-execution.md` superseding ADR 0015.
- Protocol and coroutine `.spectra` fixtures under `tests/validation/` plus integration fixtures under package tests.

### Files to modify
- `packages/spectra-api/Cargo.toml`, `src/lib.rs`, `src/api_registration.rs`, host registry, and the HTTP/2 stream/body/trailer adapter.
- `compiler/src/semantic/builtin_api_core.rs`, `builtin_contract.rs`, async semantic CFG liveness/drop checks, and language service snapshots.
- `midend/src/lowering_std_host_tensor_ml.rs` or its API split, `midend/src/ir.rs`, builder, lowering, verifier, DCE/inlining/tail-call/escape/drop passes, and pretty printer.
- `backend/src/codegen_instruction_*`, function declaration maps, JIT/AOT function-pointer/data relocation handling, and debug location emission.
- `runtime/src/stdlib/async_*`, the unified task-record/frame registry, reactor wake queues, ABI catalog, and host binding constants.
- `packages/spectra-contract/catalog/stdlib.toml` and API contract/catalog tests.
- `scripts/validate_r2406_http3_decision.py`, async validation scripts, fixtures, and ADR/capability documentation.
- Relevant validation scripts and docs/ADR references.

### Files to avoid
- Existing HTTP/1.1 parser behavior unless a shared type change requires a narrowly tested adapter.
- Existing HTTP/2 server semantics except adding reusable stream/body/trailer primitives required by gRPC.
- Public Task handle encoding and manual-memory frame-zero semantics; extend through a new domain/registry rather than changing existing handle meaning.

## Ordered Tasks

**Suspension safety matrix:** The implementation must explicitly audit and test `constant_folding`, `concurrent_spawn_join_fusion`, `function_inlining`, dead-code elimination, tail-call marking, alloca/escape analysis, scope-drop emission, IR verification, pretty printing, and native debug-location capture. Any pass that cannot prove suspension-safe behavior must reject or skip the poll function rather than transform across an await.
1. **Foundation contracts and dependencies** (serial). Pin `h3=0.0.8`, `h3-quinn=0.0.10`, `quinn=0.11.11`; select `async-graphql=7.2.1` with `dynamic-schema`; use `h2=0.4` and `prost=0.13` for the gRPC raw codec/interoperability lane. Add bounded message/stream/error handle types and contract entries before host wiring. No feature is advertised until its runtime registration and catalog entry exist.
2. **HTTP/3/QUIC transport core** (parallel with the isolated gRPC/GraphQL source files only after task 1). Implement Quinn endpoint/TLS ALPN `h3`, h3 server/client connections, request/response DATA and trailers, stream flow control, cancellation, GOAWAY/drain, and deterministic localhost certificate tests. Shared manifests, exports, router adapters, and host tables are edited in a serialized integration step.
The Cargo contract is explicit: `http3 = ["dep:quinn", "dep:h3", "dep:h3-quinn"]` is enabled by default for `spectra-api`; direct `quinn` enables `runtime-tokio`, `rustls-ring`, and `ring`; the effective workspace minimum supported Rust version becomes 1.89 (the stricter requirement from `async-graphql` 7.2.1; Quinn itself requires 1.85). `async-graphql` is pinned to 7.2.1 with `dynamic-schema`; the workspace's current stable toolchain and the 1.89 MSRV lane must be recorded in CI. `prost` is used for test/interoperability messages while the public gRPC message handle preserves arbitrary bytes.
3. **gRPC transport core** (parallel only in its isolated source/tests after task 1). Implement incremental gRPC envelopes, metadata/trailers/status/deadline/reset, unary/client-stream/server-stream/bidi APIs, and independent-peer interop tests. Do not use UTF-8 strings for arbitrary protobuf bytes.
4. **GraphQL schema and executor** (parallel only in its isolated source/tests after task 1). Integrate `async-graphql` dynamic schema, register scalar/object/input/enum/interface/union fields, attach runtime-owned resolver callbacks through a worker executor, expose request variables/operation selection, collect error paths/locations, support introspection and subscription streams, and adapt HTTP request/response bodies without blocking the server loop.
5. **Unified coroutine task record** (serial before lowering). Extend the current AsyncTask owner rather than adding a second handle domain: state index, initialized-slot/drop bitmap, poll/drop callbacks, result/error storage, child waiter/waker, cancellation, affinity, and one-poll-at-a-time guard. Implement the internal poll ABI above, release registry locks around callbacks, and retain `wait`/`block_on` as compatibility drivers over polling.
6. **Async state-machine lowering** (serial after task 5; protocol tests may proceed independently). Split async ramp from poll body. Compute CFG liveness at every await, promote live values into frame slots, generate dispatch/state blocks, replace await with child poll/subscribe and `Pending` return, reload on resume, and generate state-specific drop cleanup for normal return, error/`?`, cancellation, and partial initialization. Define and enforce async block return/break/continue boundaries.
7. **Backend and optimization integration** (serial after task 6). Add frame memory operations, poll/drop function declarations, relocatable `FuncAddr` handling for JIT/AOT, non-inlineable suspension barriers, DCE/escape/drop/verification support, and source/debug mapping for state/frame slots. Validate reentrancy and AOT link ownership before enabling optimizations.
8. **Serialized public binding integration** (after protocol cores and coroutine runtime). Add compiler semantic signatures, midend descriptors, runtime registrations, package bindings, and contract catalog entries. Expose opaque byte/stream/task handles with fixed typed arity; update shared exports only after each implementation's direct tests pass.
9. **End-to-end fixtures and evidence affordances** (after tasks 2–8). Add real-wire protocol fixtures and coroutine state tests; include negative cases for malformed frames, trailers/status errors, GraphQL validation, cancellation, no-eager execution, double poll, drop, and callback reentrancy.
10. **Cutover/documentation cleanup** (last implementation step). Supersede ADR 0015, remove decision-only HTTP/3 checks and eager-await wording, update named validation scripts/fixtures, and remove obsolete fake/placeholder paths.
11. **Verification gates** (serial): build all feature configurations; run protocol integration tests; run coroutine/JIT/AOT tests; run workspace and full validation; inspect diff/fixtures and generated artifacts.

**Plan Review Gate:** Requires PRE review before execution. Do not modify implementation files until the plan reviewer reports aligned or all blocker findings are explicitly resolved.
