# SpectraLang — Documento de Arquitetura do Compilador

> **Versão:** Análise do estado atual do repositório (alpha)  
> **Escopo:** Fluxo completo desde o primeiro caractere do arquivo-fonte até a validação final e execução/JIT.

---

## 1. Visão Geral do Pipeline

O compilador SpectraLang é organizado como um pipeline de fases sequenciais, onde cada fase consome o produto da anterior e produz uma representação mais elevada (ou, no caso do backend, uma representação de máquina). O fluxo macroscópico é:

```
Arquivo .spectra
       │
       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  FRONT-END (compiler crate)                                                 │
│  1. Lexical Analysis  → Vec<Token>                                          │
│  2. Parsing           → AST (Module)                                        │
│  3. Semantic Analysis → AST validado + tabelas de símbolos                  │
│  4. Lint              → Vec<LintDiagnostic>                                 │
└─────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  MID-END (midend crate)                                                     │
│  5. Lowering (AST → IR) → IRModule (SSA)                                    │
│  6. Otimizações         → IRModule modificado                               │
│  7. Verificação         → Validação estrutural do IR                        │
└─────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  BACK-END (backend crate)                                                   │
│  8. Code Generation (JIT) → Cranelift JITModule → código nativo na memória  │
│  9. Code Generation (AOT) → ObjectModule → arquivo .o/.obj                  │
└─────────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  RUNTIME (runtime crate)                                                    │
│  10. Execução JIT / chamadas a host functions / GC híbrido                  │
└─────────────────────────────────────────────────────────────────────────────┘
```

A orquestração completa reside em `tools/spectra-cli/src/compiler_integration.rs`, na struct `SpectraCompiler`, que implementa o trait `BackendDriver` e acopla todas as fases.

---

## 2. Fluxo Detalhado: Do Primeiro Caractere à Validação

### 2.1 Entrada do Sistema — CLI e Descoberta de Fontes

**Arquivo principal:** `tools/spectra-cli/src/main.rs`

1. **Parse de argumentos (`parse_cli`)**: O binário `spectralang` recebe o comando (`run`, `check`, `compile`, `lint`, etc.) e flags (`-O2`, `--dump-ast`, `--enable-experimental`).
2. **Descoberta de projeto**: Se o diretório de entrada contiver `spectra.toml`, o CLI carrega a configuração (`config::try_load_config`) e descobre os módulos via `discovery::discover_sources`.
3. **Sintetização de declaração de módulo**: Em `compile_plan`, se um arquivo não contiver a declaração `module <nome>;` na primeira linha não-comentada, o CLI **prefixa** o código-fonte com uma declaração sintética derivada do nome do arquivo (`source_has_module_decl`).
4. **Criação do `SpectraCompiler`**: Instancia `CompilationPipeline<FullPipelineBackend>` com as opções coletadas.

### 2.2 Fase 1 — Análise Léxica (Lexer)

**Arquivo principal:** `compiler/src/lexer/mod.rs`

**Entrada:** `&str` (conteúdo UTF-8 do arquivo)  
**Saída:** `Result<Vec<Token>, Vec<LexError>>`

#### 2.2.1 Algoritmo do Lexer

O lexer é um **autômato finito implementado manualmente** que itera sobre `source.char_indices()`:

1. **Inicialização**: Cria um vetor de tuplas `(byte_offset, char)` via `char_indices()` para permitir lookahead.
2. **Loop principal (`while index < length`)**:
   - Determina `start_location` (linha, coluna) com base em um contador manual (`line`, `column`).
   - Faz **pattern matching** no caractere atual.
3. **Categorias de tokens**:
   - **Whitespace** (`' '`, `\t`, `\r`, `\n`): Avança posição, não emite token.
   - **Comentários** (`//`): Consome tudo até `\n` ou EOF.
   - **Identificadores/Keywords** (`_`, `[a-zA-Z]`): Consome caracteres alfanuméricos e `_`. Depois consulta `Keyword::from_identifier()` para decidir se é keyword ou identificador.
   - **Literais numéricos** (`[0-9]`): Consome dígitos. Se encontrar `.` seguido de mais dígitos, trata como float.
   - **Strings** (`"`): Processa escape sequences (`\\`, `\n`, `\t`, etc.). Aceita newline dentro da string (não é erro).
   - **F-strings** (`f"`): Similar à string, mas marcada como `TokenKind::FStringLiteral`.
   - **Char literals** (`'`): Processa escape. Rejeita vazio.
   - **Símbolos/Operadores** (`.`, `=`, `+`, `-`, etc.): Faz lookahead de 1–2 caracteres para detectar operadores compostos (`==`, `!=`, `<=`, `>=`, `&&`, `||`, `->`, `=>`, `..`, `..=`).
   - **Caractere inesperado**: Emite `LexError`.
