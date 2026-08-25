# Error Code Reference

Updated: 2026-06-04
Roadmap item: `R-105` (`complete`)

This file defines the stable diagnostic-code ranges currently implemented in Phase 1 and the high-frequency diagnostics tooling can depend on.

## Code Families

| Range | Phase | Meaning |
| --- | --- | --- |
| `L001-L099` | lexer | Tokenization and literal scanning errors |
| `P001-P099` | parser | Syntax and feature-gate errors |
| `E001-E099` | semantic | Name resolution, typing, control flow, and trait validation errors |
| `E2101-E2120` | semantic | Phase 21 async/await, task safety, and Send/Sync diagnostics |
| `lint(<rule>)` | lint | Lint warnings or denied lint findings |
| `midend` | midend | Internal IR/lowering errors without a stable subcode yet |
| `backend` | backend | Codegen or backend execution errors without a stable subcode yet |
| `io` | CLI | Host filesystem/process I/O errors |
| `cli` | CLI | Command-line planning or project-discovery failures |

## High-Frequency Diagnostics

The following set is the current stable Phase 1 table for high-frequency diagnostics with actionable hints.

| Code | Phase | Meaning | Expected hint/action |
| --- | --- | --- | --- |
| `L001` | lexer | unexpected character | remove the character or escape it appropriately |
| `L002` | lexer | unterminated string literal | close the string with `"` |
| `L003` | lexer | unterminated character literal | close the literal with `'` |
| `L004` | lexer | empty character literal | provide exactly one character |
| `L005` | lexer | unterminated f-string literal | close the f-string with `"` |
| `L006` | lexer | unterminated block comment | close the comment with `*/` |
| `P001` | parser | expected keyword | insert the missing keyword or fix item order |
| `P002` | parser | expected or synthesized symbol | insert the required delimiter such as `)`, `}`, or `:` |
| `P003` | parser | expected identifier | provide a valid identifier in the current grammar slot |
| `P004` | parser | future experimental feature disabled | rerun with the documented feature gate once an active experimental feature exists |
| `P005` | parser | misplaced or incomplete `async` syntax | use `async func` in declaration position, `async { ... }`, or `async |...| ...` |
| `P006` | parser | `await` outside async context | move the expression into `async func`, `async { ... }`, or an async closure |
| `P013` | parser | nesting too deep | reduce nesting of expressions, statements, blocks, or patterns (parser recursion guard) |
| `P999` | parser | generic syntax failure | inspect nearby syntax; parser context and hint should narrow the issue |
| `E001` | semantic | undefined variable or function | declare/import the symbol or fix the name |
| `E002` | semantic | argument count mismatch | pass the expected number of arguments |
| `E003` | semantic | function return mismatch | align the returned value with the declared return type |
| `E004` | semantic | type mismatch in expression or assignment | convert or correct the incompatible type |
| `E005` | semantic | invalid field or member access | use an existing field/member on the target type |
| `E006` | semantic | invalid method call or method resolution failure | call a method that exists for the target type and signature |
| `E007` | semantic | `break` outside loop | move `break` into a loop body |
| `E008` | semantic | `continue` outside loop | move `continue` into a loop body |
| `E009` | semantic | generic or trait-bound inference failure | provide required type arguments or satisfy the bound |
| `E010` | semantic | unsatisfied generic trait bound | implement the required trait for the concrete type before calling the generic function |
| `E011` | semantic | unknown qualified module member | use an exported member from the module or import alias; inspect the candidate export list |

This satisfies the Phase 1 acceptance target of at least 20 high-frequency diagnostics with actionable remediation guidance.

## Phase 2 OOP Diagnostics (R-208)

The following codes are the stable object-oriented/trait diagnostic range for
records, traits, impl blocks, and `dyn` casts.

| Code | Phase | Meaning | Expected hint/action |
| --- | --- | --- | --- |
| `E012` | semantic | trait used as an `impl` target is not defined | declare or import the trait before the impl block |
| `E013` | semantic | method is already defined for the same type | remove the duplicate method or give it a distinct name |
| `E014` | semantic | method declares more than one `self` parameter | keep exactly one `self` receiver |
| `E015` | semantic | parent trait of a trait declaration is not defined | declare the parent trait before the child trait |
| `E016` | semantic | required trait method is not implemented | implement the method (or rely on its default implementation) in the `impl Trait for Type` block |
| `E017` | semantic | method not found for the receiver type | call an existing method or add an impl block with that method |
| `E018` | semantic | `self`-taking method called as a static/associated function (or vice versa) | call it on a value (`value.method(...)`) or as `Type::method(...)` per the signature |
| `E019` | semantic | struct literal is missing a required field | provide a value for every field of the record |
| `E020` | semantic | struct literal field has an unknown name or wrong type | use the declared field names and types of the record |
| `E021` | semantic | struct used in a literal or field access is not defined | declare or import the record before using it |
| `E022` | semantic | invalid `as dyn Trait` cast: type does not implement the trait | implement the trait for the concrete type before casting |
| `E023` | semantic | trait impl signature mismatch (parameter count/types or return type) | match the exact signature declared by the trait |
| `E024` | semantic | `self` receiver appears after other parameters | move the `self` receiver to the first parameter position |
| `E025` | semantic | impl type arguments do not match the target type's type parameters | use the type parameters declared by the generic record, in order |
| `E027` | semantic | module-qualified inherent impl target cannot be resolved | import or declare the module and use an exported struct or enum as the `impl module::Type` target |

## Module, Exhaustiveness, and Receiver Diagnostics (E028-E033)

The following codes cover module resolution, duplicate declarations,
`match` exhaustiveness, and method receiver validation.

