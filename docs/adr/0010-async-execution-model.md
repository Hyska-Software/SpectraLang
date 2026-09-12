# ADR 0010: Async/Await Execution Model

Status: Superseded by docs/adr/0015-async-execution-model.md (execution
model only; syntax surface, types, cancellation, and Send/Sync rules below
remain normative)

Date: 2026-06-15

Roadmap item: R-2101

## Context

SpectraLang is extending from AI/ML core workloads into native API and
event-driven service development. The API workstream depends on a first-class
asynchronous language model before HTTP servers, clients, WebSocket, SSE,
database drivers, streaming inference, and model-serving runtimes can be
implemented without callback-heavy APIs or runtime-specific special cases.

The async model must be stable before parser, lowering, reactor, and API
surface work begins. This ADR freezes the public syntax, logical types,
lowering contract, scheduler boundary, cancellation rules, and sendability
rules for Phase 21.

## Decision

Spectra uses stackless async functions lowered by the compiler into explicit
state-machine SSA. The runtime drives these state machines through a polling
scheduler integrated with a platform reactor.

The accepted model is:

- `async func` declares a function that starts no work when called.
- Calling an `async func` returns a `Task<T>` future handle.
- `async { ... }` creates an async block and returns `Task<T>`.
- `await expr` is an expression that suspends the current async state machine
  until `expr` is ready, then yields its output value.
- `Stream<T>` is the asynchronous sequence type. Pulling the next element is an
  async operation that returns `Option<T>`.
- `Pin<T>` is not exposed as a user-facing API in the initial language surface.
  The compiler and runtime own pinning internally through immovable task
  frames.
- Async closures are deferred until the async function and async block model is
  implemented and validated.

This is a polling model, not a callback model. Callbacks may exist as adapter
APIs, but they are not the semantic foundation of async execution.

## Public Syntax Surface

The Phase 21 syntax surface is:

```spectra
async func fetch_user(id: int) returns User {
    let response = await http_get("/users/" + id.to_string())
    return decode_user(response)
}

func handler(id: int) returns Task<User> {
    return fetch_user(id)
}

let task: Task<int> = async {
    let left = await read_left()
    let right = await read_right()
    return left + right
}
```

Rules:

- `async` is valid before `func` and before a block expression.
- `await` is valid only inside an async context.
- `await` binds to the following expression.
- `async func f(...) returns T` has call type `func(...) returns Task<T>`.
- `async { ... }` has type `Task<T>`, where `T` is inferred from block returns.
- `Stream<T>` exposes async pull semantics through `next(stream) -> Task<Option<T>>`.
- Synchronous functions may create and pass tasks but may not use `await`.

## Logical Types

`Task<T>` is the logical future type for a single eventual result.

`Stream<T>` is the logical future-backed sequence type.

`Task<T>` states:

- pending: the task is suspended and requires a wakeup or scheduled poll.
- ready: the task completed with a value of type `T`.
- failed: the task completed with a runtime error or propagated diagnostic.
- cancelled: the task was cancelled before producing a value.

`Stream<T>` states:

- pending: the next item is not available yet.
- item: the next item is available.
- done: the stream is exhausted.
- failed: the stream failed.
- cancelled: stream consumption was cancelled.

Task and stream handles are runtime-managed values. They are not raw OS handles
and are not exposed as pointer-like integers at the language level.

## Lowering Contract

Every async function and async block lowers to a compiler-generated state
machine. The generated task frame contains:

- the current state id;
- arguments and captured locals that live across an `await`;
- temporary values live across suspension points;
- cancellation state;
- result storage;
- panic or host-error status storage;
- debug metadata for diagnostics and stack traces.

Lowering rules:

- each `await` becomes an explicit suspend/resume boundary;
- locals not live across an `await` remain ordinary SSA values;
- locals live across an `await` are promoted into the task frame;
- early `return` writes the result and transitions to ready;
- errors transition to failed with structured diagnostic metadata;
- cancellation checks are inserted at every suspension boundary and at runtime
  calls that declare themselves cancellation points.

The midend represents async lowering with explicit suspend, resume, ready, and
cancel edges before backend codegen. The backend compiles the state machine as
ordinary functions plus runtime scheduler calls; it does not implement async
semantics independently.

## Scheduler Interface

The runtime scheduler polls tasks through a stable internal ABI:

```text
poll(task_handle, scheduler_context) -> PollStatus
```

`PollStatus` variants:

- `Pending`: task saved its frame and must be resumed by wakeup.
- `Ready`: task produced a result.
- `Failed`: task produced a structured runtime failure.
- `Cancelled`: task observed cancellation.

The scheduler owns:

- ready queue management;
- wakeup registration;
- timer integration;
- cancellation propagation;
- parent-child task scopes;
- executor metrics;
- bridging to the platform reactor.

The reactor owns OS event readiness only. It does not know Spectra language
types. Phase 21 requires reactor abstraction points for `epoll`, `IOCP`, and
`kqueue`; concrete platform backends are R-2104.

## Structured Concurrency

Async tasks are structured by default. A task spawned inside a scope is a child
of that scope unless explicitly detached through a future item that will define
detached task policy.

Rules:

- parent scopes wait for children unless the child is cancelled or detached by
  an explicit API;
- leaving a scope with live children cancels those children deterministically;
- a failed child propagates failure to the scope unless handled by an explicit
  join or result API;
- scheduler APIs must preserve parent-child relationships for diagnostics and
  cancellation.

Unstructured background tasks are not part of the initial Phase 21 contract.

## Cancellation

Cancellation is cooperative and deterministic.

Rules:

- cancellation requests set a task cancellation flag;
- a task observes cancellation at `await`, timer, I/O, channel, and explicit
  cancellation-check points;
