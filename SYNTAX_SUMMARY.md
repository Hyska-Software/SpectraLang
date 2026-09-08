# SpectraLang — Guia de Sintaxe Completo

> Compilado a partir dos exemplos em `examples/`. Comparações com Rust, Python e TypeScript.

---

## Sumário

1. [Estrutura do Programa](#1-estrutura-do-programa)
2. [Declaração de Módulo](#2-declaração-de-módulo)
3. [Imports](#3-imports)
4. [Variáveis e Tipos](#4-variáveis-e-tipos)
5. [Funç��es](#5-funções)
6. [Controle de Fluxo](#6-controle-de-fluxo)
7. [Loops](#7-loops)
8. [Ranges](#8-ranges)
9. [Strings e F-strings](#9-strings-e-f-strings)
10. [Arrays](#10-arrays)
11. [Tuplas](#11-tuplas)
12. [Structs](#12-structs)
13. [Enums](#13-enums)
14. [Pattern Matching (match)](#14-pattern-matching-match)
15. [if let / while let](#15-if-let--while-let)
16. [Traits / OOP](#16-traits--oop)
17. [Impl blocks](#17-impl-blocks)
18. [Visibilidade (pub / internal / private)](#18-visibilidade-pub--internal--private)
19. [Generics](#19-generics)
20. [Closures (Lambdas)](#20-closures-lambdas)
21. [Operators Overload](#21-operators-overload)
22. [Coerção dyn Trait (Casting)](#22-coerção-dyn-trait-casting)
23. [Option\<T\> e Result\<T, E\> built-in](#23-optiont-e-resultt-e-built-in)
24. [Lint / Warnings](#24-lint--warnings)
25. [AI/ML — Tensores e ML](#25-aiml--tensores-e-ml)
26. [API HTTP](#26-api-http)
27. [SQLite / DB](#27-sqlite--db)
28. [CLI / argv](#28-cli--argv)
29. [Tabela Comparativa Geral](#29-tabela-comparativa-geral)

---

## 1. Estrutura do Programa

```
module nome;              // declaração obrigatória na 1ª linha não-comentada
import std.io;            // imports
pub fn main() { ... }     // entry point público
```

- Todo arquivo `.spectra` começa com `module nome;`
- `pub fn main()` é o entry point
- Funções privadas por default

---

## 2. Declaração de Módulo

```spectra
module basic;
module test_beta_closures;
module ai_tensor_graph_elementwise_fusion;
module rest_sqlite_crud;
```

- Obrigatória, no topo do arquivo
- CLI sintetiza automaticamente se faltar (derivado do nome do arquivo)

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `module x;` | `mod x;` | (nome do arquivo) | `export module x` |

---

## 3. Imports

```spectra
// Import simples
import std.io;

// Named import
import { println, print } from std.io;

// Alias import
import std.math as math;
import std.tensor as tensor;

// Multi-import
import {
    Server,
    new,
    listen,
    serve,
    shutdown,
} from std.api.server;
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `import X` | `use X` | `import X` | `import X` |
| `import {a,b} from X` | `use X::{a,b}` | `from X import a,b` | `import {a,b} from X` |
| `import {a as b} from X` | `use X::{a as b}` | `from X import a as b` | `import {a as b} from X` |
| `import X as Y` | `use X as Y` | `import X as Y` | `import X as Y` |

→ **Python-like** com chaves estilo TS para named imports. Brace imports desaguam na
mesma forma de named import do parser (`NamedImport`), então alias, re-export
(`public`/`internal`) e semântica são idênticos ao canonical `from X import ...`.

---

## 4. Variáveis e Tipos

```spectra
let x = 10;                   // inferência → int
let y: int = 20;              // tipo explícito
let name = "Spectra";         // string
let flag = true;              // bool
let pi = 3.14;                // float
let arr = [1, 2, 3];          // array
let tup = (10, true);         // tupla

// Reatribuição
x = x + 1;
```

### Tipos primitivos

| Tipo | Descrição |
|---|---|
| `int` | Inteiro |
| `float` | Ponto flutuante |
| `bool` | Booleano (`true`/`false`) |
| `string` | Texto |
| `[T]` | Array de T |
| `(T, U)` | Tupla |
| `fn(T) -> R` | Tipo função |

- Tipagem **estática e forte**
- `let` permite reatribuição (diferente de Rust onde `let` é imutável por default com `mut` explícito)

---

## 5. Funç��es

```spectra
// Sem retorno
fn greet(name: string) {
    println(f"Hello, {name}!");
}

// Com retorno
fn add(a: int, b: int) -> int {
    return a + b;
}

// Retorno implícito (última expressão)
fn double(x: int) -> int {
    x * 2
}

// Após loop/if, expressão final vira retorno
fn test_after_loop() -> int {
    let i = 0;
    while i < 3 { i = i + 1; }
    i  // implicit return
}

// Função pública
pub fn main() -> int { ... }

// Void return
fn test() { return; }
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `fn nome(p: T) -> R` | `fn nome(p: T) -> R` | `def nome(p: T) -> R` | `function nome(p: T): R` |
| retorno implícito | retorno implícito | ❌ | ❌ |
| `return` ou expr | `return` ou expr | `return` | `return` |

---

## 6. Controle de Fluxo

```spectra
// if simples
if x < y { println("menor"); }

// if/else
if x > 100 {
    println("grande");
} else {
    println("pequeno ou igual");
}

// if / elif / else
if x > 100 {
    println("Large");
} elif x > 50 {
    println("Medium");
} elif x > 10 {
    println("Small");
} else {
    println("Very small");
}

// if como expressão
let message = if can_proceed {
    "ok"
} else {
    "not ok"
};

// unless (inverso do if)
unless x < 0 {
    result = x * 2;
}
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `if/elif/else` | `if/else if/else` | `if/elif/else` | `if/else if/else` |
| `unless cond { }` | ❌ | ❌ | ❌ |
| if como expressão | `let x = if cond { }` | ❌ | ❌ (ternário `?:`) |

> `unless` é diferencial da linguagem: executa o bloco **a menos que** a condição seja verdadeira.

---

## 7. Loops

```spectra
// while
let i = 0;
while i < 5 {
    println(i);
    i = i + 1;
}

// while com condição composta
while i <= max_val && found == 0 {
    j = j + 1;
}

// for...in (itera sobre array/range)
for i in numbers {
    println(i);
}

// for...of
for item of collection {
    println(item);
}

// break / continue
for num in data {
    if num % 2 == 0 { break; }
    if num <= 0 { continue; }
}
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `while` | `while` | `while` | `while` |
| `for x in y` | `for x in y` | `for x in y` | `for...of` / `for...in` |
| `loop` | `loop` | ❌ | ❌ |
| `break` / `continue` | ✅ | ✅ | ✅ |

---

## 8. Ranges

```spectra
// Range exclusivo (0..n)
let r1 = 0..10;     // 0, 1, 2, ..., 9

// Range inclusivo (0..=n)
let r2 = 1..=5;     // 1, 2, 3, 4, 5

// Em for loops
for i in 0..5 { }
for i in 1..=5 { }

// Em argumentos de função
fn sum_range(start: int, end_val: int) -> int {
    for i in start..end_val { ... }
}
```

→ **Rust-like:** `..` (exclusivo) e `..=` (inclusivo), mesmos operadores.

---

## 9. Strings e F-strings

```spectra
// String literal
let s1 = "Hello";
let s2 = " World";

// Concatenação com +
let msg = s1 + s2;
let greeting = "Hello" + " " + "World" + "!";

// F-string (interpolação)
let name = "World";
let msg = f"Hello, {name}!";
let pi = 3.14;
let circle = f"Pi ≈ {pi}";
let sum = f"Result of {a} + {b} = {a + b}";
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `"string"` | `"string"` | `"string"` | `"string"` |
| `s1 + s2` | `format!("{}{}", s1, s2)` | `s1 + s2` | `s1 + s2` |
| `f"{expr}"` | ❌ (macro `format!`) | `f"{expr}"` | `` `${expr}` `` |

→ **Python-like:** f-strings nativas com `{expressão}` dentro de `f"..."`.

---

## 10. Arrays

```spectra
let arr = [1, 2, 3, 4, 5];  // literal
let x = arr[2];              // acesso por índice (0-based)
arr[1] = 99;                 // atribuição

// Iteração
let sum = 0;
let i = 0;
while i < 5 {
    sum = sum + arr[i];
    i = i + 1;
}

// Comprimento
let len = length(arr);
```

→ **C/Rust-like:** colchetes, índice 0-based.

---

## 11. Tuplas

```spectra
let t1 = (10, true);          // (int, bool)
let t2 = (1, 2, 3);           // (int, int, int)
let t3 = (42, "hello", 3.14); // (int, string, float)

// Função retornando tupla
fn split() -> (int, int) {
    return (10, 20);
}

// Desestruturação
// (implícita via match/destructuring)
```

---

## 12. Structs

```spectra
struct Point {
    x: int,
    y: int,
}

struct Person {
    age: int,
    height: int,
}

// Construção
let p = Point { x: 10, y: 20 };

// Acesso
p.x + p.y

// Campos com visibilidade
struct BankAccount {
    pub owner: string,           // público
    internal balance: int,       // visível no módulo
    secret_pin: int,             // privado (só impl)
}
```

→ **Rust-like:** mesma sintaxe de struct, vírgulas entre campos, `pub`/privado.

---

## 13. Enums

```spectra
// Unit variants (C-style)
enum Color {
    Red,
    Green,
    Blue,
}

// Tuple variants (com dados associados)
enum Option<T> {
    Some(T),
    None,
}

enum Result<T, E> {
    Ok(T),
    Err(E),
}

// Uso
let c = Color::Red;
let opt = Option::Some(42);
let err = Result::Err("deu ruim");
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `enum { V, V }` | `enum { V, V }` | `Enum` class | `enum { V, V }` |
| `V(T)` com dados | `V(T)` | ❌ | ❌ |
| `Enum::Variant` | `Enum::Variant` | `Enum.Variant` | `Enum.Variant` |

→ **Rust-like:** enums algébricos com variantes unit e tuple.

---

## 14. Pattern Matching (match)

```spectra
// match básico
match c {
    Color::Red => 1,
    Color::Green => 2,
    Color::Blue => 3,
}

// Wildcard
match c {
    Color::Red => 1,
    _ => 999,
}

// Extraindo valores de enum
match some_val {
    Option::Some(value) => value,
    Option::None => 0,
}

// match como expressão
let result = match opt {
    Option::Some(n) => n,
    Option::None => -1,
};

// match aninhado
match opt {
    Option::Some(x) => {
        match x {
            _ => x + 10,
        }
    },
    Option::None => 0,
};
```

| SpectraLang | Rust | Python (3.10+) | TypeScript |
|---|---|---|---|
| `match { pat => }` | `match { pat => }` | `match:` | `switch` |
| exaustivo | exaustivo | exaustivo | ❌ |
| `_` wildcard | `_` | `_` | `default` |
| match é expressão | match é expressão | match é expressão | switch não |

→ **Rust-like:** match exaustivo, wildcard `_`, expressão.

---

## 15. if let / while let

```spectra
// if let — extrai valor se variante bater
let maybe = find_positive(42);
if let Option::Some(value) = maybe {
    let doubled = value * 2;
}

// if let com else
if let Result::Ok(q) = safe_divide(10, 2) {
    let msg = q;
} else {
    let fallback = -1;
}

// while let — extrai enquanto bater
let counter = 3;
while let Option::Some(n) = find_positive(counter) {
    counter = counter - 1;
}
```

→ **Rust-like:** `if let` / `while let` para pattern matching condicional.

---

## 16. Traits / OOP

```spectra
// Definição de trait
trait Shape {
    fn area(&self) -> int;
    fn perimeter(&self) -> int;
    fn name(&self) -> string;
}

// Implementação
impl Shape for Circle {
    fn area(&self) -> int {
        3 * self.radius * self.radius
    }
    fn perimeter(&self) -> int {
        6 * self.radius
    }
    fn name(&self) -> string {
        "Círculo"
    }
}

// Dispatch dinâmico (dyn Trait)
fn print_shape_info(shape: dyn Shape) {
    println(shape.name());
}

fn total_area(s1: dyn Shape, s2: dyn Shape) -> int {
    s1.area() + s2.area()
}
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `trait { fn }` | `trait { fn }` | ABC / Protocol | `interface` |
| `impl Trait for S` | `impl Trait for S` | class herda | `class implements` |
| `dyn Trait` | `dyn Trait` | Duck typing | interface |

→ **Rust-like:** traits com métodos, `impl Trait for Struct`, `dyn Trait`.

---

## 17. Impl blocks

```spectra
struct Circle { pub radius: int }

impl Circle {
    pub fn new(r: int) -> Circle {
        Circle { radius: r }
    }

    fn area(&self) -> int {
        3 * self.radius * self.radius
    }

    fn scale(&mut self, factor: int) {
        self.radius = self.radius * factor;
    }
}
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `impl S { fn }` | `impl S { fn }` | dentro da class | dentro da class |
| `&self` | `&self` | `self` | `this` |
| `&mut self` | `&mut self` | ❌ | ❌ |

---

## 18. Visibilidade (pub / internal / private)

```spectra
struct BankAccount {
    pub owner: string,           // acessível fora do módulo
    internal balance: int,       // acessível dentro do módulo
    secret_pin: int,             // privado (só este impl acessa)
}

impl BankAccount {
    pub fn new(...) -> BankAccount { }     // público
    internal fn adjust_balance(...) { }    // visível no módulo
    fn verify_pin(...) -> bool { }        // privado
}
```

| Nível | Acesso |
|---|---|
| `pub` | Qualquer módulo |
| `internal` | Dentro do mesmo módulo |
| (sem keyword) | Privado, só o `impl` |

→ Diferencial: `internal` vs Rust que tem `pub(crate)`.

---

## 19. Generics

```spectra
enum Option<T> {
    Some(T),
    None,
}

enum Result<T> {
    Ok(T),
    Err(T),
}

fn unwrap_or<T>(opt: Option<T>, default: T) -> T {
    match opt {
        Option::Some(value) => value,
        Option::None => default,
    }
}
```

→ **Rust-like:** `Tipo<T>` com parâmetros genéricos em funções, enums e structs (visto em enums).

---

## 20. Closures (Lambdas)

```spectra
// Lambda básica: |params| body
let double = |x: int| x * 2;
let square = |x: int| x * x;

// Multi-parâmetros
let add = |a: int, b: int| a + b;

// Zero parâmetros
let forty_two = || 42;

// Com corpo em bloco
let abs_val = |x: int| {
    if x < 0 { return x * -1; }
    return x;
};

// Higher-order functions
fn apply(x: int, f: fn(int) -> int) -> int { f(x) }
let result = apply(5, |x: int| x * 3);

// Tipo closure: fn(T) -> R
fn apply_twice(x: int, f: fn(int) -> int) -> int {
    f(f(x))
}
```

| SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|
| `\|x\| expr` | `\|x\| expr` | `lambda x:` | `x => expr` |
| `fn(T) -> R` | `fn(T) -> R` | `Callable` | `(x: T) => R` |

→ **Rust-like:** mesma sintaxe de closures.

---

## 21. Operators Overload

```spectra
impl Add for Vec2D {
    fn add(&self, other: Vec2D) -> Vec2D {
        Vec2D { x: self.x + other.x, y: self.y + other.y }
    }
}

impl Mul for Vec2D {
    fn mul(&self, other: Vec2D) -> Vec2D {
        Vec2D { x: self.x * other.x, y: self.y * other.y }
    }
}

impl Eq for Vec2D {
    fn eq(&self, other: Vec2D) -> bool {
        self.x == other.x && self.y == other.y
    }
}
```

→ **Rust-like:** traits `Add`, `Mul`, `Eq` com método `add`, `mul`, `eq`.

---

## 22. Coerção dyn Trait (Casting)

```spectra
let c: Circle = Circle::new(5);
let shape_cast: dyn Shape = c as dyn Shape;     // cast explícito
let area_cast = shape_cast.area();
```

→ **Rust-like:** `expr as dyn Trait` para upcasting.

---

## 23. Option\<T\> e Result\<T, E\> built-in

```spectra
fn safe_divide(a: int, b: int) -> Result<int, string> {
    if b == 0 { return Result::Err("divisao por zero"); }
    return Result::Ok(a / b);
}

fn find_positive(x: int) -> Option<int> {
    if x > 0 { return Option::Some(x); }
    return Option::None;
}

// Uso com match
let answer = match find_positive(4) {
    Option::Some(n) => n,
    Option::None => -1,
};

// if let
if let Result::Ok(q) = safe_divide(10, 2) {
    // usa q
}
```

- `Option<T>` e `Result<T, E>` são **built-in** (não precisa declarar)
- Segue o padrão Rust

---

## 24. Lint / Warnings

```spectra
// Warnings detectados:
// - unused parameters
// - código inalcançável após return
// - shadowing de variáveis
// - variáveis não usadas

fn unused_params(x: int, y: int, z: int) -> int {
    return x;  // warning: y, z não usados
}

fn unreachable_after_return() -> int {
    return 42;
    let dead = 10;  // warning: inalcançável
}
```

---

## 25. AI/ML — Tensores e ML

```spectra
import std.tensor as tensor;
import std.ml as ml;

// Criação de tensores
let t = tensor.full_f(8, 4.0);           // [8] de floats
let t2 = tensor.reshape(t, 2, 4);        // reshape
let ids = tensor.arange(0, 10, 1);       // range

// Ativações
let activated = tensor.relu(input);
let normalized = tensor.sqrt_f(input);
let projected = tensor.tanh_f(input);

// Gradientes
tensor.set_grad_enabled(true);
let w = tensor.requires_grad(tensor.full_f(1, 0.0), true);
tensor.backward(loss);

// ML primitives
let pred = ml.linear(x, weight, bias);
let loss = ml.mse_loss(pred, target);
let loss2 = ml.bce_loss(probs, labels);
ml.sgd_step(weight, 0.1);

// Módulos / Dataset / Dataloader
let model = ml.module_new();
ml.module_add_parameter(model, weight);
let dataset = ml.dataset_from_tensors(x, target, 4);
let loader = ml.dataloader_new(dataset, batch_size, seed);

// Transformer primitives
let embedded = ml.embedding_lookup(table, token_ids);
let positions = ml.positional_encoding(seq_len, dim);
let attended = ml.attention(query, key, value);
let normed = ml.layer_norm(x, scale, bias);
let ffwd = ml.swiglu(a, b);

// Serving
let server = serve.server_new(threads);
serve.server_warmup(server);
let req = serve.server_enqueue(server, data);
serve.server_process_batch(server, batch_size);
let result = serve.server_result(server, req);

// Métricas
let cls = ml.metrics_classification(labels, preds);
let reg = ml.metrics_regression(expected, actual);
let ranking = ml.metrics_ranking(relevance, scores, k);

// IO
fs.fs_write("path.txt", "content");
```

| SpectraLang | Python (PyTorch) |
|---|---|
| `tensor.full_f(n, v)` | `torch.full([n], v)` |
| `tensor.relu(x)` | `torch.relu(x)` |
| `tensor.backward(loss)` | `loss.backward()` |
| `ml.linear(x, w, b)` | `torch.nn.functional.linear` |
| `ml.mse_loss(p, t)` | `torch.nn.functional.mse_loss` |
| `ml.sgd_step(w, lr)` | `optimizer.step()` |
| `ml.attention(q, k, v)` | `torch.nn.functional.scaled_dot_product_attention` |

→ **PyTorch-like:** API funcional, tudo via módulo `tensor` e `ml`.

---

## 26. API HTTP

```spectra
import { Server, new, listen, serve, shutdown } from std.api.server;
import { Router, router, get, route_id } from std.api.routing;
import { HandlerHandle, text, with_header, register_sync, dispatch_sync } from std.api.handler;
import { Request, Response, method_get, request, response_status } from std.api.http;

let routes = router(get("/hello", handler));
let handler: HandlerHandle = register_sync(route_id(route), response);
let request_value: Request = request(method_get(), "/hello");
let dispatched: Response = dispatch_sync(handler, request_value);
let server: Server = new();
listen(server, 0);
block_on(serve(server, routes));
shutdown(server);
```

→ **Estilo Go + Express:** funções livres, `new()`, handles de ID.

---

## 27. SQLite / DB

```spectra
import { SqliteConnection, SqliteStatement, open, close, prepare, step, finalize } from std.api.db.sqlite;

let db: SqliteConnection = open("database.sqlite");
let stmt: SqliteStatement = prepare(db, "CREATE TABLE IF NOT EXISTS todos(...)");
step(stmt);
finalize(stmt);
close(db);
```

→ **Estilo C SQLite:** `prepare`, `step`, `finalize` — funcional, sem ORM.

---

## 28. CLI / argv

```spectra
import std.env;
import std.io;

fn main() {
    let count = env_args_count();
    let i = 0;
    while i < count {
        println(env_arg(i));
        i = i + 1;
    }
}
```

---

## 29. Tabela Comparativa Geral

| Feature | SpectraLang | Rust | Python | TypeScript |
|---|---|---|---|---|
| **Módulo** | `module x;` | `mod x;` | (arquivo) | `export module` |
| **Import** | `import X` / `import {a} from X` | `use X` | `import X` / `from X import a` | `import X` / `import {a} from X` |
| **Variável** | `let x = v` | `let x = v` / `let mut x = v` | `x = v` | `let x = v` |
| **Funç��o** | `fn nome(p: T) -> R` | `fn nome(p: T) -> R` | `def nome(p: T) -> R` | `function nome(p: T): R` |
| **Retorno implícito** | ✅ (última expr) | ✅ (última expr) | ❌ | ❌ |
| **Closure** | `\|x\| expr` | `\|x\| expr` | `lambda x:` | `x => expr` |
| **Struct** | `struct { f: T }` | `struct { f: T }` | `@dataclass` | `interface` / `class` |
| **Enum algébrico** | `enum { V(T) }` | `enum { V(T) }` | ❌ | `discriminated union` |
| **Match** | `match { pat => }` | `match { pat => }` | `match` (3.10+) | `switch` |
| **Exaustivo** | ✅ | ✅ | ✅ | ❌ |
| **if let / while let** | ✅ | ✅ | ❌ | ❌ |
| **Trait/Interface** | `trait { fn }` | `trait { fn }` | ABC / Protocol | `interface` |
| **dyn Trait** | `dyn Trait` | `dyn Trait` | duck typing | interface |
| **Generics** | `T<T>` | `T<T>` | `Generic[T]` | `T<T>` |
| **F-strings** | `f"{x}"` | ❌ (macro `format!`) | `f"{x}"` | `` `${x}` `` |
| **Unless** | ✅ | ❌ | ❌ | ❌ |
| **Ranges** | `..` e `..=` | `..` e `..=` | `range()` | ❌ |
| **Operator overload** | `impl Add` | `impl Add` | `__add__` | ❌ |
| **Visibilidade** | `pub` / `internal` / privado | `pub` / `pub(crate)` / privado | `_` prefixo | `public` / `private` |
| **Tensors nativos** | ✅ `Tensor<T,R>` | ❌ (libs externas) | ✅ (numpy/torch) | ❌ |
| **Autograd** | ✅ `tensor.backward()` | ❌ | ✅ (torch) | ❌ |
| **HTTP server** | ✅ `std.api.server` | ✅ (actix/warp/axum) | ✅ (flask/fastapi) | ✅ (express/koa) |
| **SQLite** | ✅ `std.api.db.sqlite` | ✅ (rusqlite) | ✅ (sqlite3) | ✅ (better-sqlite3) |
| **Async/await** | ✅ (`block_on`) | ✅ (`tokio::main`) | ✅ (`async def`) | ✅ (`async/await`) |
| **Codegen** | Cranelift (JIT + AOT) | LLVM / Cranelift | Interpretado | V8 / Node (JIT) |
| **Linter** | �� embutido | `clippy` | `pylint` / `ruff` | `eslint` |
| **JIT** | ✅ (`spectralang run`) | ❌ (só AOT) | ✅ (interpretado) | ✅ (Node/Deno) |

---

### 💎 Resumo

**SpectraLang** pega o melhor de cada mundo e combina num ecossistema único:

| Inspiração | O que pegou |
|---|---|
| **Rust** | `fn`, `let`, `struct`, `enum`, `match`, `trait`, `impl`, `&self`, `dyn Trait`, closures `\|x\|`, `..`/`..=`, generics `T<T>`, operator overload |
| **Python** | `import`/`as`, `elif`, `f"strings"`, indentação, simplicidade, PyTorch-like API |
| **PyTorch** | API tensorial funcional (`tensor.full_f`, `.relu()`, `.backward()`, `sgd_step`, `ml.linear`) |
| **Go** | Estilo funcional nas APIs (`new()`, funções livres) |
| **Próprio** | `unless`, `module` obrigatório, `internal` visibility, f-strings nativas, JIT embutido |
