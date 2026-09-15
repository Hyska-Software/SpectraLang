# Findings

## F-001 — Remote MCP metadata leaked into unrelated run grant checks

**Observed:** A language-level reproducer registered a local `local_value` tool, started a run with `allow: ["mcp"]`, served the local tool over MCP, discovered it back into the process under `mcp__<authority>__local_value`, ended that run, then started a second run with `allow: []`. The second run attempted the independent local tool call and failed before execution:

```text
local call: capability_denied: tool 'mcp__127_0_0_1_<port>__local_value' has effect 'mcp.127.0.0.1:<port>', which the run grant [] does not authorize
```

The temporary reproducer was `target/agent_remote_scope_probe.spectra`; it exited with status `5` before the fix and was removed after the observation.

**Root cause:** `packages/spectra-agent/src/tools.rs::enforce_run_grant` iterated the process-global registry. Remote descriptors are installed globally by `mcp::client::install`, so a remote tool discovered by an earlier run participated in every later run's pre-dispatch grant check. `act` also exposed the same global descriptors to every model turn.

**Impact:** A run with no relationship to a prior MCP server could be denied for its own local tool surface. The leak was primarily an availability/isolation defect, and global descriptor exposure could also reveal a prior run's remote surface to a later provider.

**Correction:** Track the live run handles that discovered each remote registry entry. Add run-filtered registry views for grant enforcement, model definitions, MCP `tools/list`, and A2A card skills, and remove a handle's entries when that generational run is released. Local compiled tools remain process-wide; a remote descriptor is visible only to runs that installed it. Existing per-server grant checks remain in force for every visible remote call.

**Permanent evidence:** `examples/agent/39-remote-scope-isolation` and `tests/validation/429_agent_remote_scope.spectra` reproduce the two-run boundary and require the second local call to succeed. Rust registry tests cover run-filtered visibility without relying on a socket.

## F-002 — The certification gate ran only `main.spectra` for JIT examples

**Observed:** The first full gate after adding `38-cross-module-tools` stopped at
the JIT phase with:

```text
module 'main' imports 'tools', but no file declaring 'module tools' was found
```

The project itself ran successfully when passed to `spectralang run`; the gate
was passing only `src/main.spectra` to its JIT command, so imported source
modules were outside the compiler input.

**Correction:** `check_examples` now invokes `spectralang run <project>` for
JIT, matching its existing AOT invocation and the contributor-facing command.
The source-file existence check remains, so a missing `src/main.spectra` still
fails with a named example.
