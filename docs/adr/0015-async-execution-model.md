# ADR 0015: Async Execution Model — Threads, Worker Pools, and Reactor-Park Waiting

Status: Accepted

Date: 2026-08-25

Roadmap items: R-2101, R-2103

Supersedes: the execution-model decision of
`docs/adr/0010-async-execution-model.md` (stackless state-machine lowering).
The syntax surface (`async func`, `async {}`, `await`), logical types
(`Task<T>`, `Stream<T>`), structured-concurrency rules, cancellation contract,
and `Send`/`Sync` rules defined by ADR 0010 remain normative. Only the
execution strategy below replaces ADR 0010's polling state-machine model.

## Context

ADR 0010 fixed the async surface around stackless async functions lowered into
an explicit suspend/resume state machine in SSA. The IR carried that intent as
dedicated marker instructions:

- `InstructionKind::AsyncSuspend { task, state }`
- `InstructionKind::AsyncResume { task, state }`

In practice these markers never acquired semantics. The backend emitted no code
for them (they were pure no-op arms in codegen, DCE, verification, pretty
printing, and alloca/escape analysis), and every real waiting behavior of
`await` was already carried end-to-end by host calls:

- `spectra.async.task.wait`: parks the calling lane inside the reactor until
  the task reaches a terminal state (`complete_task` / `fail_task` /
  `cancel_task` all enqueue a reactor wake event, so there is no CPU spin);
- `spectra.async.task.result`: re-checks the task and surfaces cancellation or
  failure as a runtime error;
- `spectra.std.concurrent.task_spawn_fn`: dispatches a real JIT closure with a
  capture onto the persistent executor worker pool, giving true parallelism
  today through the existing closure ABI.

This ADR closes the architectural question the markers left open: how does
Spectra execute asynchronous work?

## Decision

Spectra's async execution model is **thread/worker-pool based concurrency with
efficient waiting via reactor-park**:

1. Real parallelism comes from OS threads managed by a persistent executor
   worker pool. Closures are dispatched onto the pool through
   `spectra.std.concurrent.task_spawn_fn` using the existing closure ABI
   (`spectra_rt_invoke_closure` over a `{code pointer, capture}` object); the
   pool is reused across tasks, never one OS thread per task.
2. Waiting is efficient and cooperative at the lane level: an `await` lowers to
   a `spectra.async.task.wait` host call that parks the current lane inside the
   platform reactor (epoll / IOCP / kqueue) and is resumed by a reactor wake
   event when the task completes, fails, or is cancelled. No polling loop, no
   CPU spin.
3. `await` keeps its single lowering shape: `task.wait` followed by
   `task.result`. The IR contains only `AsyncReady` plus ordinary host calls;
   there are no suspend/resume boundary instructions.
4. `Send`/`Sync` evidence stays statically verifiable: tasks move between pool
   lanes only when the values live across an `await` satisfy the sendability
   rules of ADR 0010, checked by the compiler diagnostics `E2101`–`E2104`.

## Why Preemptive Suspension / Stackless Coroutines Was Rejected

The alternative — resuming ADR 0010's state-machine direction and eventually
compiling each `await` into a split control-flow graph with resumable frames —
was rejected for three reasons:

1. **Split-CFG transform cost over an IR without generators.** SpectraLang's IR
   has no generator/yield construct to build on. Implementing stackless
   coroutines requires a whole-function CFG splitting pass (identify values
   live across suspension points, promote them into a heap frame, cut blocks at
   every boundary, thread state ids through PHI nodes), plus changes to every
   pass that walks the IR (inlining, tail-call fusion, DCE, verification,
   escape analysis). That is a permanent complexity tax on the midend and
   backend in exchange for behavior the host-call path already delivers.
2. **The closure ABI already resolves real parallelism.**
   `concurrent.task_spawn_fn` runs actual JIT closures concurrently on the
   worker pool; fixtures such as `fanout_fanin_real_concurrency.v2` prove
   multi-worker fan-out/fan-in without one-thread-per-task. Stackless
   coroutines add *concurrency structure*, not additional parallelism, and the
   structured-concurrency requirements are met by the scope/cancellation rules
   Spectra already enforces.
3. **Static Send/Sync verifiability is preserved.** With explicit host-call
   boundaries and value-level sendability checks, what crosses a lane is known
   at compile time. Split-CFG resumption would move liveness decisions into a
   transform whose correctness (what is captured in the resumable frame, when
   self-references appear) must be re-proven per version, weakening the simple
   static story documented in ADR 0010 and enforced since R-2110.

## Consequences

- The dead markers `InstructionKind::AsyncSuspend` / `AsyncResume` are removed
  from the IR and from every pass arm, backend dispatch, builder helper
  (`build_async_suspend` / `build_async_resume`), and the lowering state-id
  counter. `await` emits only `task.wait` + `task.result`.
- Backend codegen for async shrinks to `AsyncReady` handling; DCE treats async
  effects exclusively through effectful host calls, which it already never
  removes.
- The reactor-park contract (`std_async_task_wait`, bounded park with wake
  events on completion/failure/cancellation) becomes the normative waiting
  mechanism and must be preserved by future scheduler work (R-2104+).
- Future work on preemptive scheduling or green threads must start from this
  ADR, not by resurrecting suspend/resume IR markers.
- Validation gates updated: `scripts/validate_r2103_async_lowering.py` and
  `scripts/validate_r2108_async_trait_objects.py` no longer expect
  `async.suspend` / `async.resume` in dumped IR.

## Alternatives Considered and Dismissed

- **Stackless coroutines (split-CFG state machines)** — rejected; see above.
- **Stackful coroutines (fiber-per-task)** — rejected: non-portable stack
  management, hostile to host embedding and to precise native debug info, and
  it hides sendability (any fiber can resume on any lane), contradicting the
  static `Send`/`Sync` story.
- **Callback-first async** — rejected in ADR 0010 and still rejected: worse
  ergonomics and duplicates control-flow semantics `await` already expresses.
- **Pure busy-wait join (no reactor)** — rejected: burns a full lane per pending
  await and does not scale to concurrent server workloads; reactor-park exists
  precisely to avoid this.

## Acceptance Evidence

- `cargo test -p spectra-midend`, `cargo test -p spectra-backend`,
  `cargo test -p spectra-compiler` pass after marker removal.
- `grep -rn "AsyncSuspend\|AsyncResume"` returns zero matches in Rust sources.
- Async validation fixtures `tests/validation/121_async_await_lowering.spectra`
  and `tests/validation/349_async_wait_out_of_order.spectra` keep passing under
  the central sweep with the reduced IR.