4. **EOF**: Sempre emite um token `EndOfFile` com `Span` apontando para o final do arquivo.

#### 2.2.2 Posicionamento (Spans)

O sistema de spans é **baseado em byte offsets** e **linha/coluna 1-based**:
- `Span.start` / `Span.end`: offsets de bytes (inclusive/exclusive).
- `Location`: `(line, column)`.
- A convenção de coluna é **exclusive** no `end_location` (ou seja, `end_location.column` aponta uma posição *após* o último caractere do token).

#### 2.2.3 Cache de Parse (ModuleLoader)

**Arquivo:** `compiler/src/parser/workspace.rs`

O `ModuleLoader` implementa cache incremental:
- Calcula um hash (`DefaultHasher`) sobre o source + feature flags habilitadas.
- Se o hash coincidir com a entrada em cache, retorna o `Module` clonado (ou erros clonados) sem re-executar lexer/parser.
- Se o source mudou, re-executa lexer e parser e atualiza o cache.

### 2.3 Fase 2 — Análise Sintática (Parser)

**Arquivo principal:** `compiler/src/parser/mod.rs`  
**Submódulos:** `module.rs`, `item.rs`, `statement.rs`, `expression.rs`, `type_annotation.rs`

**Entrada:** `Vec<Token>`  
**Saída:** `Result<Module, Vec<ParseError>>`

#### 2.3.1 Estrutura do Parser

O parser é um **recursive-descent parser (top-down) com lookahead de 1 token** e recuperação de erros básica:
- Mantém um índice `position` no vetor de tokens.
- Usa um `eof_sentinel` para evitar panics quando o índice ultrapassa o vetor.
- Guarda `trait_signatures` (mapa de nomes de trait para assinaturas) para validar `impl Trait for Type` no momento do parse.

#### 2.3.2 Hierarquia de Parse

```
parse_module()
  ├── parse_import()          (se keyword == Import)
  ├── parse_item()
  │     ├── parse_function()
  │     ├── parse_struct()
  │     ├── parse_enum()
  │     ├── parse_impl_block() / parse_trait_impl_block()
  │     ├── parse_trait_declaration()
  │     ├── parse_type_alias()
  │     ├── parse_const_decl()
  │     └── parse_static_decl()
  │
  └── (dentro de funções/blocos)
        parse_block()
        parse_statement()
        parse_expression()
```

#### 2.3.3 Expressões e Precedência

O parse de expressões usa o algoritmo **Pratt / top-down operator precedence** (implementado em `expression.rs`), com níveis de precedência para:
1. Atribuição (`=`)
2. Operadores lógicos (`&&`, `||`)
3. Comparação (`==`, `!=`, `<`, `>`, `<=`, `>=`)
4. Aditivo (`+`, `-`)
5. Multiplicativo (`*`, `/`, `%`)
6. Unário (`-`, `!`)
7. Pós-fixo (call, index, field access, method call)
8. Primário (literals, identifiers, grouping, block-expr, etc.)

#### 2.3.4 Feature Gating

Certas construções (`loop`, `unless`, `do-while`, `switch`) são protegidas por **feature flags experimentais**. O parser verifica `enabled_features` (um `HashSet<String>` passado do CLI) e rejeita o parse se a feature não estiver habilitada.

#### 2.3.5 Recuperação de Erros

Quando o parser encontra um token inesperado:
- Emite um `ParseError`.
- Chama `synchronize()`, que avança tokens até encontrar um delimitador de fronteira (`;`, `}`, ou keyword de topo como `fn`, `struct`, `let`, etc.).
- Continua o parse a partir daí, permitindo detectar múltiplos erros em um único passe.

### 2.4 Fase 3 — Análise Semântica

**Arquivo principal:** `compiler/src/semantic/mod.rs`  
**Submódulos:** `builtin_modules.rs`, `module_registry.rs`

**Entrada:** `&mut Module` (AST mutável)  
**Saída:** `Vec<SemanticError>` (vazio = sucesso)

#### 2.4.1 Arquitetura do SemanticAnalyzer

O analisador semântico é um **visitor de múltiplas passadas** sobre a AST. Ele mantém:
- `symbols: Vec<HashMap<String, SymbolInfo>>` — pilha de escopos para variáveis locais.
- `functions: HashMap<String, FunctionSignature>` — tabela de funções globais.
- `struct_infos` / `enum_infos` — metadados de tipos definidos pelo usuário.
- `methods: HashMap<String, HashMap<String, FunctionSignature>>` — métodos por tipo.
- `traits` / `trait_impls` — registry de traits e implementações.
- `registry: Arc<RwLock<ModuleRegistry>>` — registry compartilhado entre módulos para resolução de imports cross-module.

