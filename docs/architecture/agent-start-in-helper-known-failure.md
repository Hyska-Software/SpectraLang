# Known failure: `agent_start` behind a helper called from an async function

Status: **open, pre-existing, reproduced on 2026-09-13**. It is independent of
the `std.agent` list support added in the same change and of the aggregate
lifetime work: the reproduction below fails identically on a tree without those
changes (verified by stashing the working copy, rebuilding and re-running).

## Symptom

The JIT path (`spectralang run`) never finishes: the compiler's main thread
exhausts its stack (1 MiB default on Windows) and, on a tree where the CLI runs
its pipeline on a 256 MiB worker thread, hangs instead of dying. The AOT path
(`spectralang compile --emit-exe`) compiles and runs the same module correctly,
and `--dump-ir` prints the complete module before the failure, so the hang
happens after lowering.

## Reproduction

```spectra
module repro

import std.agent

func spec() returns string {
    return "{\"goal\":\"repro\",\"model\":\"mock/echo\",\"endpoint\":\"mock:\",\"allow\":[],\"max_tokens\":0,\"max_cost_micros\":0,\"max_seconds\":0,\"max_tool_calls\":0,\"untrusted\":\"approve\",\"seed\":1,\"journal\":\"\"}"
}

func start_run() returns Result<Run, Error> {
    return agent_start(spec())
}

public func main() returns int {
    return block_on(work())
}

async func work() returns int {
    let started = start_run()
    if let Result::Ok(run) = started {
        let ended = agent_end(run)
        if let Result::Err(_end_error) = ended {
            return 1
        }
        return 0
    }
    return 2
}
```

`spectralang run repro.spectra` overflows the stack (or hangs, with a large
enough stack). No `#[agent_tool]` declaration is needed: the run itself is
enough.

## Trigger matrix (measured)

| Shape | Result |
|---|---|
| `agent_start` inline in a sync `main` | ok |
| `agent_start` behind a helper, sync `main` | ok |
| `agent_start` inline in an async function driven by `block_on` | ok |
| `agent_start` behind a helper, called from an async function | **fails** |
| same module through `compile --emit-exe` | ok |
| `--dump-ir` | prints the module, then the failure |

So the trigger is the combination of a *dispatch host call inside a callee* and
an *async caller*; everything else about the module is irrelevant.

## Workaround

Call `agent_start` at the site that needs it -- the convention every existing
fixture and example already follows (`agent_start(spec(...))` directly inside
the flow, `spec()` as the small helper). 392 was written with a
`journal_free_start()` wrapper and hit this; it now inlines the call.

## Fix direction

The failure is in the path that lowers a coroutine whose caller reaches a
dispatch host call through a user function: the walk over callees does not
terminate. Two acceptable outcomes, in order of preference:

1. Make the async lowering's callee walk cycle-safe (visit set / work-list) so
   the helper shape compiles like the inline shape.
2. If the shape is genuinely unsupported, reject it during lowering with a
   diagnostic that names the callee and the dispatch call, instead of hanging.

Either way the reproduction above belongs in `tests/errors/` or as a
`tests/validation/` fixture once it passes.
