# SpectraLang — Complete Language Reference for AI Agents

**Version:** Alpha  
**Source:** Verified against the compiler source code and working example projects (`examples/projects/spectra_academy`, `examples/projects/test_corpus`, `examples/projects/complex_demo`).  
**Purpose:** This document is the authoritative reference for AI agents generating SpectraLang code. Every rule is stated unambiguously. All code examples are known to compile and run correctly.

---

## Table of Contents

1. [Language Overview](#1-language-overview)
2. [Source File Structure](#2-source-file-structure)
3. [Module System & Imports](#3-module-system--imports)
4. [Primitive Types](#4-primitive-types)
5. [Variables](#5-variables)
6. [Operators](#6-operators)
7. [Functions](#7-functions)
8. [Control Flow](#8-control-flow)
9. [Arrays](#9-arrays)
10. [Tuples](#10-tuples)
11. [Records](#11-records)
12. [Enums](#12-enums)
13. [Impl Blocks & Methods](#13-impl-blocks--methods)
14. [Traits](#14-traits)
15. [Pattern Matching](#15-pattern-matching)
16. [Generics](#16-generics)
17. [Closures / Lambdas](#17-closures--lambdas)
18. [F-Strings](#18-f-strings)
19. [Type Casting](#19-type-casting)
20. [Constants & Statics](#20-constants--statics)
21. [Type Aliases](#21-type-aliases)
22. [Visibility](#22-visibility)
23. [Standard Library](#23-standard-library)
24. [Multi-Module Projects](#24-multi-module-projects)
25. [CLI Reference](#25-cli-reference)
26. [Complete Working Examples](#26-complete-working-examples)
27. [Interop Baseline](#27-interop-baseline)
28. [Package Manager Baseline](#28-package-manager-baseline)
29. [Tooling Baseline](#29-tooling-baseline)

---

## 1. Language Overview

SpectraLang is a statically typed, compiled, general-purpose language that:
- Compiles to native machine code via Cranelift (JIT and AOT modes)
- Has strict static typing — no implicit conversions except `int` → `float`
- Supports modules, generics, traits, enums, records, and closures
- Every source file is one module; each module is one file
- Documents `public func main() returns int { ... }` as the canonical entry point form

**File extension:** `.spectra` or `.spc`

---

## 2. Source File Structure

Every `.spectra` file must follow this exact order:

```
1. module declaration   (MANDATORY — first non-comment line)
2. import statements    (optional, zero or more)
3. top-level items      (functions, records, enums, traits, impls, consts, statics, type aliases)
```

### Module Declaration

```spectra
module module_name
```

- The module name becomes the public identity of the file.
- Module names use dot-separated identifiers for logical grouping: `module app.utils`
- By convention, the name mirrors the file path, but the compiler does not enforce this.
- The CLI uses the module name declared in the file for dependency resolution.

### Minimal Valid Program

```spectra
module hello

import std.io

public func main() returns int {
    println("Hello, World!")
    return 0
}
```

### Rules

- `module` must be the first non-comment, non-blank line.
- Only `//` line comments are supported. Block comments (`/* */`) are **not** supported.
- The canonical runnable entry point is `public func main() returns int { ... }`.
- Statements end at a line break or closing brace; semicolons are rejected by the parser.

---

## 3. Module System & Imports

### Import Forms

```spectra
// Form 1: stdlib module import
import std.io
// Usage: println("hello") and std.io.println("hello")

// Form 2: alias import
import std.math as math
// Usage: math.sqrt_f(25.0)

// Form 3: named import
from std.io import println, print
// Usage: println("hello")

// Form 4: public re-export
public from std.io import println
```

### Stdlib Module Names

| Import Statement | Module |
|---|---|
| `import std.io;` | I/O operations |
| `import std.math;` | Math functions |
| `import std.string;` | String operations |
| `import std.convert;` | Type conversions |
| `import std.collections;` | Dynamic lists |
| `import std.random;` | Random numbers |
| `import std.fs;` | File system |
| `import std.env;` | Environment & args |
| `import std.option;` | Option helpers |
| `import std.result;` | Result helpers |
| `import std.char;` | Char classification |
| `import std.time;` | Timestamps, sleep |
| `import std.range;` | Stored range handles |

### User Module Import

To use functions/types from another `.spectra` file in the same project:
```spectra
// In main.spectra
import sa_math
   // imports sa_math.spectra
import sa_grades
 // imports sa_grades.spectra

public func main() returns int {
    let g = gcd(48, 18)
         // from sa_math (unqualified)
    let grade = score_to_grade(95)
 // from sa_grades (unqualified)
    return 0
}
```

### Re-exports

```spectra
public from std.io import println  // makes println available to importers of this module
```

---

## 4. Primitive Types

| Type | Description | Literal Examples |
|------|-------------|-----------------|
| `int` | 64-bit signed integer | `0`, `42`, `-7`, `1_000` |
| `float` | 64-bit IEEE 754 double | `3.14`, `-0.5`, `1.0` |
| `bool` | Boolean | `true`, `false` |
| `string` | UTF-8 string | `"hello"`, `""` |
| `char` | Unicode code point | `'a'`, `'\n'`, `'\t'` |

**Unit type:** Functions with no return value or returning no meaningful result use `unit` (written as `()` in type contexts or simply omitted as the return type).

### Type Compatibility Rules

- **Allowed implicit conversion:** `int` → `float` in arithmetic expressions only.
- **All other conversions are explicit** — use `std.convert` functions.
- `bool` is **not** an integer. `bool + int` is a compile error.
- `int` and `float` cannot be used in the same arithmetic expression without explicit conversion.

---

## 5. Variables

### Declaration

```spectra
let name = value
           // type inferred
let name: Type = value
     // explicit type
let name: Type
             // declared without initializer (type required)
```

### Reassignment

All local variables are mutable by default. Simply assign a new value:

```spectra
let x = 10
x = 20
           // valid — x is now 20
x = x + 1
        // valid
```

### Rules

- `let` declares a new variable.
- Variables are scoped to their enclosing block `{ ... }`.
- There is no `const` evaluation for local variables — use top-level `const` for compile-time constants.
- `let mut` is accepted syntactically but all `let` variables are mutable by default.

### Examples

```spectra
module vars

public func main() returns int {
    let count = 0
    let name: string = "Alice"
    let flag: bool = true
    let ratio: float = 1.5

    count = count + 1
   // reassignment
    flag = false

    return count
}
```

---

## 6. Operators

### Arithmetic

| Operator | Operation | Types |
|----------|-----------|-------|
| `+` | Addition | `int`, `float` |
| `-` | Subtraction | `int`, `float` |
| `*` | Multiplication | `int`, `float` |
| `/` | Division (integer division for `int`) | `int`, `float` |
| `%` | Modulo | `int` |
| `-x` | Unary negation | `int`, `float` |

### Comparison

| Operator | Operation |
|----------|-----------|
| `==` | Equal |
| `!=` | Not equal |
| `<` | Less than |
| `>` | Greater than |
| `<=` | Less or equal |
| `>=` | Greater or equal |

### Logical

| Operator | Operation |
|----------|-----------|
| `&&` | Logical AND |
| `\|\|` | Logical OR |
| `!` | Logical NOT |

### Other

| Operator | Meaning |
|----------|---------|
| `..` | Exclusive range: `0..10` = 0 to 9 |
| `..=` | Inclusive range: `0..=10` = 0 to 10 |
| `as` | Type cast: `x as float` |
| `?` | Error propagation (try operator) |

### Compound Assignment

| Operator | Equivalent |
|----------|-----------|
| `+=` | `x = x + rhs` |
| `-=` | `x = x - rhs` |
| `*=` | `x = x * rhs` |
| `/=` | `x = x / rhs` |
| `%=` | `x = x % rhs` |

### Operator Precedence (highest to lowest)

1. `(expr)`, `f()`, `x.field`, `x[i]` — primary
2. `-x`, `!x` — unary
3. `*`, `/`, `%` — multiplicative
4. `+`, `-` — additive
5. `<`, `>`, `<=`, `>=` — relational
6. `==`, `!=` — equality
7. `&&` — logical AND
8. `||` — logical OR

---

## 7. Functions

### Declaration Syntax

```spectra
// Private function (default)
func name(param1: Type1, param2: Type2) returns ReturnType {
    // body
}

// Public function
public func name(param1: Type1) returns ReturnType {
    // body
}

// Function with no return value (unit return)
public func greet(name: string) {
    println(f"Hello, {name}!")
}

// Generic function
func identity<T>(x: T) returns T {
    return x
}

// Generic function with trait bound
func process<T: Clone>(item: T) returns int {
    return item.clone()
}
```

### Return

- `return expr` — explicit return.
- The last expression in a block is also a return value.
- `return` — returns `unit`.

```spectra
func add(a: int, b: int) returns int {
    return a + b
      // explicit return
}

func add2(a: int, b: int) returns int {
    a + b              // implicit return (last expression, no semicolon)
}
```

### Entry Point

Every runnable program must have exactly one `public func main() returns int { ... }`. The return value of `main` becomes the process exit code (0 = success).

```spectra
public func main() returns int {
    // program logic
    return 0
}
```

### Recursion

Functions can call themselves recursively:

```spectra
public func factorial(n: int) returns int {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}
```

---

## 8. Control Flow

### if / else if / else

```spectra
if condition {
    // ...
} else if other_condition {
    // ...
} else {
    // ...
}
```

- `else if` is the only accepted spelling for additional branches.
- Conditions must be `bool` — no implicit bool coercion.
- Braces `{ }` are always required.

```spectra
func classify(score: int) returns int {
    if score >= 90 {
        return 4
    } else if score >= 70 {
        return 3
    } else if score >= 50 {
        return 2
    } else {
        return 1
    }
}
```

### if not

`if not condition { ... }` is the canonical negated conditional form.

```spectra
if not value < 0 {
    println("value is non-negative")
}

// With else:
if not flag {
    // runs when flag is false
} else {
    // runs when flag is true
}
```

### while

```spectra
while condition {
    // body
}
```

```spectra
let i = 0
while i < 10 {
    i = i + 1
}
```

### do-while

```spectra
do {
    // body
} while condition
```

```spectra
let x = 0
do {
    x = x + 1
} while x < 5
// x is 5 after this
```

### for (range and iterator)

```spectra
// Exclusive range: i = 0, 1, 2, ..., 9
for i in 0..10 {
    // ...
}

// Inclusive range: i = 0, 1, 2, ..., 10
for i in 0..=10 {
    // ...
}

// Stored Range handle
let r: Range = 2..5
for i in r {
    // i = 2, 3, 4
}

// Array iteration
let arr = [10, 20, 30, 40, 50]
for item in arr {
    println(item)
}
```

Note: the canonical collection iteration syntax is `for x in iterable`.

### loop (infinite loop)

```spectra
loop {
    // body
    if condition {
        break
    }
}
```

### break and continue

```spectra
let i = 0
while i < 100 {
    if i == 5 { break
 }      // exit loop
    if i % 2 == 0 { continue
 } // skip even
    i = i + 1
}
```

### switch

`switch` compares a value against literal cases. The default arm uses `else => { ... }`:

```spectra
switch day {
    case 1: {
        println("Monday")
    }
    case 2: {
        println("Tuesday")
    }
    case 3: {
        println("Wednesday")
    }
}
```

With default:
```spectra
switch option {
    case 1: { result = 10
 }
    case 2: { result = 20
 }
    else: { result = 0
 }
}
```

**Note:** `switch` is stable syntax. `match` remains preferred for pattern-oriented branching.

---

## 9. Arrays

### Creation

```spectra
let arr = [1, 2, 3, 4, 5]
               // inferred type: [int]
let arr: [int] = [10, 20, 30]
           // explicit
let matrix = [[1, 2], [3, 4]]
           // nested arrays
```

### Access and Modification

```spectra
let arr = [10, 20, 30, 40, 50]
let first = arr[0]
     // 10
let last = arr[4]
      // 50

arr[2] = 99
            // set element
```

### Iteration

```spectra
let arr = [1, 2, 3, 4, 5]
let sum = 0
let i = 0
while i < 5 {
    sum = sum + arr[i]
    i = i + 1
}
```

Or with `for`:
```spectra
let sum = 0
for x in arr {
    sum = sum + x
}
```

### Rules

- Arrays are zero-indexed.
- Out-of-bounds access at runtime causes a panic.
- Array size is fixed at creation (for stack arrays).
- For dynamic arrays, use `std.collections.list_*` functions.

---

## 10. Tuples

### Creation

```spectra
let t = (1, "hello", true)
              // (int, string, bool)
let pair: (int, int) = (10, 20)
```

### Access

Use `.0`, `.1`, `.2`, ... to access tuple elements:

```spectra
let t = (42, "world")
let n = t.0
    // 42
let s = t.1
    // "world"
```

---

## 11. Structs

### Declaration

```spectra
record Point {
    x: int,
    y: int,
}

// With visibility modifiers on fields
public record Person {
    public name: string,
    public age: int,
}

// Generic record
record Wrapper<T> {
    value: T,
}
```

### Instantiation

```spectra
let p = Point { x: 3, y: 4 }
let x = 3
let y = 4
let p2 = Point { x, y }
 // shorthand for Point { x: x, y: y }
let person = Person { name: "Alice", age: 30 }
```

### Field Access

```spectra
let x = p.x
    // 3
let y = p.y
    // 4
```

### Field Mutation

```spectra
p.x = 10
       // direct field assignment
```

### Rules

- All fields require explicit type annotations.
- Fields are private by default — only accessible within `impl` blocks unless declared `public`.
- When instantiating, all fields must be provided (no defaults).
- Field shorthand is supported in literals: `Point { x }` means `Point { x: x }`.
- Field names must match exactly (case-sensitive).

---

## 12. Enums

### Unit Variants

```spectra
public enum Direction {
    North,
    South,
    East,
    West,
}
```

Usage:
```spectra
let d = Direction::North
let code = match d {
    when Direction::North then 1,
    when Direction::South then 2,
    when Direction::East then 3,
    when Direction::West then 4,
}
```

### Tuple Variants (carrying data)

```spectra
public enum Shape {
    Circle(int),           // radius
    Rect(int, int),        // width, height
    Dot,                   // unit variant
}

public enum Status {
    Active(int),
    Inactive,
    Error(int),
}
```

Usage:
```spectra
let s = Shape::Circle(5)
let area = match s {
    when Shape::Circle(r) then r * r * 3,
    when Shape::Rect(w, h) then w * h,
    when Shape::Dot then 0,
}

let status = Status::Active(200)
let code = match status {
    when Status::Active(v) then v,
    when Status::Inactive then 0,
    when Status::Error(e) then e * -1,
}
```

### Struct Variants (named fields)

```spectra
enum Event {
    Click { x: int, y: int },
    KeyPress { key: int },
    Quit,
}
```

Usage:
```spectra
let e = Event::Click { x: 10, y: 20 }
match e {
    when Event::Click { x, y } then {
        println(f"Click at {x},{y}")
    }
    when Event::KeyPress { key } then {
        println(key)
    }
    when Event::Quit then {
        println("quit")
    }
}
```

### Built-in Generic Enums

SpectraLang provides `Option<T>` and `Result<T, E>` as built-in generic enums:

```spectra
// Option<T>
let some_val: Option<int> = Option::Some(42)
let none_val: Option<int> = Option::None

let result = match some_val {
    when Option::Some(v) then v,
    when Option::None then 0,
}

// Result<T, E>
let ok_val: Result<int, string> = Result::Ok(100)
let err_val: Result<int, string> = Result::Err("failed")
```

### Rules

- Enum variants are always accessed with `EnumName::VariantName`.
- When matching a cross-module enum, the bare `VariantName` (without `EnumName::`) can be used in patterns if the enum type is clear from context.
- Generic enums require type arguments in some contexts: `Option<int>`, `Result<int, string>`.

---

## 13. Impl Blocks & Methods

### Inherent Impl (adding methods to a type)

```spectra
record Counter {
    value: int,
    step: int,
}

impl Counter {
    // Static constructor (no self)
    func new(step: int) returns Counter {
        Counter { value: 0, step: step }
    }

    // Immutable method (&self — read-only access)
    public func get(&self) returns int {
        self.value
    }

    // Returns a new value (pure functional style)
    public func increment(&self) returns Counter {
        Counter { value: self.value + self.step, step: self.step }
    }

    // Mutable method (&mut self)
    public func reset(&mut self) {
        self.value = 0
    }
}
```

Usage:
```spectra
let c = Counter::new(5)
    // static call: TypeName::method_name(args)
let c2 = c.increment()
     // method call: instance.method_name(args)
let v = c2.get()
           // 5
```

### Method Receivers

| Receiver | Syntax | Meaning |
|----------|--------|---------|
| None | `func new(...)` | Static/associated function — called as `Type::new(...)` |
| Value | `func method(self)` | Takes ownership of self |
| Immutable ref | `func method(&self)` | Read-only access to self's fields |
| Mutable ref | `func method(&mut self)` | Mutable access to self's fields |

### Self Inside Methods

Inside an `impl` block, `self` refers to the instance. Field access uses `self.field_name`:

```spectra
impl Point {
    public func magnitude_sq(&self) returns int {
        self.x * self.x + self.y * self.y
    }
}
```

### Visibility in Impl Blocks

- Methods inside `impl Type { }` are **private** by default.
- Prefix with `public` to make them callable from outside the module.
- Methods inside `impl Trait for Type { }` are **always public**.

---

## 14. Traits

### Trait Declaration

```spectra
trait Shape {
    func area(&self) returns int
                    // abstract method (no body)
    func perimeter(&self) returns int
               // abstract method
    func describe(&self) returns int {               // default implementation
        return 0
    }
}

// Trait with parent trait (inheritance)
trait Scalable: Shape {
    func scale(&self, factor: int) returns int
}
```

### Trait Implementation

```spectra
record Circle {
    radius: int,
}

impl Shape for Circle {
    func area(&self) returns int {
        self.radius * self.radius * 3
    }

    func perimeter(&self) returns int {
        self.radius * 6
    }
    // describe() uses default implementation
}
```

### Calling Trait Methods

Once a type implements a trait, call its methods like regular methods:

```spectra
let c = Circle { radius: 5 }
let a = c.area()
         // 75
let p = c.perimeter()
    // 30
```

### Rules

- All non-default trait methods must be implemented.
- Default methods may be overridden in the impl.
- Trait method signatures in the impl must exactly match the trait declaration.
- `Self` inside a trait refers to the type implementing the trait.

### Self Keyword in Traits

```spectra
trait Clone {
    func clone(self) returns Self
     // Self = implementing type
}

impl Clone for Point {
    func clone(self) returns Point {   // Point substituted for Self
        return Point { x: self.x, y: self.y }
    }
}
```

---

## 15. Pattern Matching

### match Expression

```spectra
match scrutinee {
    when Pattern1 then expression_or_block,
    when Pattern2 then expression_or_block,
    otherwise then default_expression,
}
```

### Pattern Types

#### Wildcard

```spectra
match value {
    otherwise then println("catch-all"),
}
```

#### Literal Patterns

```spectra
match score {
    when 100 then println("perfect"),
    when 0 then println("zero"),
    otherwise then println("other"),
}
```

#### Binding Patterns (capture the value)

```spectra
match x {
    when n then println(n),    // n is bound to the value of x
}
```

#### Enum Unit Variant Patterns

```spectra
match direction {
    when Direction::North then 1,
    when Direction::South then 2,
    when Direction::East then 3,
    when Direction::West then 4,
}
```

#### Enum Tuple Variant Patterns (destructuring)

```spectra
match status {
    when Status::Active(code) then code,
    when Status::Inactive then 0,
    when Status::Error(e) then e * -1,
}
```

Multiple fields:
```spectra
match shape {
    when Shape::Circle(r) then r * r * 3,
    when Shape::Rect(w, h) then w * h,
    when Shape::Dot then 0,
}
```

#### Enum Struct Variant Patterns

```spectra
match event {
    when Event::Click { x, y } then {
        return x + y
    }
    when Event::KeyPress { key } then {
        return key
    }
    when Event::Quit then {
        return 0
    }
}
```

#### Tuple and Struct Destructuring in let

```spectra
let pair = (10, 20)
let (left, right) = pair

let point = Point { x: left, y: right }
let Point { x, y: renamed_y } = point
```

#### OR-patterns

```spectra
match token {
    when Token::Number(value) then value,
    when Token::Plus | Token::Minus then 1,
}
```

#### Nested Patterns

```spectra
match wrapped {
    when Option::Some(inner) then match inner {
        when Option::Some(value) then value,
        when Option::None then 0,
    },
    when Option::None then 0,
}
```

### Match Arm Body

A match arm body can be:
- A single expression: `Pattern => expr,`
- A block: `Pattern => { stmts; expr }`
- A block with return: `Pattern => { return value; }`

```spectra
let result = match x {
    when 0 then 0,
    when 1 then 1,
    when n then n * 2,
}
```

### Exhaustiveness

Every `match` must cover all possible cases. Always add `_ =>` if not all cases are listed explicitly.

### Cross-Module Enum Matching

When matching an enum imported from another module, you can use bare variant names in patterns (the enum type is inferred from the scrutinee):

```spectra
import cx_geometry

let shape = cx_geometry::Shape::Circle(10)
let area = match shape {
    when Shape::Circle(r) then r * r * 3,   // bare variant name is OK in patterns
    when Shape::Rect(w, h) then w * h,
    when Shape::Dot then 0,
}
```

### if let

Pattern-match and bind in a conditional:

```spectra
if let Option::Some(v) = maybe_value {
    println(v)
} else {
    println("nothing")
}
```

### while let

Loop while a pattern matches:

```spectra
while let Option::Some(v) = get_next() {
    process(v)
}
```

---

## 16. Generics

### Generic Functions

```spectra
func identity<T>(x: T) returns T {
    return x
}

func first<T>(a: T, b: T) returns T {
    return a
}
```

### Generic Functions with Trait Bounds

```spectra
func process<T: Clone>(item: T) returns int {
    return item.clone()
}

// Multiple bounds with +
func debug_and_clone<T: Debug + Clone>(item: T) returns int {
    return item.debug()
}
```

### Generic Structs

```spectra
record Wrapper<T> {
    value: T,
}

let w = Wrapper { value: 42 }
let v = w.value
    // 42
```

### Generic Enums

```spectra
enum Option<T> {
    Some(T),
    None,
}

enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

Usage with type annotation when needed:
```spectra
let opt: Option<int> = Option::Some(100)
let res: Result<int, string> = Result::Ok(42)
```

### Rules

- Generic type parameters use `PascalCase` names (`T`, `E`, `Key`, `Value`).
- Generic argument inference is limited — when the compiler cannot infer the type, add explicit type annotations.
- Generic implementations work via monomorphization (the compiler generates specialized versions for each concrete type used).

---

## 17. Closures / Lambdas

### Syntax

```spectra
|param1, param2| expression

|param1: Type, param2: Type| expression

|param| {
    // multi-line body
    expression
}
```

### Examples

```spectra
let double = |x| x * 2
let add = |a, b| a + b

let result = double(5)
    // 10
let sum = add(3, 4)
       // 7
```

With type annotations:
```spectra
let multiply: func(int, int) returns int = |a: int, b: int| a * b
```

### Passing Closures to Functions

Closures are used with higher-order stdlib functions:
```spectra
import std.collections

let lst = list_new()
list_push(lst, 1)
list_push(lst, 2)
list_push(lst, 3)

let doubled = list_map(lst, |x| x * 2)
let evens = list_filter(lst, |x| x % 2 == 0)
let total = list_reduce(lst, 0, |acc, x| acc + x)
```

### Captures

Closures capture external variables by value when the closure is created:

```spectra
func make_adder(delta: int) returns func(int) returns int {
    return |x: int| x + delta
}
```

Captured variables cannot be assigned directly inside the closure body. Return a new value instead of relying on mutable/reference captures.

---

## 18. F-Strings

F-strings allow inline expression interpolation:

```spectra
let name = "Alice"
let age = 30
let greeting = f"Hello, {name}! You are {age} years old."
println(greeting)
// Output: Hello, Alice! You are 30 years old.
```

### Rules

- Prefix the string literal with `f`: `f"text {expr} more text"`
- Expressions inside `{}` are evaluated and converted to string automatically.
- F-strings produce a `string` value.
- Any expression can be interpolated: variables, function calls, arithmetic.

```spectra
let x = 10
let y = 20
println(f"Sum: {x + y}")
              // Sum: 30
println(f"Area: {3.14 * 5.0 * 5.0}")
 // Area: 78.5
```

---

## 19. Type Casting

### as Operator

Cast between numeric types:

```spectra
let n: int = 42
let f: float = n as float
     // int → float

let pi: float = 3.14
let i: int = pi as int
        // float → int (truncates)
```

Numeric aliases currently accepted by the compiler:

```spectra
let a: i32 = 10
let b: u64 = 20
let c: f32 = 1.5
let d: bf16 = 2.5
```

Alpha ABI note: integer aliases canonicalize to `int`; float aliases canonicalize to `float`. Exact-width storage and overflow behavior are future backend/runtime work.

### std.convert Functions

For all other conversions:

```spectra
import std.convert

let n = 42
let s = int_to_string(n)
           // "42"
let f = int_to_float(n)
            // 42.0

let text = "123"
let parsed = string_to_int(text)
   // 123
let safe = string_to_int_or(text, 0)
 // 0 if parse fails
```

---

## 20. Constants & Statics

### Constants

Compile-time constant — value is evaluated at compile time:

```spectra
const MAX_SIZE: int = 1000
const PI: float = 3.14159
public const VERSION: string = "1.0.0"
```

### Statics

Module-level mutable variable — initialized once at startup:

```spectra
static counter: int = 0
public static global_flag: bool = false
```

### Rules

- `const` values cannot be mutated.
- `const` initializers must be compile-time expressions: literals, earlier constants, grouping, unary/binary operations, valid casts, and string concatenation.
- `static` variables are mutable globals.
- Both `const` and `static` can have `public` visibility.
- Type annotation is optional but recommended.

---

## 21. Type Aliases

```spectra
type Score = int
type Name = string
public type IntPair = (int, int)
```

Usage:
```spectra
let s: Score = 95
let n: Name = "Alice"
```

---

## 22. Visibility

| Modifier | Scope |
|----------|-------|
| `public` | Public — accessible from any module that imports this one |
| `internal` | Internal — accessible only within the same package |
| *(none)* | Private — accessible only within this module |

### Visibility Rules

- Functions: `public func`, `func` (private)
- Records: `public record`, `record` (private)
- Record fields: `public field: Type`, `field: Type` (private — only accessible from `impl` blocks)
- Enums: `public enum`, `enum` (private)
- Impl methods: `public func`, `func` (private by default in inherent impls)
- Trait impl methods: always public
- Constants/statics: `pub const`, `const`, `pub static`, `static`

### Public Visibility Requirement for Cross-Module Use

For a function, record, or enum to be usable from another module, it must be declared `public`. Fields that need to be accessed outside of the type's own `impl` block must also be `public`.

```spectra
// sa_student.spectra
public record Student {         // public record — accessible from other modules
    id: int,                 // private field — only accessible in impl Student
    public name: string,        // pub field — accessible everywhere
}

public func student_average(...) returns int { ... }   // public func — accessible from other modules
func internal_helper() returns int { ... }          // private — only within this module
```

---

## 23. Standard Library

All stdlib functions become available unqualified after importing their module. For example, `import std.io` makes `println`, `print`, etc. available directly.

### std.io — Input / Output

```spectra
import std.io
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `println` | `(value: any) returns unit` | Print value + newline to stdout |
| `print` | `(value: any) returns unit` | Print value (no newline) to stdout |
| `eprint` | `(value: any) returns unit` | Print value (no newline) to stderr |
| `eprintln` | `(value: any) returns unit` | Print value + newline to stderr |
| `flush` | `() returns unit` | Flush stdout buffer |
| `read_line` | `() returns string` | Read one line from stdin (strips newline) |
| `input` | `(prompt: string) returns string` | Print prompt, flush, read line |

```spectra
import std.io

println("Hello!")
              // Hello!\n
print("Enter: ")
let line = read_line()
let name = input("Name: ")
```

### std.math — Mathematics

```spectra
import std.math
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `abs` | `(n: int) returns int` | Absolute value |
| `min` | `(a: int, b: int) returns int` | Minimum of two ints |
| `max` | `(a: int, b: int) returns int` | Maximum of two ints |
| `clamp` | `(v: int, lo: int, hi: int) returns int` | Clamp v to [lo, hi] |
| `sign` | `(n: int) returns int` | Returns -1, 0, or 1 |
| `gcd` | `(a: int, b: int) returns int` | Greatest common divisor |
| `lcm` | `(a: int, b: int) returns int` | Least common multiple |
| `sqrt_f` | `(x: float) returns float` | Square root |
| `pow_f` | `(base: float, exp: float) returns float` | Power |
| `floor_f` | `(x: float) returns float` | Floor |
| `ceil_f` | `(x: float) returns float` | Ceiling |
| `round_f` | `(x: float) returns float` | Round |
| `sin_f` | `(x: float) returns float` | Sine |
| `cos_f` | `(x: float) returns float` | Cosine |
| `tan_f` | `(x: float) returns float` | Tangent |
| `log_f` | `(x: float) returns float` | Natural log |
| `log2_f` | `(x: float) returns float` | Log base 2 |
| `log10_f` | `(x: float) returns float` | Log base 10 |
| `atan2_f` | `(y: float, x: float) returns float` | atan2 |
| `pi` | `() returns float` | π constant |
| `e_const` | `() returns float` | e constant |
| `abs_f` | `(x: float) returns float` | Absolute value (float) |
| `is_nan_f` | `(x: float) returns bool` | Is NaN? |
| `is_infinite_f` | `(x: float) returns bool` | Is infinite? |

```spectra
import std.math

let m = max(10, 20)
                  // 20
let a = abs(-5)
                      // 5
let r = sqrt_f(16.0)
                 // 4.0
let pi_val = pi()
                    // 3.14159...
let clamped = clamp(150, 0, 100)
    // 100
```

### std.string — String Operations

```spectra
import std.string
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `len` | `(s: string) returns int` | Length in bytes |
| `contains` | `(s: string, sub: string) returns bool` | Contains substring? |
| `starts_with` | `(s: string, prefix: string) returns bool` | Starts with prefix? |
| `ends_with` | `(s: string, suffix: string) returns bool` | Ends with suffix? |
| `to_upper` | `(s: string) returns string` | Uppercase |
| `to_lower` | `(s: string) returns string` | Lowercase |
| `trim` | `(s: string) returns string` | Remove leading/trailing whitespace |
| `concat` | `(a: string, b: string) returns string` | Concatenate two strings |
| `substring` | `(s: string, start: int, end: int) returns string` | Slice [start, end) |
| `replace` | `(s: string, from: string, to: string) returns string` | Replace all occurrences |
| `index_of` | `(s: string, sub: string) returns int` | First occurrence index (-1 if not found) |
| `split_first` | `(s: string, sep: string) returns string` | Part before first sep |
| `split_last` | `(s: string, sep: string) returns string` | Part after last sep |
| `split_by` | `(s: string, sep: string) returns int` | Split into list (returns list handle) |
| `is_empty` | `(s: string) returns bool` | Is empty? |
| `count_occurrences` | `(s: string, sub: string) returns int` | Count occurrences |
| `char_at` | `(s: string, i: int) returns int` | Char code at index (-1 if OOB) |
| `repeat_str` | `(s: string, n: int) returns string` | Repeat string n times |
| `pad_left` | `(s: string, width: int, pad_char: int) returns string` | Left-pad with char |
| `pad_right` | `(s: string, width: int, pad_char: int) returns string` | Right-pad with char |
| `reverse_str` | `(s: string) returns string` | Reverse |

```spectra
import std.string

let s = "  Hello, World!  "
let trimmed = trim(s)
                          // "Hello, World!"
let upper = to_upper("hello")
                  // "HELLO"
let n = len("abc")
                             // 3
let has = contains("foobar", "oba")
            // true
let joined = concat("foo", "bar")
              // "foobar"
let sub = substring("hello", 1, 4)
            // "ell"
```

### std.convert — Type Conversions

```spectra
import std.convert
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `int_to_string` | `(n: int) returns string` | Int to string |
| `float_to_string` | `(f: float) returns string` | Float to string |
| `bool_to_string` | `(b: bool) returns string` | Bool to "true"/"false" |
| `string_to_int` | `(s: string) returns int` | Parse int (0 on error) |
| `string_to_float` | `(s: string) returns float` | Parse float (0.0 on error) |
| `string_to_int_or` | `(s: string, default: int) returns int` | Parse int with fallback |
| `string_to_float_or` | `(s: string, default: float) returns float` | Parse float with fallback |
| `string_to_bool` | `(s: string) returns bool` | "true" → true |
| `int_to_float` | `(n: int) returns float` | Int to float |
| `float_to_int` | `(f: float) returns int` | Float to int (truncates) |
| `bool_to_int` | `(b: bool) returns int` | true→1, false→0 |

```spectra
import std.convert

let s = int_to_string(42)
          // "42"
let n = string_to_int("123")
       // 123
let f = int_to_float(7)
            // 7.0
let i = float_to_int(3.99)
         // 3 (truncated)
```

### std.collections — Typed Collections

```spectra
import std.collections
```

The source contract uses typed `List<T>` and `Map<K,V>` values. Runtime handles
are opaque implementation details and must not be treated as user-visible
integers. Operations whose result may be absent return `Option<T>`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `list_new<T>` | `() returns List<T>` | Create a typed list |
| `list_push<T>` | `(List<T>, T) returns unit` | Append value |
| `list_len<T>` | `(List<T>) returns int` | Length |
| `list_get<T>` | `(List<T>, int) returns Option<T>` | Get element or `None` |
| `list_set<T>` | `(List<T>, int, T) returns unit` | Set element |
| `list_pop<T>` | `(List<T>) returns Option<T>` | Remove and return last or `None` |
| `list_pop_front<T>` | `(List<T>) returns Option<T>` | Remove and return first or `None` |
| `list_insert_at<T>` | `(List<T>, int, T) returns unit` | Insert at index |
| `list_remove_at<T>` | `(List<T>, int) returns Option<T>` | Remove at index or `None` |
| `list_contains<T>` | `(List<T>, T) returns bool` | Contains value? |
| `list_index_of<T>` | `(List<T>, T) returns int` | First index or `-1` |
| `list_sort` | `(List<int>) returns unit` | Sort ascending in-place |
| `list_sort_by` | `(List<int>, func(int,int) returns int) returns unit` | Sort with comparator |
| `list_map` | `(List<int>, func(int) returns int) returns List<int>` | Map to a new list |
| `list_filter` | `(List<int>, func(int) returns bool) returns List<int>` | Filter to a new list |
| `list_reduce` | `(List<int>, int, func(int,int) returns int) returns int` | Reduce to single value |
| `list_clear<T>` | `(List<T>) returns unit` | Remove all elements |
| `list_free<T>` | `(List<T>) returns unit` | Release list resources |
| `list_free_all` | `() returns int` | Free all lists |

```spectra
import std.collections
import std.option as option
from std.collections import List

let lst: List<int> = list_new()
list_push(lst, 10)
list_push(lst, 20)
list_push(lst, 30)

let n = list_len(lst)
           // 3
let first = list_get(lst, 0)
    // Some(10)
if option.is_none(first) {
    return 1
}

let doubled = list_map(lst, |x: int| x * 2)
let total = list_reduce(lst, 0, |acc: int, x: int| acc + x)
  // 60

list_free(lst)
```

Legacy sentinel behavior is available only through an explicit
`import std.compat.collections` import. It is not the stable default.

### std.tensor — Tensor Handles

```spectra
import std.tensor as tensor
```

The current production tensor ABI uses opaque `int` handles managed by the runtime. New code can use first-class annotations such as `Tensor<float, rank1, dim3, row_major, cpu>` and `Tensor<float, rank2, dim2, dim2, row_major, cpu>`; the compiler keeps dtype/rank/dimension/layout/device metadata while lowering to the existing handle ABI. Tensors store CPU data with dtype `int` or `float`, shape, strides, layout, shared storage, and safe view offsets. Shared-storage mutation uses copy-on-write.

| Function | Signature | Description |
|----------|-----------|-------------|
| `vector_f` | `(size: int, value: float) returns Tensor<float, rank1>` | 1D float tensor |
| `matrix_f` | `(rows: int, cols: int, value: float) returns Tensor<float, rank2>` | 2D float tensor |
| `zeros`, `ones` | `(size: int) returns int` | 1D int tensor |
| `full` | `(size: int, value: int) returns int` | 1D int tensor |
| `full_f` | `(size: int, value: float) returns int` | 1D float tensor |
| `arange` | `(start: int, end: int, step: int) returns int` | 1D int range |
| `zeros2`, `ones2` | `(rows: int, cols: int) returns int` | 2D int tensor |
| `full2`, `full2_f` | `(rows: int, cols: int, value) returns int` | 2D tensor |
| `uniform`, `uniform_f`, `normal_f`, `bernoulli`, `categorical` | random fills | Seeded tensor samples |
| `len`, `rank`, `dim`, `rows`, `cols` | metadata queries | Shape metadata |
| `get`, `get_f`, `get2`, `get2_f` | indexed reads | Scalar value |
| `set`, `set_f`, `set2`, `set2_f` | indexed writes | `unit` |
| `reshape`, `flatten`, `permute`, `slice` | view-capable shape transforms | New tensor handle |
| `concat`, `stack` | combine compatible tensors | New tensor handle |
| `add`, `sub`, `mul`, `div` | elementwise ops | New tensor handle |
| `neg`, `relu`, `exp_f`, `log_f`, `sqrt_f`, `sigmoid_f`, `tanh_f` | unary kernels | New tensor handle |
| `sum`, `sum_f`, `mean_f`, `min`, `max`, `argmax` | reductions | Scalar value |
| `sum_t`, `mean_t`, `dot_t` | differentiable scalar tensor losses | Tensor handle |
| `matmul`, `matmul_batched`, `transpose`, `dot` | matrix/vector kernels | Handle or scalar |
| `requires_grad`, `backward`, `grad`, `zero_grad` | reverse-mode autodiff | Training support |
| `set_grad_enabled`, `grad_enabled`, `stats_graph_nodes` | inference mode and graph lifecycle | Autograd control |
| `seed`, `stats_*`, `reset_stats` | RNG and runtime metrics | Determinism and observability |
| `free`, `free_all` | release handles | `unit` / freed count |

```spectra
let typed: Tensor<float, rank1, dim3, row_major, cpu> = [1.0, 2.0, 3.0]
let any_len: Tensor<float, rank1, dynamic_dim, row_major, cpu> = typed
let matrix_typed: Tensor<float, rank2, dim2, dim2, row_major, cpu> = [[1.0, 2.0], [3.0, 4.0]]

let a = tensor.arange(1, 5, 1)
let b = tensor.full(4, 2)
let c = tensor.add(a, b)

let total = tensor.sum(c)
       // 18
let matrix = tensor.reshape(tensor.arange(1, 7, 1), 2, 3)
let product = tensor.matmul(matrix, tensor.ones2(3, 2))
let random = tensor.uniform(8, 0, 10)

tensor.free_all()
```

Current baseline: tensors are still handle-backed, but Phase 14 is complete for the current production surface. `Tensor<dtype, rankN, dimN|dynamic_dim, layout, device>` and rank1/rank2 literals are available with stable mismatch diagnostics. Operation-aware static shape checks cover declared compatibility, elementwise tensor operations, `tensor.matmul`, `tensor.reshape`, and `ml.linear`.

Phase 5 autodiff is available for float tensors through `std.tensor`. Use `requires_grad(x, true)`, produce a scalar tensor loss with `sum_t`, `mean_t`, `dot_t`, or `std.ml` loss functions, then either call `backward(loss)` or use the language-level block `diff { loss_expression }`. Read gradients with `grad(x)`. Use `set_grad_enabled(false)` for inference/no-grad sections. Unsupported qualified stdlib operations inside `diff { ... }` fail with `E1406`.

Phase 7 acceleration is available through CPU device `0` and optional `wgpu` device `6`. Use `device(handle)` to inspect placement, `device_available(code)` before selecting a target, `to_device(handle, 0)` / `cpu(handle)` for CPU materialization, `to_device(handle, 6)` for `wgpu` float tensors when the CLI/runtime is built with `--features gpu`, `sync(handle)` as the synchronization point, and `stats_device_transfers()` for transfer accounting. Codes `1` CUDA, `2` ROCm, `3` Metal, `4` DirectML, and `5` Vulkan are reserved. Mixed precision uses `precision(handle)` and `to_precision(handle, code)` where `0` is f64, `1` is f32, `2` is f16, and `3` is bf16; loss-scaling flows use `std.ml.unscale_grad(parameter, scale)`.

### std.ml — ML Framework Handles

```spectra
import std.ml as ml
```

Phase 6 exposes a runtime-backed ML layer over `std.tensor`:

| Function group | Functions |
|----------------|-----------|
| modules | `module_new`, `module_add_parameter`, `module_parameter_count`, `module_parameter`, `module_set_training`, `module_is_training` |
| layers | `linear`, `conv2d`, `dropout`, `max_pool2d` |
| losses | `mse_loss`, `bce_loss`, `cross_entropy_loss`, `nll_loss` |
| optimizers | `sgd_step`, `sgd_momentum_step`, `adam_step`, `adamw_step`, `exp_lr` |
| data | `dataset_from_tensors`, `dataset_len`, `dataloader_new`, `dataloader_batch_count`, `dataloader_batch_features`, `dataloader_batch_labels` |

Runtime tests validate MLP and convolutional toy-model convergence. Spectra examples `72_ml_phase6_mlp_training.spectra` and `73_ml_phase6_cnn_training.spectra` verify public API integration.

### std.random — Random Numbers

```spectra
import std.random
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `random_seed` | `(seed: int) returns unit` | Set RNG seed |
| `random_int` | `(min: int, max: int) returns int` | Random int in [min, max] |
| `random_float` | `() returns float` | Random float in [0.0, 1.0) |
| `random_bool` | `() returns bool` | Random bool |

```spectra
import std.random

random_seed(42)
let n = random_int(1, 100)
    // 1 to 100
let f = random_float()
        // 0.0 to 0.999...
let b = random_bool()
```

### std.fs — File System

```spectra
import std.fs as fs
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `fs_read` | `(path: string) returns Result<string, Error>` | Read entire file |
| `fs_write` | `(path: string, content: string) returns Result<bool, Error>` | Write file, creating missing parent directories when possible |
| `fs_append` | `(path: string, content: string) returns Result<bool, Error>` | Append to file, creating missing parent directories when possible |
| `fs_exists` | `(path: string) returns Result<bool, Error>` | File exists? Missing paths are `Ok(false)` |
| `fs_remove` | `(path: string) returns Result<bool, Error>` | Delete file |

Filesystem operations return `Result` values for ordinary filesystem failures;
inspect `std.error` instead of guessing from a sentinel. Legacy behavior is
available only through an explicit `import std.compat.fs`.

```spectra
import std.fs as fs

let write_result = fs.fs_write("target/artifacts/output.txt", "Hello\n")
let content_result = fs.fs_read("output.txt")
let exists_result = fs.fs_exists("output.txt")
```

### std.error — Structured Errors

```spectra
import std.error as error
from std.error import ErrorCode

let failure = error.new(ErrorCode::NotFound, "missing", "fs_read", "data.txt", "agent", false)
let code = error.code(failure)
let operation = error.operation(failure)
```

`Error` also exposes `message`, `context`, `origin`, and `retryable` through
the corresponding accessors. `ErrorCode` is closed and currently maps
`InvalidArgument`, `NotFound`, `PermissionDenied`, `Io`, `Internal`, and
`Unsupported` to stable numeric codes.

### std.env — Environment

```spectra
import std.env
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `env_get` | `(key: string) returns Option<string>` | Get env var without conflating absence and an empty value |
| `env_set` | `(key: string, value: string) returns bool` | Set env var |
| `env_args_count` | `() returns int` | Number of CLI arguments |
| `env_arg` | `(index: int) returns Option<string>` | Argument at index, or None if out of bounds |

```spectra
import std.env
import std.option

let home_option = env_get("HOME")
let home = option_unwrap_or(home_option, "")
let argc = env_args_count()
let first_arg_option = env_arg(0)
let first_arg = option_unwrap_or(first_arg_option, "")
```

For legacy empty-string sentinel behavior, import `std.compat.env` explicitly;
new code should keep absence represented as `Option<string>`.

### std.option — Option Helpers

```spectra
import std.option
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `is_some` | `(opt: any) returns bool` | Is Some? |
| `is_none` | `(opt: any) returns bool` | Is None? |
| `option_unwrap` | `(opt: any) returns any` | Unwrap (runtime error on None) |
| `option_unwrap_or` | `(opt: any, default: any) returns any` | Unwrap or default |

```spectra
import std.option

let v: Option<int> = Option::Some(42)
let has = is_some(v)
                      // true
let val = option_unwrap(v)
                // 42
let safe = option_unwrap_or(v, 0)
        // 42
```

### std.result — Result Helpers

```spectra
import std.result
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `is_ok` | `(res: any) returns bool` | Is Ok? |
| `is_err` | `(res: any) returns bool` | Is Err? |
| `result_unwrap` | `(res: any) returns any` | Unwrap Ok (runtime error on Err) |
| `result_unwrap_or` | `(res: any, default: any) returns any` | Unwrap Ok or default |
| `result_unwrap_err` | `(res: any) returns any` | Unwrap Err (runtime error on Ok) |

```spectra
import std.result

let r: Result<int, string> = Result::Ok(100)
let ok = is_ok(r)
                           // true
let val = result_unwrap(r)
                   // 100
```

### std.char — Character Operations

```spectra
import std.char
```

All functions take a Unicode code point as `int`:

| Function | Signature | Description |
|----------|-----------|-------------|
| `is_alpha` | `(c: int) returns bool` | Is letter? |
| `is_digit_char` | `(c: int) returns bool` | Is digit (0-9)? |
| `is_whitespace_char` | `(c: int) returns bool` | Is whitespace? |
| `is_upper_char` | `(c: int) returns bool` | Is uppercase? |
| `is_lower_char` | `(c: int) returns bool` | Is lowercase? |
| `is_alphanumeric` | `(c: int) returns bool` | Is letter or digit? |
| `to_upper_char` | `(c: int) returns int` | Uppercase code point |
| `to_lower_char` | `(c: int) returns int` | Lowercase code point |

```spectra
import std.char
import std.string

let code = char_at("Hello", 0)
   // 72 (= 'H')
let is_upper = is_upper_char(code)
  // true
let lower_code = to_lower_char(code)
 // 104 (= 'h')
```

### std.time — Time Functions

```spectra
import std.time
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `time_now_millis` | `() returns int` | Milliseconds since Unix epoch (-1 on error) |
| `time_now_secs` | `() returns int` | Seconds since Unix epoch (-1 on error) |
| `sleep_ms` | `(ms: int) returns unit` | Sleep for ms milliseconds |
| `monotonic_millis` | `() returns int` | Monotonic milliseconds since runtime start |
| `monotonic_nanos` | `() returns int` | Monotonic nanoseconds since runtime start |
| `duration_ms` | `(ms: int) returns Duration` | Create an opaque duration handle |
| `duration_secs` | `(secs: int) returns Duration` | Create an opaque duration handle |
| `duration_millis` | `(duration: Duration) returns int` | Read a duration in milliseconds |
| `duration_secs_value` | `(duration: Duration) returns int` | Read a duration in whole seconds |
| `duration_add` | `(lhs: Duration, rhs: Duration) returns Duration` | Checked duration addition |
| `duration_sub` | `(lhs: Duration, rhs: Duration) returns Duration` | Checked duration subtraction |
| `instant_now` | `() returns Instant` | Capture a monotonic instant |
| `instant_elapsed_ms` | `(instant: Instant) returns int` | Milliseconds elapsed since instant |
| `instant_add` | `(instant: Instant, duration: Duration) returns Instant` | Create a deadline instant |
| `instant_has_elapsed` | `(instant: Instant) returns bool` | Check whether a deadline passed |
| `sleep` | `(duration: Duration) returns unit` | Sleep for a checked duration |
| `unix_to_utc` | `(secs: int) returns UtcDateTime` | Convert Unix seconds to UTC |
| `utc_year/month/day/hour/minute/second` | `(dt: UtcDateTime) returns int` | Extract UTC fields |

```spectra
import std.time

let start = time_now_millis()
sleep_ms(100)
let elapsed = time_now_millis() - start

let deadline = instant_add(instant_now(), duration_ms(10))
sleep(duration_ms(10))
let done = instant_has_elapsed(deadline)

let epoch = unix_to_utc(0)
let year = utc_year(epoch)
 // 1970
```

---

## 24. Multi-Module Projects

### Project Structure

A SpectraLang project is a directory containing `.spectra` files. The CLI discovers all `.spectra` files in the directory (and subdirectories), resolves dependencies based on `import` statements, and compiles them in topological order.

```
my_project/
├── spectra.toml     (optional project manifest)
├── main.spectra     (module main_project — has public func main())
├── utils.spectra    (module utils)
├── models.spectra   (module models)
└── helpers.spectra  (module helpers)
```

### spectra.toml (Optional Project Manifest)

```toml
[project]
name = "my_project"
version = "0.1.0"
entry = "main.spectra"    # optional, explicit entry point

# Optional source directories (default: src/)
src_dirs = ["src", "lib"]
```

### Module Dependency Rules

1. Each `.spectra` file declares exactly one module with `module name`.
2. Module names must be **unique** across the project.
3. Modules that depend on others list them with `import`.
4. Circular dependencies are not allowed.
5. Standard library modules (`std.*`) do not require corresponding `.spectra` files.

### Running a Multi-Module Project

```bash
# Run all .spectra files in a directory
spectralang run my_project/

# Run specific files (for examples/projects/complex_demo with mixed modules)
spectralang run main.spectra module_a.spectra module_b.spectra
```

### Cross-Module Function Calls

After `import module_name;`, public items from that module participate in name resolution for the current compilation:

```spectra
// sa_report.spectra
import sa_grades

public func score_to_gpa(score: int) returns int {
    let g = score_to_grade(score)
    return grade_points(g)
}
```

### Cross-Module Struct/Enum Usage

```spectra
// cx_ledger.spectra
public record Account {
    id: int,
    balance: int,
}

public enum TxKind {
    Credit(int),
    Debit(int),
}

// In cx_main.spectra
import cx_ledger

let acc = make_account(1, 500)
      // factory function (recommended)
let credit = TxKind::Credit(100)
```

**Important:** Direct record construction across modules is only possible if the record and its fields are both `public`. The recommended pattern is to provide a factory function (`public func new(...)`) in the owning module.

---

## 25. CLI Reference

### Commands

| Command | Description |
|---------|-------------|
| `spectralang run <files/dir>` | Compile and execute via JIT |
| `spectralang compile <files/dir>` | Compile without executing |
| `spectralang check <files/dir>` | Type-check only (no code generation) |
| `spectralang lint <files/dir>` | Run lint checks |
| `spectralang fmt <files>` | Format source files |
| `spectralang repl` | Start interactive REPL |
| `spectralang new <name>` | Scaffold new project |
| `spectralang help` | Show help |

### Common Flags

| Flag | Description |
|------|-------------|
| `--run` / `-r` | Execute after compilation (JIT) |
| `--emit-object <path>` | Generate native object file (AOT) |
| `--emit-exe <path>` | Generate native executable (AOT) |
| `--no-optimize` / `-O0` | No optimizations |
| `-O1` | Constant folding |
| `-O2` | Constant folding + dead code elimination (default) |
| `-O3` | All optimizations |
| `--dump-ast` | Print AST to stderr (debug) |
| `--dump-ir` | Print IR to stderr (debug) |
| `--verbose` / `-v` | Verbose build output |
| `--summary` | Per-module pipeline summary |
| `--json` | JSON diagnostic output (`compile`, `check`, `lint`, and `repl --json`) |

### Stable Control-Flow Features

| Feature | Description |
|---------|-------------|
| `switch` | `switch/case` statement |
| `if not` | Negated conditional |
| `do-while` | `do { } while` loop |
| `loop` | Infinite `loop { }` |

> Note: There is no experimental gating mechanism. Stable syntax parses unconditionally.

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `64` | Usage error (bad arguments) |
| `65` | Compilation error |
| `74` | I/O error |

### Lint Rules

| Rule | Description |
|------|-------------|
| `unused-binding` | Variable declared but never used |
| `unreachable-code` | Code after `return` is unreachable |
| `shadowing` | Variable shadows a variable in a parent scope |

---

## 26. Complete Working Examples

### Example 1: Basic Math Module + Main

**math.spectra**
```spectra
module math

public func gcd(a: int, b: int) returns int {
    if b == 0 { return a
 }
    return gcd(b, a % b)
}

public func factorial(n: int) returns int {
    if n <= 1 { return 1
 }
    return n * factorial(n - 1)
}

public func power(base: int, exp: int) returns int {
    let result = 1
    let i = 0
    while i < exp {
        result = result * base
        i = i + 1
    }
    return result
}

public func is_prime(n: int) returns bool {
    if n < 2 { return false
 }
    if n == 2 { return true
 }
    if n % 2 == 0 { return false
 }
    let i = 3
    while i * i <= n {
        if n % i == 0 { return false
 }
        i = i + 2
    }
    return true
}
```

**main.spectra**
```spectra
module main

import std.io
import math

public func main() returns int {
    println(gcd(48, 18))
        // 6
    println(factorial(6))
       // 720
    println(power(2, 10))
       // 1024
    println(is_prime(17))
       // true
    return 0
}
```

Run:
```bash
spectralang run main.spectra math.spectra
```

---

### Example 2: Structs, Enums, and Traits

```spectra
module shapes

import std.io

trait Area {
    func area(&self) returns int
}

trait Perimeter {
    func perimeter(&self) returns int
}

record Rectangle {
    width: int,
    height: int,
}

record Circle {
    radius: int,
}

impl Rectangle {
    public func new(w: int, h: int) returns Rectangle {
        Rectangle { width: w, height: h }
    }
}

impl Area for Rectangle {
    func area(&self) returns int {
        self.width * self.height
    }
}

impl Perimeter for Rectangle {
    func perimeter(&self) returns int {
        2 * (self.width + self.height)
    }
}

impl Area for Circle {
    func area(&self) returns int {
        self.radius * self.radius * 3
    }
}

impl Perimeter for Circle {
    func perimeter(&self) returns int {
        self.radius * 6
    }
}

public func main() returns int {
    let r = Rectangle::new(5, 3)
    let c = Circle { radius: 4 }

    println(r.area())
       // 15
    println(r.perimeter())
  // 16
    println(c.area())
       // 48
    println(c.perimeter())
  // 24

    return 0
}
```

---

### Example 3: Enums with Match and Option

```spectra
module grade_system

import std.io

public enum Grade {
    A,
    B,
    C,
    D,
    F,
}

public func score_to_grade(score: int) returns Grade {
    if score >= 90 { return Grade::A
 }
    if score >= 80 { return Grade::B
 }
    if score >= 70 { return Grade::C
 }
    if score >= 60 { return Grade::D
 }
    return Grade::F
}

public func grade_to_points(g: Grade) returns int {
    match g {
        when Grade::A then 4,
        when Grade::B then 3,
        when Grade::C then 2,
        when Grade::D then 1,
        when Grade::F then 0,
    }
}

func safe_divide(a: int, b: int) returns Option<int> {
    if b == 0 { return Option::None
 }
    return Option::Some(a / b)
}

public func main() returns int {
    let g1 = score_to_grade(95)
    let g2 = score_to_grade(73)
    let g3 = score_to_grade(55)

    println(grade_to_points(g1))
  // 4
    println(grade_to_points(g2))
  // 2
    println(grade_to_points(g3))
  // 0

    let result = safe_divide(10, 2)
    let val = match result {
        when Option::Some(v) then v,
        when Option::None then 0,
    }
    println(val)
  // 5

    return 0
}
```

---

### Example 4: Multi-Module Project with Traits

**scorable.spectra**
```spectra
module scorable

public trait Scorable {
    func total(&self) returns int
    func average(&self) returns int
    func best(&self) returns int
}
```

**student.spectra**
```spectra
module student

import scorable

public record Student {
    public name: string,
    math: int,
    science: int,
    english: int,
}

impl Student {
    public func new(name: string, math: int, sci: int, eng: int) returns Student {
        Student { name: name, math: math, science: sci, english: eng }
    }
}

impl Scorable for Student {
    func total(&self) returns int {
        self.math + self.science + self.english
    }

    func average(&self) returns int {
        (self.math + self.science + self.english) / 3
    }

    func best(&self) returns int {
        let m = self.math
        let s = self.science
        let e = self.english
        if m >= s {
            if m >= e { return m
 }
            return e
        }
        if s >= e { return s
 }
        return e
    }
}
```

**main.spectra**
```spectra
module main

import std.io
import student

public func main() returns int {
    let alice = Student::new("Alice", 95, 88, 92)
    let bob = Student::new("Bob", 72, 68, 75)

    println(alice.total())
    // 275
    println(alice.average())
  // 91
    println(alice.best())
     // 95

    println(bob.average())
    // 71

    return 0
}
```

---

### Example 5: Using Standard Library

```spectra
module stdlib_demo

import std.io
import std.math
import std.string
import std.convert
import std.collections
import std.option as option
from std.collections import List
import std.random

public func main() returns int {
    // std.math
    let m = max(10, 20)
               // 20
    let a = abs(-42)
                  // 42
    let r = sqrt_f(25.0)
              // 5.0
    println(m)
    println(a)
    println(r)

    // std.string
    let s = "Hello, World!"
    println(len(s))
                    // 13
    println(to_upper(s))
               // "HELLO, WORLD!"
    println(contains(s, "World"))
     // true
    println(substring(s, 0, 5))
       // "Hello"

    // std.convert
    let n_str = int_to_string(42)
    println(n_str)
                    // "42"
    let parsed = string_to_int("123")
    println(parsed)
                   // 123

    // std.collections
    let lst: List<int> = list_new()
    list_push(lst, 10)
    list_push(lst, 30)
    list_push(lst, 20)
    list_sort(lst)
    let first = list_get(lst, 0)
    let second = list_get(lst, 1)
    let third = list_get(lst, 2)
    if option.is_none(first) or option.is_none(second) or option.is_none(third) {
        list_free(lst)
        return 1
    }
    println(option.option_unwrap(first))
         // 10
    println(option.option_unwrap(second))
         // 20
    println(option.option_unwrap(third))
         // 30
    list_free(lst)

    // std.random
    random_seed(42)
    let rn = random_int(1, 10)
    println(rn)
                       // deterministic with seed

    return 0
}
```

---

### Example 6: f-strings and String Operations

```spectra
module greeting

import std.io
import std.string
import std.convert

func make_greeting(name: string, score: int) returns string {
    let grade = if score >= 90 { "A" } else if score >= 80 { "B" } else { "C" }
    return f"Hello, {name}! Your grade is {grade} (score: {score})."
}

public func main() returns int {
    let greeting = make_greeting("Alice", 95)
    println(greeting)
    // Hello, Alice! Your grade is A (score: 95).

    let name = "Bob"
    let age = 25
    println(f"{name} is {age} years old.")
    // Bob is 25 years old.

    let n = 42
    let doubled = n * 2
    println(f"{n} doubled is {doubled}.")
    // 42 doubled is 84.

    return 0
}
```

---

### Example 7: Algorithms with Arrays

```spectra
module algorithms

import std.io

public func bubble_sort(arr: [int], n: int) {
    let i = 0
    while i < n - 1 {
        let j = 0
        while j < n - 1 - i {
            if arr[j] > arr[j + 1] {
                let tmp = arr[j]
                arr[j] = arr[j + 1]
                arr[j + 1] = tmp
            }
            j = j + 1
        }
        i = i + 1
    }
}

public func binary_search(arr: [int], n: int, target: int) returns int {
    let lo = 0
    let hi = n - 1
    while lo <= hi {
        let mid = (lo + hi) / 2
        if arr[mid] == target { return mid
 }
        if arr[mid] < target { lo = mid + 1
 }
        else { hi = mid - 1
 }
    }
    return -1
}

public func main() returns int {
    let arr = [64, 34, 25, 12, 22]
    bubble_sort(arr, 5)
    // arr is now [12, 22, 25, 34, 64]

    let i = 0
    while i < 5 {
        println(arr[i])
        i = i + 1
    }

    let sorted = [10, 20, 30, 40, 50]
    let idx = binary_search(sorted, 5, 30)
    println(idx)
  // 2

    return 0
}
```

---

## 27. Interop Baseline

SpectraLang currently exposes a Phase 8 interoperability baseline for AI/ML workflows through:

- `python/spectra_bridge.py` for Python-to-Spectra CLI/JIT calls and NumPy `.npy` tensor exchange.
- `tools/spectra-interop` for Rust helper APIs and stable C ABI exports.
- `tools/spectra-interop/include/spectra_interop.h` for C callers.

Supported data format:

- NumPy `.npy` v1.0
- little-endian `f64` (`<f8`)
- one-dimensional C-order arrays

Validation commands:

```powershell
cargo test -p spectra-interop
cargo run -p spectra-interop --example rust_ffi_sample
python python\demo_phase8.py
```

The C sample is checked in at `tools/spectra-interop/examples/c_ffi_sample.c` and is validated locally with LLVM `clang` against `target\release\spectra_interop.dll.lib`.

For the full interop contract, see `docs/interop.md`.

---

## 28. Package Manager Baseline

SpectraLang supports a Phase 9 package manager, local registry baseline, and
Git-backed package catalog flow.

Core commands:

```powershell
spectralang package lock --root .
spectralang package build --root .
spectralang package check --root .
spectralang package run --root .
spectralang package test --root .
spectralang package bench --root .
spectralang package doc --root .
spectralang package add core --root . --path ../core --version 0.1.0
spectralang package search math --root .
spectralang package add gitmath --root .
spectralang package register --root . --git https://github.com/org/gitmath.git --tag v1.2.3 --catalog ./catalog
spectralang package update --root .
```

Local registry commands:

```powershell
spectralang package publish --root packages/core --registry .spectra-registry
spectralang package add core --root . --registry .spectra-registry --version 0.1.0
```

Manifest features:

- `[project]` with `name`, `version`, `entry`, and `src_dirs`
- `[workspace] members = [...]`
- `[dependencies]` entries with local `path` and `version`
- exact semver versions in `MAJOR.MINOR.PATCH` form

The lockfile is `spectra.lock`. It records deterministic package order, package versions, path sources, manifest hashes, and resolved dependencies.

For the full package manager contract, see `docs/package-manager.md`.

---

## 29. Tooling Baseline

SpectraLang currently includes a Phase 10 tooling baseline.

LSP capabilities in `tools/spectra-lsp`:

- diagnostics
- hover
- go to definition
- references
- rename
- document/workspace symbols
- formatting
- completion
- signature help
- inlay hints
- semantic tokens
- selected quick fixes

Benchmark command:

```powershell
spectralang bench --bench-json target/bench.json tests/validation/01_basic_syntax.spectra
```

Package benchmark command:

```powershell
spectralang package bench --root .
```

Runtime diagnostic baseline:

- `spectralang run` emits `error[runtime]` with the source location of `func main` when the program exits with a non-zero status.
- The diagnostic includes stack frame `0: main()` for the current runtime entrypoint failure path.
- `spectralang compile --emit-object` and `--emit-exe` write a sibling `.spectra-debug.json` map for native debugger workflows with `gdb`/`lldb` symbols.
- Native DWARF/PDB source stepping is not claimed by the current baseline.

For the full tooling contract, see `docs/tooling.md`.

---

## Phase 11 Runtime Baseline

Concurrency and serving are implemented as stdlib modules, not new syntax.

Available modules:

- `std.concurrent`: task handles, deterministic `task_join`, FIFO channels,
  counters, stats/reset, and `pipeline_sum(start, count, workers)`.
- `std.serve`: local in-process server handles, warmup, queueing, batching,
  cancellation, timeout state, model residency lookup, result lookup, and
  deterministic `server_benchmark`.

Validation files:

- `tests/validation/77_concurrency_pipeline.spectra`
- `tests/validation/78_serving_foundations.spectra`

User-facing reference:

- `docs/concurrency-serving.md`

Current limit: Phase 11 does not provide HTTP/gRPC, sockets, async I/O, or
distributed model serving. Treat those as future hardening, not completed
Phase 11 scope.

---

## Phase 12 Security And Operations Baseline

Release security:

- `scripts/release_security.py create` generates `release-manifest.json`,
  `SHA256SUMS`, `release-provenance.json`, `release-sbom.cdx.json`, and
  `release-manifest.json.sig`.
- Production release signing requires `SPECTRA_RELEASE_SIGNING_KEY`.
- Local validation may use `--allow-dev-key`; release workflows must not.
- `.github/workflows/release.yml` verifies evidence before publishing assets.

Dependency scanning:

- CI runs `cargo audit`.
- CI runs `npm audit --audit-level=high` for the VS Code extension.

Stress/soak:

- `scripts/stress_soak.py` runs compile, runtime/JIT, tensor/autodiff,
  concurrency/serving, and package workflow stress suites.
- Reports are JSON and include timeout plus RSS data when available.

Runtime hardening:

- `spectra_rt_debug_invariants_check()` validates host registry and manual
  allocation state.
- `spectra_rt_host_invoke(...)` validates buffers and returns internal error on
  contained host panic paths.

User-facing reference:

- `docs/security-operations.md`

---

## Phase 13 Documentation And Adoption Baseline

The production adoption path is checked in and validated.

Book:

- `docs/book/README.md`
- `docs/book/01-language-basics.md`
- `docs/book/02-numerics.md`
- `docs/book/03-tensors.md`
- `docs/book/04-autodiff.md`
- `docs/book/05-model-authoring.md`
- `docs/book/06-deployment-export.md`
- `docs/book/07-stdlib-runtime-packages.md`
- `docs/book/08-benchmarks-and-comparisons.md`

AI reference examples:

- `examples/ai/linear_regression_train_export.spectra`
- `examples/ai/logistic_regression_train_export.spectra`
- `examples/ai/mlp_training_serving.spectra`
- `examples/ai/cnn_image_classifier.spectra`
- `examples/ai/toy_transformer_inference.spectra`
- `examples/ai/data_preprocessing_pipeline.spectra`

Validation:

- `scripts/validate_ai_book.py` checks required book chapters and example
  discoverability.
- `scripts/ai_examples_benchmark.py` emits JSON timing evidence for all AI
  examples.
- `run_tests.ps1` runs the Phase 13 book validation and all AI examples.
- `scripts/validate_r1501_bench.py` runs the Phase 15 release numerical
  benchmark gate, writes `target/r1501-benchmark-report.json`, and compares
  runtime results against `docs/performance/r1501-benchmark-baseline.json`.
- `std.tensor.memory_report()` returns schema `spectra.tensor.memory_report.v1`
  JSON with tensor lifetimes, allocation sites, active/peak bytes, and reuse
  metrics; `tests/validation/83_tensor_memory_planner.spectra` is the R-1502
  language-level validation surface.
- `scripts/validate_r1503_correctness.py` runs the Phase 15 numerical
  correctness gate, writes `target/r1503-correctness-report.json`, and compares
  RNG/reduction/matmul/convolution/optimizer checks against
  `docs/performance/r1503-correctness-baseline.json`.

Current limit: Phase 13 examples use deterministic toy datasets and text export
artifacts. Large real datasets, network serving examples, notebooks backed by
external kernels, and production checkpoint formats remain future adoption work.

---

## Appendix A: Reserved Keywords

| Keyword | Status | Purpose |
|---------|--------|---------|
| `module` | ✅ Implemented | Declare module |
| `import` | ✅ Implemented | Import module |
| `from` | ✅ Implemented | Named imports |
| `public` | ✅ Implemented | Public visibility |
| `returns` | ✅ Implemented | Return type annotation |
| `internal` | ✅ Implemented | Package-internal visibility |
| `func` | ✅ Implemented | Declare function |
| `record` | ✅ Implemented | Declare record |
| `enum` | ✅ Implemented | Declare enum |
| `impl` | ✅ Implemented | Implementation block |
| `trait` | ✅ Implemented | Declare trait |
| `let` | ✅ Implemented | Variable declaration |
| `mut` | ✅ Accepted (optional) | Mutability hint |
| `Self` | ✅ Implemented | Implementing type in trait/impl |
| `if` | ✅ Implemented | Conditional |
| `else if` | ✅ Implemented | Else-if |
| `when` | ✅ Implemented | Match arm |
| `then` | ✅ Implemented | Match arm separator |
| `otherwise` | ✅ Implemented | Default match arm |
| `else` | ✅ Implemented | Else branch |
| `and` | ✅ Implemented | Logical conjunction |
| `or` | ✅ Implemented | Logical disjunction |
| `not` | ✅ Implemented | Logical negation |
| `while` | ✅ Implemented | While loop |
| `do` | Stable | `do { } while` is enabled by default |
| `for` | ✅ Implemented | For loop |
| `in` | ✅ Implemented | For x in iterable |
| `loop` | Stable | Enabled by default |
| `match` | ✅ Implemented | Pattern matching |
| `switch` | Stable | Enabled by default |
| `case` | ✅ Implemented | Switch arm |
| `return` | ✅ Implemented | Return from function |
| `break` | ✅ Implemented | Exit loop |
| `continue` | ✅ Implemented | Next loop iteration |
| `true` | ✅ Implemented | Boolean literal |
| `false` | ✅ Implemented | Boolean literal |
| `const` | ✅ Implemented | Compile-time constant |
| `static` | ✅ Implemented | Module-level mutable |
| `type` | ✅ Implemented | Type alias |
| `as` | ✅ Implemented | Type cast |
| `dyn` | ✅ Accepted | Dynamic dispatch |
| `export` | 🚧 Reserved | Future use |
| `class` | 🚧 Reserved | Future use |
| `foreach` | 🚧 Reserved | Future use |
| `repeat` | 🚧 Reserved | Future use |
| `until` | 🚧 Reserved | Future use |
| `cond` | 🚧 Reserved | Future use |
| `yield` | 🚧 Reserved | Future use |
| `goto` | 🚧 Reserved | Future use |

---

## Appendix B: Common Errors and Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| `module declaration missing` | No `module name` at top of file | Add `module name` as the first line |
| `main not found` | No `public func main() returns int` | Add entry point function |
| `type mismatch: int and float` | Mixing int/float without conversion | Use `int_to_float(x)` or `float_to_int(x)` |
| `cannot assign to immutable` | Rare compiler edge case | Variables are mutable by default; check the context |
| `non-exhaustive match` | Not all enum variants covered | Add an `otherwise` arm |
| `undefined variable 'x'` | Using variable before `let` or out of scope | Move `let x = ...` to the correct scope |
| `undefined function 'f'` | Calling a function not imported or defined | Import the module or define the function |
| `break/continue outside loop` | Used outside `while`/`for`/`loop` | Move inside a loop body |
| `field not found` | Accessing a record field that doesn't exist | Check field name spelling |
| `missing field in record literal` | Not all record fields provided | Provide all required fields |
| `cyclic dependency` | Module A imports B, B imports A | Restructure to break the cycle |
| `duplicate module name` | Two files declare the same module | Ensure each module name is unique |
| `unresolved import 'mod'` | Importing a module with no matching file | Create a file with `module mod` |

---

## Appendix C: Naming Conventions

| Construct | Convention | Example |
|-----------|-----------|---------|
| Variables | `snake_case` | `my_var`, `total_count` |
| Functions | `snake_case` | `calculate_area`, `get_name` |
| Parameters | `snake_case` | `func f(total_score: int)` |
| Structs | `PascalCase` | `Point`, `UserAccount` |
| Enums | `PascalCase` | `Color`, `Status` |
| Enum Variants | `PascalCase` | `Color::Red`, `Status::Active` |
| Traits | `PascalCase` | `Printable`, `Comparable` |
| Type Params | Short `PascalCase` | `T`, `E`, `Key`, `Val` |
| Modules | `snake_case` | `module my_lib` |
| Files | `snake_case.spectra` | `my_module.spectra` |
| Constants | `UPPER_SNAKE_CASE` | `const MAX_SIZE: int = 100;` |

---

*End of SpectraLang AI Agent Reference*