#### 2.4.2 Passadas de Análise

A análise de um módulo ocorre em **quatro passadas** sequenciais:

**Passada 0 — Resolução de Imports:**
- Coleta todos os `Item::Import` do módulo.
- Para cada import, consulta o `ModuleRegistry` compartilhado.
- Injeta assinaturas de funções e tipos no escopo do analisador.
- Preenche `module.std_import_aliases` e `module.imported_function_return_types` para consumo do midend.

**Passada 1 — Coleta de Declarações:**
- Itera sobre todos os `Item`s e registra:
  - Funções em `self.functions`
  - Structs em `self.struct_infos`
  - Enums em `self.enum_infos`
  - Traits em `self.traits`
- Detecta duplicatas (erro E006 para funções).
- Valida visibilidade de tipos expostos em itens públicos (`enforce_visibility_rules`).

**Passada 2 — Análise de Corpos:**
- Para cada função/método:
  - Cria um novo escopo.
  - Declara parâmetros como símbolos locais.
  - Analisa cada statement via `analyze_statement()`.
  - Valida tipos de retorno (`validate_function_block_return`).
- Para cada `impl` block:
  - Valida que métodos de `impl Trait for Type` satisfazem a assinatura do trait (`validate_trait_impl`).
  - Copia métodos default do trait para o tipo (`copy_default_trait_methods`).

**Passada 3 — Inferência de Tipos Genéricos:**
- Tenta inferir argumentos de tipo genérico em construções como `Option::Some(x)` ou chamadas a funções genéricas, quando a anotação de tipo está ausente.

**Passada 4 — Preenchimento de Tipos em Method Calls:**
- Preenche o campo `type_name` em `ExpressionKind::MethodCall` para uso do midend.

#### 2.4.3 Sistema de Tipos

O tipo interno `Type` (definido em `compiler/src/ast/mod.rs`) é uma enumeração com variantes para:
- Escalares: `Int`, `Float`, `Bool`, `String`, `Char`, `Unit`
- Compostos: `Array`, `Tuple`, `Struct`, `Enum`
- Genéricos: `TypeParameter`, `SelfType`
- Funções: `Fn { params, return_type }`
- Trait objects: `DynTrait`
- `Unknown` (placeholder para inferência incompleta)

A compatibilidade de tipos é testada por `types_match()`, que implementa:
- Promoção automática `Int → Float`.
- Coerência de `Unknown` com qualquer tipo.
- Coerência de `SelfType` com qualquer `Struct`.
- Comparabilidade de `TypeParameter` com qualquer tipo (alpha limitation).

#### 2.4.4 Registry Cross-Module

O `ModuleRegistry` é um HashMap global (protegido por `RwLock`) que mapeia nomes de módulo para `ModuleExports`. Quando um módulo termina a análise semântica com sucesso, seus exports são registrados via `collect_module_exports()`, tornando-os visíveis para módulos subsequentes.

### 2.5 Fase 4 — Lint

**Arquivo:** `compiler/src/lint/mod.rs`

Executado **após** a análise semântica, o lint faz uma travessia do AST independente para detectar:
- **Unused bindings**: Variáveis/parâmetros declarados mas nunca usados.
- **Unreachable code**: Código após `return`, `break`, `continue`.
- **Shadowing**: Re-declaração de um nome já presente em um escopo externo.

O lint pode ser configurado via CLI (`--allow`, `--deny`) ou via `spectra.toml` (`[lint]` section).

### 2.6 Fase 5 — Lowering (AST → IR)

**Arquivo principal:** `midend/src/lowering.rs`

**Entrada:** `&ASTModule`  
**Saída:** `IRModule`

#### 2.6.1 Estrutura do Lowering

O lowering converte a AST de alto nível para uma representação SSA de baixo nível. Ele mantém:
- `value_map: ScopeStack` — mapeia nomes de variáveis para `Value` (IDs SSA).
- `variable_types: TypeScopeStack` — tipos IR associados a cada variável.
- `alloca_map` — alocações na pilha para variáveis mutáveis.
- `generic_functions` / `generic_structs` / `generic_enums` — definições genéricas para monomorfização.
- `pending_specializations` — fila de requisições de especialização de tipos genéricos.

#### 2.6.2 Processo de Lowering

