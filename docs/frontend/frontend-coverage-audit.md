# Frontend Coverage Audit

Updated: 2026-09-29
Roadmap item: `R-101`

This document is the Phase 1 source of truth for frontend language coverage. It classifies the lexer and parser surface against the currently documented SpectraLang syntax.

Status labels:

- `supported`: implemented and covered by automated tests
- `gated`: implemented behind `--enable-experimental <feature>`
- `partial`: parses in common cases, but has edge-case or ergonomics gaps
- `deferred`: intentionally not implemented yet

## Lexer Coverage

| Surface | Status | Notes |
| --- | --- | --- |
| ASCII identifiers and keywords | supported | Includes reserved words such as `as`, `dyn`, `public`, `internal`, `match`, `switch`, `record`, `func`, `loop`. |
| Integer literals | supported | Decimal, hexadecimal, octal and binary forms; underscores are accepted between digits. |
| Float literals | supported | Decimal and scientific notation forms are covered by the current lexer contract. |
| String literals | supported | Basic escapes supported. |
| Char literals | supported | Includes common escapes. |
| F-string literals | supported | Parsed as a dedicated token kind and expanded in the parser. |
| Line comments `//` | supported | Ignored by the token stream. |
| Block comments `/* ... */` | supported | Unterminated comments emit coded lexical diagnostics. |
| Operators `+ - * / % == != <= >= && || .. ..=` | supported | Word-form `and`, `or`, and `not` are the canonical boolean spellings; symbolic forms remain available in expression contexts. |
| Symbols `(){}[],:.=<>!&|?@` | supported | Statement/declaration semicolons are rejected; the parser uses line termination and closing braces. |
| Unicode identifiers | deferred | The lexer intentionally limits identifiers to ASCII today. |
| Numeric separators / hex / binary | deferred | Covered by future numeric expansion work. |
| Raw strings / multiline string modes | deferred | Not part of the current language contract. |

## Module-Level Syntax Coverage

| Syntax form | Status | Notes |
| --- | --- | --- |
| `module name` | supported | Required file header. |
| `import std.io` | supported | Basic module import. |
| `import std.math as math` | supported | Alias import accepts `as` as a keyword. |
| `from std.io import println, print` | supported | Named import surface is stable. |
| `public from std.io import println` | supported | Re-export surface is stable. |
| `internal import ...` | supported | Parsed and carried into the AST. |
| `public func`, `internal func` | supported | Visibility-aware item parsing is implemented. |
| `record`, `enum`, `trait`, `impl`, `type`, `const`, `static` | supported | Parsed in the current frontend. |
| `class` declarations | deferred | `class` is reserved and rejected with `P007`; use `record`, `impl` and `trait`. |
| Cross-file workspace parsing | supported | Covered by parser workspace tests and project discovery in the CLI. |

## Type Syntax Coverage

| Syntax form | Status | Notes |
| --- | --- | --- |
| Simple types `int`, `string`, `Foo` | supported | |
| Qualified types `std.io.Writer` | supported | |
| Generic types `Option<int>` | supported | |
| Tuple types `(int, string)` | supported | |
| Function types `func(int) returns int` | supported | |
| Trait objects `dyn Shape` | supported | |
| Higher-kinded types | deferred | |
| Lifetime syntax | deferred | Not part of the language today. |

## Statement Coverage

| Syntax form | Status | Notes |
| --- | --- | --- |
| `let` bindings | supported | `mut` is an accepted, redundant marker; local bindings are reassignable by default. |
| Assignment statements | supported | Includes indexed and field-based targets through expression parsing. |
| `return`, `break`, `continue` | supported | |
| Expression statements | supported | Statements terminate at a line break, closing brace, or end of file. |
| `if / else if / else` | supported | |
| `if let` | supported | Implemented as a dedicated parser path. |
| `while` | supported | |
| `while let` | supported | Implemented as a dedicated parser path. |
| `for ... in ...` | supported | |
| `for ... of ...` | rejected | The canonical collection iteration form is `for ... in ...`. |
| `loop` | supported | Stable syntax; executes through JIT regression coverage. |
| `do { } while ...` | supported | Stable syntax; executes through JIT regression coverage. |
| `switch` | supported | Stable syntax; executes through JIT regression coverage. |
| `repeat/until`, `foreach`, `goto`, `yield` | deferred | Tokens exist, parser implementation is intentionally absent. |

## Expression Coverage

| Syntax form | Status | Notes |
| --- | --- | --- |
| Literals and identifiers | supported | |
| Arithmetic and comparison operators | supported | |
| Boolean operators `and`, `or` | supported | Symbolic `&&` and `||` remain accepted in expression contexts. |
| Unary operators `-`, `not` | supported | Symbolic `!` remains accepted in expression contexts. |
| Function calls | supported | |
| Qualified calls `std.io.println(...)` | supported | Parsed as chained call expressions. |
| Method-style calls `value.method(...)` | supported | |
| Field access | supported | |
| Type casts `expr as Type` | supported | |
| Propagation `expr?` | supported | Parsed as a postfix expression; semantic validation accepts `Option<T>`/`Result<T, E>` and checks early-return compatibility (`E049`/`E050`). |
| Blocks `{ ... }` as expressions | supported | |
| `if` expressions | supported | |
| `if not` expressions | supported | Canonical negative conditional surface. |
| `match` expressions | supported | |
| Lambda/closure expressions | supported | Current parser surface accepts the documented closure syntax. |
| Array literals | partial | Lower stages do not yet treat them as a production-ready numeric container surface. |
| Comprehensions | deferred | |

## Pattern Coverage

| Syntax form | Status | Notes |
| --- | --- | --- |
| Wildcard `_` | supported | |
| Identifier binding | supported | |
| Literal patterns | supported | |
| Tuple patterns | supported | |
| Enum variant tuple patterns | supported | |
| Enum variant struct patterns | supported | |
| Guarded match arms `when pattern if guard then ...` | supported | The parser stores the guard; semantic analysis checks its `bool` type (`E040`) and excludes it from unconditional exhaustiveness coverage (`E031` when coverage remains incomplete). |
| OR-patterns | supported | The parser builds OR-patterns and semantic exhaustiveness checks each alternative. |
| Slice patterns | deferred | |

## Recovery and Diagnostics Coverage

| Area | Status | Notes |
| --- | --- | --- |
| Missing statement terminator, `)`, `}` recovery | supported | Synthetic spans prevent stuck parsing. |
| Experimental feature gates | supported | No active syntax gates are registered; `--enable-experimental` is a compatibility no-op. |
| Import syntax diagnostics | supported | Covered by parser tests for import forms. |
| Infinite-loop recovery regression | supported | Recovery now guarantees progress on error. |
| Full syntax snapshot testing | partial | Parser unit tests exist, but grammar-wide snapshots are part of ongoing test growth. |

## Deferred Frontend Backlog

- `R-203`: expand pattern ergonomics and destructuring beyond the current match surface
- future frontend item: Unicode identifier support
- future frontend item: raw strings and advanced literal modes
- future frontend item: parser snapshot generation from a broader corpus
