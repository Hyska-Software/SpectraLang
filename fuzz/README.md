# SpectraLang Fuzz Targets

This directory contains the R-104 fuzz targets for the compiler test pyramid.
They are intentionally outside the default workspace so normal `cargo test`
remains fast and does not require `cargo-fuzz`.

## Targets

- `parser`: lexes and parses arbitrary UTF-8 input.
- `semantic`: runs lexer, parser, and semantic analysis on parseable modules.
- `pipeline`: runs the production compilation pipeline with the no-op backend.
- `lowering`: lowers parseable AST modules into midend IR.

All targets reject non-UTF-8 input and sources larger than 16 KiB
(`-max_len=16384` keeps a run inside this cap).

## Corpus

`corpus/<target>/` holds committed seed inputs so every run starts from real,
varied coverage instead of an empty corpus:

- `parser/`: 33 seeds — fixture-derived modules plus malformed stress cases
  (unterminated f-strings, deep nesting, unicode/control characters, operator
  and delimiter soup, numeric/char edge cases).
- `semantic/`, `lowering/`: 33 seeds each — parseable fixtures plus
  semantically dubious modules (unknown types, duplicate definitions,
  non-exhaustive matches) that survive parsing and reach these stages.
- `pipeline/`: 53 seeds — the union of the above.

Seeds are small (< 1.1 KiB each) and valid UTF-8; new crash artifacts should be
minimized with `cargo fuzz tmin` and checked in here as regression seeds after
converting them into tests where practical.

## Running with cargo-fuzz

Install once:

```powershell
cargo install cargo-fuzz
```

Run a target:

```powershell
cargo fuzz run parser
cargo fuzz run semantic
cargo fuzz run pipeline
cargo fuzz run lowering
```

Each command automatically picks up its seed directory from
`corpus/<target>/`. Crash artifacts land in `fuzz/artifacts/` (or the path
given via `-artifact_prefix=`).

## Building without cargo-fuzz

Targets are split on `cfg(fuzzing)`, which cargo-fuzz defines automatically:

- Under cargo-fuzz, `libfuzzer_sys::fuzz_target!` drives each target.
- With plain cargo (`--cfg fuzzing` absent), each binary exposes a replay
  `main` that feeds files passed as arguments through the same `run` body.
  This is what keeps `cargo check -p spectralang-fuzz` green on any machine:

```powershell
cargo check -p spectralang-fuzz   # from this directory
```

You can also replay a whole corpus without cargo-fuzz, which doubles as a
deterministic smoke test over all committed seeds:

```powershell
cargo build -p spectralang-fuzz --bins
target\debug\parser corpus\parser
target\debug\semantic corpus\semantic
target\debug\pipeline corpus\pipeline
target\debug\lowering corpus\lowering
```

A non-zero exit means a seed crashed or could not be read.

## Crash triage

Reproduce with
`cargo fuzz run <target> fuzz/artifacts/<crash-file>`, minimize with
`cargo fuzz tmin`, add a regression seed to `corpus/<target>/`, fix the
compiler bug, and add a checked-in regression test before closing the issue.

## Corpus maintenance

Corpus refresh is a manual, local step — run the target for as long as is
useful, then minimize:

```powershell
cargo fuzz cmin <target>
```

Review the `corpus/<target>/` diff for oversized or sensitive inputs before
committing the minimized seeds.
