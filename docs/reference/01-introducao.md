# SpectraLang — Introdução / Introduction

> **Nível / Level:** Iniciante / Beginner  
> **Parte / Part:** 1 de 6

---

## Sumário / Table of Contents

1. [O que é SpectraLang? / What is SpectraLang?](#1-o-que-é-spectralang--what-is-spectralang)
2. [Filosofia da Linguagem / Language Philosophy](#2-filosofia-da-linguagem--language-philosophy)
3. [Pipeline de Compilação / Compilation Pipeline](#3-pipeline-de-compilação--compilation-pipeline)
4. [Instalação e Configuração / Installation & Setup](#4-instalação-e-configuração--installation--setup)
5. [Interface de Linha de Comando / Command-Line Interface](#5-interface-de-linha-de-comando--command-line-interface)
6. [Estrutura de um Arquivo Fonte / Source File Structure](#6-estrutura-de-um-arquivo-fonte--source-file-structure)
7. [Primeiro Programa / First Program](#7-primeiro-programa--first-program)
8. [Convenções e Boas Práticas / Conventions & Best Practices](#8-convenções-e-boas-práticas--conventions--best-practices)

---

## 1. O que é SpectraLang? / What is SpectraLang?

**PT-BR:**  
SpectraLang é uma linguagem de programação compilada de propósito geral, projetada para ser expressiva, segura e de alto desempenho. Ela combina a clareza sintática de linguagens modernas com um sistema de tipos estático e rigoroso, geração de código nativo via Cranelift e um modelo de memória híbrido (gerenciado + manual). SpectraLang é compilada para código de máquina nativo tanto via JIT (Just-In-Time) quanto AOT (Ahead-Of-Time).

**EN-US:**  
SpectraLang is a general-purpose compiled programming language designed to be expressive, safe, and high-performance. It combines the syntactic clarity of modern languages with a strict static type system, native code generation via Cranelift, and a hybrid memory model (managed + manual). SpectraLang compiles to native machine code through both JIT (Just-In-Time) and AOT (Ahead-Of-Time) compilation.

---

## 2. Filosofia da Linguagem / Language Philosophy

**PT-BR:**  
SpectraLang é guiada por três princípios centrais:

1. **Clareza antes de concisão** — O código deve ser legível por humanos em primeiro lugar. Construções ambíguas são rejeitadas em favor de expressões explícitas.
2. **Segurança de tipos rigorosa** — Conversões implícitas são proibidas, exceto o alargamento numérico de `int` para `float`. O compilador rejeita código que mistura tipos incompatíveis.
3. **Desempenho sem cerimônia** — O programador não precisa gerenciar memória manualmente na maioria dos casos; o runtime cuida disso com um coletor de lixo de rastreamento, mas oferece alocação manual para casos de baixo nível.

**EN-US:**  
SpectraLang is guided by three core principles:

1. **Clarity over conciseness** — Code must be human-readable first. Ambiguous constructs are rejected in favor of explicit expressions.
2. **Strict type safety** — Implicit conversions are forbidden except for numeric widening from `int` to `float`. The compiler rejects code that mixes incompatible types.
3. **Performance without ceremony** — Programmers don't need to manually manage memory in most cases; the runtime handles this with a tracing garbage collector, but manual allocation is available for low-level use cases.

---

## 3. Pipeline de Compilação / Compilation Pipeline

**PT-BR:**  
O código fonte SpectraLang passa pelas seguintes etapas antes de ser executado:

**EN-US:**  
SpectraLang source code goes through the following stages before execution:

```
Código Fonte (.spectra)
Source Code (.spectra)
        │
        ▼
┌─────────────────┐
│     Lexer       │  Tokenização / Tokenization
│  (token.rs)     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│    Parser       │  AST Generation
│  (parser/)      │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Semantic       │  Type checking, symbol resolution
│  Analysis       │
│  (semantic/)    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│    Midend       │  IR Lowering + Optimization passes
│  (midend/)      │  (constant folding, dead code elimination)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│    Backend      │  Cranelift code generation
│  (backend/)     │  JIT (in-memory) or AOT (object files)
└────────┬────────┘
         │
         ▼
   Código Nativo / Native Code
```

**PT-BR:**  
O compilador gera código de máquina nativo via [Cranelift](https://cranelift.dev/), um backend de geração de código de alto desempenho. O modo JIT compila e executa o programa em memória diretamente. O modo AOT gera arquivos objeto (`.o`/`.obj`) que podem ser linkados com a biblioteca de runtime para produzir executáveis.

**EN-US:**  
The compiler generates native machine code via [Cranelift](https://cranelift.dev/), a high-performance code generation backend. JIT mode compiles and executes the program in memory directly. AOT mode generates object files (`.o`/`.obj`) that can be linked with the runtime library to produce executables.

---

## 4. Instalação e Configuração / Installation & Setup

**PT-BR:**  
SpectraLang é construída com Rust. Para compilar a toolchain a partir do código-fonte:

**EN-US:**  
SpectraLang is built with Rust. To build the toolchain from source:

```bash
# Clonar o repositório / Clone the repository
git clone <repositório>
cd SpectraLang

# Compilar toda a toolchain / Build the entire toolchain
cargo build --release

# Verificar a instalação / Verify the installation
./target/release/spectralang --help
```

**PT-BR:**  
Requisitos / Requirements:

- **Rust** 1.75+ (com `cargo`)
- **Plataformas suportadas / Supported platforms:** Windows, Linux, macOS (x86_64, aarch64)

---

## 5. Interface de Linha de Comando / Command-Line Interface

**PT-BR:**  
O CLI do SpectraLang (`spectralang`) oferece os seguintes comandos e opções:

**EN-US:**  
The SpectraLang CLI (`spectralang`) provides the following commands and options:

### Comandos / Commands

| Comando / Command | Descrição PT-BR | Description EN-US |
|---|---|---|
| `spectralang run <arquivo>` | Compila e executa via JIT | Compile and execute via JIT |
| `spectralang compile <arquivo>` | Compila via JIT sem executar | Compile via JIT without executing |
| `spectralang check <arquivo>` | Verifica tipos sem compilar | Type-check only, no code generation |
| `spectralang lint <arquivo>` | Executa verificações de lint | Run lint checks |
| `spectralang fmt <arquivo>` | Formata o código-fonte | Format source code |
| `spectralang repl` | Inicia o REPL interativo | Start the interactive REPL |
| `spectralang new <nome>` | Cria um novo projeto | Scaffold a new project |

### Flags

| Flag | Descrição PT-BR | Description EN-US |
|---|---|---|
| `--run` / `-r` | Executa após compilar (JIT) | Execute after compilation (JIT) |
| `--emit-object <saída>` | Gera arquivo objeto AOT | Generate AOT object file |
| `--no-optimize` / `-O0` | Desativa todas as otimizações | Disable all optimizations |
| `-O1` | Constant folding ativado | Constant folding enabled |
| `-O2` | Constant folding + eliminação de código morto **(padrão)** | Constant folding + dead code elimination **(default)** |
| `-O3` | Todas as otimizações agressivas | All aggressive optimizations |
| `--dump-ast` | Exibe a árvore AST para debug | Print the AST for debugging |
| `--dump-ir` | Exibe o IR para debug | Print the IR for debugging |
| `--timings` / `-T` | Coleta métricas de compilação | Collect compilation metrics |
| `--summary` | Exibe sumário do pipeline por módulo | Show per-module pipeline summaries |
| `--verbose` / `-v` | Detalhes adicionais do build | Print additional build details |
| `--lint` | Ativa verificações de lint | Enable lint checks |
| `--allow <rule>` | Permite (suprime) uma regra de lint | Allow (suppress) a lint rule |
| `--deny <rule>` | Eleva uma regra de lint a erro | Escalate a lint rule to error |

### Exemplos de Uso / Usage Examples

```bash
# Executar um programa / Run a program
spectralang run hello.spectra

# Verificar tipos apenas / Type-check only
spectralang check meu_programa.spectra

# Compilar com otimização nível 2 / Compile with level-2 optimization
spectralang compile -O2 algoritmos.spectra

# Gerar arquivo objeto AOT / Generate AOT object file
spectralang compile --emit-object meu_programa.o programa.spectra

# Formatar o código / Format the code
spectralang fmt meu_arquivo.spectra

# Lint com saída JSON / Lint with JSON output
spectralang lint --json meu_arquivo.spectra

# Iniciar REPL / Start REPL
spectralang repl
```

### Códigos de Saída / Exit Codes

| Código / Code | Significado PT-BR | Meaning EN-US |
|---|---|---|
| `0` | Sucesso | Success |
| `64` | Erro de uso (argumentos inválidos) | Usage error (invalid arguments) |
| `65` | Erro de compilação | Compilation error |
| `74` | Erro de I/O | I/O error |

---

## 6. Estrutura de um Arquivo Fonte / Source File Structure

**PT-BR:**  
Todo arquivo SpectraLang deve seguir esta estrutura obrigatória:

**EN-US:**  
Every SpectraLang source file must follow this mandatory structure:

```spectra
// 1. Declaração de módulo (OBRIGATÓRIA / REQUIRED)
module nome.do.modulo

// 2. Importações (opcionais / optional)
import std.io
import std.math as math
from std.io import println, print

// 3. Declarações de nível superior (top-level items)
//    - Funções (fn)
//    - Structs (struct)
//    - Enums (enum)
//    - Implementações (impl)
//    - Traits (trait)

public func main() returns int {
    println("Hello, World!")
    return 0
}
```

**PT-BR:**  
**Regras obrigatórias:**
- A declaração `module` deve ser **a primeira linha** do arquivo (antes de qualquer código, após comentários opcionais).
- O nome do módulo usa **pontos** como separador de caminhos: `module fisica.vetor` (por convenção espelha a hierarquia de pastas, mas não é obrigatório).
- Importações participam da resolução semântica, e o CLI consegue compilar projetos multi-arquivo e conjuntos explícitos de arquivos.
- `import std.io` expõe símbolos da stdlib como `println` diretamente, e chamadas qualificadas como `std.io.println(...)` também são aceitas.
- Alias de importação, named imports e re-exportações públicas fazem parte da superfície atual da linguagem.

**EN-US:**  
**Mandatory rules:**
- The `module` declaration must be the **first line** of the file (before any code, after optional comments).
- Module names use **dots** as path separators: `module physics.vector` (by convention it mirrors the folder hierarchy, but this is not enforced).
- Imports participate in semantic resolution, and the CLI can compile multi-file projects and explicit file sets.
- `import std.io` makes stdlib symbols such as `println` available directly, and qualified calls like `std.io.println(...)` are also accepted.
- Alias imports, named imports, and public re-exports are part of the current language surface.

### Módulos e Caminhos / Modules and Paths

```spectra
// Nomes válidos de módulo / Valid module names
module app
module app.utils
module app.controllers.user
module std.collections

// Identificadores válidos / Valid identifiers
// [A-Za-z_][A-Za-z0-9_]*
// Exemplos: minhaFuncao, meu_struct, _privado, Tipo1
```

---

## 7. Primeiro Programa / First Program

**PT-BR:**  
Vamos escrever o programa clássico "Hello, World!" em SpectraLang:

**EN-US:**  
Let's write the classic "Hello, World!" program in SpectraLang:

```spectra
module hello

import std.io

public func main() returns int {
    println("Hello, World!")
    return 0
}
```

**PT-BR:**  
Também podemos importar `println` diretamente para não precisar qualificar o nome:

**EN-US:**  
We can also import `println` directly to avoid qualifying the name:

```spectra
module hello

from std.io import println

public func main() returns int {
    println("Hello, World!")
    return 0
}
```

**PT-BR:**  
Para executar:

**EN-US:**  
To run:

```bash
spectralang run hello.spectra
# Saída / Output: Hello, World!
```

### Um Programa Mais Completo / A More Complete Program

```spectra
module saudacao

from std.io import println, print
import std.convert

public func main() returns int {
    let nome = "Maria"
    let idade = 25

    // F-string para interpolação / F-string for interpolation
    println(f"Olá, {nome}! Você tem {idade} anos.")

    // Chamada de função / Function call
    let saudacao = criar_saudacao(nome, idade)
    println(saudacao)
    return 0
}

func criar_saudacao(nome: string, idade: int) returns string {
    if idade < 18 {
        return f"Olá, {nome}! Bem-vindo(a), jovem!"
    } else {
        return f"Olá, {nome}! Bem-vindo(a)!"
    }
}
```

```bash
spectralang run saudacao.spectra
# Saída / Output:
# Olá, Maria! Você tem 25 anos.
# Olá, Maria! Bem-vindo(a)!
```

---

## 8. Convenções e Boas Práticas / Conventions & Best Practices

### Nomenclatura / Naming Conventions

**PT-BR:**

| Construto / Construct | Convenção / Convention | Exemplo / Example |
|---|---|---|
| Variáveis / Variables | `snake_case` | `minha_variavel`, `contador` |
| Funções / Functions | `snake_case` | `calcular_area`, `obter_nome` |
| Structs | `PascalCase` | `Ponto`, `RetanguloColorido` |
| Enums | `PascalCase` | `Cor`, `EstadoConexao` |
| Variantes de Enum / Enum Variants | `PascalCase` | `Vermelho`, `Conectado` |
| Módulos / Modules | `snake_case` com pontos | `app.controladores` |
| Constantes / Constants | `SCREAMING_SNAKE_CASE` | `MAX_SIZE`, `PI` |
| Parâmetros de Tipo / Type Params | `PascalCase` curto | `T`, `E`, `Chave`, `Valor` |

**EN-US:**

| Construct | Convention | Example |
|---|---|---|
| Variables | `snake_case` | `my_variable`, `counter` |
| Functions | `snake_case` | `calculate_area`, `get_name` |
| Structs | `PascalCase` | `Point`, `ColoredRectangle` |
| Enums | `PascalCase` | `Color`, `ConnectionState` |
| Enum Variants | `PascalCase` | `Red`, `Connected` |
| Modules | `snake_case` with dots | `app.controllers` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_SIZE`, `PI` |
| Type Parameters | Short `PascalCase` | `T`, `E`, `Key`, `Value` |

### Organização de Arquivos / File Organization

```
projeto/                   # Raiz do projeto / Project root
├── spectra.toml           # Manifesto do projeto / Project manifest
├── src/
│   ├── main.spectra       # module main / public func main() returns int { ... }
│   ├── utils.spectra      # module utils
│   └── modelos/
│       ├── usuario.spectra  # module modelos.usuario
│       └── produto.spectra  # module modelos.produto
└── tests/
    └── testes.spectra     # module tests
```

### Formatação / Formatting

**PT-BR:**  
Use `spectralang fmt` para formatar automaticamente. As regras padrão são:

**EN-US:**  
Use `spectralang fmt` to format automatically. The default rules are:

- **Indentação / Indentation:** 4 espaços / 4 spaces
- **Comprimento máximo de linha / Max line length:** 100 caracteres / characters
- **Espaçamento em operadores / Operator spacing:** `a + b` (não `a+b`)
- **Chaves / Braces:** Abre na mesma linha / Opens on same line: `func foo() {`

---

> **Próximo / Next:** [02 — Fundamentos / Fundamentals](02-fundamentos.md)
