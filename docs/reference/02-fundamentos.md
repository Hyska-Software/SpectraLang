# SpectraLang — Fundamentos / Fundamentals

> **Nível / Level:** Iniciante / Beginner  
> **Parte / Part:** 2 de 6

---

## Sumário / Table of Contents

1. [Comentários / Comments](#1-comentários--comments)
2. [Variáveis e Mutabilidade / Variables & Mutability](#2-variáveis-e-mutabilidade--variables--mutability)
3. [Tipos Primitivos / Primitive Types](#3-tipos-primitivos--primitive-types)
4. [Literais / Literals](#4-literais--literals)
5. [Operadores / Operators](#5-operadores--operators)
6. [Controle de Fluxo / Control Flow](#6-controle-de-fluxo--control-flow)
7. [Funções / Functions](#7-funções--functions)

---

## 1. Comentários / Comments

**PT-BR:**  
SpectraLang suporta comentários de linha única iniciados com `//`. Tudo após `//` até o fim da linha é ignorado pelo compilador.

**EN-US:**  
SpectraLang supports single-line comments starting with `//`. Everything after `//` until the end of the line is ignored by the compiler.

```spectra
module exemplos

// Este é um comentário de linha
// This is a line comment

public func main() {
    let x = 10
 // Comentário inline / Inline comment
    // let y = 20; // Linha comentada / Commented-out line
}
```

> **Nota / Note:** Comentários de bloco (`/* ... */`) **não são suportados** na versão atual. Use múltiplas linhas com `//`.  
> Block comments (`/* ... */`) are **not supported** in the current version. Use multiple `//` lines.

---

## 2. Variáveis e Mutabilidade / Variables & Mutability

### Declaração de Variáveis / Variable Declaration

**PT-BR:**  
Variáveis são declaradas com a palavra-chave `let`. A anotação de tipo é **opcional** — o compilador infere o tipo a partir do valor inicial quando possível.

**EN-US:**  
Variables are declared with the `let` keyword. The type annotation is **optional** — the compiler infers the type from the initial value when possible.

```spectra
module variaveis

public func main() {
    // Com inferência de tipo / With type inference
    let x = 10
           // int
    let pi = 3.14
        // float
    let ativo = true
     // bool
    let nome = "Alice"
   // string
    let letra = 'A'
      // char

    // Com anotação de tipo explícita / With explicit type annotation
    let contador: int = 0
    let temperatura: float = 36.5
    let mensagem: string = "Olá"
    let flag: bool = false
    let caractere: char = 'Z'
}
```

### Reatribuição / Reassignment

**PT-BR:**  
Em SpectraLang, todas as variáveis são **reatribuíveis** após a declaração — não existe a distinção `let`/`var` ou `const`/`let`. A palavra-chave `mut` existe na gramática mas a mutabilidade é implícita para variáveis locais.

**EN-US:**  
In SpectraLang, all variables are **reassignable** after declaration — there is no `let`/`var` or `const`/`let` distinction. The `mut` keyword exists in the grammar but mutability is implicit for local variables.

```spectra
module mutabilidade

public func main() {
    let contador = 0
    contador = contador + 1
  // OK — reatribuição / reassignment
    contador = 10
            // OK

    let nome = "Alice"
    nome = "Bob"
             // OK

    // Também funciona com campos de array / Also works with array elements
    let arr = [1, 2, 3]
    arr[0] = 99
              // Modifica o primeiro elemento / Modifies first element
}
```

### Escopo / Scope

**PT-BR:**  
Variáveis existem no escopo do bloco `{ }` em que foram declaradas. Variáveis em escopos internos podem sombrear variáveis externas.

**EN-US:**  
Variables exist in the scope of the `{ }` block in which they were declared. Variables in inner scopes can shadow outer variables.

```spectra
module escopo

public func main() {
    let x = 10

    if x > 5 {
        let y = x * 2
    // 'y' só existe dentro do if / 'y' only exists inside the if
        let x = 999
      // sombra o 'x' externo / shadows outer 'x'
        // aqui x == 999 / here x == 999
    }
    // aqui x == 10, 'y' não existe / here x == 10, 'y' does not exist
}
```

---

## 3. Tipos Primitivos / Primitive Types

**PT-BR:**  
SpectraLang tem 5 tipos primitivos principais, aliases numéricos e o tipo `unit` para ausência de valor:

**EN-US:**  
SpectraLang has 5 main primitive types, numeric aliases, and the `unit` type for absence of value:

| Tipo / Type | Descrição PT-BR | Description EN-US | Exemplos / Examples |
|---|---|---|---|
| `int` | Inteiro com sinal (64 bits) | Signed integer (64-bit) | `0`, `42`, `-100`, `1_000_000` |
| `float` | Ponto flutuante (64 bits) | Double-precision float (64-bit) | `3.14`, `-0.5`, `1.0` |
| `bool` | Booleano | Boolean | `true`, `false` |
| `string` | Cadeia de caracteres UTF-8 | UTF-8 character string | `"hello"`, `""`, `"linha\nova"` |
| `char` | Caractere Unicode único | Single Unicode character | `'a'`, `'\n'`, `'Z'` |
| `unit` | Ausência de valor (implícito) | Absence of value (implicit) | (retorno de funções void) |

Aliases numéricos aceitos atualmente:

| Família / Family | Tipos / Types | ABI atual / Current ABI |
|---|---|---|
| Inteiros com sinal / Signed integers | `i8`, `i16`, `i32`, `i64`, `isize` | Canonicalizados para `int` |
| Inteiros sem sinal / Unsigned integers | `u8`, `u16`, `u32`, `u64`, `usize` | Canonicalizados para `int` |
| Floats | `f16`, `bf16`, `f32`, `f64` | Canonicalizados para `float` |

Esses aliases deixam a superfície da linguagem pronta para código científico, mas a ABI alpha ainda usa `int` e `float` como representação canônica. Semântica de largura exata, overflow e armazenamento compacto são trabalho de backend/runtime futuro.

### Promoção Numérica / Numeric Promotion

**PT-BR:**  
A única conversão implícita permitida em SpectraLang é o **alargamento de `int` para `float`**. Todas as outras conversões devem ser explícitas via `as` quando suportado ou via `std.convert`.

**EN-US:**  
The only implicit conversion allowed in SpectraLang is **widening from `int` to `float`**. All other conversions must be explicit via supported `as` casts or `std.convert`.

```spectra
module tipos

import std.convert

public func main() {
    let i: int = 42

    // Promoção implícita permitida / Implicit promotion allowed
    let f: float = i
         // int → float: OK

    // Conversão explícita necessária / Explicit conversion required
    let de_volta: int = std.convert.float_to_int(3.9)
  // → 3 (truncado)
    let como_texto: string = std.convert.int_to_string(42)
 // → "42"
    let parsed: int = std.convert.string_to_int("100")
     // → 100

    // ERRO — conversão implícita não permitida / ERROR — implicit conversion not allowed
    // let s: string = 42;    // Erro: tipos incompatíveis
    // let b: bool = 1;       // Erro: int não vira bool implicitamente
}
```

### Estreitamento Explícito / Explicit Narrowing

**PT-BR:**
Estreitamentos via `as` para inteiros de largura exata (`i8`/`u8`/`i16`/...) são **verificados em tempo de execução**: valores fora da faixa falham ruidosamente (`E2903: value is outside <tipo> range`) em vez de truncar ou embrulhar. Isso vale também para `char as <inteiro>`, que valida o codepoint Unicode — `'€' as u8` falha, `'ÿ' as u8` resulta em `255`, `'A' as u8` resulta em `65`. Alargamentos (`char` → `int`) são exatos e nunca falham.

**EN-US:**
Narrowing via `as` to exact-width integers (`i8`/`u8`/`i16`/...) is **runtime-checked**: out-of-range values fail loudly (`E2903: value is outside <type> range`) instead of truncating or wrapping. This also covers `char as <integer>`, which validates the Unicode codepoint — `'€' as u8` fails, `'ÿ' as u8` yields `255`, `'A' as u8` yields `65`. Widenings (`char` → `int`) are exact and never fail.

---

## 4. Literais / Literals

### Inteiros / Integer Literals

```spectra
let a = 0
let b = 42
let c = -7
let d = 1000000
```

### Ponto Flutuante / Float Literals

```spectra
let pi = 3.14159
let e  = 2.71828
let negativo = -1.5
let inteiro_como_float = 1.0
  // O ponto indica float / The dot indicates float
```

### Booleanos / Boolean Literals

```spectra
let verdadeiro = true
let falso = false
```

### Strings / String Literals

**PT-BR:**  
Strings são delimitadas por aspas duplas `"..."`. As seguintes sequências de escape são suportadas:

**EN-US:**  
Strings are delimited by double quotes `"..."`. The following escape sequences are supported:

| Escape | Caractere / Character |
|---|---|
| `\\` | Barra invertida / Backslash |
| `\"` | Aspas duplas / Double quote |
| `\n` | Nova linha / Newline |
| `\t` | Tabulação / Tab |
| `\r` | Retorno de carro / Carriage return |
| `\0` | Nulo / Null |

```spectra
let simples = "Olá, mundo!"
let com_aspas = "Ele disse \"Olá\""
let com_quebra = "Linha 1\nLinha 2"
let com_tab = "Coluna1\tColuna2"
let vazia = ""
```

### Caracteres / Char Literals

```spectra
let letra: char = 'A'
let digito: char = '7'
let espaco: char = ' '
let nova_linha: char = '\n'
let tab: char = '\t'
```

### F-Strings (Strings com Interpolação / Interpolated Strings)

**PT-BR:**  
F-strings permitem embutir expressões dentro de strings usando a sintaxe `f"texto {expressão}"`.

**EN-US:**  
F-strings allow embedding expressions inside strings using the `f"text {expression}"` syntax.

```spectra
module fstrings

from std.io import println

public func main() {
    let nome = "Alice"
    let idade = 30
    let pi = 3.14159

    // Variáveis / Variables
    println(f"Olá, {nome}!")
                          // Olá, Alice!

    // Expressões aritméticas / Arithmetic expressions
    println(f"Daqui a 5 anos: {idade + 5} anos")
     // Daqui a 5 anos: 35 anos

    // Chamadas de função / Function calls
    println(f"PI arredondado: {arredondar(pi)}")

    // Literais / Literals
    println(f"A resposta é {42}")
                     // A resposta é 42

    // Expressões complexas / Complex expressions
    let x = 4
    println(f"Quadrado: {x * x}")
                     // Quadrado: 16
}

func arredondar(f: float) returns int {
    return 3
 // simplificado
}
```

---

## 5. Operadores / Operators

### Operadores Aritméticos / Arithmetic Operators

| Operador / Operator | Operação / Operation | Tipos / Types | Exemplo / Example |
|---|---|---|---|
| `+` | Adição / Addition | `int`, `float`, `string`+`string` | `3 + 4` → `7` |
| `-` | Subtração / Subtraction | `int`, `float` | `10 - 3` → `7` |
| `*` | Multiplicação / Multiplication | `int`, `float` | `3 * 4` → `12` |
| `/` | Divisão / Division | `int`, `float` | `10 / 3` → `3` (int) |
| `%` | Módulo (resto) / Modulo (remainder) | `int`, `float` | `10 % 3` → `1` |

```spectra
let a = 10
let b = 3

let soma = a + b
         // 13
let diff = a - b
         // 7
let prod = a * b
         // 30
let quoc = a / b
         // 3  (divisão inteira / integer division)
let resto = a % b
        // 1

// Com floats / With floats
let x = 10.0
let y = 3.0
let div_float = x / y
    // 3.333...
```

> **Nota / Note:** Divisão entre dois `int` é **divisão inteira** (trunca). Use `float` para divisão decimal.  
> Division between two `int`s is **integer division** (truncates). Use `float` for decimal division.

### Operadores de Comparação / Comparison Operators

**PT-BR:**  
Todos os operadores de comparação retornam `bool`.

**EN-US:**  
All comparison operators return `bool`.

| Operador / Operator | Significado / Meaning | Exemplo / Example |
|---|---|---|
| `==` | Igual a / Equal to | `5 == 5` → `true` |
| `!=` | Diferente de / Not equal to | `5 != 3` → `true` |
| `<` | Menor que / Less than | `3 < 5` → `true` |
| `>` | Maior que / Greater than | `5 > 3` → `true` |
| `<=` | Menor ou igual / Less or equal | `3 <= 3` → `true` |
| `>=` | Maior ou igual / Greater or equal | `5 >= 3` → `true` |

```spectra
let x = 10
let y = 20

let igual = x == y
       // false
let dif = x != y
         // true
let menor = x < y
        // true
let maior = x > y
        // false
let men_ig = x <= 10
     // true
let mai_ig = y >= 20
     // true
```

### Operadores Lógicos / Logical Operators

| Operador / Operator | Significado / Meaning | Exemplo / Example |
|---|---|---|
| `&&` | E lógico / Logical AND | `true && false` → `false` |
| `\|\|` | Ou lógico / Logical OR | `true \|\| false` → `true` |
| `!` | Negação lógica / Logical NOT | `!true` → `false` |

```spectra
let a = true
let b = false

let e = a && b
           // false
let ou = a || b
          // true
let nao_a = not a
           // false
let nao_b = not b
           // true

// Condições compostas / Compound conditions
let x = 5
if x > 0 && x < 10 {
    // x está entre 1 e 9 / x is between 1 and 9
}
```

### Operadores Unários / Unary Operators

| Operador / Operator | Significado / Meaning | Tipo / Type | Exemplo / Example |
|---|---|---|---|
| `-` | Negação aritmética / Arithmetic negation | `int`, `float` | `-x`, `-3.14` |
| `!` | Negação lógica / Logical negation | `bool` | `!flag` |

```spectra
let positivo = 42
let negativo = -positivo
    // -42

let verdadeiro = true
let falso = not verdadeiro
     // false
```

### Operadores de Intervalo / Range Operators

| Operador / Operator | Tipo / Type | Exemplo / Example |
|---|---|---|
| `..` | Exclusivo (não inclui o fim) / Exclusive (excludes end) | `0..5` → 0,1,2,3,4 |
| `..=` | Inclusivo (inclui o fim) / Inclusive (includes end) | `1..=5` → 1,2,3,4,5 |

```spectra
// Usados principalmente em for loops / Mainly used in for loops
for i in 0..5 {
    // i = 0, 1, 2, 3, 4
}

for i in 1..=5 {
    // i = 1, 2, 3, 4, 5
}

// Ranges também podem ser armazenados / Ranges can also be stored
let r = 0..10
let r_inc = 1..=100
```

### Precedência de Operadores / Operator Precedence

**PT-BR:**  
Da maior para a menor precedência (operadores mais acima são avaliados primeiro):

**EN-US:**  
From highest to lowest precedence (operators higher up are evaluated first):

| Nível / Level | Operadores / Operators |
|---|---|
| 1 (mais alto / highest) | `!` (unário), `-` (unário) |
| 2 | `*`, `/`, `%` |
| 3 | `+`, `-` (binário) |
| 4 | `<`, `>`, `<=`, `>=` |
| 5 | `==`, `!=` |
| 6 | `&&` |
| 7 (mais baixo / lowest) | `\|\|` |

```spectra
let resultado = 2 + 3 * 4
       // 14 (não 20 / not 20)
let resultado2 = (2 + 3) * 4
    // 20
let cond = x > 0 && y < 10
      // comparações antes de && / comparisons before &&
```

---

## 6. Controle de Fluxo / Control Flow

### `if` / `else if` / `else`

**PT-BR:**  
A instrução `if` avalia uma condição booleana e executa o bloco correspondente. Use `else if` para condições adicionais e `else` para o caso padrão.

**EN-US:**  
The `if` statement evaluates a boolean condition and executes the corresponding block. Use `else if` for additional conditions and `else` for the default case.

```spectra
module controle

from std.io import println

public func main() {
    let nota = 75

    // if simples / simple if
    if nota >= 60 {
        println("Aprovado!")
    }

    // if / else
    if nota >= 60 {
        println("Aprovado!")
    } else {
        println("Reprovado.")
    }

    // if / else if / else
    if nota >= 90 {
        println("A")
    } else if nota >= 80 {
        println("B")
    } else if nota >= 70 {
        println("C")
    } else if nota >= 60 {
        println("D")
    } else {
        println("F")
    }
}
```

**PT-BR:**  
`if` também é uma **expressão** — pode retornar um valor:

**EN-US:**  
`if` is also an **expression** — it can return a value:

```spectra
let classificacao = if nota >= 60 { "Aprovado" } else { "Reprovado" }
```

### `if not`

> **Status:** stable. Enabled by default.

**PT-BR:**  
`if not` é a forma canônica da condição negativa — executa quando a condição é **falsa**.

**EN-US:**  
`if not` is the canonical negative conditional — it executes when the condition is **false**.

```spectra
let autenticado = false

if not autenticado {
    println("Acesso negado!")
}

// Forma canônica / Canonical form:
if not autenticado {
    println("Acesso negado!")
}
```

### `while`

**PT-BR:**  
Executa enquanto a condição for verdadeira. A condição é verificada **antes** de cada iteração.

**EN-US:**  
Executes while the condition is true. The condition is checked **before** each iteration.

```spectra
let i = 0
while i < 5 {
    println(f"i = {i}")
    i = i + 1
}
// Saída / Output: i = 0, i = 1, i = 2, i = 3, i = 4

// Com múltiplas condições / With multiple conditions
let x = 0
let y = 10
while x < 5 && y > 0 {
    x = x + 1
    y = y - 2
}
```

### `do...while`

> **Status:** stable. Enabled by default.

**PT-BR:**  
Executa o bloco **pelo menos uma vez** e depois verifica a condição.

**EN-US:**  
Executes the block **at least once** and then checks the condition.

```spectra
let i = 0
do {
    println(f"Executado: {i}")
    i = i + 1
} while i < 3
// Imprime mesmo se i já fosse ≥ 3 / Prints even if i were already ≥ 3
```

### `for...in`

**PT-BR:**  
Itera sobre intervalos (ranges). Suporta intervalos exclusivos (`..`) e inclusivos (`..=`).

**EN-US:**  
Iterates over ranges. Supports exclusive (`..`) and inclusive (`..=`) ranges.

```spectra
// Intervalo exclusivo / Exclusive range: 0, 1, 2, 3, 4
for i in 0..5 {
    println(f"i = {i}")
}

// Intervalo inclusivo / Inclusive range: 1, 2, 3, 4, 5
for i in 1..=5 {
    println(f"i = {i}")
}

// Percorrer array / Iterate over array
let arr = [10, 20, 30, 40, 50]
for i in 0..5 {
    println(f"arr[{i}] = {arr[i]}")
}
```

### `loop`

> **Status:** stable. Enabled by default.

**PT-BR:**  
Loop infinito. Use `break` para sair.

**EN-US:**  
Infinite loop. Use `break` to exit.

```spectra
let contador = 0
loop {
    contador = contador + 1
    if contador >= 5 {
        break
    }
}
// contador == 5
```

### `switch...case`

> **Status:** stable. Enabled by default.

**PT-BR:**  
`switch` compara um valor contra múltiplos padrões explícitos. Requer um caso `else` padrão ou cobertura exaustiva.

**EN-US:**  
`switch` compares a value against multiple explicit patterns. Requires a default `else` case or exhaustive coverage.

```spectra
let dia = 3

switch dia {
    case 1: {
        println("Segunda / Monday")
    }
    case 2: {
        println("Terça / Tuesday")
    }
    case 3: {
        println("Quarta / Wednesday")
    }
    else: {
        println("Outro dia / Other day")
    }
}
```

### `break` e `continue`

**PT-BR:**  
- `break` sai imediatamente do loop mais interno.
- `continue` pula para a próxima iteração do loop mais interno.

**EN-US:**  
- `break` immediately exits the innermost loop.
- `continue` skips to the next iteration of the innermost loop.

```spectra
// break — sai ao encontrar 3 / exits when finding 3
for i in 0..10 {
    if i == 3 {
        break
    }
    println(f"{i}")
  // Imprime 0, 1, 2 / Prints 0, 1, 2
}

// continue — pula números pares / skips even numbers
for i in 0..8 {
    if i % 2 == 0 {
        continue
    }
    println(f"{i}")
  // Imprime 1, 3, 5, 7 / Prints 1, 3, 5, 7
}
```

### Loops Aninhados / Nested Loops

```spectra
for i in 0..3 {
    for j in 0..3 {
        println(f"{i},{j}")
    }
}

// break sai apenas do loop interno / break exits only the inner loop
for i in 0..5 {
    for j in 0..5 {
        if j == 2 {
            break
  // Sai do for j / Exits the for j
        }
        println(f"{i},{j}")
    }
}
```

---

## 7. Funções / Functions

### Declaração Básica / Basic Declaration

**PT-BR:**  
Funções são declaradas com `func`. Parâmetros requerem anotação de tipo. O tipo de retorno é indicado após `returns`. Se omitido, a função retorna `unit` (vazio).

**EN-US:**  
Functions are declared with `func`. Parameters require type annotations. The return type is indicated after `returns`. If omitted, the function returns `unit` (void).

```spectra
module funcoes

// Função sem parâmetros e sem retorno / Function with no parameters and no return
func saudacao() {
    println("Olá!")
}

// Função com parâmetros / Function with parameters
func soma(a: int, b: int) returns int {
    return a + b
}

// Parâmetros de diferentes tipos / Parameters of different types
func formatar(nome: string, idade: int) returns string {
    return f"{nome} tem {idade} anos"
}

// Função booleana / Boolean function
func eh_par(n: int) returns bool {
    return n % 2 == 0
}
```

### Retorno Implícito / Implicit Return

**PT-BR:**  
A **última expressão** de uma função é retornada automaticamente sem a palavra-chave `return`. Isso é chamado de retorno implícito.

**EN-US:**  
The **last expression** of a function is automatically returned without the `return` keyword. This is called implicit return.

```spectra
// Retorno explícito / Explicit return
func dobrar_explicito(x: int) returns int {
    return x * 2
}

// Retorno implícito / Implicit return
func dobrar_implicito(x: int) returns int {
    x * 2          // Sem ponto e vírgula = expressão de retorno / No semicolon = return expression
}

// Retorno implícito com bloco / Implicit return with block
func maximo(a: int, b: int) returns int {
    if a > b {
        a          // Retorna a / Returns a
    } else {
        b          // Retorna b / Returns b
    }
}
```

> **Regra importante / Important rule:** A superfície canônica não usa `;` para terminar declarações ou instruções. A expressão final da função termina pela quebra de linha, `}`, ou fim do arquivo.

### Retorno Antecipado / Early Return

**PT-BR:**  
Use `return` para sair da função antes do fim, retornando um valor ou `unit`.

**EN-US:**  
Use `return` to exit the function early, returning a value or `unit`.

```spectra
func dividir(a: int, b: int) returns int {
    if b == 0 {
        return 0
   // Retorno antecipado / Early return
    }
    return a / b
}

func validar(nome: string) {
    if nome == "" {
        return
     // Retorno antecipado sem valor / Early return without value
    }
    println(f"Nome válido: {nome}")
}
```

### Visibilidade de Funções / Function Visibility

**PT-BR:**  
Funções são **privadas por padrão** (acessíveis apenas dentro do módulo). Use `public` para torná-las públicas.

**EN-US:**  
Functions are **private by default** (accessible only within the module). Use `public` to make them public.

```spectra
module meu.modulo

// Pública: acessível de outros módulos / Public: accessible from other modules
public func calcular_area(largura: int, altura: int) returns int {
    return largura * altura
}

// Privada: apenas neste módulo / Private: only in this module
func helper_interno() returns int {
    return 42
}
```

### Funções como Valores / Functions as Values

**PT-BR:**  
Funções podem ser passadas como argumentos usando o tipo `func(T) returns R`.

**EN-US:**  
Functions can be passed as arguments using the `func(T) returns R` type.

```spectra
func aplicar(x: int, f: func(int) returns int) returns int {
    return f(x)
}

func dobrar(x: int) returns int {
    return x * 2
}

func triplicar(x: int) returns int {
    return x * 3
}

public func main() {
    let r1 = aplicar(5, dobrar)
     // 10
    let r2 = aplicar(5, triplicar)
  // 15

    // Com closure inline / With inline closure
    let r3 = aplicar(5, |x: int| x * x)
  // 25
}
```

### Funções Genéricas / Generic Functions

**PT-BR:**  
Funções podem aceitar parâmetros de tipo genéricos com `<T>`. Bounds de trait (restrições) são especificados com `:`.

**EN-US:**  
Functions can accept generic type parameters with `<T>`. Trait bounds (constraints) are specified with `:`.

```spectra
// Função genérica simples / Simple generic function
func identidade<T>(valor: T) returns T {
    return valor
}

// Com trait bound / With trait bound
func processar<T: Processavel>(item: T) returns T {
    return item.processar()
}

public func main() {
    let n = identidade(42)
        // int
    let s = identidade("hello")
   // string
}
```

### Chamadas de Função / Function Calls

```spectra
// Chamada simples / Simple call
let resultado = soma(3, 4)
      // 7

// Chamada aninhada / Nested call
let r = soma(dobrar(2), triplicar(3))
   // soma(4, 9) = 13

// Chamada com resultado em expressão / Call in expression
let area = calcular_area(10, 20) * 2
    // 400
```

---

> **Próximo / Next:** [03 — Tipos Compostos / Composite Types](03-tipos-compostos.md)  
> **Anterior / Previous:** [01 — Introdução / Introduction](01-introducao.md)
