# Spectra CLI Tooling Plan

## Logging & Exit Codes (Ready)

- CLI emits errors prefixed with `error:` to simplify log parsing and automation hooks.
- Exit codes are stable and public:

| Code | Meaning                                      |
|-----:|----------------------------------------------|
|   0  | success                                      |
|  64  | usage error (invalid flags, missing inputs)  |
|  65  | compilation failed after diagnostics         |
|  74  | I/O failure while reading or writing files   |

- Help text documents these values so build pipelines and editors can react appropriately.

## Formatter Roadmap

### Current Status

- ✅ CLI command `spectra fmt` formats files in-place, supports `--check`, `--stdin`, and `--stdout`, and exits with `65` when changes are needed.
- ✅ Formatter normalizes indentation, operator spacing, grouped `let` alignment (line-length aware), blank-line coalescing, and preserves line endings.
- ✅ `[formatter]` section in `spectra.toml` now supports `indent_width` and `max_line_length`; settings are auto-discovered from the nearest manifest.
- ✅ `spectra fmt --config <path>` allows explicit config selection and reports unknown `[formatter]` keys as structured errors.
- ✅ Formatter caches `spectra.toml` lookups per directory to avoid redundant IO across large projects.
- ✅ Added formatter-focused regression tests under `tools/spectra-cli`.
- ✅ Usage and configuration documented in `docs/cli/formatter-guide.md`, including sample `spectra.toml` snippets.
- ✅ Sample GitHub Actions workflow (`tools/spectra-cli/.github/workflows/spectra-fmt-check.yml`) demonstrates `spectra fmt --check` gating.
- ✅ Token-aware CST formatting path introduced (with legacy fallback) to preserve trivia-aware spacing and unary operator handling.
- ✅ `spectra fmt --explain` surfaces line-oriented diffs for files that need formatting and reuses the compiler exit codes for gating.
- ✅ `spectra fmt --explain=json` emits structured diff payloads for editor and automation integrations.
- ✅ Formatter guide documents the explain payload schema and new run-statistics output for downstream tooling.
- ✅ `spectra fmt --stats` emits a JSON summary (processed, changed, cache hits/misses) for CI dashboards and editor integrations.

### Upcoming Work

1. **CST policy extensions**
   - Layer configurable policies (brace style, trailing commas, import ordering) on top of the CST traversal.
   - Expand AST-aware passes to cover doc comment wrapping, nested comment indentation, and trailing comma heuristics.
2. **Editor & automation integration**
   - Version the JSON explain and stats payloads so integrations can detect breaking changes.
   - Provide CLI toggles or environment hooks for routing explain/stats output to specific log files.
   - Update the sample GitHub Actions workflow to capture explain/stats JSON payloads as build artifacts for regression analysis.
   - Publish sample scripts that consume `--stats` output to annotate CI runs or editor diagnostics.
3. **Performance & UX**
   - Benchmark large workspaces and explore parallel formatting of independent files.
   - Reuse cached configuration state across successive CLI invocations (daemon or IPC-friendly mode).
   - Refine `--explain` presentation with hunked output and optional color for large diffs.

## Linter Roadmap

### Status Overview

- ✅ Rule engine foundation integrated into the semantic pipeline, covering `unused-binding`, `unreachable-code`, and `shadowing` with deny escalation support.
- ✅ CLI surface in place: `spectra lint` command, `--lint` opt-in for build flows, and per-rule `--allow`/`--deny` switches with `[lint]` configuration in `spectra.toml`.
- ✅ Output conventions aligned with the shared reporter: lint diagnostics now reuse the `warning:`/`error:` prefixes and denied rules exit with code `65`.

### Next Steps

1. **Rule coverage expansion**
   - Extend the rule set (imports, match exhaustiveness hints, redundant patterns) with focused regression suites.
   - Annotate each rule with documentation links surfaced via `spectra lint --list`.
2. **Configuration ergonomics**
   - Introduce lint groups/presets in `[lint]` to simplify enabling common bundles.
   - Allow per-module overrides and ignore directives for incremental adoption.
3. **Automation integration**
   - Emit optional JSON summaries for CI ingestion alongside the human-readable reporter output.
   - Document the exit-code contract and reporter format in `docs/cli/tooling-plan.md` and the CLI help topics.

## VS Code Extension & Syntax Highlighting

1. **Language server scaffold**
   - Publish a minimal VS Code extension that shells out to `spectra repl --json` to provide diagnostics.
   - Ship Spectra grammar for TextMate highlighting, generated from parser tokens.
2. **Formatter & linter wiring**
   - Expose `Format Document` and `Lint` commands by invoking the new CLI subcommands (formatter streaming modes now available for editors).
   - Cache CLI lookups and present errors using the CLI exit codes to differentiate failures.
3. **Roadmap alignment**
   - Track editor features (hover, go-to-definition) in the same repo to keep parity with CLI improvements.
   - Document extension usage in `docs/cli` and update alpha checklist as milestones land.

### VS Code Extension Checklist

- [x] Establish extension workspace scaffold (`tools/vscode-extension/`), select package manager, and document architecture in `ROADMAP.md`.
- [ ] Register VS Code commands that shell out to `spectra repl --json`; handle process lifetime, cancellation, and JSON diagnostics parsing.
- [x] Populate diagnostics via `vscode.DiagnosticCollection`, mapping Spectra spans to VS Code ranges with severity derived from CLI output.
- [x] Generate and bundle the Spectra TextMate grammar from parser tokens; add language configuration (comments, brackets, indentation).
- [x] Implement formatter integration invoking `spectra fmt` streaming modes, with stdin/stdout fallback and error propagation respecting exit codes.
- [ ] Wire lint command execution to `spectra lint`, surface warnings/errors in the Problems view, and mirror CLI exit-code semantics. (Current implementation shells out to `spectra check --lint` per file; update to the dedicated CLI entrypoint once stable.)
- [x] Cache CLI capability probes (version, features, config paths) to avoid redundant process spawns during editor sessions.
- [ ] Plan advanced language server features (hover, go-to-definition) and track parity items within the extension roadmap.
- [ ] Add automated extension tests (colorization, command smoke tests) using `@vscode/test-electron` or equivalent harness.
- [ ] Update `docs/cli` with installation, configuration, and troubleshooting guidance; align alpha checklist milestones before publishing.

## Automation & CI Considerations

- Add formatter/linter checks to CI once the commands are available, failing builds on exit codes `65`.
- Provide sample GitHub Actions workflow in `tools/spectra-cli` to demonstrate format/lint gating.
- Capture `spectra fmt --stats` output in CI logs or artifacts so teams can track formatting drift over time.
- Encourage projects to use `spectra fmt --check` before publishing to maintain consistent style.