- cancellation propagates from parent scope to child tasks;
- cleanup code runs before a task reaches cancelled when the language surface
  supports cleanup constructs;
- cancelling a completed task is a no-op;
- cancellation is represented distinctly from normal failure.

Timeouts are defined as cancellation sources layered on top of timers:

```spectra
let value = await with_timeout(seconds(5), fetch_user(id))
```

`with_timeout` is implemented in R-2105 and must use the same cancellation
propagation contract defined here.

## Send and Sync Rules

Spectra uses explicit sendability rules for async values.

Definitions:

- `Send` means a value may move between executor threads.
- `Sync` means shared references to a value may be used by multiple executor
  threads.
- A task is `Send` only if every value live across an `await` is `Send`.
- A task is `Sync` only if its shared state is `Sync`.
- Non-sendable tasks are allowed, but they are bound to the executor lane where
  they were created.
- Formal evidence is expressed in the type system through `T: Send`,
  `T: Sync`, `dyn Trait + Send`, and `dyn Trait + Send + Sync`.

Compiler diagnostics must report the exact value that prevents a task from
being sendable. The compiler may reject APIs that require `Task<T>: Send` when
the lowered state frame contains non-sendable values.

R-2112 makes this evidence explicit for generic parameters and dynamic trait
objects. Unconstrained `T` is not treated as `Send` by async validation.
Plain `dyn Trait` remains backward-compatible with the existing async
trait-object model, while `dyn Trait + Send` and `dyn Trait + Sync` carry
formal evidence for APIs that require explicit auto-trait bounds.

The first production executor may be single-threaded, but the type and
diagnostic rules must not pretend all tasks are sendable. This prevents a later
multi-threaded executor from becoming a breaking semantic change.

## Pinning and Frame Stability

Spectra does not expose user-facing `Pin<T>` in Phase 21. Async task frames are
runtime-managed and stable while the task is scheduled.

Rules:

- user code cannot move or inspect task frames;
- the runtime may relocate a task only at safe points where no self-reference
  or reactor registration would be invalidated;
- if self-referential frames become necessary, pinning remains internal and is
  represented in runtime metadata;
- APIs must not require user-authored pin projections.

The public language stays simpler while preserving a sound internal frame
stability model.

## Blocking and Host Calls

Async contexts must not block the executor lane with long synchronous host
calls. Host calls used by async code are classified as:

- nonblocking: safe to call directly from async tasks;
- cancellation point: safe to suspend or observe cancellation;
- blocking: must run through `spawn_blocking` or an equivalent dedicated worker
  API;
- forbidden in async: rejected by semantic diagnostics.

This classification is mandatory for filesystem, TCP, database, model loading,
and inference APIs before they are exposed as async standard library functions.

## Diagnostics

The compiler and runtime must provide stable diagnostics for:

- `await` outside async context;
- `async` in unsupported syntactic positions;
- task result type mismatch;
- non-sendable value captured across `await` where `Send` is required;
- blocking host call used directly in async context;
- cancellation propagated through a task scope;
- task polled after completion;
- leaked or detached task without explicit policy.

Diagnostics must include stable error codes once R-2110 begins implementation.
R-2110 reserves `E2101` through `E2120` for async diagnostics. The first
implemented Send/Sync gates are:

- `E2101`: a non-`Send` value is live across an `await`;
- `E2102`: a `RefCell`/interior-mutable value is held across an `await`;
- `E2103`: a `!Send` value crosses a spawn-style task boundary.
- `E2104`: formal `Send`/`Sync` evidence is missing for a generic bound or a
  `dyn Trait + Send/Sync` object.

## Consequences

- R-2102 must implement parser and AST support for `async func` and `async {}` as
  specified here.
- R-2103 must lower async constructs to explicit state-machine SSA rather than
  backend-specific callback code.
- R-2104 must implement the reactor behind the scheduler interface, not as a
  language semantic layer.
- R-2105 must implement cancellation and timeout APIs using the parent-child
  propagation contract in this ADR.
- R-2107 async stdlib APIs must classify host calls before exposing them to
  async Spectra code.
- R-2201 and later API-platform work may rely on `Task<T>`, `Stream<T>`, and
  structured concurrency as stable design inputs.

## Rejected Alternatives

Stackful coroutines were rejected because they complicate portable runtime
integration, stack management, diagnostics, and host embedding.

Callback-first async was rejected because it would make the public API platform
less ergonomic and would duplicate control-flow semantics already represented
by `async` and `await`.

User-facing `Pin<T>` was rejected for Phase 21 because it exposes internal
runtime frame mechanics before Spectra has a mature ownership and lifetime
surface.

Implicit detached tasks were rejected because they make cancellation,
observability, and service shutdown unreliable.

## Acceptance Evidence

The Phase 31 `fanout_fanin_real_concurrency.v2` fixture also validates the
runtime boundary of this model. Its `std.concurrent` batch compatibility path
uses a persistent worker pool, registers multiple pending executable units
before fan-in, and never creates one OS thread per task. This scheduler evidence
does not replace the language-level stackless state-machine lowering defined by
this ADR.

- This ADR fixes the Phase 21 syntax surface for `async func`, `async {}`,
  `await`, `Task<T>`, `Stream<T>`, and the absence of public `Pin<T>`.
- This ADR defines stackless state-machine SSA lowering and the scheduler
  polling interface.
- This ADR defines structured concurrency, cancellation propagation, and
  `Send`/`Sync` rules.
- `scripts/validate_r2101_async_adr.py` checks that the required design
  decisions remain present.
- `run_tests.ps1` includes the R-2101 validation gate.
