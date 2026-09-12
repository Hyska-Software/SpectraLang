# Spectra Formatter Guide

The Spectra CLI bundles a formatter that normalizes indentation, spacing, blank lines, and grouped `let` bindings. Use it to keep your project consistent and to integrate with editors and CI pipelines.

## Command Overview

Run the formatter over one or more files or directories:

```shell
spectra fmt <paths>
```

Key flags:

- `--check`: verify formatting without writing changes. Returns exit code `65` if changes are needed.
- `--stdin`: read source from standard input and emit the formatted version to standard output.
- `--stdout`: format a single on-disk file and write the result to standard output rather than editing the file in place.
- `--explain[=json]`: show diffs for any file that would change. Text output is default; `json` produces a machine-readable payload and implies `--check`.
- `--stats`: print a JSON summary of the formatter run (processed, changed, cache hit metrics) after normal output.
- `--config <path>`: load formatter settings from an explicit `spectra.toml` file, useful when editors work on scratch copies outside the project tree.

## Explain Output

Use `spectra fmt --check --explain` to review differences without touching files. The text mode lists unified-style diffs for each file:

```text
diff --spectra src/main.spectra
--- original
+++ formatted
 fn main() {
-    return 0;
+    return 1;
 }
```

Switch to `--explain=json` for tool-friendly output. The CLI prints a JSON object with a `summary` and per-file `operations` stream. Each operation is tagged with an `op` of `equal`, `insert`, or `remove` so downstream tooling can rebuild the diff:

```json
{
"summary": {
"processed": 2,
"changed": 1,
"updated": 0,
"unchanged": 1,
"mode": "check",
"config_cache_lookups": 2,
"config_cache_hits": 1,
"config_cache_misses": 1
},
"files": [
{
"path": "src/main.spectra",
"operations": [
{ "op": "equal", "text": "fn main() {" },
{ "op": "remove", "text": "    return 0;" },
{ "op": "insert", "text": "    return 1;" },
{ "op": "equal", "text": "}" }
]
}
]
}
```

The formatter exits with code `0` when no files need changes and `65` when diffs are present.

## Run Statistics

Append `--stats` to any formatter invocation to receive a standalone JSON summary after the regular formatter output (diffs, status messages, or formatted source). The fields align with the explain summary:

```json
{
	"processed": 5,
	"changed": 2,
	"updated": 2,
	"unchanged": 3,
	"mode": "write",
	"config_cache_lookups": 4,
	"config_cache_hits": 3,
	"config_cache_misses": 1
}
```

The `mode` indicates whether the formatter ran in `check` (verification) or `write` (in-place updates) mode. `updated` reports how many files were rewritten on disk (always `0` when `--check` is active) while the cache counters expose configuration lookups resolved from `spectra.toml` manifests.

When `--stats` and `--explain=json` are used together, the CLI prints the explain payload first and then emits the stats object on a new line so downstream tooling can parse each document independently.

## Configuration

Formatter settings live under the `[formatter]` table inside `spectra.toml`. The CLI searches from each formatted file up to the filesystem root and uses the nearest manifest it finds. The search order is skipped when `--config` is supplied.

Supported keys:

| Key               | Type | Default | Description |
|-------------------|------|---------|-------------|
| `indent_width`    | int  | `4`     | Number of spaces per indentation level. Must be between `1` and `12`. |
| `max_line_length` | int  | `100`   | Maximum desired line length. Guard rails alignment of grouped bindings. Minimum value is `40`. |

Unknown keys produce a usage error so typos are caught early.

### Example `spectra.toml`

```toml
[package]
name = "spectra_app"
version = "0.1.0"

[build]
entry = "src/main.spectra"

[formatter]
indent_width = 2
max_line_length = 88
```

Place this file at the project root. The formatter will apply the configuration to any file under that directory tree.

## Editor Integration

- Use `spectra fmt --stdin --stdout` for editor commands that stream file contents through the CLI.
- Combine `--config` with an absolute path when your editor writes temporary files outside the project tree.

## Continuous Integration

Add a formatting gate to your CI workflow to keep pull requests consistent. A minimal shell step looks like:

```shell
spectra fmt --check .
```

When the formatter reports changes, the command exits with code `65`, causing the build to fail until the author applies `spectra fmt` locally.

For richer telemetry, add `--stats` so CI logs include machine-readable counts of processed and changed files. The JSON summary slots easily into dashboards or annotations.

The repository includes an end-to-end example workflow at `tools/spectra-cli/.github/workflows/spectra-fmt-check.yml` that runs the formatter in CI and surfaces diffs when formatting fails.
