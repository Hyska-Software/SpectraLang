# Semantic Coverage Audit

Updated: 2026-05-21  
Roadmap item: `R-102`

This matrix maps the current AST surface to semantic validation coverage. No AST family is left unclassified.

Status labels:

- `supported`: validated end-to-end in the semantic pass
- `partial`: validated in the common path, but still limited or not production complete
- `deferred`: present in syntax or tokens, but intentionally not semantically supported yet

## Items

| AST family | Status | Notes |
| --- | --- | --- |
| Module declarations | supported | Module names, imports, and export visibility are validated. |
| Imports and re-exports | supported | Plain, alias, named, and `public from` forms resolve through the registry. |
| Functions | supported | Signatures, return types, body analysis, and return-path checks are active. |
| Record declarations | supported | Field typing and visibility are validated. |
| Enum declarations | supported | Variant registration and payload typing are validated. |
| Trait declarations | supported | Method signatures and default bodies are tracked. |
| Trait impl blocks | supported | Signature conformance is validated. |
| Inherent impl blocks | supported | Method registration and lookup are active. |
| Type aliases | supported | Aliases are parsed and registered in current semantic flows. |
| Const/static items | supported | `const` and scalar `static` initializers are compile-time checked; scalar statics lower to mutable IR globals, export/import relocatable data symbols, and execute through local and cross-module JIT/AOT. Aggregate/string/tensor/handle globals and synchronized task sharing remain outside the stable scalar contract. |
| Class declarations | reserved | `class` is lexed as a reserved keyword and rejected by the parser with `P007`; use `struct` + `impl` + `trait`. |

## Statements

| AST family | Status | Notes |
| --- | --- | --- |
| `let` bindings | supported | Type inference, shadowing checks, and lint hooks are active. |
| Assignments | supported | Undefined targets and type mismatches are diagnosed. |
| `return` | supported | Function return compatibility and path coverage are validated. |
| `break` / `continue` | supported | Loop-context validation emits stable semantic codes. |
| `if` / `else if` / `else` | supported | Branch typing and control-flow analysis are active. |
| `if let` | supported | Pattern bindings receive concrete types in the branch scope. |
| `while` | supported | |
| `while let` | supported | Pattern bindings receive concrete types inside the loop body. |
| `for ... in ...` | supported | Current collection/range forms are validated. |
| `loop` | supported | Semantic stage assumes parser gating has already occurred. |
| `do-while` | supported | Semantic stage assumes parser gating has already occurred. |
| `switch` | supported | Semantic stage assumes parser gating has already occurred. |

## Expressions

| AST family | Status | Notes |
| --- | --- | --- |
| Literals | supported | Primitive literal typing is stable. |
| Identifier lookup | supported | Stable diagnostics for missing symbols. |
| Binary operators | supported | Built-in numeric/bool rules plus trait-based overloading hooks. |
| Unary operators | supported | Includes float negation. |
| Function calls | supported | Arity and parameter typing are validated. |
| Qualified stdlib calls | supported | `std.io.println(...)` and alias-imported calls resolve. |
| Method calls | supported | Includes methods reached through imported namespaces. |
| Field access | supported | Struct field lookup and visibility checks are active. |
| Indexing | partial | Supported where current lowered representation exists; not yet a tensor-grade abstraction. |
| Casts `as` | supported | Current numeric, char, and dyn-trait cast rules are implemented. |
| Blocks as expressions | supported | Final-expression typing is validated. |
| `if` expressions | supported | Branch result typing is unified. |
| `if not` expressions | supported | Semantic stage analyzes the canonical negative conditional as a negated `if`. |
| `match` expressions | supported | Pattern coverage, arm typing, and exhaustiveness checks exist for the current surface. |
| Closures/lambdas | supported | Explicit function types and parameter-driven inference now work in current examples. |
| Trait object expressions `dyn Trait` | supported | Current semantic model handles concrete-to-dyn coercion and method dispatch typing. |

## Patterns

| AST family | Status | Notes |
| --- | --- | --- |
| Wildcard | supported | |
| Binding pattern | supported | |
| Literal pattern | supported | |
| Tuple pattern | supported | |
| Enum variant tuple pattern | supported | |
| Enum variant struct pattern | supported | |
| Pattern guards | supported | |
| OR-patterns | supported | Parsed, validated, included in exhaustiveness checks, and covered by validation tests. |
| Slice patterns | deferred | No parser or semantic contract yet. |

## Type System Surface

| Area | Status | Notes |
| --- | --- | --- |
| Primitive types | supported | `int`, `float`, `bool`, `char`, `string`, `unit`, plus numeric aliases over the current canonical ABI. |
| Tuples | supported | |
| Functions as types | supported | |
| Generics | supported | Includes current monomorphization pipeline. |
| Generic enums and nested inference | supported | Stabilized in the current test suite. |
| Trait bounds | supported | Current validation handles the supported generic method surface. |
| Trait objects | supported | Current dyn-trait flows compile in the test suite. |
| Higher-ranked generics | deferred | |
| Production-grade scientific numeric lattice | partial | Numeric aliases are accepted and checked; exact-width storage/overflow semantics remain future work. |
| Tensor handles | supported baseline | `std.tensor` Phase 3 APIs are typed as runtime handles plus host calls with accepted ADR semantics; first-class static tensor types remain future work. |
| Tensor autodiff | supported baseline | `std.tensor` Phase 5 APIs expose reverse-mode autodiff as typed host calls over float tensor handles; compiler-native autodiff syntax remains future work. |
| ML framework handles | supported baseline | `std.ml` Phase 6 APIs are typed as runtime handles plus host calls for modules, layers, losses, optimizers, and dataloaders. |

## Known Partial or Deferred Areas

- aggregate, string, tensor, and handle global initialization beyond the stable scalar `static` surface
- deeper class model finalization: future semantic backlog
- production-grade indexed collections and shape-aware tensor types: `R-201` through `R-304`
- exact-width numeric runtime semantics beyond the canonical alpha ABI
