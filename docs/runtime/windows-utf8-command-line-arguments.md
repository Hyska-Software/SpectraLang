# Windows AOT command-line arguments lose UTF-8 values

## Status

Fixed and verified in the AOT runtime startup path. The original failure was
reproduced with the `SpectraBoard` example on Windows. The JIT path forwards
Rust `String` values directly and does not use this C `argv` decoder.

## User-visible behavior

An AOT SpectraLang executable can misread command-line options when an argument
contains characters that the Windows C runtime does not encode as UTF-8. The
argument decoder silently drops each value that fails UTF-8 decoding. Every
later argument then appears at the wrong index, so a valid command may fail with
an unrelated parser error.

Reproduction from the repository root, after compiling the example:

```powershell
spectralang compile --debug-info=none --emit-exe target/spectraboard-dev.exe examples/complete/19-spectraboard
& .\target\spectraboard-dev.exe add --title 'Corrigir resolução de módulos / Ω' --priority high --due 2026-10-01 --tag compiler
```

Observed before correction:

```text
erro: argumento inválido para add: high
```

The same command with an ASCII-only title reaches the service layer. In a
directory initialized with `spectraboard init`, the Unicode command should
create task 1 and preserve the exact title for `show 1` and `list`.

## Root cause

`runtime/src/ffi_lifecycle.rs`, in
`spectra_rt_startup_with_args(argc, argv)`, walks the native `argv` array and
uses `str::from_utf8(...).ok()` inside `filter_map`. On Windows, the narrow
`main(argc, argv)` interface is not a reliable UTF-8 interface: the C runtime
may encode arguments using the active code page. An unrepresentable character
or a byte sequence outside UTF-8 therefore produces `None`.

`filter_map` discards that value instead of preserving its position. For
example, after dropping the title value, the sequence

```text
add, --title, --priority, high, --due, 2026-10-01, --tag, compiler
```

is passed to the app parser. This explains why it later reports `high` as an
invalid option. The public `std.env.env_arg` implementation reads the stored
vector correctly; the corruption happens before that vector is populated.

## Correction

On Windows, populate the runtime argument vector from `std::env::args_os()`,
which reads the native wide command line and preserves Unicode. On platforms
where the supplied C `argv` is UTF-8, decode all entries as one operation. If
any entry is null or invalid UTF-8, fall back to `args_os()` rather than
silently removing one entry. This keeps argument count and ordering intact.

The runtime now reads native Windows arguments through `std::env::args_os()`.
On other platforms it decodes the full supplied vector as UTF-8 and falls back
to `args_os()` if the vector is invalid, so a bad entry cannot shift subsequent
arguments. The previous per-entry `filter_map` has been removed.

Regression coverage checks malformed and null entries at the decoder boundary,
and checks exact Unicode argument values through both JIT and AOT execution.
The `SpectraBoard` integration harness also checks parsing and round-tripping
the title through the example's persistent database.

## Verification after the fix

Commands run on Windows x64:

```powershell
cargo test -p spectra-runtime argv_decoder_tests -- --nocapture
cargo build -p spectra-cli
.\target\debug\spectralang.exe run tests/validation/619_stdlib_utf8_program_arguments.spectra -- "título Ω — café" next-argument
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/argv-utf8-regression.exe tests/validation/619_stdlib_utf8_program_arguments.spectra
& .\target\argv-utf8-regression.exe "título Ω — café" next-argument
pwsh -NoProfile -File tests/spectraboard-integration.ps1
```

All commands exited successfully. The integration harness confirmed that the
full Unicode title survives command parsing, database persistence, and a later
process invocation.
