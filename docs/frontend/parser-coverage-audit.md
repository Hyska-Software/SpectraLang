# Parser & Lexer Coverage Audit

_Updated: 2026-09-29_

## Scope

This document records the lexer/parser surface in the current source tree. The
broader feature matrix is maintained in
[`frontend-coverage-audit.md`](frontend-coverage-audit.md); semantic behavior is
tracked separately in
[`semantic-coverage-audit.md`](../semantic/semantic-coverage-audit.md).
Parsing a construct does not by itself prove type checking, lowering, or runtime
execution.

## Lexer

- Identifiers use the documented ASCII form. Unicode identifiers remain
  deferred.
- Numeric literals include decimal, hexadecimal, octal and binary integers,
  underscore-separated digits, and scientific-notation floats. Malformed
  separators and incomplete exponents have lexical diagnostics.
- Strings, character literals, f-strings, common escapes, line comments and
  block comments are tokenized by the current lexer.
- The operator/symbol set includes the punctuation used by patterns, casts,
  references, propagation, and declarations. Statement/declaration semicolons
  are not the language's terminator model.
- Reserved words such as `class`, `foreach`, `repeat`, `until`, `yield` and
  `goto` can be lexed without being usable syntax. `class` is deliberately
  rejected with `P007`.

## Parser Surface

### Modules and declarations

- `module`, qualified/aliased imports, named imports, and public re-exports.
- `func`, visibility modifiers, records, enums, traits, inherent/trait impls,
  type aliases, constants and module statics.
- Current generic parameter and type-argument forms, including qualified
  types, tuples, function types, trait objects, tensor annotations and async
  types. Unsupported higher-kinded/lifetime syntax remains outside the grammar.

### Statements and expressions

- `let`, assignment, `return`, `break`, `continue`, `if`/`else`, `if let`,
  `while`, `while let`, `for ... in`, `loop`, `do ... while`, `switch` and
  `match`.
- `mut` after `let` is accepted as a redundant marker; it does not request a
  distinct immutable/mutable binding mode.
- Calls, method/field access, indexing, casts, blocks and `if`/`match`
  expressions, lambdas/closures, async blocks and `await` are represented by
  the parser. `?` is parsed as a propagation operator; its operand and return
  context are semantic checks, not parser guarantees.
- Match patterns include wildcard, binding, literal, tuple, enum payload,
  struct-style and OR-pattern forms. Match guards are parsed and stored with
  their arms.

## Parser Organization

The recursive-descent parser is split across `compiler/src/parser/`: module and
workspace discovery; item/declaration/trait/impl parsing; statement parsing;
expression precedence and primary expressions; pattern parsing; and type
annotations. `Parser::parse` builds the AST, while recovery uses source spans
and synchronization so malformed input can produce localized diagnostics.

## Boundaries

- Parsing a match guard does not alone prove that its expression is boolean or
  that pattern bindings are in scope; those checks belong to semantic analysis.
- Parsing `?` does not alone prove the operand is `Option<T>` or `Result<T, E>`
  or that the enclosing function accepts the propagated `None`/`Err` path.
- OR-pattern support and parsing do not imply that guarded branches count as
  unconditional exhaustive coverage.
- No experimental syntax gate is currently active. `--enable-experimental`
  remains a compatibility no-op.
- The parser does not implement `class`, Unicode identifiers, lifetimes,
  `foreach`, `repeat/until`, `goto`, or `yield` as usable language constructs.

This document update records the intended current contract and source
boundaries. It does not report newly executed tests or gates as passing.