1. **Pré-registro de aliases de stdlib**: Copia `std_import_aliases` do módulo AST para resolver calls não-qualificados.
2. **Primeira passada**: Coleta definições de structs, enums, traits e impls. Registra generics.
3. **Segunda passada**: Lowering de funções.
   - Para funções não-genéricas: chama `lower_function()` diretamente.
   - Para funções genéricas: armazena a definição AST e adia o lowering.
4. **Processamento de especializações pendentes** (`process_monomorphization_requests`):
   - Cria versões concretas de funções genéricas (monomorfização).
   - Limita a 512 especializações para evitar expansão infinita.
   - Gera nomes "mangled" (ex: `process_Point` para `process<Point>`).
5. **Emissão de lambdas**: Funções lambda coletadas durante o lowering são adicionadas como funções IR de nível superior.

#### 2.6.3 Instruções IR

O IR (`midend/src/ir.rs`) usa:
- **Valores SSA**: Cada valor tem um `id: usize` único por função.
- **Basic Blocks**: Blocos com ID, label, lista de instruções e um terminator.
- **Terminators**: `Return`, `Branch`, `CondBranch`, `Switch`, `Unreachable`.
- **Instruções**: Aritmética (`Add`, `Sub`, etc.), comparações (`Eq`, `Lt`, etc.), memória (`Alloca`, `Load`, `Store`, `GetElementPtr`), calls (`Call`, `HostCall`, `CallIndirect`), constantes, casts, e operações de fat pointer para `dyn Trait`.

### 2.7 Fase 6 — Otimizações e Verificação do IR

**Arquivos:** `midend/src/passes/`

O pipeline de otimização é condicional ao `opt_level`:
- **Nível 0**: Nenhuma otimização.
- **Nível 1+**: `ConstantFolding` — avalia expressões constantes em tempo de compilação.
- **Nível 2+**: `DeadCodeElimination` — remove blocos e instruções não alcançáveis.

Após as otimizações, executa:
- **`LoopStructureValidation`**: Verifica que loops IR têm um único header block e back-edges bem formados.
- **`verify_module` (pré e pós)**: Verificações estruturais do IR (ex: todos os blocos referenciados existem, terminators presentes, etc.).

### 2.8 Fase 7 — Geração de Código (Backend)

#### 2.8.1 JIT — CodeGenerator

**Arquivo:** `backend/src/codegen.rs`

Usa **Cranelift JIT** (`cranelift_jit::JITModule`):
1. **Declaração**: Para cada função IR, declara a assinatura no Cranelift (`declare_function`).
2. **Definição**: Gera o corpo da função usando `FunctionBuilder`:
   - Mapeia parâmetros IR para `Value`s Cranelift.
   - Cria blocos Cranelift para cada basic block IR.
   - Traduz cada `InstructionKind` para instruções Cranelift equivalentes.
   - Gerencia alocações manuais via runtime imports (`spectra_rt_manual_alloc`, `spectra_rt_manual_free`).
   - Emite frame enter/exit para o runtime.
3. **Finalização**: `finalize_definitions()` resolve símbolos e aloca memória executável.
4. **Execução**: `get_function_ptr()` retorna um ponteiro de função nativo. `execute_entry_point()` faz `transmute` para o tipo de função correto e chama.

#### 2.8.2 AOT — AotCodeGenerator

**Arquivo:** `backend/src/aot.rs`

Usa **Cranelift ObjectModule** (`cranelift_object`):
- Similar ao JIT, mas emite um arquivo objeto nativo (COFF/ELF/Mach-O).
- Pre-interna nomes de host functions como seções `.rodata` para endereços relocáveis.
- Suporta modo `--emit-exe`: renomeia `main` para `spectra_user_main` e sintetiza um shim C `main(argc, argv)` que inicializa o runtime e chama a entry point Spectra.

#### 2.8.3 Seam HostCall/ABI

O contrato de imports nativos usado por JIT e AOT tem uma única fonte no
catálogo interno `runtime/src/abi.rs` (`RuntimeImport`, `FastHostCall` e
`HostCallClass`). O catálogo é parte do toolchain, está oculto da documentação
pública e contém, para cada import, nome Spectra quando aplicável, símbolo
nativo, assinatura Cranelift, aridade e endereço JIT.

`backend/src/hostcall_abi.rs` é o adapter comum dos dois backends:

- `RuntimeBindings` mantém os `FuncId`s em uma tabela indexada pelo enum, sem
  `HashMap` no caminho de geração;
- `HostCallLoweringContext` agrupa bindings, nomes/literais internados, estado
  do planner de batching e estatísticas;
- JIT registra os endereços e AOT declara os mesmos símbolos por meio do mesmo
  catálogo e da mesma tradução de assinatura.

