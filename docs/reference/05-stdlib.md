# SpectraLang — Biblioteca Padrão / Standard Library

> **Nível / Level:** Intermediário–Avançado / Intermediate–Advanced  
> **Parte / Part:** 5 de 6

---

**PT-BR:**  
A Biblioteca Padrão (stdlib) do SpectraLang é implementada como funções hospedadas (*host functions*) que são registradas pelo runtime e chamadas pelo código JIT via FFI. Existem **12 módulos** com mais de **100 funções**.

**EN-US:**  
SpectraLang's Standard Library (stdlib) is implemented as host functions registered by the runtime and called from JIT code via FFI. There are **12 modules** with over **100 functions**.

## Exact-width numeric contract (R-2901)

The scalar types `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`,
`isize`, `usize`, `f32`, and `f64` are represented explicitly in semantic and
midend IR. `int` remains the compatibility spelling for `i64`, and `float`
for `f64`. `as` casts are checked by default; `as wrapping` is reserved for
integer modular conversion. The certified exact-width scalar matrix is stable:
dynamic overflow diagnostics, AOT/interop evidence, and the C ABI gate pass in
R-2901. Scalar forms outside this matrix remain deferred.

`std.numeric.wrapping_add_*`, `wrapping_sub_*`, and `wrapping_mul_*` provide
explicit modular operations for the supported integer widths.

---

## Sumário / Table of Contents

