# SpectraLang — Referência Rápida / Quick Reference

> **Nível / Level:** Todos / All  
> **Parte / Part:** 6 de 6  
> Este arquivo é um **cartão de consulta rápida** — ele não é didático. Para aprendizado, veja os arquivos 01–05.  
> This file is a **quick reference card** — it is not didactic. For learning, see files 01–05.

---

## Sumário / Table of Contents

1. [Palavras-Chave / Keywords](#1-palavras-chave--keywords)
2. [Operadores e Precedência / Operators & Precedence](#2-operadores-e-precedência--operators--precedence)
3. [Tipos / Types](#3-tipos--types)
4. [Sequências de Escape / Escape Sequences](#4-sequências-de-escape--escape-sequences)
5. [Visibilidade / Visibility](#5-visibilidade--visibility)
6. [Módulos — Formas de Importação / Import Forms](#6-módulos--formas-de-importação--import-forms)
7. [Estruturas de Controle — Resumo / Control Flow Summary](#7-estruturas-de-controle--resumo--control-flow-summary)
8. [Padrões em match / Patterns in match](#8-padrões-em-match--patterns-in-match)
9. [Erros Comuns / Common Errors](#9-erros-comuns--common-errors)
10. [Convenções de Nomenclatura / Naming Conventions](#10-convenções-de-nomenclatura--naming-conventions)
11. [Gramática Informal / Informal Grammar](#11-gramática-informal--informal-grammar)
12. [Módulos da Stdlib — Resumo / Stdlib Summary](#12-módulos-da-stdlib--resumo--stdlib-summary)
13. [Flags do CLI / CLI Flags](#13-flags-do-cli--cli-flags)

---

## 1. Palavras-Chave / Keywords

**39 palavras-chave reservadas / 39 reserved keywords**

| Palavra-chave | Status | Uso / Use |
|---------------|--------|-----------|
| `module` | ✅ Implementado | Declara o módulo do arquivo |
| `import` | ✅ Implementado | Importa módulos/símbolos |
| `export` | 🚧 Reservado | (futuro / future) |
| `public` | ✅ Implementado | Visibilidade pública |
| `internal` | ✅ Implementado | Visibilidade interna ao módulo |
| `func` | ✅ Implementado | Declara função |
| `returns` | ✅ Implementado | Tipo de retorno |
| `record` | ✅ Implementado | Declara record |
| `enum` | ✅ Implementado | Declara enum |
| `impl` | ✅ Implementado | Bloco de implementação |
| `trait` | ✅ Implementado | Declara trait |
| `class` | 🚧 Reservado | (futuro / future) |
| `let` | ✅ Implementado | Declara variável |
| `mut` | ✅ Implementado | Mutabilidade |
| `Self` | ✅ Implementado | Tipo do impl atual |
| `self` | ✅ Implementado | Receptor do método |
| `if` | ✅ Implementado | Condicional |
| `else if` | ✅ Implementado | Senão-se |
| `else` | ✅ Implementado | Senão |
| `not` | ✅ Implementado | Negação lógica |
| `while` | ✅ Implementado | Laço condicional |
| `do` | Estável | `do { ... } while ...` ativado por padrão |
| `for` | ✅ Implementado | Laço `for in` |
| `foreach` | 🚧 Reservado | (futuro / future) |
| `in` | ✅ Implementado | Em `for x in` |
| `loop` | Estável | Ativado por padrão |
| `repeat` | 🚧 Reservado | (futuro / future) |
| `until` | 🚧 Reservado | (futuro / future) |
| `match` | ✅ Implementado | Pattern matching |
| `switch` | Estável | Ativado por padrão |
| `case` | ✅ Implementado | Braço de switch |
| `cond` | 🚧 Reservado | (futuro / future) |
| `return` | ✅ Implementado | Retorno explícito |
| `break` | ✅ Implementado | Sai do laço |
| `continue` | ✅ Implementado | Próxima iteração |
| `yield` | 🚧 Reservado | (futuro / future) |
| `goto` | 🚧 Reservado | (futuro / future) |
| `true` | ✅ Implementado | Literal booleano |
| `false` | ✅ Implementado | Literal booleano |

---

## 2. Operadores e Precedência / Operators & Precedence

**Do mais alto ao mais baixo / From highest to lowest:**

| Nível | Operador(es) | Tipo / Type | Associatividade |
|-------|-------------|-------------|-----------------|
| 1 (alto) | `(expr)`, `f()`, `x.y`, `x[i]` | Primário | Esq. / Left |
| 2 | `-x` (unário), `!x` | Unário | Direita / Right |
| 3 | `*`, `/`, `%` | Multiplicativo | Esq. / Left |
| 4 | `+`, `-` | Aditivo | Esq. / Left |
| 5 | `<`, `>`, `<=`, `>=` | Relacional | Esq. / Left |
| 6 | `==`, `!=` | Igualdade | Esq. / Left |
| 7 | `&&` | E lógico / Logical AND | Esq. / Left |
| 8 (baixo) | `\|\|` | Ou lógico / Logical OR | Esq. / Left |

**Operadores de atribuição / Assignment operators:**

| Operador | Equivalente a |
|----------|--------------|
| `=` | atribuição / assignment |
| `+=` | `x = x + rhs` |
| `-=` | `x = x - rhs` |
| `*=` | `x = x * rhs` |
| `/=` | `x = x / rhs` |
| `%=` | `x = x % rhs` |

**Nota:** Operadores de atribuição **não** são expressões — não retornam valor.  
**Note:** Assignment operators are **not** expressions — they don't return a value.

---

## 3. Tipos / Types

### Primitivos / Primitives

| Tipo | Tamanho / Size | Valores / Values | Exemplo |
|------|----------------|------------------|---------|
| `int` | 64-bit signed | -2⁶³ … 2⁶³-1 | `42`, `-7`, `0` |
| `float` | 64-bit IEEE 754 | ±1.8×10³⁰⁸ | `3.14`, `-0.5` |
| `bool` | 1-bit | `true`, `false` | `true` |
| `string` | Ponteiro UTF-8 | qualquer texto | `"hello"` |
| `char` | Unicode codepoint | caractere único | `'a'`, `'\n'` |
| `unit` | 0 bits | `()` | implícito |

### Compostos / Composite

| Tipo | Sintaxe | Exemplo |
|------|---------|---------|
| Array | `[T; N]` | `[1, 2, 3]` |
| Array dinâmico | `[T]` | `let a: [int] = [1, 2, 3]` |
| Tupla | `(T1, T2)` | `(1, "hi")` |
| Record | `record Nome { ... }` | `Ponto { x: 0, y: 0 }` |
| Enum | `enum Nome { ... }` | `Cor::Azul` |
| Option | `Option<T>` | `Option::Some(42)` |
| Result | `Result<T, E>` | `Result::Ok(v)` |

### Compatibilidade / Compatibility

| De / From | Para / To | Como / How |
|-----------|-----------|------------|
| `int` | `float` | `std.convert.int_to_float(x)` |
| `float` | `int` | `std.convert.float_to_int(x)` (trunca / truncates) |
| `int` | `string` | `std.convert.int_to_string(x)` |
| `float` | `string` | `std.convert.float_to_string(x)` |
| `bool` | `string` | `std.convert.bool_to_string(x)` |
| `bool` | `int` | `std.convert.bool_to_int(x)` → 0 ou 1 |
| `string` | `int` | `std.convert.string_to_int(x)` (0 se erro) |
| `string` | `float` | `std.convert.string_to_float(x)` (0.0 se erro) |
| `int`↔`float` | operação mista | **ERRO** — deve converter explicitamente |
| `int`↔`bool` | aritmética | **ERRO** — bool não é numérico |

---

## 4. Sequências de Escape / Escape Sequences

| Escape | Significado | Unicode |
|--------|-------------|---------|
| `\n` | Nova linha / Newline | U+000A |
| `\t` | Tabulação / Tab | U+0009 |
| `\r` | Retorno de carro / Carriage return | U+000D |
| `\\` | Barra invertida / Backslash | U+005C |
| `\"` | Aspas duplas / Double quote | U+0022 |
| `\'` | Aspas simples / Single quote | U+0027 |
| `\0` | Nulo / Null | U+0000 |

**Aplicam-se em / Apply in:** `string`, `char`, `f-string`

---

## 5. Visibilidade / Visibility

| Modificador | Acessível de / Accessible from |
|-------------|-------------------------------|
| `public` | Qualquer código / Any code |
| `internal` | Mesmo pacote/projeto / Same package/project |
| *(sem/none)* | Apenas no módulo/escopo atual / Current module/scope only |

**Regras / Rules:**
- A forma documentada e recomendada do ponto de entrada é `public func main() returns int`.
- Records com campos `public` precisam declarar cada campo com `public`.
- Funções sem `public` ou `internal` são privadas ao bloco de declaração.

---

## 6. Módulos — Formas de Importação / Import Forms

```spectra
// Forma 1: importar módulo completo / Import whole module
import std.math
// Uso: std.math.sqrt_f(x)

// Forma 2: importar com alias / Import with alias
import std.math as m
// Uso: m.sqrt_f(x)

// Forma 3: importar símbolos nomeados / Import named symbols
from std.io import println, print
// Uso: println("hello")

// Forma 4: re-exportar publicamente / Public re-export
public from std.io import println

// Módulo do arquivo / File module declaration
module meu_modulo
```

---

## 7. Estruturas de Controle — Resumo / Control Flow Summary

```spectra
// if / else if / else
if condicao {
    // ...
} else if outra {
    // ...
} else {
    // ...
}

// if not
if not condicao {
    // ...
}

// while
while condicao {
    // ...
}

// do-while
do {
    // ...
} while condicao

// for com range / for with range
for i in 0..10 { /* 0 a 9 */ }
for i in 0..=10 { /* 0 a 10 */ }

// for com array / for with array
for item in arr { /* ... */ }

// loop infinito / infinite loop
loop {
    // ...
    break
    // necessário para sair / needed to exit
}

// switch (comparação por valor / value comparison)
switch valor {
    case 1: { println("um")
 }
    case 2: { println("dois")
 }
    else: { println("outro")
 }
}

// match (pattern matching)
match opcao {
    when Option::Some(v) then println(f"Tem: {v}")
    otherwise println("Vazio")
}

// if let
if let Option::Some(v) = possivel_valor {
    // ...
}

// Control: break / continue
for i in 0..10 {
    if i == 3 { continue
 }
    if i == 7 { break
 }
}
```

---

## 8. Padrões em match / Patterns in match

| Padrão | Sintaxe | Exemplo |
|--------|---------|---------|
| Literal inteiro | `42` | `when 42 then ...` |
| Literal string | `"hello"` | `when "ok" then ...` |
| Literal bool | `true` / `false` | `when true then ...` |
| Wildcard | `_` | `otherwise ...` |
| Binding | `nome` | `when x then println(x)` |
| Enum unit | `Enum::Var` | `when Cor::Azul then ...` |
| Enum tuple | `Enum::Var(a, b)` | `when Ponto::Par(x, y) then ...` |
| Enum struct | `Enum::Var { campo }` | `when Forma::Rect { w, h } then ...` |
| Multi-padrão | `pat1 \| pat2` | `when 1 \| 2 then ...` |
| Guard | `when pat if cond then ...` | Não implementado ainda |

**Regra de exhaustividade / Exhaustiveness rule:**  
Todo `match` deve cobrir **todos** os casos possíveis. Use `_` como wildcard.

---

## 9. Erros Comuns / Common Errors

| Erro | Causa | Solução |
|------|-------|---------|
| `type mismatch: int and float` | Operação mista sem conversão | Use `int_to_float()` ou `float_to_int()` |
| `non-exhaustive match` | Casos faltando no `match` | Adicione `otherwise` ou os casos faltantes |
| `return outside function` | `return` fora de função | Mova para dentro de uma função |
| `undefined variable` | Uso fora do escopo | Garanta que `let` está no escopo correto |
| `break/continue outside loop` | Fora de laço | Mova para dentro de `while`/`for`/`loop` |
| `module declaration missing` | Arquivo sem `module nome` | Adicione `module nome` no topo |
| `main not found` | Sem ponto de entrada válido | Adicione `public func main() returns int { return 0 }` |
| `cannot use bool in arithmetic` | `bool + int` | Use `bool_to_int()` explicitamente |
| `field not found` | Campo inexistente no record | Verifique o nome do campo |

---

## 10. Convenções de Nomenclatura / Naming Conventions

| Construto | Convenção | Exemplo |
|-----------|-----------|---------|
| Variáveis | `snake_case` | `meu_valor`, `nome_usuario` |
| Funções | `snake_case` | `calcular_area()` |
| Parâmetros | `snake_case` | `func f(valor_x: int)` |
| Records | `PascalCase` | `Ponto`, `UsuarioCadastrado` |
| Enums | `PascalCase` | `Cor`, `ResultadoOp` |
| Variantes de Enum | `PascalCase` | `Cor::VermelhoEscuro` |
| Traits | `PascalCase` | `Exibivel`, `Calculavel` |
| Módulos | `snake_case` | `module minha_lib` |
| Arquivos | `snake_case.spectra` | `minha_lib.spectra` |
| Constantes | `UPPER_SNAKE` (convenção, `let` não diferencia) | `let MAX_PONTOS = 100` |

---

## 11. Gramática Informal / Informal Grammar

```ebnf
programa      = declaracao_modulo? import* declaracao* ;

declaracao_modulo = "module" IDENT ;

import        = "import" caminho_modulo
              | "import" caminho_modulo "as" IDENT
              | "from" caminho_modulo "import" IDENT ("," IDENT)*
              | "public" "from" caminho_modulo "import" IDENT ("," IDENT)* ;

declaracao    = decl_fn | decl_struct | decl_enum | decl_impl | decl_trait | decl_trait_impl ;

decl_fn       = visib? "func" IDENT genericos? "(" params? ")" ("returns" tipo)? bloco ;

decl_struct   = visib? "record" IDENT genericos? "{" campo* "}" ;

decl_enum     = visib? "enum" IDENT genericos? "{" variante* "}" ;

variante      = IDENT                                    (* unit *)
              | IDENT "(" tipo ("," tipo)* ")"           (* tupla *)
              | IDENT "{" campo* "}"                     (* record *)
              ;

decl_impl     = "impl" genericos? IDENT ("{" metodo* "}")? ;

decl_trait    = visib? "trait" IDENT (":" IDENT)? "{" assinatura* "}" ;

bloco         = "{" stmt* expr? "}" ;

stmt          = decl_let | atribuicao | retorno | expr | laço | condicional | match_stmt ;

match_stmt    = "match" expr "{" braço* "}" ";"? ;

decl_let      = "let" "mut"? IDENT (":" tipo)? "=" expr ;

expr          = literal | IDENT | binario | unario | chamada | met_call | f_string
              | "if" expr bloco ("else" "if" expr bloco)* ("else" bloco)?
              | "if" "not" expr bloco ("else" bloco)?
              | "match" expr "{" braço* "}"
              | "|" params? "|" (expr | bloco)
              | "[" (expr ("," expr)*)? "]"
              | "(" expr ("," expr)+ ")"
              ;

tipo          = "int" | "float" | "bool" | "string" | "char"
              | IDENT                       (* tipo nomeado *)
              | IDENT "<" tipo ("," tipo)* ">"   (* genérico *)
              | "[" tipo "]"               (* array *)
              | "(" tipo ("," tipo)* ")"   (* tupla *)
              ;

visib         = "public" | "internal" ;

genericos     = "<" IDENT (":" IDENT ("+" IDENT)*)? ("," ...)* ">" ;
```

---

## 12. Módulos da Stdlib — Resumo / Stdlib Summary

| Módulo | Funções Principais / Key Functions |
|--------|-----------------------------------|
| `std.io` | `print`, `println`, `eprint`, `eprintln`, `read_line`, `input`, `flush` |
| `std.string` | `len`, `contains`, `to_upper`, `to_lower`, `trim`, `substring`, `replace`, `split_first`, `split_last`, `index_of`, `concat`, `is_empty`, `reverse_str`, `pad_left`, `pad_right` |
| `std.math` | `abs`, `min`, `max`, `clamp`, `sign`, `gcd`, `lcm`, `sqrt_f`, `pow_f`, `floor_f`, `ceil_f`, `round_f`, `sin_f`, `cos_f`, `tan_f`, `log_f`, `pi`, `e_const` |
| `std.convert` | `int_to_string`, `float_to_string`, `bool_to_string`, `string_to_int`, `string_to_float`, `int_to_float`, `float_to_int`, `string_to_int_or`, `bool_to_int` |
| `std.collections` | `list_new`, `list_push`, `list_len`, `list_get`/`list_pop`/`list_remove_at` (`Option<T>`), `list_set`, `list_sort`, `list_contains`, `list_index_of`, `list_free` |
| `std.compat.collections` | `list_get`, `list_pop`, `list_pop_front`, `list_remove_at`, `map_get`, `map_remove` com sentinelas legados |
| `std.random` | `random_seed`, `random_int`, `random_float`, `random_bool` |
| `std.fs` | `fs_read`, `fs_write`, `fs_append`, `fs_exists`, `fs_remove` retornando `Result<T, Error>` |
| `std.compat.fs` | Adaptadores legados de FS com sentinelas explícitos |
| `std.error` | `new`, `code`, `message`, `operation`, `context`, `origin`, `retryable` |
| `std.env` | `env_get`/`env_arg` (`Option<string>`), `env_set`, `env_args_count` |
| `std.compat.env` | `env_get`, `env_arg` com sentinela `""` legado |
| `std.option` | `is_some`, `is_none`, `option_unwrap`, `option_unwrap_or` |
| `std.result` | `is_ok`, `is_err`, `result_unwrap`, `result_unwrap_or`, `result_unwrap_err` |
| `std.char` | `is_alpha`, `is_digit_char`, `is_whitespace_char`, `is_alphanumeric`, `to_upper_char`, `to_lower_char` |
| `std.time` | `time_now_millis`, `time_now_secs`, `sleep_ms` |
| `std.range` | `create`, `len`, `at`, `eq`, `start`, `end`, `is_inclusive` |

---

## 13. Flags do CLI / CLI Flags

```
spectralang <comando> [flags] <arquivo>...
```

### Comandos / Commands

| Comando | Descrição PT-BR | Description EN-US |
|---------|-----------------|-------------------|
| `compile` | Compila módulos (padrão) | Compile modules (default) |
| `check` | Verifica tipos sem gerar código | Type-check only, no code generation |
| `run` | Compila e executa via JIT | Compile and execute via JIT |
| `lint` | Executa verificações de lint | Run lint checks |
| `fmt` | Formata arquivos fonte | Format source files |
| `repl` | Inicia o REPL interativo | Start the interactive REPL |
| `new` | Cria um novo projeto | Scaffold a new project |
| `help` | Exibe a mensagem de ajuda | Print help message |

### Flags de Compilação / Compilation Flags

| Flag | Descrição PT-BR | Description EN-US |
|------|-----------------|-------------------|
| `--run` / `-r` | Executa após compilar (JIT) | Execute after compilation (JIT) |
| `--emit-object <saída>` | Gera arquivo objeto AOT | Generate AOT object file |
| `--emit-exe <saída>` | Gera executável AOT | Generate AOT executable |
| `--no-optimize` / `-O0` | Desativa otimizações | Disable all optimizations |
| `-O1` | Otimizações básicas | Basic optimizations |
| `-O2` | Otimizações moderadas **(padrão)** | Moderate optimizations **(default)** |
| `-O3` | Todas as otimizações | All optimizations |
| `--dump-ast` | Exibe a AST para debug | Print the AST for debugging |
| `--dump-ir` | Exibe o IR para debug | Print the IR for debugging |
| `--timings` / `-T` | Coleta métricas de compilação | Collect compilation timings |
| `--summary` | Exibe sumário do pipeline | Show pipeline summaries |
| `--verbose` / `-v` | Detalhes adicionais do build | Print additional build details |

### Flags de Lint / Lint Flags

| Flag | Descrição PT-BR | Description EN-US |
|------|-----------------|-------------------|
| `--lint` | Ativa verificações de lint | Enable lint checks |
| `--allow <rule>` | Permite (suprime) uma regra | Allow (suppress) a lint rule |
| `--deny <rule>` | Eleva uma regra a erro | Escalate a lint rule to error |
| `--json` | Emite diagnósticos em JSON | Emit diagnostics as JSON |


### Regras de Lint Disponíveis / Available Lint Rules

| Regra / Rule | Descrição PT-BR | Description EN-US |
|--------------|-----------------|-------------------|
| `unused-binding` | Variável declarada mas não usada | Variable declared but not used |
| `unreachable-code` | Código após `return` inacessível | Code after `return` is unreachable |
| `shadowing` | Variável oculta outra do mesmo escopo pai | Variable shadows one in a parent scope |

### Códigos de Saída / Exit Codes

| Código / Code | Significado PT-BR | Meaning EN-US |
|---------------|-------------------|---------------|
| `0` | Sucesso | Success |
| `64` | Erro de uso (argumentos inválidos) | Usage error (invalid arguments) |
| `65` | Erro de compilação | Compilation error |
| `74` | Erro de I/O | I/O error |

---

## Índice de Todos os Arquivos / Index of All Files

| Arquivo | Conteúdo |
|---------|---------|
| [01-introducao.md](01-introducao.md) | O que é SpectraLang, filosofia, pipeline, CLI, Hello World |
| [02-fundamentos.md](02-fundamentos.md) | Variáveis, tipos, operadores, controle de fluxo, funções |
| [03-tipos-compostos.md](03-tipos-compostos.md) | Arrays, tuplas, ranges, structs, enums, impl, traits, genéricos, closures |
| [04-avancado.md](04-avancado.md) | Pattern matching, if let, while let, Option, Result, ?, módulos, visibilidade |
| [05-stdlib.md](05-stdlib.md) | Referência completa dos 12 módulos da stdlib |
| **06-referencia-rapida.md** | **Este arquivo — consulta rápida** |

---

> **Fim da documentação SpectraLang Alpha**  
> **End of SpectraLang Alpha documentation**  
>
> Para contribuir ou reportar erros, veja o repositório do projeto.  
> To contribute or report errors, see the project repository.
