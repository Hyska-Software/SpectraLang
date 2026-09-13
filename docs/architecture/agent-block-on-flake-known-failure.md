# Known flake: `block_on` failure inside a fixture (observed once, not reproduced)

Status: **open observation, recorded 2026-09-13**. Reproduced once, in the R-3221
gate, and not since.

## Observation

One certification-gate run stopped the fixture set with:

```
fixture set stopped at item 7: fixture nested_dispatch (AOT) exited 101:
runtime error: host call 'spectra.async.task.block_on' failed
```

The same fixture passed its JIT half in that run, and the next gate run passed
all seven checks. The fixture is
`tests/validation/390_agent_nested_dispatch.spectra`, whose AOT half drives three
tool calls through `block_on`, one of which starts a second run (`spawn`) and
ends it inside the tool body.

## Reproduction attempts (all clean)

- 20 sequential AOT runs of the compiled fixture: 0 failures.
- 24 AOT runs with 12-way parallelism, each in its own working directory: 0
  failures.
- 2 further full gate runs (`--jobs 4`): both passed.

So the trigger is either racy at a rate below roughly 1 in 50, or it depends on
machine state (the failing run overlapped other validator processes on the same
box).

## Triage when it recurs

`block_on` reports a failure when the task it drives does not reach completion:
the runtime protocol returns a failure status instead of a value. Collect, in
this order:

1. the gate report (`target/r3221-agent-conformance/report.json`), which holds
   the fixture's stdout for both engines;
2. whether the JIT half of the same fixture failed too (it did not, here);
3. `SPECTRA_TRACE=1` (the agent trace sink) around the failing run to see which
   task never completed.

The most likely place to look is the run registry while a tool body starts and
ends a second run: `spawn` is the only shape in this fixture that nests run
lifecycles inside a dispatch.

## Why it is recorded rather than fixed

The failure is not reproducible on demand, and the code path involved (task
protocol + run registry) has no candidate defect visible from the surface
contracts the other fixtures pin. Recording it keeps the observation available
without pretending a fix was validated.
