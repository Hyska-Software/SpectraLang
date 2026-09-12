# Parser & Lexer Coverage Audit

_Date: 2025-11-06_
_Branch: devlop_

## Scope
- Reviewed the current lexer (`compiler/src/lexer/mod.rs`) and parser modules (`compiler/src/parser/*`).
- Cross-referenced behaviour with the frozen alpha language reference (`docs/language-reference-alpha.md`).

## Lexer Findings
- Supports single-line `//` comments and skips whitespace/newlines; block comments and nested comments are not recognised.
- Tokenises string literals without escape-sequence handling; unterminated strings raise a lex error but do not recover inline.
- Numbers accept a single optional fractional part (`123.45`); there is no support for exponent notation, digit separators, or numeric suffixes.
- Identifiers follow the documented `[A-Za-z_][A-Za-z0-9_]*` pattern; Unicode identifiers are rejected.
- Keyword table includes reserved tokens (`foreach`, `repeat`, `until`, `cond`, `yield`, `goto`, `class`, `export`) that the parser does not currently consume.
- Symbols cover the documented operators plus `@`; `@` is lexed but unused downstream.

## Parser Findings

### Module & Imports
- Enforces a `module <path>` header before items; missing headers trigger a parse error followed by synchronisation.
- `import` supports:
  - dotted module imports (`import std.io`)
  - alias imports (`import std.math as math`)
  - named imports (`from std.io import println, print`)
  - public re-exports (`public from std.io import println`)

### Items & Visibility
- Handles `public func/record/enum` correctly; `public impl` falls back to inherent impl parsing but visibility is discarded (consistent with current AST).
- `class` keyword is recognised lexically but has no parser entry point.
- Generic parameters are parsed for functions, structs, and enums; there is no support for where clauses, default type parameters, or const generics.

### Traits & Trait Inheritance
- `trait Name: Parent + Another { .. }` is accepted with `+` separators. Comma-separated parent lists are rejected.
- Default method bodies and receiver qualifiers (`self`, `&self`, `&mut self`) are parsed and recorded.
- Trait inheritance, default methods, trait-bound method resolution, and `Self` substitution are exercised by the validation suite.

### Impl Blocks
- Inherent impls parse method lists with receiver variants and typed parameters.
- `impl Type` accepts simple identifiers, qualified paths (`impl module::Type`),
  and generic type arguments (`impl Par<T>`) since R-211; trait impls accept
  `impl Trait<Args> for Type` and the generic clause `impl<T: Bound> Trait for Type`
  since R-213.

### Statements & Control Flow
- Control-flow constructs implemented: `while`, `do { } while`, `for name in`, `loop`, `switch`, `break`, `continue`, `if let`, `while let`.
- Reserved keywords `foreach`, `repeat`, `until`, `yield`, `goto` remain unparsed despite being lexed.
- `switch` accepts `case` arms and an optional `else` block.
- Assignments only accept identifiers or index expressions on the LHS; destructuring assignments are not allowed.

### Expressions & Calls
- Method chaining (`obj.method().field`) and tuple indexing (`tuple.0`) are supported.
- Struct literals differentiate from enum variants by disallowing `::` in field initialisers.
- Generic type arguments on identifiers (`Type::<T>::Variant`) are parsed, though the semantic layer handles association.
- Lambda/closure literals are supported. Spread operators and inline `if` expressions without blocks are not.

### Pattern Ergonomics
- `match` patterns cover wildcard (`_`), identifier bindings, literal patterns, enum variants with tuple payloads, and struct-style enum patterns.
- `let` supports tuple, struct, and enum destructuring patterns in the validated surface.
- `if let` and `while let` are fully parsed and lowered.
- OR-patterns (`A | B`) are supported in the validated match surface.
- Match guards remain unsupported.

## Remaining Gaps
1. Trait and impl generics are still narrower than the long-term planned surface.
2. `match` guards remain unsupported.
3. Control-flow keywords flagged in docs (`foreach`, `repeat`, `until`, `yield`, `goto`) remain deferred.

## Suggested Follow-Up Tasks
1. Extend trait and impl parsing to accept the remaining generic forms from the roadmap.
2. Introduce parser branches for the reserved control-flow keywords that emit deliberate "deferred" diagnostics instead of generic errors.
3. Expand pattern parsing with guards to align with planned match ergonomics.