O lowering classifica um nome uma vez em `FastHostCall`. A aridade é validada
antes da interceptação: nome conhecido com aridade incorreta e nome desconhecido
seguem o caminho genérico cacheado `spectra_rt_host_invoke_cached`. Fast paths
nunca são enviados ao batch genérico; dependências entre resultados, o limite
de oito chamadas e o orçamento de 4096 bytes permanecem política do backend.
O cache é um slot `SpectraHostCallCache` por nome, mantido vivo pelo JIT ou
emitido como dado local gravável no AOT. Hits validam a geração global com
atomics e não adquirem o lock do registry; misses e invalidações consultam o
registry pelo caminho existente. Registro, substituição, remoção e `clear`
continuam imediatamente visíveis a código já compilado. O batch cacheado usa
um descriptor de sete palavras e uma única fronteira de `catch_unwind`, mas
os imports legados permanecem disponíveis sem alteração de contrato.

O runtime continua owner do registry, do contexto, dos status, da publicação
atômica e do dispatch dinâmico. As alocações atuais de argumentos/resultados
permanecem fora desta fase. A atribuição causal de performance continua
dependente da evidência oficial Linux de R-3102.

### 2.9 Fase 8 — Runtime e Execução

**Arquivo principal:** `runtime/src/lib.rs`

O runtime é inicializado uma única vez por processo (`OnceLock<RuntimeState>`):
- **Memória**: `ManualMemory` gerencia o heap manual (`spectra_rt_manual_alloc`) com telemetria por alocação; não há garbage collector no runtime.
- **Host calls**: Funções da stdlib (I/O, math, collections) são registradas como callbacks invocáveis pelo nome via os entry points genérico legado ou cacheado.
- **Args**: `set_program_args` configura `std.env.env_arg` / `std.env.env_args_count`.
- **Execution**: Após JIT, o resultado de `main()` (se houver) é propagado como exit code do processo.

---

## 3. Estruturas de Dados Principais

### 3.1 Token

```rust
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub enum TokenKind {
    Identifier(String),
    Number(String),
    Keyword(Keyword),
    Symbol(char),
    Operator(Operator),
    StringLiteral(String),
    CharLiteral(char),
    FStringLiteral(String),
    EndOfFile,
}
```

### 3.2 AST — Module

```rust
pub struct Module {
    pub name: String,
    pub span: Span,
    pub items: Vec<Item>,
    pub std_import_aliases: Vec<(String, Vec<String>)>,
    pub imported_function_return_types: Vec<(String, Type)>,
}
```

### 3.3 IR — Module

```rust
pub struct Module {
    pub name: String,
    pub functions: Vec<Function>,
    pub globals: Vec<Global>,
    pub vtables: Vec<VTableDef>,
}

pub struct Function {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Type,
    pub blocks: Vec<BasicBlock>,
    pub next_value_id: usize,
    pub next_block_id: usize,
}

pub struct BasicBlock {
    pub id: usize,
    pub label: String,
    pub instructions: Vec<Instruction>,
    pub terminator: Option<Terminator>,
}
```

---

## 4. Problemas de Lógica e Implementação Identificados

### 4.1 Críticos — Podem causar comportamento incorreto ou crashes

#### P1. Uso de `panic!` no lowering em vez de erro controlado ✅ CORRIGIDO
**Local:** `midend/src/lowering.rs` (múltiplas funções: `ensure_struct_definition`, `ensure_enum_definition`, `specialize_function`).

~~O lowering usa `panic!` quando não encontra uma definição de struct/enum ou quando há violação de trait bound em monomorfização. Isso **termina o processo inteiro** em vez de retornar um erro de compilação elegante.~~

**Correção aplicada:** Todos os `panic!` foram substituídos por acumulação de erros em um vetor interno `errors: Vec<MidendError>`. A função `lower_module` agora retorna `Result<IRModule, Vec<MidendError>>`. Os callers (`compiler_integration.rs` e testes) foram atualizados para tratar o `Result` e converter os erros em `CompilerError::Midend`. O compilador agora retorna erros estruturados em vez de abortar o processo.

#### P2. CodeGenerator JIT é reutilizado entre módulos sem isolamento adequado ✅ CORRIGIDO
**Local:** `compiler_integration.rs` — `FullPipelineBackend` mantém `codegen: Option<CodeGenerator>`.

~~Embora o comentário afirme que a reutilização permite referências cruzadas entre módulos, o Cranelift JITModule não suporta descarregar/redefinir funções. Se dois módulos definirem funções com o mesmo nome, o comportamento é indefinido.~~

