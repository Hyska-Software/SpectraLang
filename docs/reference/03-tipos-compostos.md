# SpectraLang — Tipos Compostos / Composite Types

> **Nível / Level:** Intermediário / Intermediate  
> **Parte / Part:** 3 de 6

---

## Sumário / Table of Contents

1. [Arrays](#1-arrays)
2. [Tuplas / Tuples](#2-tuplas--tuples)
3. [Intervalos / Ranges](#3-intervalos--ranges)
4. [Structs](#4-structs)
5. [Enums](#5-enums)
6. [Blocos impl — Métodos / impl Blocks — Methods](#6-blocos-impl--métodos--impl-blocks--methods)
7. [Traits](#7-traits)
8. [Genéricos / Generics](#8-genéricos--generics)
9. [Closures / Lambdas](#9-closures--lambdas)

---

## 1. Arrays

### Declaração e Literais / Declaration and Literals

**PT-BR:**  
Arrays são coleções de elementos do mesmo tipo. São criados via literais `[elem1, elem2, ...]`. O tipo é inferido a partir dos elementos.

**EN-US:**  
Arrays are collections of elements of the same type. They are created via `[elem1, elem2, ...]` literals. The type is inferred from the elements.

```spectra
module arrays

from std.io import println

public func main() {
    // Arrays de inteiros / Integer arrays
    let numeros = [1, 2, 3, 4, 5]
    let primos = [2, 3, 5, 7, 11, 13]

    // Arrays de strings / String arrays
    let nomes = ["Alice", "Bob", "Carol"]

    // Arrays de floats / Float arrays
    let temperaturas = [36.5, 37.0, 38.2]

    // Arrays de booleanos / Boolean arrays
    let flags = [true, false, true, true]
}
```

### Indexação / Indexing

**PT-BR:**  
Arrays são indexados com `array[indice]`, começando em `0`.

**EN-US:**  
Arrays are indexed with `array[index]`, starting at `0`.

```spectra
let arr = [10, 20, 30, 40, 50]

let primeiro = arr[0]
    // 10
let segundo  = arr[1]
    // 20
let ultimo   = arr[4]
    // 50
```

### Modificação / Modification

**PT-BR:**  
Elementos de array podem ser modificados com atribuição indexada.

**EN-US:**  
Array elements can be modified with indexed assignment.

```spectra
let arr = [1, 2, 3, 4, 5]
arr[0] = 99
    // [99, 2, 3, 4, 5]
arr[4] = 100
   // [99, 2, 3, 4, 100]
```

### Iteração / Iteration

```spectra
let arr = [10, 20, 30, 40, 50]

// Com for e range / With for and range
for i in 0..5 {
    println(f"arr[{i}] = {arr[i]}")
}

// Com while / With while
let i = 0
while i < 5 {
    println(f"{arr[i]}")
    i = i + 1
}

// Somando todos os elementos / Summing all elements
let soma = 0
for i in 0..5 {
    soma = soma + arr[i]
}
// soma == 150
```

### Arrays como Parâmetros / Arrays as Parameters

```spectra
func soma_array(arr: [int], tamanho: int) returns int {
    let total = 0
    for i in 0..tamanho {
        total = total + arr[i]
    }
    return total
}

public func main() {
    let meu_arr = [1, 2, 3, 4, 5]
    let s = soma_array(meu_arr, 5)
    // 15
}
```

### Arrays Multidimensionais / Multidimensional Arrays

```spectra
// Array de arrays (matriz) / Array of arrays (matrix)
let matriz = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]

let elem = matriz[1][2]
    // 6 (linha 1, coluna 2)
```

---

## 2. Tuplas / Tuples

**PT-BR:**  
Tuplas são coleções de tamanho fixo com elementos de tipos possivelmente diferentes. São criadas com parênteses e acessadas via `.0`, `.1`, `.2`, etc.

**EN-US:**  
Tuples are fixed-size collections with elements of potentially different types. They are created with parentheses and accessed via `.0`, `.1`, `.2`, etc.

```spectra
module tuplas

from std.io import println

public func main() {
    // Tupla de int e string / Tuple of int and string
    let par = (42, "resposta")
    let n = par.0
    // 42
    let s = par.1
    // "resposta"

    // Tripla / Triple
    let tripla = (10, 3.14, true)
    let inteiro  = tripla.0
    // 10
    let flutuante = tripla.1
   // 3.14
    let booleano = tripla.2
    // true

    // Tupla com anotação de tipo / Tuple with type annotation
    let coordenada: (int, int) = (100, 200)
    let x = coordenada.0
    // 100
    let y = coordenada.1
    // 200
}
```

### Funções que Retornam Tuplas / Functions Returning Tuples

**PT-BR:**  
Tuplas são úteis para retornar múltiplos valores de uma função.

**EN-US:**  
Tuples are useful for returning multiple values from a function.

```spectra
func min_max(arr: [int], n: int) returns (int, int) {
    let min = arr[0]
    let max = arr[0]
    for i in 1..n {
        if arr[i] < min {
            min = arr[i]
        }
        if arr[i] > max {
            max = arr[i]
        }
    }
    return (min, max)
}

public func main() {
    let nums = [5, 1, 8, 3, 9, 2]
    let resultado = min_max(nums, 6)
    
    println(f"Min: {resultado.0}, Max: {resultado.1}")
    // Min: 1, Max: 9
}
```

### Desestruturação de Tuplas / Tuple Destructuring

```spectra
// Acesso direto via campo / Direct access via field
let ponto = (10, 20)
let x = ponto.0
let y = ponto.1

// Desestruturação em let / Destructuring in let
let (a, b) = ponto

// Usando em match / Using in match
match ponto_resultado {
    when (0, 0) then println("Origem"),
    when (x, 0) then println(f"Eixo X em {x}"),
    when (0, y) then println(f"Eixo Y em {y}"),
    when (x, y) then println(f"Ponto ({x}, {y})")
}
```

Structs também podem ser desestruturadas em `let`:

```spectra
record Point {
    public x: int,
    public y: int,
}

let p = Point { x: 10, y: 20 }
let Point { x, y: renamed_y } = p
```

---

## 3. Intervalos / Ranges

**PT-BR:**  
Intervalos (ranges) representam sequências de valores numéricos. Existem dois tipos: exclusivo (`..`) e inclusivo (`..=`).

**EN-US:**  
Ranges represent sequences of numeric values. There are two types: exclusive (`..`) and inclusive (`..=`). A stored range has type `Range` and is backed by a runtime handle, so it can be passed to functions and iterated later without losing its bounds.

```spectra
// Exclusivo: não inclui o valor final / Exclusive: does not include the final value
let r1 = 0..10
     // 0, 1, 2, ..., 9

// Inclusivo: inclui o valor final / Inclusive: includes the final value
let r2 = 1..=10
    // 1, 2, 3, ..., 10

func soma_intervalo(r: Range) returns int {
    let total = 0
    for i in r {
        total = total + i
    }
    return total
}

let stored = 2..5
let total = soma_intervalo(stored)
 // 2 + 3 + 4 = 9

// Em for loops / In for loops
for i in 0..5 {
    // i = 0, 1, 2, 3, 4
}

for i in 1..=5 {
    // i = 1, 2, 3, 4, 5
}

// Com variáveis / With variables
let inicio = 5
let fim = 10
for i in inicio..fim {
    // i = 5, 6, 7, 8, 9
}

// Empty descending range / intervalo descendente vazio
let empty = 5..2
for i in empty {
    // no iterations
}
```

`std.range` exposes handle inspection helpers:

| Function | Signature | Meaning |
|---|---|---|
| `create` | `func(int, int, bool) returns Range` | Build a range handle |
| `len` | `func(Range) returns int` | Count produced values |
| `at` | `func(Range, int) returns int` | Read value by zero-based index |
| `eq` | `func(Range, Range) returns bool` | Compare range bounds/inclusive flag |
| `start` | `func(Range) returns int` | Original start bound |
| `end` | `func(Range) returns int` | Original end bound |
| `is_inclusive` | `func(Range) returns bool` | True for `..=` |

Invalid range handles, negative indexes, indexes outside `len`, invalid
`create` flags, and length overflow fail in the runtime with
`HOST_STATUS_INVALID_ARGUMENT`.

---

## 4. Records

### Declaração / Declaration

**PT-BR:**  
Records são tipos de dados com campos nomeados e tipados. São declarados no nível superior do módulo.

**EN-US:**  
Records are data types with named, typed fields. They are declared at the module top-level.

```spectra
module structs

// Record simples / Simple record
record Ponto {
    x: int,
    y: int
}

// Record com vários tipos / Record with multiple types
record Pessoa {
    nome: string,
    idade: int,
    altura: float,
    ativo: bool
}

// Record aninhado / Nested record
record Retangulo {
    canto_superior_esquerdo: Ponto,
    largura: int,
    altura: int
}
```

### Instanciação / Instantiation

```spectra
public func main() {
    // Instanciação de record / Record instantiation
    let p = Ponto { x: 10, y: 20 }
    let x = 5
    let y = 8
    let p2 = Ponto { x, y }
 // shorthand: x == x: x, y == y: y
    let pessoa = Pessoa {
        nome: "Alice",
        idade: 30,
        altura: 1.65,
        ativo: true
    }

    // Record aninhado / Nested record
    let rect = Retangulo {
        canto_superior_esquerdo: Ponto { x: 0, y: 0 },
        largura: 100,
        altura: 50
    }
}
```

### Acesso a Campos / Field Access

```spectra
let p = Ponto { x: 10, y: 20 }

let coord_x = p.x
    // 10
let coord_y = p.y
    // 20

// Acesso aninhado / Nested access
let rect = Retangulo {
    canto_superior_esquerdo: Ponto { x: 5, y: 10 },
    largura: 200,
    altura: 100
}

let x_do_canto = rect.canto_superior_esquerdo.x
    // 5
let area = rect.largura * rect.altura
              // 20000
```

### Structs Genéricas / Generic Structs

**PT-BR:**  
Structs podem ter parâmetros de tipo genéricos, tornando-as reutilizáveis para diferentes tipos de dados.

**EN-US:**  
Structs can have generic type parameters, making them reusable for different data types.

```spectra
record Par<T> {
    primeiro: T,
    segundo: T
}

record Mapa<K, V> {
    chave: K,
    valor: V
}

public func main() {
    let par_int = Par { primeiro: 1, segundo: 2 }
    let par_str = Par { primeiro: "a", segundo: "b" }

    let mapa = Mapa { chave: "nome", valor: "Alice" }
}
```

---

## 5. Enums

### Variantes Simples / Unit Variants

**PT-BR:**  
A forma mais simples de enum tem apenas variantes sem dados associados (variantes unitárias).

**EN-US:**  
The simplest form of enum has only variants with no associated data (unit variants).

```spectra
module enums

enum Cor {
    Vermelho,
    Verde,
    Azul
}

enum DiaSemana {
    Segunda,
    Terca,
    Quarta,
    Quinta,
    Sexta,
    Sabado,
    Domingo
}

public func main() {
    let cor = Cor::Vermelho
    let dia = DiaSemana::Sexta
}
```

### Variantes com Dados Tuple / Tuple Variant Enums

**PT-BR:**  
Variantes podem carregar dados anônimos (estilo tupla).

**EN-US:**  
Variants can carry anonymous data (tuple-style).

```spectra
enum Opcao<T> {
    Algum(T),
    Nenhum
}

enum Resultado<T, E> {
    Ok(T),
    Err(E)
}

enum Mensagem {
    Sair,
    Mover(int, int),          // x, y
    EscreverTexto(string),
    MudarCor(int, int, int)   // r, g, b
}

public func main() {
    let algum = Opcao::Algum(42)
    let nenhum: Opcao<int> = Opcao::Nenhum

    let ok = Resultado::Ok(100)
    let erro = Resultado::Err("algo deu errado")

    let msg = Mensagem::Mover(10, 20)
    let texto = Mensagem::EscreverTexto("Olá!")
}
```

### Variantes com Campos Nomeados / Struct-Style Variants

**PT-BR:**  
Variantes também podem ter campos nomeados, como um struct embutido.

**EN-US:**  
Variants can also have named fields, like an embedded struct.

```spectra
enum Forma {
    Circulo { raio: float },
    Retangulo { largura: float, altura: float },
    Triangulo { base: float, altura: float },
    Ponto                         // Variante unitária misturada / mixed unit variant
}

enum Cor {
    Rgb { r: int, g: int, b: int },
    Hsv { h: float, s: float, v: float },
    Nomeada(string)               // Variante tuple misturada / mixed tuple variant
}

public func main() {
    let circulo = Forma::Circulo { raio: 5.0 }
    let rect = Forma::Retangulo { largura: 4.0, altura: 6.0 }
    let vermelho = Cor::Rgb { r: 255, g: 0, b: 0 }
    let azul_ceu = Cor::Nomeada("azul céu")
}
```

### Usando Enums com Match / Using Enums with Match

```spectra
func calcular_area(forma: Forma) returns float {
    match forma {
        when Forma::Circulo { raio } then raio * raio * 3.14159,
        when Forma::Retangulo { largura, altura } then largura * altura,
        when Forma::Triangulo { base, altura } then base * altura * 0.5,
        when Forma::Ponto then 0.0
    }
}

func descrever_cor(cor: Cor) returns string {
    match cor {
        when Cor::Rgb { r, g, b } then f"RGB({r}, {g}, {b})",
        when Cor::Hsv { h, s, v } then f"HSV({h}, {s}, {v})",
        when Cor::Nomeada(nome) then nome
    }
}
```

### Enums Genéricos / Generic Enums

```spectra
// Tipos built-in / Built-in types
enum Option<T> {
    Some(T),
    None
}

enum Result<T, E> {
    Ok(T),
    Err(E)
}

// Usando / Using
let talvez: Option<int> = Option::Some(42)
let nada: Option<string> = Option::None
let sucesso: Result<int, string> = Result::Ok(200)
let falha: Result<int, string> = Result::Err("não encontrado")
```

---

## 6. Blocos impl — Métodos / impl Blocks — Methods

**PT-BR:**  
Métodos são funções associadas a um tipo, declaradas em blocos `impl`. Um método pode ser um **método de instância** (recebe `self`) ou um **método estático** (não recebe `self`, funciona como construtor ou utilitário).

**EN-US:**  
Methods are functions associated with a type, declared in `impl` blocks. A method can be an **instance method** (receives `self`) or a **static method** (does not receive `self`, works as a constructor or utility).

### Receptores / Receivers

| Receptor / Receiver | Semântica PT-BR | Semantics EN-US |
|---|---|---|
| `self` | Consome o valor (move) | Consumes the value (move) |
| `&self` | Referência imutável | Immutable reference |
| `&mut self` | Referência mutável | Mutable reference |

### Exemplo Completo / Complete Example

```spectra
module metodos

from std.io import println

record Ponto {
    x: int,
    y: int
}

impl Ponto {
    // Método estático (construtor) / Static method (constructor)
    func novo(x: int, y: int) returns Ponto {
        Ponto { x: x, y: y }
    }

    // Método de leitura / Read method
    func obter_x(&self) returns int {
        self.x
    }

    func obter_y(&self) returns int {
        self.y
    }

    // Método de cálculo / Calculation method
    func distancia_da_origem(&self) returns float {
        let soma_quadrados = (self.x * self.x + self.y * self.y)
        // simplificado — sem sqrt / simplified — without sqrt
        soma_quadrados
    }

    // Método que retorna novo valor / Method returning new value
    func mover(&self, dx: int, dy: int) returns Ponto {
        Ponto { x: self.x + dx, y: self.y + dy }
    }

    // Método com mut self / Method with mut self
    func escalar(&mut self, fator: int) {
        self.x = self.x * fator
        self.y = self.y * fator
    }

    // Método para exibição / Display method
    func para_string(&self) returns string {
        f"({self.x}, {self.y})"
    }
}

public func main() {
    // Construtor estático / Static constructor
    let p1 = Ponto::novo(10, 20)

    // Métodos de instância / Instance methods
    let x = p1.obter_x()
    // 10
    let y = p1.obter_y()
    // 20

    // Method chaining (encadeamento) / Method chaining
    let p2 = p1.mover(5, 5).mover(3, 3)
    // p2 = (18, 28)

    println(p1.para_string())
    // (10, 20)
    println(p2.para_string())
    // (18, 28)
}
```

### Métodos em Enums / Methods on Enums

```spectra
enum Forma {
    Circulo { raio: float },
    Retangulo { largura: float, altura: float }
}

impl Forma {
    func area(&self) returns float {
        match self {
            when Forma::Circulo { raio } then raio * raio * 3.14159,
            when Forma::Retangulo { largura, altura } then largura * altura
        }
    }

    func perimetro(&self) returns float {
        match self {
            when Forma::Circulo { raio } then 2.0 * 3.14159 * raio,
            when Forma::Retangulo { largura, altura } then 2.0 * (largura + altura)
        }
    }
}

public func main() {
    let c = Forma::Circulo { raio: 5.0 }
    let r = Forma::Retangulo { largura: 4.0, altura: 6.0 }

    println(f"Área do círculo: {c.area()}")
        // ~78.54
    println(f"Área do retângulo: {r.area()}")
      // 24.0
}
```

### Métodos em Structs Aninhadas / Methods on Nested Structs

```spectra
record Retangulo {
    canto: Ponto,
    largura: int,
    altura: int
}

impl Retangulo {
    func novo(x: int, y: int, l: int, a: int) returns Retangulo {
        Retangulo {
            canto: Ponto::novo(x, y),
            largura: l,
            altura: a
        }
    }

    func area(&self) returns int {
        self.largura * self.altura
    }

    func canto_x(&self) returns int {
        self.canto.obter_x()    // Chama m�todo do Ponto aninhado
    }
}
```

### Impl Gen�rico / Generic Impl Blocks (R-211)

**PT-BR:**
Um bloco `impl` pode ser gen�rico escrevendo os par�metros de tipo do record
ap�s o nome: `impl Par<T>`. Os m�todos valem para todas as instancia��es e o
compilador especializa o corpo por instancia��o concreta (despacho est�tico).

**EN-US:**
An `impl` block can be generic by writing the record's type parameters after
the name: `impl Par<T>`. The methods apply to every instantiation and the
compiler specializes the body per concrete instantiation (static dispatch).

```spectra
record Par<T> {
    valor: T
}

impl Par<T> {
    func obter(&self) returns T {
        self.valor
    }

    func definir(&mut self, v: T) {
        self.valor = v
    }
}

public func main() returns int {
    let p = Par<int> { valor: 40 }
    p.definir(42)
    p.obter() // 42
}
```

O alvo do impl tamb�m pode ser qualificado por m�dulo:
`impl modulo::Tipo { ... }` (R-211).

---

## 7. Traits

### Declaração / Declaration

**PT-BR:**  
Traits definem contratos de comportamento que tipos podem implementar. Um trait declara assinaturas de métodos e pode fornecer implementações padrão.

**EN-US:**  
Traits define behavioral contracts that types can implement. A trait declares method signatures and can provide default implementations.

```spectra
module traits

// Trait simples / Simple trait
trait Exibivel {
    func exibir(&self) returns string
}

// Trait com implementação padrão / Trait with default implementation
trait Saudavel {
    func saudar(&self) returns string

    func saudar_alto(&self) returns string {
        // Implementação padrão usa o método abstrato / Default impl uses abstract method
        let s = self.saudar()
        return f"OLÁ! {s}"
    }
}

// Herança de trait / Trait inheritance
trait Animado: Exibivel {
    func mover(&self) returns string
    // Também precisa implementar Exibivel / Also needs to implement Exibivel
}
```

### Implementação de Traits / Trait Implementation

```spectra
record Pessoa {
    nome: string,
    idade: int
}

// Implementação do trait / Trait implementation
impl Exibivel for Pessoa {
    func exibir(&self) returns string {
        f"{self.nome} (idade: {self.idade})"
    }
}

impl Saudavel for Pessoa {
    func saudar(&self) returns string {
        f"Olá, eu sou {self.nome}!"
    }
    // saudar_alto() já tem implementação padrão — não precisa repetir
    // saudar_alto() has default implementation — no need to repeat
}

public func main() {
    let p = Pessoa { nome: "Alice", idade: 30 }

    let repr = p.exibir()
        // "Alice (idade: 30)"
    let s1 = p.saudar()
          // "Olá, eu sou Alice!"
    let s2 = p.saudar_alto()
     // "OLÁ! Olá, eu sou Alice!"
}
```

### Traits como Bounds em Genéricos / Traits as Generic Bounds

```spectra
// Parâmetro genérico T deve implementar Exibivel / Generic T must implement Exibivel
func imprimir_todos<T: Exibivel>(items: [T], n: int) {
    for i in 0..n {
        println(items[i].exibir())
    }
}

// Múltiplos bounds / Multiple bounds
func processar<T: Exibivel + Saudavel>(item: T) returns string {
    return f"{item.saudar()} — {item.exibir()}"
}
```

---

## 8. Genéricos / Generics

**PT-BR:**  
Genéricos permitem escrever código que funciona com múltiplos tipos sem duplicação. SpectraLang suporta parâmetros de tipo em funções, structs, enums e traits.

**EN-US:**  
Generics allow writing code that works with multiple types without duplication. SpectraLang supports type parameters in functions, structs, enums, and traits.

### Funções Genéricas / Generic Functions

```spectra
// Parâmetro de tipo simples / Simple type parameter
func primeiro<T>(arr: [T], n: int) returns T {
    return arr[0]
}

// Múltiplos parâmetros / Multiple parameters
func trocar<T>(a: T, b: T) returns (T, T) {
    return (b, a)
}

public func main() {
    let arr_int = [1, 2, 3]
    let arr_str = ["a", "b", "c"]

    let p1 = primeiro(arr_int, 3)
   // 1
    let p2 = primeiro(arr_str, 3)
   // "a"

    let (x, y) = trocar(10, 20)
     // x=20, y=10
}
```

### Structs Genéricas / Generic Structs

```spectra
record Pilha<T> {
    dados: [T],
    tamanho: int
}

impl Pilha<T> {
    func nova() returns Pilha<T> {
        Pilha { dados: [], tamanho: 0 }
    }

    func topo(&self) returns T {
        self.dados[self.tamanho - 1]
    }
}
```

### Enums Genéricos / Generic Enums

```spectra
// Option e Result são built-in mas poderiam ser definidos assim:
// Option and Result are built-in but could be defined like this:

enum Opcao<T> {
    Algum(T),
    Nenhum
}

enum Resultado<V, E> {
    Exito(V),
    Falha(E)
}
```

### Trait Bounds / Trait Bounds

```spectra
trait Comparavel {
    func comparar(&self, outro: &Self) returns int
  // -1, 0, ou 1
}

// Função que requer T implementar Comparavel / Function requiring T to implement Comparavel
func ordenar<T: Comparavel>(arr: [T], n: int) {
    // ... lógica de ordenação / sorting logic
}

// Múltiplos bounds com + / Multiple bounds with +
func exibir_e_comparar<T: Exibivel + Comparavel>(a: T, b: T) {
    println(a.exibir())
    let resultado = a.comparar(b)
}
```

---

## 9. Closures / Lambdas

**PT-BR:**  
Closures são funções anônimas que podem ser armazenadas em variáveis ou passadas como argumentos. São definidas com a sintaxe `|parâmetros| corpo`.

**EN-US:**  
Closures are anonymous functions that can be stored in variables or passed as arguments. They are defined with the `|parameters| body` syntax.

### Sintaxe Básica / Basic Syntax

```spectra
module closures

from std.io import println

public func main() {
    // Closure de um parâmetro / Single-parameter closure
    let dobrar = |x: int| x * 2
    let resultado = dobrar(5)
    // 10

    // Closure de múltiplos parâmetros / Multi-parameter closure
    let somar = |a: int, b: int| a + b
    let s = somar(3, 4)
          // 7

    // Closure sem parâmetros / Zero-parameter closure
    let quarenta_e_dois = || 42
    let n = quarenta_e_dois()
    // 42

    // Closure com bloco / Closure with block
    let valor_absoluto = |x: int| {
        if x < 0 {
            return x * -1
        }
        return x
    }

    println(f"{valor_absoluto(-5)}")
    // 5
    println(f"{valor_absoluto(7)}")
     // 7
}
```

### Closures como Argumentos / Closures as Arguments

**PT-BR:**  
Uma função que aceita uma closure usa o tipo `func(T) returns R` como parâmetro.

**EN-US:**  
A function that accepts a closure uses the `func(T) returns R` type as a parameter.

```spectra
func aplicar(x: int, f: func(int) returns int) returns int {
    return f(x)
}

func aplicar_duas_vezes(x: int, f: func(int) returns int) returns int {
    return f(f(x))
}

func mapear(arr: [int], n: int, f: func(int) returns int) returns [int] {
    let resultado = [0, 0, 0, 0, 0]
  // pré-alocado
    for i in 0..n {
        resultado[i] = f(arr[i])
    }
    return resultado
}

public func main() {
    // Passando closure diretamente / Passing closure directly
    let r1 = aplicar(5, |x: int| x * 3)
          // 15
    let r2 = aplicar_duas_vezes(2, |x: int| x + 1)
  // 4

    // Passando função nomeada / Passing named function
    let dobro = |x: int| x * 2
    let r3 = aplicar(10, dobro)
    // 20

    // Usando em operação de mapa / Using in map operation
    let nums = [1, 2, 3, 4, 5]
    let quadrados = mapear(nums, 5, |x: int| x * x)
    // quadrados = [1, 4, 9, 16, 25]
}
```

### Tipo de Closure / Closure Type

```spectra
// O tipo func(int) returns int descreve uma closure de int para int
// The type func(int) returns int describes a closure from int to int

// Closures como variáveis tipadas / Closures as typed variables
let transformador: func(int) returns int = |x: int| x + 10

// Funções que retornam closures / Functions that return closures
func escolher(incrementar: bool) returns func(int) returns int {
    if incrementar {
        return |x: int| x + 1
    }
    return |x: int| x - 1
}

public func main() {
    let transformar = escolher(true)
    let resultado = transformar(7)
    // 8
}
```

Field shorthand is supported when a local variable has the same name as the
struct field. `Ponto { x, y }` is equivalent to `Ponto { x: x, y: y }`.

Closures capturam variáveis externas por valor no momento em que a closure é criada.
Closures capture external variables by value when the closure is created.

```spectra
func make_adder(delta: int) returns func(int) returns int {
    return |x: int| x + delta
}
```

Mutar diretamente uma variável capturada dentro da closure não é permitido nesta versão; passe um valor novo explicitamente ou retorne o novo estado.
Directly mutating a captured variable inside the closure is not allowed in this version; pass a new value explicitly or return the new state.

### Closures em Expressões / Closures in Expressions

```spectra
// Closure diretamente em expressão / Closure directly in expression
let val = aplicar(4, |n: int| n * n) + 1
   // 17

// Closures em condições / Closures in conditions
let verificar = |x: int| x > 0 && x < 100
if verificar(42) {
    println("Válido!")
}
```

---

> **Próximo / Next:** [04 — Avançado / Advanced](04-avancado.md)  
> **Anterior / Previous:** [02 — Fundamentos / Fundamentals](02-fundamentos.md)
