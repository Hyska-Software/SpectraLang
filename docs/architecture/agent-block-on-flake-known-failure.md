# Known flake: `block_on` failure from a contended nested dispatch (fixed)

Status: **root cause fixed 2026-09-13**; the flake has not been observed since.

## What was observed

One certification-gate run stopped the fixture set with:

```
fixture set stopped at item 7: fixture nested_dispatch (AOT) exited 101:
runtime error: host call 'spectra.async.task.block_on' failed
```

The same fixture passed its JIT half in that run, and later gate runs passed. The
message is a single interned literal, so the failure could not be localised: the
AOT binary of the fixture has five `block_on` call sites (its flow plus one per
tool wrapper).

## Root cause

`spectra_rt_coroutine_poll_child` (`runtime/src/async_abi.rs`) mapped
`AsyncPollOutcome::AlreadyPolling` — and `AffinityRejected` — to
`AsyncPollStatus::Failed`:

```rust
Ok(AsyncPollOutcome::Failed | AsyncPollOutcome::AlreadyPolling) => Failed,
Ok(AsyncPollOutcome::Stale | AsyncPollOutcome::AffinityRejected) => Failed,
```

`AlreadyPolling` is not failure: it means another context is *inside* the child
right now. That is expected in this runtime, because a tool dispatch always runs
as a background task on a four-worker pool
(`packages/spectra-agent/src/hosts.rs`, `write_run_task`) whose body drives the
tool's coroutine with `block_on`, while the caller waits on the same task tree.
When the caller's poll arrived while the worker was inside the child, the parent
saw "failed", the awaiting coroutine failed, the flow's `block_on` returned a
failure status, and the backend printed the uninformative message above.

The neighbouring paths already treat it as contention: `std_async_task_join` and
`std_async_task_join_status` both map `AlreadyPolling` to "pending" (3). The ABI
was the outlier.

## Fix

- `runtime/src/async_abi.rs`: the mapping is now the pure function
  `child_poll_status`; `AlreadyPolling` and `AffinityRejected` map to
  `AsyncPollStatus::Pending`, `Stale` stays terminal, and
  `spectra_rt_coroutine_poll_child` calls it. The wait loop needs no change: it
  parks at most `TASK_WAIT_PARK` (50 ms) and re-polls, and the parent is woken by
  the child's subscription.
- `runtime/src/stdlib/async_task_stream.rs`: a failure now names itself.
  `block_on_task_value` and `std_async_task_block_on` print one line to stderr
  before returning the failing status —
  `spectra.async.task.block_on: task <id> failed error value <i64>` / `cancelled`
  / `missing` — and `spectra_rt_coroutine_poll_child` prints
  `spectra.async.task.poll_child: task <id> outcome failed|stale|unavailable`.
  The error value is read from the frame registry
  (`AsyncFrameRegistry::error_host_value`, new; `FrameRecord.error` was recorded
  by the coroutine ABI and never read) and printed as a plain scalar.
- `tests/validation/397_agent_nested_dispatch_stress.spectra`: repeats the
  nested-dispatch shape 50 times in one run (150 dispatches) as a standing
  regression case; registered in the R-3221 gate's fixture set.

## Evidence

Falsification, run before the fix was kept (the two arms temporarily reverted to
the old mapping): the new unit test
`runtime::async_abi::tests::concurrent_polls_report_contention_not_failure`
failed in several threads at once with

```
assertion `left != right` failed: a contended poll was reported as a failure
  left: 2
 right: 2
```

With the fix in place the same test passes, and so does
`child_poll_status_maps_every_outcome` (all eight outcomes).

Fixture-level reproduction, independently of the unit test: the fixture compiled
and run with the pre-fix binary exited 101 with exactly the observed line
(`runtime error: host call 'spectra.async.task.block_on' failed`), and after the
rebuild it exits 0 in both engines (JIT ≈ 7.8 s, AOT ≈ 7.6 s).

The observed gate failure matches this defect in every respect the report
preserves: same fixture, same engine, same message, and a shape whose whole point
is nesting a run inside a tool dispatch. The 44 clean standalone runs recorded
earlier are consistent with a race that needs the tool-dispatch worker to be
inside the child at the moment the caller polls.

## Triage when it recurs

`python scripts/stress_agent_block_on.py --iterations 300` repeats the fixture in
many processes (each in its own working directory) and saves the full output of
the first failing iteration under `target/r3290-stress/`. The runtime's new
`spectra.async.task.*` stderr lines name the task handle and whether it was
cancelled, failed (with the recorded error value) or missing, and the gate's
report keeps the last 25 lines of each engine's output.

There is no `SPECTRA_TRACE` environment variable in this repository — an earlier
version of this note said otherwise. Tracing is programmatic
(`set_trace_sink`), and its events do not cover task or run lifecycle.