**Correção aplicada:** O `FullPipelineBackend::run()` agora cria um `CodeGenerator` fresco a cada compilação (`let mut codegen = CodeGenerator::new();`). O campo `self.codegen` ainda existe para permitir `execute()` posteriormente, mas ele é sobrescrito a cada `run()`, eliminando o risco de colisão de símbolos entre módulos compilados em sequência.

#### P3. `types_match` aceita `Type::Unknown` como universal ✅ CORRIGIDO
**Local:** `compiler/src/semantic/mod.rs`.

```rust
(Type::Unknown, _) | (_, Type::Unknown) => true,
```

~~Isso significa que se a inferência de tipos falhar silenciosamente (produzindo `Unknown`), o compilador **nunca reportará erro de tipo**.~~

**Correção aplicada:** A relação `Unknown` como universal ainda existe em `types_match` (necessária para fases intermediárias de inferência), mas agora os pontos de checagem principais emitem erros explícitos quando encontram `Unknown`:
- **Assignment (`check_assignment`)**: se o valor é `Unknown`, emite erro pedindo anotação de tipo.
- **Argumentos de função (`ExpressionKind::Call`)**: se um argumento é `Unknown`, emite erro com hint para adicionar anotação.
- **Return statements (`check_return_statement`)**: se a expressão retornada é `Unknown`, emite erro pedindo anotação explícita.

Isso garante que tipos não-resolvidos não passem despercebidos para o backend.

#### P4. Falta de validação de UTF-8 inválido no lexer
**Local:** `compiler/src/lexer/mod.rs`.

O lexer recebe `&str` (já validado como UTF-8 pelo Rust), mas se o arquivo contiver sequências UTF-8 inválidas, `fs::read_to_string` falha antes do lexer. No entanto, para fontes vindas de outros mecanismos (ex: REPL, macros), não há tratamento de `String` malformada no nível do token.

#### P5. `is_cast_valid` permite casts perigosos sem verificação em runtime ✅ CORRIGIDO
**Local:** `compiler/src/semantic/mod.rs`.

```rust
(Struct { .. }, DynTrait { .. }) => true,
```

~~Qualquer struct pode ser cast para qualquer `dyn Trait` sem verificar se existe uma implementação. O backend gera código que passa o ponteiro diretamente.~~

**Correção aplicada:** O analisador semântico agora verifica, no momento do cast `expr as dyn TraitName`, se o tipo concreto (`Struct { name }`) possui uma entrada correspondente em `self.trait_impls` para o trait de destino. Se não houver implementação registrada, um erro semântico é emitido imediatamente:

```
Cannot cast `MyStruct` to `dyn MyTrait`: type `MyStruct` does not implement trait `MyTrait`
```

Além disso, se o tipo de origem for `Unknown`, um erro separado pede anotação de tipo antes do cast. O cast genérico ainda é permitido, mas apenas quando a implementação está comprovada na tabela de traits.

### 4.2 Sérios — Limitam robustez ou corretude

#### P6. Parser não lida com comentários de bloco (`/* */`) ✅ CORRIGIDO
**Local:** `compiler/src/lexer/mod.rs`.

Apenas comentários de linha (`//`) eram suportados. Comentários de bloco eram tratados como tokens inválidos ou sequências de operadores.

**Correção:** Adicionado tratamento de comentários de bloco `/* */` no lexer, com suporte a múltiplas linhas e atualização correta de posição. Comentários não terminados emitem `LexError` com hint.

#### P7. Recuperação de erros do parser pode entrar em loop infinito ✅ CORRIGIDO
**Local:** `compiler/src/parser/mod.rs` — `synchronize()`.

Se o parser encontrar um erro seguido de um token que não é reconhecido como fronteira de sincronização, ele avança um token e tenta novamente. Em certos casos (ex: stream de tokens totalmente malformado), `synchronize()` pode não avançar suficientemente, causando loop de erros repetidos no mesmo ponto.

**Correção:** `synchronize()` agora verifica se o token atual já é uma fronteira de sincronização *antes* do `advance()` inicial, evitando pular tokens válidos. Extraído para método auxiliar `is_at_boundary()`.

#### P8. Semantic Analyzer faz múltiplas passadas desnecessárias
**Local:** `compiler/src/semantic/mod.rs`.

A análise semântica percorre `module.items` quatro vezes (imports, declarações, corpos, inferência genérica, preenchimento de method calls). Isso é **O(5n)** na AST. Em teoria, uma passada com estados de resolução seria suficiente para a maioria das linguagens.

#### P9. `ModuleRegistry` usa `RwLock` mas não trata poison ✅ CORRIGIDO
**Local:** `compiler/src/semantic/module_registry.rs` e `mod.rs`.