1. [std.io — Entrada e Saída / Input & Output](#1-stdio--entrada-e-saída--input--output)
2. [std.string — Manipulação de Strings / String Manipulation](#2-stdstring--manipulação-de-strings--string-manipulation)
3. [std.math — Matemática / Mathematics](#3-stdmath--matemática--mathematics)
4. [std.convert — Conversão de Tipos / Type Conversion](#4-stdconvert--conversão-de-tipos--type-conversion)
5. [std.collections — Coleções / Collections](#5-stdcollections--coleções--collections)
6. [std.random — Números Aleatórios / Random Numbers](#6-stdrandom--números-aleatórios--random-numbers)
7. [std.fs — Sistema de Arquivos / File System](#7-stdfs--sistema-de-arquivos--file-system)
8. [std.error — Erros estruturados / Structured Errors](#81-stderror--erros-estruturados--structured-errors)
9. [std.env — Ambiente / Environment](#9-stdenv--ambiente--environment)
10. [std.option — Operações em Option / Option Operations](#10-stdoption--operações-em-option--option-operations)
11. [std.result — Operações em Result / Result Operations](#11-stdresult--operações-em-result--result-operations)
12. [std.char — Operações em Caracteres / Character Operations](#12-stdchar--operações-em-caracteres--character-operations)
13. [std.time — Tempo / Time](#13-stdtime--tempo--time)

---

## 1. std.io — Entrada e Saída / Input & Output

**PT-BR:**  
Módulo para entrada e saída de texto. Este é o módulo mais frequentemente importado.

**EN-US:**  
Module for text input and output. This is the most frequently imported module.

```spectra
import std.io
// ou / or
from std.io import println, print, read_line
```

### Funções / Functions

#### `println(value: any) -> unit`

**PT-BR:** Imprime um valor seguido de uma nova linha na saída padrão.  
**EN-US:** Prints a value followed by a newline to standard output.

```spectra
std.io.println("Olá, mundo!")
       // Olá, mundo!\n
std.io.println(42)
                  // 42\n
std.io.println(3.14)
                // 3.14\n
std.io.println(true)
                // true\n
std.io.println(f"Valor: {100}")
     // Valor: 100\n
```

#### `print(value: any) -> unit`

**PT-BR:** Imprime um valor **sem** nova linha.  
**EN-US:** Prints a value **without** a newline.

```spectra
std.io.print("Olá, ")
std.io.print("mundo")
std.io.println("!")
    // Olá, mundo!
```

#### `eprint(value: any) -> unit`

**PT-BR:** Imprime na saída de erro padrão (stderr) sem nova linha.  
**EN-US:** Prints to standard error (stderr) without a newline.

```spectra
std.io.eprint("Aviso: ")
std.io.eprintln("arquivo não encontrado")
```

#### `eprintln(value: any) -> unit`

**PT-BR:** Imprime na saída de erro padrão com nova linha.  
**EN-US:** Prints to standard error with a newline.

```spectra
std.io.eprintln("Erro fatal: divisão por zero")
```

#### `flush() -> unit`

**PT-BR:** Esvazia o buffer de saída padrão.  
**EN-US:** Flushes the standard output buffer.

```spectra
std.io.print("Carregando...")
std.io.flush()
    // Garante que o texto apareça antes de operação demorada
```

#### `read_line() -> string`

**PT-BR:** Lê uma linha da entrada padrão (aguarda Enter).  
**EN-US:** Reads a line from standard input (waits for Enter).

```spectra
std.io.print("Digite seu nome: ")
let nome = std.io.read_line()
std.io.println(f"Olá, {nome}!")
```

#### `input(prompt: string) -> string`

**PT-BR:** Exibe um prompt e lê uma linha de entrada.  
**EN-US:** Displays a prompt and reads a line of input.

```spectra
let nome = std.io.input("Digite seu nome: ")
let idade_str = std.io.input("Digite sua idade: ")
```

---

## 2. std.string — Manipulação de Strings / String Manipulation

```spectra
import std.string
// ou / or
from std.string import len, trim, contains
```

### Funções / Functions

#### `len(s: string) -> int`

**PT-BR:** Retorna o número de bytes da string (não necessariamente caracteres Unicode).  
**EN-US:** Returns the number of bytes in the string (not necessarily Unicode characters).

```spectra
let n = std.string.len("hello")
        // 5
let n2 = std.string.len("")
            // 0
let n3 = std.string.len("olá")
         // pode variar com Unicode
```

#### `contains(s: string, sub: string) -> bool`

**PT-BR:** Verifica se a string contém a substring.  
**EN-US:** Checks whether the string contains the substring.

```spectra
let tem = std.string.contains("hello world", "world")
  // true
let nao = std.string.contains("hello", "xyz")
          // false
```

#### `to_upper(s: string) -> string`

**PT-BR:** Converte todos os caracteres ASCII para maiúsculo.  
**EN-US:** Converts all ASCII characters to uppercase.

```spectra
let upper = std.string.to_upper("hello")
   // "HELLO"
let mixed = std.string.to_upper("Hello!")
  // "HELLO!"
```

#### `to_lower(s: string) -> string`

**PT-BR:** Converte todos os caracteres ASCII para minúsculo.  
**EN-US:** Converts all ASCII characters to lowercase.

```spectra
let lower = std.string.to_lower("WORLD")
   // "world"
```

#### `trim(s: string) -> string`

**PT-BR:** Remove espaços em branco do início e fim da string.  
**EN-US:** Removes whitespace from the beginning and end of the string.

```spectra
let limpa = std.string.trim("  hello  ")
   // "hello"
let s2 = std.string.trim("\t texto \n")
    // "texto"
```

#### `starts_with(s: string, prefix: string) -> bool`

```spectra
let sw = std.string.starts_with("hello world", "hello")
  // true
let nao = std.string.starts_with("world", "hello")
       // false
```

#### `ends_with(s: string, suffix: string) -> bool`

```spectra
let ew = std.string.ends_with("hello.spectra", ".spectra")
  // true
```

#### `concat(a: string, b: string) -> string`

**PT-BR:** Concatena duas strings.  
**EN-US:** Concatenates two strings.

```spectra
let ab = std.string.concat("foo", "bar")
    // "foobar"
// Nota: o operador + também concatena strings / Note: the + operator also concatenates strings
let ab2 = "foo" + "bar"
    // "foobar"
```

#### `repeat_str(s: string, n: int) -> string`

**PT-BR:** Repete a string `n` vezes.  
**EN-US:** Repeats the string `n` times.

```spectra
let rep = std.string.repeat_str("ab", 3)
    // "ababab"
let linha = std.string.repeat_str("-", 40)
  // "----------------------------------------"
```

#### `char_at(s: string, index: int) -> int`

**PT-BR:** Retorna o código Unicode do caractere na posição `index`. Retorna `-1` se o índice estiver fora dos limites.  
**EN-US:** Returns the Unicode code point of the character at position `index`. Returns `-1` if the index is out of bounds.

```spectra
let c = std.string.char_at("hello", 0)
     // 104 ('h')
let e = std.string.char_at("hello", 1)
     // 101 ('e')
let oob = std.string.char_at("hi", 10)
     // -1
```

#### `substring(s: string, start: int, end: int) -> string`

**PT-BR:** Extrai a substring de `start` até `end` (exclusivo).  
**EN-US:** Extracts substring from `start` to `end` (exclusive).

```spectra
let sub = std.string.substring("hello world", 0, 5)
    // "hello"
let sub2 = std.string.substring("hello world", 6, 11)
  // "world"
```

#### `replace(s: string, from: string, to: string) -> string`

**PT-BR:** Substitui todas as ocorrências de `from` por `to`.  
**EN-US:** Replaces all occurrences of `from` with `to`.

```spectra
let r = std.string.replace("hello world", "world", "SpectraLang")
// "hello SpectraLang"
```

#### `index_of(s: string, sub: string) -> int`

**PT-BR:** Retorna a posição (índice 0) da primeira ocorrência de `sub`, ou `-1` se não encontrada.  
**EN-US:** Returns the position (0-index) of the first occurrence of `sub`, or `-1` if not found.

```spectra
let pos = std.string.index_of("hello world", "world")
  // 6
let nao = std.string.index_of("hello", "xyz")
          // -1
```

#### `split_first(s: string, sep: string) -> string`

**PT-BR:** Retorna a parte antes do primeiro separador.  
**EN-US:** Returns the part before the first separator.

```spectra
let parte = std.string.split_first("nome:Alice:30", ":")
  // "nome"
```

#### `split_last(s: string, sep: string) -> string`

**PT-BR:** Retorna a parte após o último separador.  
**EN-US:** Returns the part after the last separator.

```spectra
let ultima = std.string.split_last("nome:Alice:30", ":")
  // "30"
```

#### `count_occurrences(s: string, sub: string) -> int`

```spectra
let count = std.string.count_occurrences("banana", "a")
  // 3
```

#### `split_by(s: string, sep: string) -> List<string>` (beta)

Retorna uma lista tipada com as partes separadas por `sep`. O resultado pode
ser usado com `std.collections.list_len` e os demais acessors de `List<T>`.

```spectra
let parts = std.string.split_by("a,b,c", ",")
let count = std.collections.list_len(parts)
// 3
std.collections.list_free(parts)
```

#### `is_empty(s: string) -> bool`

```spectra
let vazio = std.string.is_empty("")
        // true
let nao   = std.string.is_empty("hello")
   // false
```

#### `pad_left(s: string, width: int, pad_char: int) -> string`

**PT-BR:** Preenche a string à esquerda com o caractere especificado até atingir `width`.  
**EN-US:** Left-pads the string with the specified character until reaching `width`.

```spectra
// pad_char é o código Unicode do caractere / pad_char is the Unicode code point
let padded = std.string.pad_left("42", 5, 48)
   // "   42" (48 = '0')
// Nota: 48 é o código de '0', 32 é espaço / Note: 48 is code for '0', 32 is space
```

#### `pad_right(s: string, width: int, pad_char: int) -> string`

```spectra
let padded = std.string.pad_right("hello", 8, 32)
  // "hello   " (32 = espaço/space)
```

#### `reverse_str(s: string) -> string`

```spectra
let rev = std.string.reverse_str("hello")
  // "olleh"
```

---

## 3. std.math — Matemática / Mathematics

```spectra
import std.math
// ou / or
import std.math as math
```

### Funções Inteiras / Integer Functions

#### `abs(x: int) -> int`

```spectra
let v = std.math.abs(-42)
     // 42
let v2 = std.math.abs(10)
     // 10
```

#### `min(lhs: int, rhs: int) -> int`

```spectra
let menor = std.math.min(3, 7)
    // 3
```

#### `max(lhs: int, rhs: int) -> int`

```spectra
let maior = std.math.max(3, 7)
    // 7
```

#### `clamp(n: int, min: int, max: int) -> int`

**PT-BR:** Restringe `n` ao intervalo `[min, max]`.  
**EN-US:** Restricts `n` to the range `[min, max]`.

```spectra
let v = std.math.clamp(150, 0, 100)
   // 100
let v2 = std.math.clamp(-5, 0, 100)
   // 0
let v3 = std.math.clamp(50, 0, 100)
   // 50
```

#### `sign(n: int) -> int`

**PT-BR:** Retorna `-1`, `0` ou `1`.  
**EN-US:** Returns `-1`, `0`, or `1`.

```spectra
let s1 = std.math.sign(-5)
   // -1
let s2 = std.math.sign(0)
    // 0
let s3 = std.math.sign(10)
   // 1
```

#### `gcd(a: int, b: int) -> int`

**PT-BR:** Máximo divisor comum.  
**EN-US:** Greatest common divisor.

```spectra
let g = std.math.gcd(12, 8)
    // 4
```

#### `lcm(a: int, b: int) -> int`

**PT-BR:** Mínimo múltiplo comum.  
**EN-US:** Least common multiple.

```spectra
let l = std.math.lcm(4, 6)
    // 12
```

### Funções de Ponto Flutuante / Float Functions

#### `abs_f(x: float) -> float`

```spectra
let v = std.math.abs_f(-3.14)
   // 3.14
```

#### `sqrt_f(x: float) -> float`

```spectra
let r = std.math.sqrt_f(16.0)
   // 4.0
let r2 = std.math.sqrt_f(2.0)
   // ~1.4142
```

#### `pow_f(base: float, exp: float) -> float`

```spectra
let p = std.math.pow_f(2.0, 10.0)
   // 1024.0
let p2 = std.math.pow_f(3.0, 0.5)
   // ~1.732 (raiz / sqrt)
```

#### `floor_f(x: float) -> float`

```spectra
let f = std.math.floor_f(3.7)
    // 3.0
let f2 = std.math.floor_f(-1.2)
  // -2.0
```

#### `ceil_f(x: float) -> float`

```spectra
let c = std.math.ceil_f(3.2)
     // 4.0
let c2 = std.math.ceil_f(-1.8)
   // -1.0
```

#### `round_f(x: float) -> float`

```spectra
let r = std.math.round_f(3.5)
    // 4.0
let r2 = std.math.round_f(3.4)
   // 3.0
```

#### `sin_f(x: float) -> float` / `cos_f(x: float) -> float` / `tan_f(x: float) -> float`

**PT-BR:** Funções trigonométricas. `x` em radianos.  
**EN-US:** Trigonometric functions. `x` in radians.

```spectra
import std.math as m

let pi = m.pi()
let seno  = m.sin_f(pi / 2.0)
    // ~1.0
let coss  = m.cos_f(0.0)
          // 1.0
let tang  = m.tan_f(pi / 4.0)
    // ~1.0
```

#### `log_f(x: float) -> float`

**PT-BR:** Logaritmo natural (base e).  
**EN-US:** Natural logarithm (base e).

```spectra
let ln_e = std.math.log_f(2.71828)
   // ~1.0
```

#### `log2_f(x: float) -> float` / `log10_f(x: float) -> float`

```spectra
let l2  = std.math.log2_f(8.0)
     // 3.0
let l10 = std.math.log10_f(1000.0)
 // 3.0
```

#### `atan2_f(y: float, x: float) -> float`

**PT-BR:** Arco-tangente de y/x, considerando o quadrante.  
**EN-US:** Arc-tangent of y/x, considering the quadrant.

```spectra
let angulo = std.math.atan2_f(1.0, 1.0)
   // pi/4 (~0.785)
```

#### `is_nan_f(x: float) -> bool` / `is_infinite_f(x: float) -> bool`

```spectra
let nan_check = std.math.is_nan_f(0.0 / 0.0)
        // true (comportamento impl-defined)
let inf_check = std.math.is_infinite_f(1.0 / 0.0)
   // true
```

### Constantes / Constants

#### `pi() -> float`

```spectra
let pi = std.math.pi()
    // ~3.14159265358979
```

#### `e_const() -> float`

```spectra
let e = std.math.e_const()
    // ~2.71828182845905
```

### Exemplo Completo / Complete Example

```spectra
module matematica

import std.math as m
from std.io import println

public func main() {
    let pi = m.pi()
    let raio = 5.0
    let area = pi * m.pow_f(raio, 2.0)
    let circunferencia = 2.0 * pi * raio

    println(f"Raio: {raio}")
    println(f"Área: {area}")
    println(f"Circunferência: {circunferencia}")

    // Teorema de Pitágoras / Pythagorean theorem
    let a = 3.0
    let b = 4.0
    let hipotenusa = m.sqrt_f(m.pow_f(a, 2.0) + m.pow_f(b, 2.0))
    println(f"Hipotenusa: {hipotenusa}")
    // 5.0
}
```

---

## 4. std.convert — Conversão de Tipos / Type Conversion

```spectra
import std.convert
```

### Funções / Functions

#### `int_to_string(val: int) -> string`

```spectra
let s = std.convert.int_to_string(42)
      // "42"
let s2 = std.convert.int_to_string(-100)
   // "-100"
```

#### `float_to_string(val: float) -> string`

```spectra
let s = std.convert.float_to_string(3.14)
  // "3.14"
```

#### `bool_to_string(val: bool) -> string`

```spectra
let s1 = std.convert.bool_to_string(true)
   // "true"
let s2 = std.convert.bool_to_string(false)
  // "false"
```

#### `string_to_int(s: string) -> int`

**PT-BR:** Converte string para int. Retorna `0` em caso de erro.  
**EN-US:** Converts string to int. Returns `0` on error.

```spectra
let n = std.convert.string_to_int("123")
   // 123
let e = std.convert.string_to_int("abc")
   // 0 (erro / error)
```

#### `string_to_float(s: string) -> float`

**PT-BR:** Converte string para float. Retorna `0.0` em caso de erro.  
**EN-US:** Converts string to float. Returns `0.0` on error.

```spectra
let f = std.convert.string_to_float("3.14")
    // 3.14
let e = std.convert.string_to_float("xyz")
     // 0.0
```

#### `int_to_float(val: int) -> float`

```spectra
let f = std.convert.int_to_float(7)
    // 7.0
```

#### `float_to_int(val: float) -> int`

**PT-BR:** Converte float para int truncando (não arredonda).  
**EN-US:** Converts float to int by truncating (not rounding).

```spectra
let i = std.convert.float_to_int(9.9)
     // 9  (truncado / truncated)
let i2 = std.convert.float_to_int(-3.7)
   // -3
```

#### `string_to_int_or(s: string, default: int) -> int`

**PT-BR:** Converte com valor padrão em caso de erro.  
**EN-US:** Converts with a default value on error.

```spectra
let n = std.convert.string_to_int_or("abc", -1)
   // -1
let n2 = std.convert.string_to_int_or("42", -1)
   // 42
```

#### `string_to_float_or(s: string, default: float) -> float`

```spectra
let f = std.convert.string_to_float_or("bad", 0.0)
   // 0.0
```

#### `string_to_bool(s: string) -> bool`

**PT-BR:** Retorna `true` se a string for `"true"` (case-insensitive), `false` caso contrário.  
**EN-US:** Returns `true` if the string is `"true"` (case-insensitive), `false` otherwise.

```spectra
let b1 = std.convert.string_to_bool("true")
    // true
let b2 = std.convert.string_to_bool("false")
   // false
let b3 = std.convert.string_to_bool("1")
       // false
```

#### `bool_to_int(b: bool) -> int`

```spectra
let i1 = std.convert.bool_to_int(true)
    // 1
let i2 = std.convert.bool_to_int(false)
   // 0
```

---

## 5. std.collections — Coleções / Collections

**PT-BR:**  
O contrato de fonte de `std.collections` usa `List<T>`, `Map<K,V>`, `Set<T>` e
`Iterator<T>` tipados. A implementação atual transporta esses valores como
handles opacos na ABI do runtime; esse detalhe não faz parte do tipo que o
programa SpectraLang deve manipular.

**EN-US:**  
The source contract of `std.collections` uses typed `List<T>`, `Map<K,V>`,
`Set<T>`, and `Iterator<T>`. The current runtime ABI transports those values as
opaque handles; that representation is not a source-level type to manipulate
directly.

```spectra
import std.collections as col
```

### Operações Básicas / Basic Operations

#### `list_new<T>() -> List<T>` (beta)

**PT-BR:** Cria uma nova lista vazia. O tipo `T` vem do contexto de uso.
**EN-US:** Creates a new empty list. `T` is inferred from the use context.

```spectra
let lista: List<int> = col.list_new()
```

#### `list_push<T>(list: List<T>, value: T) -> unit` (beta)

```spectra
let lista: List<int> = col.list_new()
col.list_push(lista, 10)
col.list_push(lista, 20)
col.list_push(lista, 30)
```

#### `list_len<T>(list: List<T>) -> int` (beta)

```spectra
let n = col.list_len(lista)
   // 3
```

#### Typed absence-safe accessors (beta)

The main `std.collections` names return `Option<T>` whenever the operation can
legitimately have no value. This keeps an absent element distinct from valid
payloads such as `-1` or `0`.

| Função / Function | Assinatura / Signature | Ausência / Missing value |
|---|---|---|
| `list_get` | `list_get<T>(list: List<T>, index: int) -> Option<T>` | `None` for an invalid index |
| `list_pop` | `list_pop<T>(list: List<T>) -> Option<T>` | `None` for an empty list |
| `list_pop_front` | `list_pop_front<T>(list: List<T>) -> Option<T>` | `None` for an empty list |
| `list_remove_at` | `list_remove_at<T>(list: List<T>, index: int) -> Option<T>` | `None` for an invalid index |
| `map_get` | `map_get<K,V>(map: Map<K,V>, key: K) -> Option<V>` | `None` for an unknown key |
| `map_remove` | `map_remove<K,V>(map: Map<K,V>, key: K) -> Option<V>` | `None` for an unknown key |
| `list_get_option` | `list_get_option<T>(list: List<T>, index: int) -> Option<T>` | `None` for an invalid index |
| `list_pop_option` | `list_pop_option<T>(list: List<T>) -> Option<T>` | `None` for an empty list |
| `list_pop_front_option` | `list_pop_front_option<T>(list: List<T>) -> Option<T>` | `None` for an empty list |
| `list_remove_at_option` | `list_remove_at_option<T>(list: List<T>, index: int) -> Option<T>` | `None` for an invalid index |
| `map_get_option` | `map_get_option<K,V>(map: Map<K,V>, key: K) -> Option<V>` | `None` for an unknown key |
| `map_remove_option` | `map_remove_option<K,V>(map: Map<K,V>, key: K) -> Option<V>` | `None` for an unknown key |

```spectra
import std.option as option

let maybe_value = col.list_get(lista, 99)
if option.is_some(maybe_value) {
    let value = option.option_unwrap(maybe_value)
    println(value)
}
```

The names ending in `_option` remain explicit aliases for the same absence-safe
operations and are useful when migrating code that already used those names.

#### `std.compat.collections` — API legada com sentinelas

Programas antigos podem importar explicitamente `std.compat.collections` para
preservar os retornos sentinela. `list_get`, `list_pop`, `list_pop_front` e
`list_remove_at` retornam `-1` quando não há elemento; `map_get` e `map_remove`
retornam `0` quando a chave não existe. Essa superfície não é a API estável
recomendada.

```spectra
import std.compat.collections as legacy

let missing = legacy.list_get(lista, 99)
// -1: comportamento legado explícito
```

#### `list_set<T>(list: List<T>, index: int, value: T) -> unit`

```spectra
col.list_set(lista, 0, 99)
    // Substitui o elemento 0 por 99
```

#### `list_pop<T>(list: List<T>) -> Option<T>`

**PT-BR:** Remove e retorna o último elemento, ou `None` se a lista estiver vazia.
**EN-US:** Removes and returns the last element, or `None` if the list is empty.

```spectra
let ultimo = col.list_pop(lista)
// Some(30)
```

#### `list_pop_front<T>(list: List<T>) -> Option<T>`

```spectra
let primeiro = col.list_pop_front(lista)
// Some(10)
```

#### `list_insert_at<T>(list: List<T>, index: int, value: T) -> unit`

```spectra
col.list_insert_at(lista, 1, 50)
    // Insere 50 na posição 1
```

#### `list_remove_at<T>(list: List<T>, index: int) -> Option<T>`

**PT-BR:** Remove o elemento no índice e o retorna, ou `None` se o índice for inválido.
**EN-US:** Removes the element at the index and returns it, or `None` if invalid.

```spectra
let removido = col.list_remove_at(lista, 0)
// Some(10)
```

#### `list_contains<T>(list: List<T>, value: T) -> bool`

```spectra
let tem = col.list_contains(lista, 20)
   // true/false
```

#### `list_index_of<T>(list: List<T>, value: T) -> int`

**PT-BR:** Retorna o índice da primeira ocorrência ou `-1`.  
**EN-US:** Returns the index of the first occurrence or `-1`.

```spectra
let idx = col.list_index_of(lista, 20)
   // índice ou -1
```

#### `list_sort<T>(list: List<T>) -> unit`

**PT-BR:** Ordena a lista em ordem crescente in-place.  
**EN-US:** Sorts the list in ascending order in-place.

```spectra
col.list_sort(lista)
```

#### `list_clear<T>(list: List<T>) -> unit`

```spectra
col.list_clear(lista)
    // Remove todos os elementos
```

#### `list_free<T>(list: List<T>) -> unit`

**PT-BR:** Libera a memória da lista. **Importante:** Chamar quando não precisar mais.  
**EN-US:** Frees the list's memory. **Important:** Call when no longer needed.

```spectra
col.list_free(lista)
    // Libera recursos
```

#### `list_free_all() -> int`

**PT-BR:** Libera todas as listas alocadas. Retorna quantas foram liberadas.  
**EN-US:** Frees all allocated lists. Returns how many were freed.

```spectra
let liberadas = col.list_free_all()
```

### Set e Iterator / Set and Iterator (beta)

`Set<T>` preserva a ordem de inserção para tornar snapshots e iteração
determinísticos; chaves `string` usam igualdade por valor. Inserção e remoção retornam `bool`; acesso posicional é seguro
e retorna `Option<T>`. `Iterator<T>` é consumível: `iterator_next` retorna
`Option<T>` e `iterator_remaining` informa quantos elementos ainda podem ser
consumidos.

`Set<T>` preserves insertion order so snapshots and iteration are deterministic;
`string` keys use value equality.
Insertion and removal return `bool`; positional access is safe and returns
`Option<T>`. `Iterator<T>` is consumable: `iterator_next` returns `Option<T>`
and `iterator_remaining` reports the remaining elements.

```spectra
from std.collections import Set, Iterator
import std.option as option

let ids: Set<int> = col.set_new()
col.set_insert(ids, 10)
col.set_insert(ids, 20)
let ids_iter: Iterator<int> = col.set_iter(ids)
let first = col.iterator_next(ids_iter)
if option.is_some(first) {
    println(option.option_unwrap(first))
}
col.iterator_free(ids_iter)
col.set_free(ids)
```

As funções `list_iter`, `set_iter` e `map_iter` criam iteradores snapshot;
`map_iter` percorre as chaves em uma ordem estável para o snapshot atual. A expressão `for`
consome o mesmo protocolo para ranges, arrays, listas, sets, mapas e iteradores
explícitos. `std.range.iter` é o adaptador público para ranges.

The `list_iter`, `set_iter`, and `map_iter` functions create snapshot
iterators; `map_iter` visits keys in a stable order for the current snapshot. The `for` expression
uses the same protocol for ranges, arrays, lists, sets, maps, and explicit
iterators. `std.range.iter` is the public range adapter.

| Função / Function | Assinatura / Signature | Resultado / Result |
|---|---|---|
| `set_new` | `set_new<T>() -> Set<T>` | conjunto vazio / empty set |
| `set_insert` | `set_insert<T>(set: Set<T>, value: T) -> bool` | `true` se inseriu / inserted |
| `set_contains` | `set_contains<T>(set: Set<T>, value: T) -> bool` | presença / membership |
| `set_remove` | `set_remove<T>(set: Set<T>, value: T) -> bool` | `true` se removeu / removed |
| `set_len` | `set_len<T>(set: Set<T>) -> int` | cardinalidade / cardinality |
| `set_get` | `set_get<T>(set: Set<T>, index: int) -> Option<T>` | snapshot posicional |
| `list_iter` / `set_iter` | `List<T>` / `Set<T> -> Iterator<T>` | snapshot iterator |
| `map_iter` | `Map<K,V> -> Iterator<K>` | iterator de chaves / key iterator |
| `iterator_next` | `iterator_next<T>(iterator: Iterator<T>) -> Option<T>` | próximo valor / next value |
| `iterator_remaining` | `iterator_remaining<T>(iterator: Iterator<T>) -> int` | itens restantes / remaining |
| `iterator_free` | `iterator_free<T>(iterator: Iterator<T>) -> unit` | libera o handle / drops handle |

### Funções de Alta Ordem / Higher-Order Functions

#### `list_map(list: List<int>, fn: func(int) returns int) -> List<int>`

**PT-BR:** Cria uma nova lista aplicando a closure tipada a cada elemento.
**EN-US:** Creates a new list by applying the typed closure to each element.

O callback de `list_map` precisa aceitar e retornar `int` na implementação
runtime atual. O resultado é uma nova `List<int>` independente.

#### `list_filter(list: List<int>, predicate: func(int) returns bool) -> List<int>`

**PT-BR:** Cria uma nova lista com apenas os elementos que satisfazem o predicado booleano.
**EN-US:** Creates a new list with only elements satisfying the boolean predicate.

#### `list_reduce(list: List<int>, initial: int, fn: func(int, int) returns int) -> int`

**PT-BR:** Reduz a lista a um único valor acumulando com a função.  
**EN-US:** Reduces the list to a single value by accumulating with the function.

#### `list_sort_by(list: List<int>, comparator: func(int, int) returns int) -> unit`

**PT-BR:** Ordena com comparador customizado. A função comparador deve retornar `-1`, `0`, ou `1`.  
**EN-US:** Sorts with a custom comparator. The comparator function must return `-1`, `0`, or `1`.

### Exemplo Completo / Complete Example

```spectra
module usando_colecoes

import std.collections as col
from std.collections import List
from std.io import println
import std.convert
import std.option as option

public func main() {
    // Criar lista / Create list
    let lista: List<int> = col.list_new()

    // Adicionar elementos / Add elements
    col.list_push(lista, 5)
    col.list_push(lista, 3)
    col.list_push(lista, 8)
    col.list_push(lista, 1)
    col.list_push(lista, 9)
    col.list_push(lista, 2)

    println(f"Tamanho: {col.list_len(lista)}")
    // 6

    // Ordenar / Sort
    col.list_sort(lista)

    // Imprimir todos / Print all
    let i = 0
    while i < col.list_len(lista) {
        let maybe_value = col.list_get(lista, i)
        println(std.convert.int_to_string(option.option_unwrap(maybe_value)))
        i = i + 1
    }
    // 1, 2, 3, 5, 8, 9

    // Verificar / Check
    println(f"Contém 5: {col.list_contains(lista, 5)}")
   // true
    println(f"Índice de 8: {col.list_index_of(lista, 8)}")
 // 4

    // Liberar / Free
    col.list_free(lista)
}
```

---

## 6. std.tensor — Tensores / Tensors

**PT-BR:**  
`std.tensor` fornece o núcleo de produção atual de tensores para IA/ML. A ABI continua usando handles opacos (`int`), mas a linguagem já reconhece anotações parciais `Tensor<dtype, rankN>` para código novo. Cada tensor tem dtype (`int` ou `float`), shape, strides, layout, armazenamento CPU compartilhado e offset base para views seguras.

**EN-US:**  
`std.tensor` provides the current production tensor core for AI/ML. The ABI still uses opaque handles (`int`), but the language now recognizes partial `Tensor<dtype, rankN>` annotations for new code. Each tensor has dtype (`int` or `float`), shape, strides, layout, shared CPU storage, and a base offset for safe views.

```spectra
import std.tensor as tensor
```

### Criação / Creation

Código novo pode usar `Tensor<float, rank1>`, `Tensor<float, rank2>` e metadados opcionais de dimensão/layout/device quando a anotação é explícita:

New code can use `Tensor<float, rank1>`, `Tensor<float, rank2>`, and optional dimension/layout/device metadata when the annotation is explicit:

```spectra
let v: Tensor<float, rank1, dim3, row_major, cpu> = [1.0, 2.0, 3.0]
let any_len: Tensor<float, rank1, dynamic_dim, row_major, cpu> = v
let m: Tensor<float, rank2, dim2, dim2, row_major, cpu> = [[1.0, 2.0], [3.0, 4.0]]
```

Rank, dtype, dimensão estática, layout e device incompatíveis falham em `check`/`compile` com códigos JSON estáveis `E1401` a `E1405`. Literais rank2 precisam ser retangulares.

Rank, dtype, static dimension, layout, and device mismatches fail during `check`/`compile` with stable JSON codes `E1401` through `E1405`. Rank2 literals must be rectangular.

| Função / Function | Assinatura / Signature | Descrição / Description |
|---|---|---|
| `vector_f` | `(size: int, value: float) -> Tensor<float, rank1>` | 1D float tensor filled with `value` |
| `matrix_f` | `(rows: int, cols: int, value: float) -> Tensor<float, rank2>` | 2D float tensor filled with `value` |
| `zeros` | `(size: int) -> int` | 1D int tensor filled with `0` |
| `ones` | `(size: int) -> int` | 1D int tensor filled with `1` |
| `full` | `(size: int, value: int) -> int` | 1D int tensor filled with `value` |
| `full_f` | `(size: int, value: float) -> Tensor<float, rank1>` | 1D float tensor filled with `value` |
| `arange` | `(start: int, end: int, step: int) -> int` | 1D int range tensor |
| `zeros2`, `ones2` | `(rows: int, cols: int) -> int` | 2D int tensors |
| `full2`, `full2_f` | `(rows: int, cols: int, value) -> int` / `Tensor<float, rank2>` | 2D tensors filled with value |
| `uniform` | `(size: int, min: int, max: int) -> int` | Seeded int tensor with values in `[min, max)` |
| `uniform_f` | `(size: int, min: float, max: float) -> int` | Seeded float tensor with values in `[min, max)` |
| `normal_f` | `(size: int, mean: float, stddev: float) -> int` | Seeded normal-distribution float tensor |
| `bernoulli` | `(size: int, p: float) -> int` | Seeded int tensor with `0/1` samples |
| `categorical` | `(size: int, weights: int) -> int` | Seeded category samples from a 1D weight tensor |
| `set_deterministic_mode` | `(enabled: int) -> int` | Enables deterministic tensor mode and resets RNG to a stable seed when enabled; returns `0` on success |
| `deterministic_mode` | `() -> int` | Reports deterministic tensor mode as `0` or `1` |
| `tolerance_abs`, `tolerance_rel` | `() -> float` | Numerical certification tolerance policy |

### Metadados e Acesso / Metadata and Access

| Função / Function | Assinatura / Signature |
|---|---|
| `len` | `(handle: int) -> int` |
| `rank` | `(handle: int) -> int` |
| `dim` | `(handle: int, axis: int) -> int` |
| `rows`, `cols` | `(handle: int) -> int` |
| `device` | `(handle: int) -> int` |
| `device_available` | `(device: int) -> bool` |
| `device_status` | `(device: int) -> int` |
| `to_device` | `(handle: int, device: int) -> int` |
| `cpu` | `(handle: int) -> int` |
| `sync` | `(handle: int) -> unit` |
| `precision` | `(handle: int) -> int` |
| `to_precision` | `(handle: int, precision: int) -> int` |
| `get`, `get_f` | `(handle: int, index: int) -> int/float` |
| `set`, `set_f` | `(handle: int, index: int, value) -> unit` |
| `get2`, `get2_f` | `(handle: int, row: int, col: int) -> int/float` |
| `set2`, `set2_f` | `(handle: int, row: int, col: int, value) -> unit` |

Views compartilham armazenamento quando possível. `set` e `set2` aplicam copy-on-write quando o armazenamento é compartilhado, evitando mutação insegura entre aliases.

Views share storage where possible. `set` and `set2` apply copy-on-write when storage is shared, avoiding unsafe alias mutation.

### Operações / Operations

| Função / Function | Descrição / Description |
|---|---|
| `reshape(handle, rows, cols)` | Returns a new handle with validated 2D shape |
| `flatten(handle)` | Returns a new 1D tensor handle |
| `permute(handle, axis_a, axis_b)` | Swaps two axes and returns a view handle |
| `slice(handle, start, end)` | Returns a 1D shared-storage slice view |
| `concat(lhs, rhs)` | Concatenates compatible tensors on axis 0 |
| `stack(lhs, rhs)` | Stacks two same-shape tensors on a new leading axis |
| `add`, `sub`, `mul`, `div` | Elementwise ops; shapes and dtypes must match |
| `neg`, `relu` | Unary ops over int or float tensors |
| `exp_f`, `log_f`, `sqrt_f`, `sigmoid_f`, `tanh_f` | Float-output unary kernels |
| `sum`, `sum_f`, `mean_f`, `min`, `max`, `argmax` | Reductions |
| `sum_t`, `mean_t` | Differentiable scalar tensor reductions for `backward` |
| `matmul(lhs, rhs)` | 2D matrix multiplication |
| `matmul_batched(lhs, rhs)` | 3D batched matrix multiplication: `[batch, m, k] x [batch, k, n]` |
| `transpose(handle)` | 2D transpose |
| `dot(lhs, rhs)` | 1D dot product; returns `int` for int tensors and f64 ABI bits for float tensors |
| `dot_t(lhs, rhs)` | Differentiable 1D dot product returning a scalar tensor |
| `seed(value)` | Sets the deterministic tensor RNG seed |
| `requires_grad(handle, enabled)` | Enables/disables gradient tracking for a float tensor |
| `backward(loss)` | Runs reverse-mode autodiff from a scalar tensor loss |
| `grad(handle)` | Returns the accumulated gradient tensor |
| `zero_grad(handle)` | Clears accumulated gradient |
| `set_grad_enabled(enabled)`, `grad_enabled()` | Controls inference/no-grad mode |
| `stats_graph_nodes()` | Counts live autograd graph nodes |
| `stats_allocations`, `stats_active`, `stats_active_bytes`, `stats_peak_bytes` | Tensor allocation metrics |
| `stats_reused_buffers`, `stats_pool_hits`, `stats_pool_misses`, `stats_scratch_reuses` | Buffer-pool and scratch metrics |
| `stats_kernel_ops`, `stats_kernel_elements`, `kernel_strategy` | Kernel work and dispatch metrics |
| `stats_device_transfers` | Device transfer metric |
| `stats_gpu_kernel_ops` | Successful GPU kernel dispatch count |
| `stats_cpu_fallbacks` | GPU kernel failures recovered through CPU fallback |
| `stats_lifetime_records`, `stats_released_lifetimes` | Tensor lifetime planning counters |
| `stats_allocation_sites`, `stats_reuse_rate_per_mille` | Allocation-site visibility and buffer reuse rate |
| `memory_report()` | JSON memory report with schema `spectra.tensor.memory_report.v1` |
| `reset_stats()` | Resets tensor metrics while preserving active tensor accounting |
| `free(handle)`, `free_all()` | Release tensor handles |

### Blocos diferenciáveis / Differentiable Blocks

`diff { ... }` marca uma região diferenciável. O bloco deve produzir um tensor escalar de loss, normalmente criado por `sum_t`, `mean_t` ou `dot_t`. O compilador baixa o bloco para `backward(loss)` e retorna o próprio `loss` para uso posterior.

`diff { ... }` marks a differentiable region. The block must produce a scalar tensor loss, usually created by `sum_t`, `mean_t`, or `dot_t`. The compiler lowers the block to `backward(loss)` and returns the same `loss` for later use.

```spectra
let initial: Tensor<float, rank1> = [3.0, 3.0, 3.0]
let weights: Tensor<float, rank1> = tensor.requires_grad(initial, true)
let loss: Tensor<float, rank0> = diff {
    tensor.sum_t(tensor.mul(weights, weights))
}
let grad: Tensor<float, rank1> = tensor.grad(weights)
```

Operações qualificadas de stdlib que não participam do grafo diferenciável, como metadados (`tensor.rank`) ou lifecycle (`tensor.free_all`), falham dentro de `diff { ... }` com o código estável `E1406`. Mova I/O, metadados e liberação de recursos para fora do bloco.

Qualified stdlib operations that do not participate in the differentiable graph, such as metadata (`tensor.rank`) or lifecycle (`tensor.free_all`), fail inside `diff { ... }` with stable code `E1406`. Move I/O, metadata, and resource release outside the block.

### Exemplo / Example

```spectra
module tensor_demo

import std.tensor as tensor

public func main() returns int {
    let a = tensor.arange(1, 5, 1)
    // [1, 2, 3, 4]
    let b = tensor.full(4, 2)
         // [2, 2, 2, 2]
    let c = tensor.add(a, b)
          // [3, 4, 5, 6]

    if tensor.sum(c) != 18 {
        return tensor.sum(c)
    }

    let m = tensor.reshape(tensor.arange(1, 7, 1), 2, 3)
    let ones = tensor.ones2(3, 2)
    let product = tensor.matmul(m, ones)
    let product_cpu = tensor.to_device(product, 0)
    tensor.sync(product_cpu)

    if tensor.get2(product_cpu, 1, 0) != 15 {
        return tensor.get2(product_cpu, 1, 0)
    }

    tensor.free_all()
    return 0
}
```

Estado Phase 3/4: `std.tensor` inclui views seguras, copy-on-write em mutação compartilhada, operações MVP de tensor, kernels CPU portáveis, RNG reproduzível por seed, distribuições básicas, categorical sampling, métricas de alocação/kernel e benchmark release reproduzível. Estado Phase 7/16: device placement é explícito para handles CPU e `wgpu`, com `device`, `device_available`, `device_status`, `to_device`, `cpu`, `sync`, `stats_device_transfers`, `stats_gpu_kernel_ops` e `stats_cpu_fallbacks`; device `0` é CPU, device `6` é `wgpu` com `--features gpu`, e os códigos `1` CUDA, `2` ROCm, `3` Metal, `4` DirectML e `5` Vulkan são reservados sem implementação no build atual. `device_status` retorna `0` para um backend implementado e disponível, `1` para `wgpu` implementado mas indisponível no build/host, e `HOST_STATUS_INVALID_ARGUMENT` para devices reservados ou desconhecidos. Mixed precision usa `precision`/`to_precision` com códigos `0` f64, `1` f32, `2` f16 e `3` bf16. Estado Phase 14: `Tensor<dtype, rankN, dimN|dynamic_dim, layout, device>`, literais rank1/rank2, validação estática de shape em operações principais e `diff { ... }` com diagnóstico `E1406` estão completos para o baseline atual. Estado Phase 15/R-1501: `scripts/validate_r1501_bench.py` executa benchmarks release de criação de tensor, unary ops, reductions, matmul, convolução, autodiff, otimizadores e data loading contra thresholds versionados. Estado Phase 15/R-1502: `std.tensor.memory_report()` e métricas `stats_lifetime_records`/`stats_reuse_rate_per_mille` expõem lifetimes, allocation sites, reuse e pressão de memória. Estado Phase 15/R-1503: `scripts/validate_r1503_correctness.py` gera artefatos portáteis de correção numérica para RNG, reductions, matmul, convolução e otimizadores com tolerância `1e-9` absoluta/relativa. Estado Phase 16/R-1603: `scripts/validate_r1603_gpu_backend.py` valida CPU fallback, WGPU opcional, diagnósticos de capability e kernels de elementwise/reductions/matmul/conv2d/autodiff.

Estado Phase 5: `std.tensor` inclui autodiff reverse-mode para tensores `float`, com `requires_grad`, `backward`, `grad`, `zero_grad`, modo inference/no-grad e liberação automática do graph após backward. Use reduções tensor-returning (`sum_t`, `mean_t`, `dot_t`) para criar losses diferenciáveis. R-3004 adiciona um grafo reverso compiler-visible (`spectralang.r3004_autodiff_ir.v1`) com seeds, valores salvos, regras versionadas, acumulação explícita e `AutodiffStep` executado por kernels reversos individuais. Blocos `diff` não usam mais o adapter interno; `tensor.backward` permanece disponível somente como API pública de compatibilidade.

---

## 7. std.ml — Machine Learning

**PT-BR:**  
`std.ml` fornece a camada de alto nível da Phase 6 para treinamento em CPU usando handles de `std.tensor`.

**EN-US:**  
`std.ml` provides the Phase 6 high-level CPU training layer on top of `std.tensor` handles.

```spectra
import std.tensor as tensor
import std.ml as ml
```

| Função / Function | Descrição / Description |
|---|---|
| `module_new()` | Creates a module handle |
| `module_add_parameter`, `module_parameter_count`, `module_parameter` | Parameter registration and discovery |
| `module_set_training`, `module_is_training` | Training/eval mode |
| `linear(input, weight, bias)` | Differentiable dense layer |
| `conv2d(input, kernel, bias, batch, in_ch, h, w, out_ch, kh, kw)` | Differentiable valid 2D convolution over flattened NCHW tensors |
| `dropout(input, p, training)` | Deterministic baseline dropout/inference helper |
| `max_pool2d(input, batch, channels, h, w, pool_h, pool_w)` | Max pooling over flattened NCHW tensors |
| `mse_loss`, `bce_loss`, `cross_entropy_loss`, `nll_loss` | Scalar tensor losses compatible with `tensor.backward` |
| `sgd_step`, `sgd_momentum_step`, `adam_step`, `adamw_step` | Optimizers that update tensor parameters in place |
| `exp_lr(base, gamma, step)` | Exponential learning-rate scheduling |
| `dataset_from_tensors`, `dataset_len` | Tensor-backed datasets |
| `dataset_from_csv`, `dataset_from_jsonl`, `dataset_from_npy`, `dataset_from_directory` | File-backed numerical datasets |
| `dataset_map_features`, `dataset_filter_label_min` | Materialized dataset transforms |
| `dataset_train_split`, `dataset_test_split` | Deterministic train/test dataset splits |
| `dataloader_new`, `dataloader_batch_count`, `dataloader_batch_features`, `dataloader_batch_labels` | Deterministic minibatching |
| `dataframe_from_csv`, `dataframe_rows`, `dataframe_cols`, `dataframe_column` | Numeric dataframe handles and column extraction |
| `experiment_start`, `experiment_finish` | Tracked experiment lifecycle |
| `experiment_set_config`, `experiment_log_metric`, `experiment_log_artifact` | Experiment config, metrics, and artifacts |
| `experiment_set_lockfile`, `experiment_set_model_output` | Reproducibility lockfile and model output records |
| `experiment_manifest_path`, `experiment_repro_command`, `experiment_compare_manifests` | Manifest path, reproduction command, and manifest comparison |
| `distributed_session_start`, `distributed_worker_step`, `distributed_global_step` | Single-machine simulated distributed training coordination |
| `distributed_checkpoint_save`, `distributed_resume`, `distributed_summary`, `distributed_worker_step_count` | Checkpoint/resume and worker progress inspection |
| `onnx_export`, `onnx_import_summary`, `onnx_validate`, `onnx_roundtrip` | Binary ONNX subset export/import/round-trip for supported AI model blocks (`onnx_export` is a deprecated deterministic fixture template, not a model export — the `deprecated-onnx-export` lint steers to `onnx_export_weights`; `onnx_export_weights(path, kind, weights)` fills the kind's initializers from live float-tensor handles in spec order, e.g. `linear` takes `[weight[2,3], bias[3]]`) |
| `embedding_lookup`, `positional_encoding`, `layer_norm`, `gelu`, `swiglu`, `attention` | Transformer tensor primitives |
| `kv_cache_new`, `kv_cache_append`, `kv_cache_keys`, `kv_cache_values`, `kv_cache_len`, `logits_sample`, `logits_sample_seeded` | LLM KV-cache and logits sampling helpers (`logits_sample` uses the global RNG; `logits_sample_seeded(seed, logits, temperature)` samples the same full-vocabulary distribution from a per-call splitmix64 stream, so a fixed seed reproduces the token) |
| `tokenizer_wordpiece`, `tokenizer_encode`, `tokenizer_decode`, `text_embed` | Deterministic tokenization utilities and `text_embed`, a deprecated deterministic hashing baseline (not a model-backed embedding; use `text_embed_model` / `text_embed_model_session`) |
| `vector_index_new`, `vector_index_insert`, `vector_index_query`, `vector_index_persist`, `vector_index_load`, `vector_index_set_metadata`, `vector_index_metrics` | Deterministic HNSW vector index APIs backed by the R-3003 Artifact Container v1; legacy JSON is rejected |
| `rag_chunk_text`, `rag_build_prompt`, `rag_evaluate_answer` | RAG chunking, prompt assembly, and token-overlap evaluation (`rag_evaluate_answer` returns token-overlap F1 scaled to permille `0..1000`, the same `answer_overlap_score` semantics as `ml.metrics_generation`; it is not a model-graded judge) |

Exemplos completos estão em:

- `tests/validation/72_ml_phase6_mlp_training.spectra`
- `tests/validation/73_ml_phase6_cnn_training.spectra`

Estado Phase 6: MLP e CNN pequenos treinam end-to-end nos testes de runtime, com exemplos Spectra compilando e executando pela API pública. Readers CSV/imagem/JSONL, serialização de modelos e prefetch paralelo são trabalho futuro.

---

## 8. std.random — Números Aleatórios / Random Numbers

```spectra
import std.random
```

#### `random_seed(seed: int) -> unit`

**PT-BR:** Define a semente do gerador de números aleatórios. Use para resultados reproduzíveis.  
**EN-US:** Sets the random number generator seed. Use for reproducible results.

```spectra
std.random.random_seed(42)
```

#### `random_int(min: int, max: int) -> int`

**PT-BR:** Retorna um inteiro aleatório em `[min, max]` (inclusivo).  
**EN-US:** Returns a random integer in `[min, max]` (inclusive).

```spectra
let dado = std.random.random_int(1, 6)
    // 1 a 6
let moeda = std.random.random_int(0, 1)
   // 0 ou 1
```

#### `random_float() -> float`

**PT-BR:** Retorna um float aleatório em `[0.0, 1.0)`.  
**EN-US:** Returns a random float in `[0.0, 1.0)`.

```spectra
let f = std.random.random_float()
    // ex: 0.7351...
```

#### `random_bool() -> bool`

```spectra
let b = std.random.random_bool()
    // true ou false
```

---

## 8. std.fs — Sistema de Arquivos / File System

```spectra
import std.fs as fs
import std.error as error
```

As operações públicas de `std.fs` retornam `Result<T, Error>`. O caminho de
sucesso fica no variant `Ok`; falhas de validação ou do sistema operacional
ficam no variant `Err`, sem usar string vazia ou `false` como erro. A superfície
está em migração beta até que os gates multiplataforma e de release certifiquem
todo o contrato.

#### `fs_read(path: string) -> Result<string, Error>`

**PT-BR:** Lê o conteúdo completo de um arquivo. Falhas são retornadas em `Err`.  
**EN-US:** Reads the full content of a file. Failures are returned as `Err`.

```spectra
let resultado = fs.fs_read("dados.txt")
if let Result::Ok(conteudo) = resultado {
    println(conteudo)
} else {
    println("não foi possível ler o arquivo")
}
```

#### `fs_write(path: string, content: string) -> Result<bool, Error>`

**PT-BR:** Escreve (substitui) o conteúdo de um arquivo. Cria diretórios pais ausentes quando possível.
**EN-US:** Writes (replaces) file content and creates missing parent directories when possible.

```spectra
let resultado = fs.fs_write("target/artefatos/saida.txt", "Hello, World!\n")
if let Result::Err(falha) = resultado {
    println(error.message(falha))
}
```

#### `fs_append(path: string, content: string) -> Result<bool, Error>`

**PT-BR:** Adiciona conteúdo ao final de um arquivo. Falhas são retornadas em `Err`.
**EN-US:** Appends content to the end of a file. Failures are returned as `Err`.

```spectra
let resultado = fs.fs_append("log.txt", "Nova entrada de log\n")
```

Falhas comuns, como caminho vazio, pai bloqueado por arquivo, permissão negada
ou arquivo inexistente em leitura/remoção, não derrubam o processo nativo. Use
`std.error` para inspecionar o código e o contexto da falha.

#### `fs_exists(path: string) -> Result<bool, Error>`

```spectra
let existe = fs.fs_exists("config.txt")
if let Result::Ok(true) = existe {
    let cfg = fs.fs_read("config.txt")
}
```

`fs_exists` retorna `Ok(false)` quando o caminho não existe; erros de metadata
que não sejam ausência retornam `Err`.

#### `fs_remove(path: string) -> Result<bool, Error>`

```spectra
let removido = fs.fs_remove("temp.txt")
```


#### `create_dir_all(path: string) -> Result<bool, Error>`

Cria o diretório e todos os pais que faltarem. Retorna `Ok(true)` em sucesso.

```spectra
let criado = fs.create_dir_all("target/artefatos/2026/agosto")
```

#### `remove_dir(path: string) -> Result<bool, Error>`

Remove um diretório **vazio**. Diretórios com conteúdo produzem `Err`
(`ErrorCode::Io`); use `fs_remove` para arquivos individuais.

```spectra
let removido = fs.remove_dir("target/artefatos/2026/agosto")
```

#### `rename(from: string, to: string) -> Result<bool, Error>`

Move (ou renomeia) arquivos e diretórios.

```spectra
let movido = fs.rename("rascunho.txt", "final.txt")
```

#### `copy(from: string, to: string) -> Result<int, Error>`

Copia o arquivo e retorna `Ok(bytes_copiados)`.

```spectra
let bytes = fs.copy("entrada.bin", "backup.bin")
```

#### `read_dir(path: string) -> Result<List<string>, Error>`

Lista os **nomes** das entradas do diretório (não caminhos completos), em
ordem determinística (ordenação lexicográfica dos nomes).

```spectra
let listagem = fs.read_dir("target/artefatos")
if let Result::Ok(nomes) = listagem {
    for nome in nomes {
        io.println(nome)
    }
}
```

---

## 8.1. std.error — Erros estruturados / Structured Errors

```spectra
import std.error as error
from std.error import ErrorCode
```

`Error` é o payload estruturado usado pelo caminho `Err` de `std.fs`. O
`ErrorCode` é um enum fechado: `InvalidArgument = 0`, `NotFound = 1`,
`PermissionDenied = 2`, `Io = 3`, `Internal = 4` e `Unsupported = 5`.

```spectra
let falha = error.new(
    ErrorCode::NotFound,
    "arquivo não encontrado",
    "fs_read",
    "dados.txt",
    "meu_modulo",
    false
)
let codigo = error.code(falha)
let operacao = error.operation(falha)
```

Além de `code`, o módulo expõe `message`, `operation`, `context`, `origin` e
`retryable`. A representação é runtime-owned; programas devem transportar o
valor e tratá-lo como dado de erro, sem depender do layout interno.

#### `std.compat.fs` — adapter legado

Programas antigos podem importar explicitamente `std.compat.fs` para preservar
as assinaturas históricas: `fs_read` retorna string vazia em falha e as outras
operações retornam `false`. Esse namespace é somente compatibilidade e não deve
ser usado como autoridade por código novo.

```spectra
import std.compat.fs as legacy_fs
let content = legacy_fs.fs_read("dados.txt")
let ok = legacy_fs.fs_write("saida.txt", content)
```

---

## 9. std.env — Ambiente / Environment

```spectra
import std.env
import std.option
```

#### `env_get(key: string) -> Option<string>` (beta)

**PT-BR:** Obtém uma variável de ambiente sem confundir ausência com string vazia.
**EN-US:** Gets an environment variable without confusing absence with an empty string.

```spectra
let maybe_home = std.env.env_get("HOME")
let maybe_path = std.env.env_get("PATH")
```

The explicit `env_get_option(key: string) -> Option<string>` alias remains
available. The public `std.env` contract never uses an empty string as an
absence sentinel; legacy behavior is isolated under `std.compat.env`.

#### `env_set(key: string, value: string) -> bool`

```spectra
let ok = std.env.env_set("MINHA_VAR", "valor")
```

#### `env_args_count() -> int`

**PT-BR:** Retorna o número de argumentos da linha de comando.  
**EN-US:** Returns the number of command-line arguments.

```spectra
let argc = std.env.env_args_count()
println(f"Argumentos: {argc}")
```

#### `env_arg(index: int) -> Option<string>`

**PT-BR:** Retorna o argumento na posição `index`, ou `None` se fora dos limites.  
**EN-US:** Returns the argument at position `index`, or `None` when out of bounds.

```spectra
let arg0 = std.env.env_arg(0)
    // nome do programa / program name
let arg1 = std.env.env_arg(1)
    // primeiro argumento / first argument

// Processando todos os argumentos / Processing all arguments
let n = std.env.env_args_count()
for i in 0..n {
    let arg = std.env.env_arg(i)
    if std.option.is_some(arg) {
        println(f"arg[{i}] = {std.option.option_unwrap(arg)}")
    }
}
```

#### `std.compat.env` — compatibilidade legada / legacy compatibility

```spectra
import std.compat.env

let legacy_value = std.compat.env.env_get("OPTIONAL_VALUE")
let legacy_arg = std.compat.env.env_arg(999999)
```

`std.compat.env.env_get` and `env_arg` return `""` when the value is absent.
Use this namespace only while migrating older programs; new code should use
the `Option<string>` functions from `std.env`.

---

## 10. std.option — Operações em Option / Option Operations

```spectra
import std.option
```

#### `is_some(opt: unknown) -> bool`

```spectra
let opt = Option::Some(42)
let tem = std.option.is_some(opt)
    // true
```

#### `is_none(opt: unknown) -> bool`

```spectra
let nada = Option::None
let vazio = std.option.is_none(nada)
    // true
```

#### `option_unwrap<T>(opt: Option<T>) -> T`

**PT-BR:** Extrai o valor de `Some`. Retorna erro de runtime controlado se for `None`.
**EN-US:** Extracts the value from `Some`. Returns a controlled runtime error if `None`.

```spectra
let val = std.option.option_unwrap(Option::Some(42))
   // 42
// std.option.option_unwrap(Option::None);  // erro de runtime controlado
```

#### `option_unwrap_or<T>(opt: Option<T>, default: T) -> T`

**PT-BR:** Extrai o valor ou retorna o padrão se `None`.  
**EN-US:** Extracts the value or returns the default if `None`.

```spectra
let val = std.option.option_unwrap_or(Option::Some(42), 0)
   // 42
let def = std.option.option_unwrap_or(Option::None, 99)
      // 99
```

#### `option_map<T, U>(opt: Option<T>, map: func(T) returns U) -> Option<U>` (beta)

Aplica a função apenas em `Some` e preserva `None`, sem usar valor sentinela
para representar a ausência.

```spectra
let mapped = std.option.option_map(Option::Some(4), |value: int| value + 1)
// mapped contém Some(5)
```

---

## 11. std.result — Operações em Result / Result Operations

```spectra
import std.result
```

#### `is_ok(res: unknown) -> bool`

```spectra
let r = Result::Ok(100)
let ok = std.result.is_ok(r)
      // true
```

#### `is_err(res: unknown) -> bool`

```spectra
let e = Result::Err("falha")
let err = std.result.is_err(e)
    // true
```

#### `result_unwrap<T, E>(res: Result<T, E>) -> T`

**PT-BR:** Extrai o valor de `Ok`. Retorna erro de runtime controlado se for `Err`.
**EN-US:** Extracts the value from `Ok`. Returns a controlled runtime error if `Err`.

```spectra
let val = std.result.result_unwrap(Result::Ok(42))
    // 42
```

#### `result_unwrap_or<T, E>(res: Result<T, E>, default: T) -> T`

```spectra
let val = std.result.result_unwrap_or(Result::Err("e"), 0)
   // 0
```

#### `result_unwrap_err<T, E>(res: Result<T, E>) -> E`

**PT-BR:** Extrai o valor de `Err`. Retorna erro de runtime controlado se for `Ok`.
**EN-US:** Extracts the value from `Err`. Returns a controlled runtime error if `Ok`.

```spectra
let msg = std.result.result_unwrap_err(Result::Err("algo errado"))
   // "algo errado"
```

#### `result_map<T, E, U>` e `result_map_err<T, E, F>` (beta)

`result_map` transforma somente `Ok`; `result_map_err` transforma somente
`Err`. A variante oposta é preservada com seu payload.

```spectra
let ok = std.result.result_map(Result::Ok(10), |value: int| value * 2)
let err = std.result.result_map_err(
    Result::Err("bad"),
    |message: string| message + " input"
)
```

---

## 12. std.char — Operações em Caracteres / Character Operations

**PT-BR:**  
As funções de `std.char` operam sobre **códigos Unicode** (inteiros), o mesmo formato retornado por `std.string.char_at()`.

**EN-US:**  
Functions in `std.char` operate on **Unicode code points** (integers), the same format returned by `std.string.char_at()`.

```spectra
import std.char
```

#### `is_alpha(c: int) -> bool`

```spectra
let sim = std.char.is_alpha(65)
     // true ('A')
let nao = std.char.is_alpha(48)
     // false ('0')
```

#### `is_digit_char(c: int) -> bool`

```spectra
let sim = std.char.is_digit_char(48)
   // true ('0')
let nao = std.char.is_digit_char(65)
   // false ('A')
```

#### `is_whitespace_char(c: int) -> bool`

```spectra
let sim = std.char.is_whitespace_char(32)
   // true (espaço / space)
let sim2 = std.char.is_whitespace_char(9)
   // true (tab)
```

#### `is_upper_char(c: int) -> bool` / `is_lower_char(c: int) -> bool`

```spectra
let upper = std.char.is_upper_char(65)
   // true ('A')
let lower = std.char.is_lower_char(97)
   // true ('a')
```

#### `is_alphanumeric(c: int) -> bool`

```spectra
let sim = std.char.is_alphanumeric(97)
   // true ('a')
let sim2 = std.char.is_alphanumeric(48)
  // true ('0')
let nao = std.char.is_alphanumeric(32)
   // false (espaço)
```

#### `to_upper_char(c: int) -> int` / `to_lower_char(c: int) -> int`

```spectra
let A = std.char.to_upper_char(97)
    // 65 ('A')
let a = std.char.to_lower_char(65)
    // 97 ('a')
```

### Exemplo: Processamento de String Caractere a Caractere

```spectra
module analisar_string

import std.string as s
import std.char as c
from std.io import println
import std.convert

func contar_digitos(texto: string) returns int {
    let count = 0
    let i = 0
    let len = s.len(texto)
    while i < len {
        let codigo = s.char_at(texto, i)
        if c.is_digit_char(codigo) {
            count = count + 1
        }
        i = i + 1
    }
    return count
}

public func main() {
    let texto = "abc123def456"
    let n = contar_digitos(texto)
    println(f"Dígitos em '{texto}': {n}")
    // 6
}
```

---

## 13. std.time — Tempo / Time

```spectra
import std.time
```

#### `time_now_millis() -> int`

**PT-BR:** Retorna os milissegundos desde a época Unix. Retorna `-1` em caso de erro.  
**EN-US:** Returns milliseconds since the Unix epoch. Returns `-1` on error.

```spectra
let inicio = std.time.time_now_millis()
// ... operação / operation ...
let fim = std.time.time_now_millis()
let duracao = fim - inicio
println(f"Duração: {duracao}ms")
```

#### `time_now_secs() -> int`

**PT-BR:** Retorna os segundos desde a época Unix. Retorna `-1` em caso de erro.  
**EN-US:** Returns seconds since the Unix epoch. Returns `-1` on error.

```spectra
let agora = std.time.time_now_secs()
println(f"Timestamp: {agora}")
```

#### `sleep_ms(ms: int) -> unit`

**PT-BR:** Pausa a execução por `ms` milissegundos.  
**EN-US:** Pauses execution for `ms` milliseconds.

```spectra
println("Aguardando...")
std.time.sleep_ms(1000)
    // Pausa 1 segundo / Pause 1 second
println("Pronto!")
```

### Tipos / Types

`std.time` tambem expoe handles opacos de tempo gerenciados pelo runtime:

- `Duration`: intervalo nao negativo.
- `Instant`: ponto monotonicamente medido pelo runtime.
- `UtcDateTime`: data/hora UTC decomposta.

### Clock monotônico / Monotonic clock

#### `monotonic_millis() -> int`

**PT-BR:** Retorna milissegundos desde a inicializacao do runtime usando clock monotônico.
**EN-US:** Returns milliseconds since runtime start using a monotonic clock.

#### `monotonic_nanos() -> int`

**PT-BR:** Retorna nanossegundos desde a inicializacao do runtime usando clock monotônico.
**EN-US:** Returns nanoseconds since runtime start using a monotonic clock.

### Duração / Duration

#### `duration_ms(ms: int) -> Duration`

Cria uma duração em milissegundos. Valores negativos falham em runtime.

#### `duration_secs(secs: int) -> Duration`

Cria uma duração em segundos. Valores negativos falham em runtime.

#### `duration_millis(duration: Duration) -> int`

Retorna a duração em milissegundos.

#### `duration_secs_value(duration: Duration) -> int`

Retorna a duração em segundos inteiros. O nome evita overload com `duration_secs(secs)`.

#### `duration_add(lhs: Duration, rhs: Duration) -> Duration`

Soma durações com checagem de overflow.

#### `duration_sub(lhs: Duration, rhs: Duration) -> Duration`

Subtrai durações. Falha se `rhs` for maior que `lhs`.

#### `sleep(duration: Duration) -> unit`

Pausa usando `Duration`. Sleeps excessivamente longos são rejeitados pelo runtime.

### Instantes / Instants

#### `instant_now() -> Instant`

Captura o instante monotônico atual.

#### `instant_elapsed_ms(instant: Instant) -> int`

Retorna os milissegundos decorridos desde `instant`.

#### `instant_add(instant: Instant, duration: Duration) -> Instant`

Retorna um novo instante deslocado por `duration`.

#### `instant_has_elapsed(instant: Instant) -> bool`

Retorna `true` quando o instante já chegou.

### UTC

#### `unix_to_utc(secs: int) -> UtcDateTime`

Converte segundos Unix para data/hora UTC usando calendario civil deterministico no runtime.

#### `utc_year(dt)`, `utc_month(dt)`, `utc_day(dt)`, `utc_hour(dt)`, `utc_minute(dt)`, `utc_second(dt)`

Extraem campos de `UtcDateTime`.

### Exemplo: Benchmark Simples

```spectra
module benchmark

import std.time
from std.io import println

func operacao_pesada(n: int) returns int {
    let soma = 0
    for i in 0..n {
        soma = soma + i
    }
    return soma
}

public func main() {
    let inicio = std.time.time_now_millis()
    let resultado = operacao_pesada(1000000)
    let fim = std.time.time_now_millis()

    println(f"Resultado: {resultado}")
    println(f"Tempo: {fim - inicio}ms")
}
```

---

## 14. std.ml — AI/ML runtime

```spectra
import std.ml as ml
```

### Evaluation metrics

`std.ml` exposes production evaluation helpers for model gates:

- `metrics_classification(labels: int, predictions: int) -> string`
- `metrics_regression(expected: int, predicted: int) -> string`
- `metrics_ranking(relevance: int, scores: int, top_k: int) -> string`
- `metrics_generation(output: string, reference: string) -> string`
- `serving_metrics(latencies_ms: int, requests: int, errors: int) -> string`
- `evaluation_report(path: string, name: string, classification: string, regression: string, ranking: string, generation: string, serving: string) -> string`

The metric functions return deterministic JSON payloads. `evaluation_report`
writes a versioned machine-readable JSON report and a human-readable `.txt`
companion report.

```spectra
import std.ml as ml
import std.tensor as tensor

let labels = tensor.arange(0, 4, 1)
let predicted = tensor.arange(0, 4, 1)
let classification = ml.metrics_classification(labels, predicted)
```

---

## 15. std.serve — Serving and guardrails

```spectra
import std.serve as serve
```

`std.serve` provides local serving queues plus guardrails for AI serving baselines:

- `server_set_input_policy(server: int, min: int, max: int) -> bool`
- `server_set_output_policy(server: int, min: int, max: int) -> bool`
- `server_set_rate_limit(server: int, limit: int) -> bool`
- `server_set_fallback(server: int, value: int) -> bool`
- `server_last_diagnostic(server: int) -> string`
- `server_audit_log(server: int) -> string`

Guardrail failures complete the request with the configured fallback value and
record structured diagnostic/audit JSON.

```spectra
import std.serve as serve

let server = serve.server_new(3)
serve.server_set_fallback(server, -999)
serve.server_set_input_policy(server, 0, 100)
serve.server_set_output_policy(server, 0, 200)
serve.server_warmup(server)
```

### Monitoring and drift

- `server_set_model_version(server: int, version: string) -> bool`
- `server_monitoring_snapshot(server: int) -> string`
- `server_distribution_summary(server: int) -> string`
- `drift_check(reference: string, live: string, threshold_per_mille: int) -> string`
- `export_monitoring(server: int, path: string, distribution: string, drift: string, audit: string) -> string`

These APIs emit versioned JSON for request metrics, latency/error/throughput,
input/output distribution summaries, drift checks, and observability export.

---

> **Próximo / Next:** [06 — Referência Rápida / Quick Reference](06-referencia-rapida.md)  
> **Anterior / Previous:** [04 — Avançado / Advanced](04-avancado.md)