| Code | Phase | Meaning | Expected hint/action |
| --- | --- | --- | --- |
| `E028` | semantic | circular import detected (including a module importing itself) | break the cycle by removing or restructuring one of the imports in the reported chain |
| `E029` | semantic | user (non-stdlib) module does not exist | check the spelling of the module path; the module must be declared as a source file in the same project or package |
| `E030` | semantic | duplicate declaration (struct/enum already defined, or duplicated struct field / enum variant) | remove the duplicate declaration, rename it, or drop the repeated field/variant |
| `E031` | semantic | `match` expression is not exhaustive (missing enum variants or missing wildcard bindings for payload variants) | add patterns for the listed `Enum::Variant` arms or a wildcard arm with payload bindings |
| `E032` | semantic | method receiver mismatch (receiver type differs from the declared `self` type, or a `self`-less method called on a value) | convert or borrow the receiver to match the signature, or call it as `Type::method(...)` |
| `E033` | semantic | unknown standard library module in an import | use one of the registered stdlib modules; the diagnostic includes a did-you-mean suggestion when close |

## Phase 21 Async Diagnostics

The following async diagnostic range is stable for tooling and documentation.
Codes are reserved even when a later phase broadens the implementation behind
the code.

| Code | Phase | Meaning | Expected hint/action |
| --- | --- | --- | --- |
| `E2101` | semantic | non-`Send` value is live across an `await` | drop or convert the value before `await`, or keep the task on a local executor lane |
| `E2102` | semantic | `RefCell`/interior-mutable value is held across an `await` | shorten the value lifetime or replace it with an async-safe synchronization primitive |
| `E2103` | semantic | `!Send` value crosses a spawn/task boundary | use a local task API or pass only `Send` values to spawn-style APIs |
| `E2104` | semantic | formal `Send`/`Sync` evidence is missing for a generic bound or `dyn Trait + Send/Sync` object | add the required bound/evidence or use a type that satisfies the auto-trait |
| `E2104` | semantic | non-`Sync` shared state is required by an async API | use synchronized shared state or avoid sharing across executor threads |
| `E2105` | semantic | `await` operand is not `Task<T>` | await only task values or remove `await` |
| `E2106` | semantic | `await` is used outside an async semantic context | move the expression into `async func`, `async {}`, or an async closure |
| `E2107` | semantic | async return type does not match `Task<T>` output | align the declared async return type and returned values |
| `E2108` | semantic | async trait method is not object-safe for `dyn Trait` | change the receiver to an object-safe form such as `&self` |
| `E2109` | semantic | async closure captures a non-`Send` value where `Send` is required | capture a `Send` value or use a local-only closure/task API |
| `E2110` | semantic | async block captures a non-`Send` value where `Send` is required | narrow the capture or use a local executor lane |
| `E2111` | semantic | task cancellation token is used after completion | stop using the token after the owning task completes |
| `E2112` | semantic | timeout scope contains a non-cancellable async operation | use cancellable operations inside timeout scopes |
| `E2113` | semantic | blocking host call is used directly in async context | route the call through `spawn_blocking` or an async API |
| `E2114` | semantic | task is detached without an explicit detach policy | join, cancel, or explicitly detach the task |
| `E2115` | semantic | task result is polled after completion | consume the result once or create a new task |
| `E2116` | semantic | stream item is awaited after stream cancellation | stop polling the stream after cancellation |
| `E2117` | semantic | async state frame contains unsupported self-reference | rewrite the value to avoid self-reference across suspend points |
| `E2118` | semantic | borrowed value escapes an async state frame | return an owned value or shorten the borrow |
| `E2119` | semantic | task-local value is used from a different executor lane | keep the value on its original lane or make it `Send` |
| `E2120` | semantic | reserved async diagnostic catch-all | file a targeted diagnostic code before relying on this code in tooling |

## Machine-Readable JSON Diagnostics

Current CLI contract:

- `spectralang compile --json <path>`
- `spectralang check --json <path>`
- `spectralang lint --json <path>`
- `spectralang repl --json <path>`

JSON schema shape:

```json
{
  "version": 1,
  "success": false,
  "files": [
    {
      "path": "D:/Lang/SpectraLang/tests/errors/type_mismatch.spectra",
      "diagnostics": [
        {
          "severity": "error",
          "code": "E004",
          "message": "type mismatch",
          "phase": "semantic",
          "hint": "expected action here",
          "range": {
            "start": { "line": 4, "column": 12 },
            "end": { "line": 4, "column": 20 }
          },
          "related": []
        }
      ]
    }
  ]
}
```

Tooling expectations:

- `code` is stable when the compiler emitted one
- `phase` remains available even when `code` is absent
- `hint` is optional but should be consumed when present
- `related` can contain additional context-only messages without a span

## Machine-Readable SARIF Diagnostics

Current CLI contract:

- `spectralang compile --sarif <path>`
- `spectralang check --sarif <path>`
- `spectralang lint --sarif <path>`

SARIF output uses version `2.1.0` and writes one `SpectraLang` run. Each
diagnostic becomes a SARIF result:

- `ruleId` is the stable diagnostic code when present, otherwise the phase
- `level` is `error` or `warning`
- `message.text` is the compiler diagnostic message
- `locations[0].physicalLocation.artifactLocation.uri` is the source path
- `locations[0].physicalLocation.region` contains line/column data
- `properties.hint` is emitted when a fix/action hint exists
- `relatedLocations` carries additional context when present

`--json` and `--sarif` are mutually exclusive. Both formats return exit code
`65` when compilation/lint diagnostics contain errors.