```rust
if let Ok(mut reg) = self.registry.write() { ... }
```

Se uma thread fizer panic enquanto segura o lock, o `RwLock` fica "envenenado" e todas as escritas subsequentes são silenciosamente ignoradas (o `if let Ok(...)` descarta o erro). Em compilação multi-threaded futura, isso causaria comportamento silenciosamente incorreto.

**Correção:** Todas as chamadas `registry.read()` e `registry.write()` agora usam `.unwrap_or_else(|p| p.into_inner())`, recuperando o valor interno mesmo em caso de lock envenenado.

#### P10. O sistema de imports não detecta ciclos de dependência
**Local:** `compiler/src/semantic/mod.rs` — `analyze_import()`.

Se o módulo A importa B e B importa A, o `ModuleRegistry` pode não ter os exports de B quando A é analisado (dependendo da ordem). Não há detecção de ciclos nem análise de dois-passos para resolver dependências circulares.

### 4.3 Médios — Melhorias de qualidade e manutenção

#### P11. `type_annotation_to_type` retorna `Unknown` para tipos não-resolvidos ✅ CORRIGIDO
**Local:** `compiler/src/semantic/mod.rs`.

Quando um tipo não é reconhecido (ex: typo em anotação), o analisador retornava `Type::Unknown` em vez de emitir um erro imediato. O erro só aparecia posteriormente (se aparecesse) em contextos de uso.

**Correção:** Adicionado método `type_annotation_to_type_checked` que envolve o método base e emite `SemanticError` quando um nome de tipo `Simple` não pode ser resolvido. Usado na análise de corpo (pass 2), especialmente em `let` statements.

#### P12. O backend AOT não implementa otimizações
**Local:** `backend/src/aot.rs`.

O AOT reutiliza `generate_block` do JIT, mas não aplica nenhuma otimização de nível de máquina (register allocation é feita pelo Cranelift, mas sem passes adicionais como inlining ou LICM).

#### P13. `Source` sem `module` declaration recebe prefixo sintético sem verificação de colisão ✅ CORRIGIDO
**Local:** `tools/spectra-cli/src/main.rs` — `source_has_module_decl()`.

O CLI prefixa `module <nome>;` ao source. Se o arquivo já tiver declarações antes do `module` (ex: comentários de bloco), elas ficavam após a declaração sintética, o que pode ser semanticamente inválido.

**Correção:** `source_has_module_decl` foi reescrita com um scanner char-by-char que ignora corretamente comentários `//` e `/* */` antes de verificar a presença de `module`.

#### P14. O campo `span` de tokens e AST não considera Unicode multi-byte
**Local:** `compiler/src/span.rs`.

O sistema de spans usa byte offsets (`usize`). Caracteres Unicode multi-byte (ex: emojis, kanji) ocupam múltiplos bytes. Se o span apontar para o meio de um caractere multi-byte, o renderizador de erros pode quebrar ao tentar extrair a linha ou calcular a coluna.

#### P15. `infer_expression_type` para `Identifier` consulta `functions` como fallback ✅ CORRIGIDO
**Local:** `compiler/src/semantic/mod.rs`.

```rust
ExpressionKind::Identifier(name) => {
    if let Some(info) = self.lookup_symbol(name) { info.ty.clone() }
    else if let Some(sig) = self.functions.get(name) { sig.return_type.clone() }
    else { Type::Unknown }
}
```

Isso confundia o **tipo da função** com o **tipo de retorno da função**. Se `foo` é uma função `fn() -> int`, o tipo de `foo` (a entidade função) deveria ser `fn() -> int`, não `int`. Isso causava inferência incorreta em contextos de higher-order functions.

**Correção:** Em `collect_declarations_pass`, funções são registradas com `Type::Fn { params, return_type }` em vez de apenas `return_type`. O fallback em `infer_expression_type` também foi corrigido para retornar o tipo `Fn` completo.

#### P16. O runtime usa `unsafe` transmute sem verificação de assinatura
**Local:** `backend/src/codegen.rs` — `execute_entry_point()`.

```rust
let func: extern "C" fn() -> i64 = std::mem::transmute(ptr);
```

Não há verificação se o ponteiro realmente aponta para uma função com a assinatura esperada. Se o IR tiver sido mal-gerado (ex: parâmetros extras), o comportamento é indefinido.

#### P17. O sistema de tipos não distingue `int` de tamanhos
**Local:** `compiler/src/ast/mod.rs`.

