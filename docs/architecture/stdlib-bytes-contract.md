# Standard-library byte contract (L-4)

Status: accepted (Wave 2, task T-04).
Owner: `ecosystem` with `runtime` review.

## Context

Spectra `string` values are UTF-8 text. The packed runtime representation is
NUL-terminated, so a string cannot carry an embedded `U+0000`, and
`std.string.from_scalar(0)` cannot round-trip through
`std.bytes.from_bytes`. Several standard-library modules (encoding, hashing,
UUIDs) need byte-level access anyway.

## Decisions

1. **Text stays UTF-8.** `string` keeps its current meaning: valid UTF-8 text,
   NUL-terminated in the packed ABI. No ASCII/binary string modes are added.
2. **Bytes are `List<int>`.** Each element is one byte in `0..=255`. Lists are
   the existing collection surface (no new primitive, no new ABI).
3. **`std.bytes` is the bridge** between text and byte lists:
   - `to_bytes(text) -> List<int>` never fails for a valid string.
   - `from_bytes(bytes) -> Option<string>` returns `None` for a value outside
     `0..=255`, for an embedded NUL, and for any sequence that is not valid
     UTF-8 (truncated, overlong, surrogate, above `U+10FFFF`).
   - `byte_at(text, index) -> Option<int>` and `is_ascii(text) -> bool` cover
     the read-only cases.
4. **NUL is rejected, not truncated.** A byte list containing `0x00` cannot be
   materialized as text; callers that need embedded NULs must keep working with
   `List<int>` end to end. This keeps one representation per concept instead of
   silently cutting payloads.
5. **`std.unicode` owns scalar-level operations** (`rune_count`, `rune_at`,
   `byte_offset`, `to_codepoints`, `from_codepoints`, `valid_utf8_bytes`,
   `slice_runes`). Text modules must call it instead of carrying private
   UTF-8 walkers (Wave 2, T-12 deletes those helpers).

## Non-goals

- A first-class `bytes` type, byte string literals, or a length-prefixed string
  ABI: out of scope for Wave 2. Revisit if binary payloads become first-class
  (for example protobuf-style APIs or in-place crypto buffers).
- Changing how host calls return text: they keep returning UTF-8 strings.

## Evidence

- `tests/validation/811_std_unicode.spectra` — scalar/rune operations,
  including rejection of overlong, surrogate, truncated and out-of-range byte
  sequences.
- `tests/validation/812_std_bytes.spectra` — round-trip, NUL rejection, range
  rejection and ASCII checks.
- `docs/reference/05-stdlib.md` §§17 (std.encoding), 26 (std.unicode) and 27
  (std.bytes) document the same contract for users.