Spectra tem apenas `Int` (presumivelmente 64-bit no backend). Não há `i32`, `i16`, `u32`, etc. Isso limita a interoperabilidade com C/FFI e pode causar overflow silencioso em plataformas onde o usuário espera tamanhos menores.

#### P18. `infer_array_element_type` no lowering usa fallback `IRType::Int`
**Local:** `midend/src/lowering.rs`.

```rust
if elements.is_empty() {
    return IRType::Int;
}
```

Um array vazio `[]` é inferido como `[int]`. Isso é arbitrário; deveria requerer anotação de tipo ou ser um erro.

#### P19. O parser de expressões aceita `if` e `unless` sem ponto-e-vírgula obrigatório
**Local:** `compiler/src/parser/statement.rs`.

```rust
let requires_semicolon = !matches!(expr.kind, ExpressionKind::If { .. } | ExpressionKind::Unless { .. }) && !self.check_symbol('}');
```

Isso cria uma ambiguidade: `if a { b } else { c }` como statement final de um bloco não requer `;`, mas se houver outro statement depois, o parser pode confundir.

#### P20. Não há verificação de exhaustividade em `match`
**Local:** `compiler/src/semantic/mod.rs`.

A análise semântica aceita `match` sem verificar se todos os variants de um enum foram cobertos. Código como:
```spectra
match opt {
    Some(x) => x,
}
```
compila sem erro, mas falha em runtime se `opt` for `None`.

---

## 5. Resumo do Fluxo de Dados

```
┌──────────────┐     char_indices()      ┌──────────────┐
│  Source      │ ───────────────────────>│  Lexer       │
│  (&str)      │                         │  (mod.rs)    │
└──────────────┘                         └──────┬───────┘
                                                │ Vec<Token>
                                                ▼
                                       ┌────────────────┐
                                       │  Parser        │
                                       │  (mod.rs)      │
                                       └───────┬────────┘
                                               │ Result<Module, ParseError>
                                               ▼
                                       ┌────────────────┐
                                       │  Semantic      │
                                       │  Analyzer      │
                                       │  (mod.rs)      │
                                       └───────┬────────┘
                                               │ Vec<SemanticError> (vazio=OK)
                                               ▼
                                       ┌────────────────┐
                                       │  Lint Runner   │
                                       │  (mod.rs)      │
                                       └───────┬────────┘
                                               │ Vec<LintDiagnostic>
                                               ▼
                                       ┌────────────────┐
                                       │  AST Lowering  │
                                       │  (lowering.rs) │
                                       └───────┬────────┘
                                               │ IRModule
                                               ▼
                                       ┌────────────────┐
                                       │  IR Passes     │
                                       │  (passes/)     │
                                       └───────┬────────┘
                                               │ IRModule (otimizado)
                                               ▼
                                       ┌────────────────┐
                                       │  CodeGen       │
                                       │  (JIT / AOT)   │
                                       └───────┬────────┘
                                               │ Native Code
                                               ▼
                                       ┌────────────────┐
                                       │  Runtime       │
                                       │  (runtime/)    │
                                       └────────────────┘
```

---

## 6. Correções Adicionais Aplicadas

### Blocos `merge` sem terminador em `if`/`unless`
**Local:** `midend/src/lowering.rs` e `midend/src/builder.rs`.

Quando todos os branches de uma expressão `if` ou `unless` terminam com `return`, o bloco `merge` ficava sem terminador, causando falha na verificação de IR (`error[internal]: block 'if.merge' is missing a terminator`).

**Correção:** Foi adicionado o método `build_unreachable` ao `IRBuilder`, que emite `Terminator::Unreachable`. O lowering agora invoca `build_unreachable` no bloco `merge` quando detecta que nenhum predecessor pode cair nele (todos os branches possuem terminador). Isso mantém a IR bem-formada sem afetar a semântica de execução.

---

## 7. Recomendações de Prioridade (Pendentes)

1. ~~**Eliminar `panic!` do lowering** (P1)~~ ✅ Corrigido.
2. ~~**Adicionar verificação de exhaustividade em match** (P20)~~ ✅ Corrigido.
3. ~~**Corrigir a semântica de `Type::Unknown`** (P3)~~ ✅ Corrigido.
4. ~~**Implementar comentários de bloco** (P6)~~ ✅ Corrigido.
5. **Adicionar detecção de ciclos em imports** (P10) — necessário para projetos multi-módulo.
6. **Revisar o tratamento de spans com UTF-8 multi-byte** (P14) — evita crashes no renderizador de erros.
7. ~~**Separar tipos de função de tipos de retorno** (P15)~~ ✅ Corrigido.

---

*Documento gerado a partir da análise do código-fonte do repositório SpectraLang.*
