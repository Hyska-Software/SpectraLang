# Expansão da Standard Library `.spectra` — Onda 2 (Lotes 5–6) Implementation Plan

**Intent:** continuar a migração incremental da standard library para módulos
`.spectra` (R-3406), removendo os bloqueios de linguagem que travam categorias
inteiras de módulos e eliminando a duplicação de helpers UTF-8 da Onda 1.

**Current Behavior (verificado, Onda 1 concluída):**

- `stdlib/src/` tem 10 módulos-fonte (81 funções públicas) + a primitiva nativa
  `std.string.from_scalar`; R-3007 verde (`blockers: 0`, catálogo 1444/1221).
- Lacunas provadas na execução (F-01…F-18 em
  `docs/goals/spectralang-stdlib-source-expansion/FINDINGS.md`):
  1. Sem **operadores bitwise/shift**: `BinaryOperator` (`compiler/src/ast/mod.rs:641`)
     só tem aritmética/comparação/lógicos; `Operator` (`compiler/src/token.rs:235`)
     só tem comparação/lógicos/range; nada de `Shl/Shr/BitAnd` no IR
     (`midend/src/ir.rs:141`) nem no backend; `is_symbol_char`
     (`compiler/src/lexer/mod.rs:878`) não inclui `^` nem `~`.
  2. Sem **`panic`/`abort`** no catálogo — sem base para `std.testing` nem para
     afirmar invariantes.
  3. **Orçamento de stack do parser (F-09)**: `MAX_STACK_USE_BYTES = 512 KiB`
     absoluto; em debug ~17 KiB/nível ⇒ `P013` em ~30 níveis legítimos em
     threads de teste. Dois módulos tiveram de ser achatados. **O mesmo orçamento
     é reutilizado pela análise semântica e pelo lint** (`semantic_init.rs`,
     `lint/mod.rs`).
  4. **Strings UTF-8 NUL-terminadas**: `from_scalar(0)` não fecha round-trip; não
     há contêiner de bytes.
  5. Helpers privados de UTF-8 duplicados em `algorithms`, `encoding`, `text`,
     `path` (~40 linhas).
- Pendência herdada: `validate_execution_coverage --update-baseline` (corpus
  JIT+AOT) não foi gravado na Onda 1.

**Expected Outcome:**

1. Linguagem: operadores `& | ^ << >>` + `~` com semântica fechada, primitiva
   `std.error.panic`, orçamento de stack relativo ao stack real (parser +
   semântica + lint), contrato de bytes registrado.
2. Fonte: `std.unicode` e `std.bytes` como camada fundacional; os quatro
   módulos da Onda 1 refatorados para consumi-las (helpers privados deletados).
3. 10 expansões de módulos existentes (incluindo `std.encoding`) e 6 módulos
   novos, cada item passando o gate R-3406.

**Target-Perspective Output:** com o CLI instalado e offline, o usuário:

- escreve `const MASK: int = 0xF0 | 0x0F; let hi = 0xFF00 >> 8;` e vê o mesmo
  resultado em JIT e AOT, com `spectralang explain` documentando os operadores;
- roda `spectralang package test` num projeto onde `std.testing.assert_eq_int`
  falha e vê a mensagem `expected 3, found 4` e exit code não-zero; rodando
  `std.error.panic("invariant")` direto vê `error: invariant` e exit 70;
- importa `std.csv`, `std.unicode`, `std.hash`, `std.uuid` e encontra cada
  função documentada em `docs/reference/05-stdlib.md`;
- inspeciona `target/r3007-stdlib-contract/after.json` com `blockers: 0` e
  `implementation = "spectra-source"` em cada símbolo novo.

**Truth Owner:** `stdlib/src/` (fonte), `compiler/src/{token,lexer,parser,semantic}`
+ `midend/src` + `backend/src` (linguagem), `scripts/stdlib_contract.toml`
(classificação/probes), `packages/spectra-contract/catalog/stdlib.toml`
(gerado — única entrada para descritores), `docs/architecture/stdlib-source-migration.md`
(ledger), `tests/execution-baseline.json` (baseline).

**Contract Boundary:** um símbolo ⇒ uma fonte semântica + um descriptor (gerado
a partir do catálogo, nunca editado à mão) + uma entrada de catálogo
(`abi`/`implementation`/`binding`/`docs`/`fixture`/`maturity`). Um operador ⇒
lexer → token → AST → semântica → IR → backend com o mesmo diagnóstico e a
mesma semântica em `const` e em runtime.

**Cutover:**

- Operadores novos substituem workarounds aritméticos **nomeados**:
  `stdlib/src/algorithms.spectra::collatz_steps` (`% 2`/`/ 2` → `& 1`/`>> 1`) e
  `stdlib/src/encoding.spectra` (extração de 6 bits do base64: `/ 4`, `% 4`,
  `* 16`, `/ 16`, `* 4`, `/ 64` → `>>`/`&`/`<<`), com as fixtures 797/798 como
  regressão.
- `std.unicode`/`std.bytes` substituem os helpers privados `collect_runes`,
  `rune_count`, `rune_at`, `push_rune`, `decode_utf8`, `rune_length_at`
  (deletados na mesma tarefa T-12).
- `std.error.panic` substitui `eprintln + return <não-zero>` onde a intenção é
  invariante violada (sem obrigar migração de código existente).

**Displaced Path:** helpers privados duplicados (T-12) e workarounds aritméticos
citados no Cutover; nenhuma API nativa é renomeada ou removida.

**Value Density:** T-01/T-02 destravam `hash`/`crc`/`testing` e completam
`encoding`/`uuid`; T-03 remove o teto de aninhamento que já forçou dois
refactors; a camada `unicode`/`bytes` tira ~40 linhas duplicadas e habilita
`csv`/`diff`; as expansões são incrementais e reusam o harness de evidência já
provado.

**Acceptance Evidence:**

- Fixture por tarefa em `tests/validation/808..828` (JIT+AOT, exit 0) e
  `tests/errors/720` para diagnóstico; casos de borda com código de retorno
  próprio.
- `validate_r3007_stdlib_contract.py --binary target/debug/spectralang.exe --require-catalog`
  → `blockers: 0`, `catalog: complete`.
- `cargo test --workspace --all-targets --no-fail-fast` sem falhas;
  `cargo test -p spectra-api --test contract_drift` verde;
  `generate_lowering_tables.py --check` current.
- `tests/execution-baseline.json` gravado com os fixtures novos (`--update-baseline`).
- Linguagem: fixture 808 cobre cada operador em `const` e runtime, todas as
  larguras exatas, `u64` acima de 2^63, contagem de shift mascarada, `~`, e
  precedência; teste de erro 720; T-03 cobre parser **e** semântica **e** lint
  em thread de 1 MiB.
- `std.testing`/`panic`: exit 70 documentado e testado em JIT, AOT e
  `spectralang package test` (projeto `tests/projects/invalid/stdlib_testing_assertion_failure/`).

**Evidence Lane:** `target/debug/spectralang.exe`, `tests/validation`,
`tests/errors`, `tests/projects/invalid`, `target/r3007-stdlib-contract/*.json`,
`target/execution-coverage/*.json`, `packages/spectra-contract/catalog/stdlib.toml`,
`cargo test`.

**Kill Criteria:** parar e reportar se

- JIT e AOT divergirem em qualquer fixture novo;
- um operador novo não tiver semântica definida (largura exata, shift ≥ largura,
  sinal, `char`/bool misturados);
- `panic` não tiver exit code documentado e testado nos três caminhos (JIT, AOT,
  `package test`);
- a mudança de orçamento de stack quebrar os testes de `P013` existentes ou não
  cobrir parser/semântica/lint;
- a auditoria R-3007 acusar símbolo não classificado, sem doc, sem probe ou probe
  falhando;
- um módulo exigir primitiva nativa não prevista neste plano.

**Non-goals (explícitos):** `std.regex`, `std.toml`, `std.json` core (Onda 3);
tipo `bytes` novo na linguagem (L-4 resolve por contrato + `List<int>`);
`std.crypto` (SHA-256/HMAC — Onda 3 com ADR próprio); módulos nativos
(fs/io/env/random/concurrent/serve/tensor/ml/net/TLS); reescrever kernels
nativos existentes.

**Architecture Slice:**

- **Files to create:** `stdlib/src/{unicode,bytes,csv,diff,vector,uuid,testing,hash}.spectra`;
  `tests/validation/808..828_*.spectra`; `tests/errors/720_*.spectra`;
  `tests/projects/invalid/stdlib_testing_assertion_failure/{spectra.toml,src/*.spectra,tests/*.spectra}`;
  `docs/architecture/stdlib-bytes-contract.md`.
- **Files to modify (linguagem):** `compiler/src/token.rs`, `compiler/src/lexer/mod.rs`,
  `compiler/src/ast/mod.rs`, `compiler/src/parser/expression_precedence.rs`,
  `compiler/src/parser/mod.rs`, `compiler/src/semantic/semantic_expression_binary.rs`,
  `compiler/src/semantic/semantic_expression_inference.rs`, `compiler/src/semantic/mod.rs`,
  `compiler/src/semantic/semantic_tensor_const.rs`, `compiler/src/semantic/semantic_init.rs`,
  `compiler/src/semantic/builtin_text_system.rs` (`make_std_error`),
  `compiler/src/lint/mod.rs`, `midend/src/{ir.rs,builder.rs,lowering.rs,lowering_expr_binary.rs}`,
  `midend/src/{lowering_impl_core,lowering_impl_blocks,lowering_impl_functions}.rs`,
  `midend/src/passes/verification.rs`, `backend/src/codegen_instruction_arithmetic.rs`.
- **Files to modify (superfície/contrato):** `runtime/src/stdlib/{stdlib_bindings.rs,error.rs,registration.rs,tests.rs}`,
  `scripts/stdlib_contract.toml`, `stdlib/src/{algorithms,encoding,text,path}.spectra` (T-12),
  `docs/reference/{05-stdlib.md,06-referencia-rapida.md}`, `docs/diagnostics/error-code-reference.md`,
  `docs/language-feature-maturity.md`, `docs/architecture/stdlib-source-migration.md`.
- **Generated / output-only (nunca editar):** `packages/spectra-contract/catalog/stdlib.toml`,
  `midend/src/lowering_std_host_*.rs`, `midend/src/lowering_std_api.rs`.
- **Files to avoid:** `backend/src/aot.rs` e demais arquivos com drift de
  `rustfmt` pré-existente; `packages/spectra-api/**`;
  `runtime/src/stdlib/{tensor_*,ml_*,serve_*}`.
- **Source of truth:** catálogo tipado (símbolos), lexer/parser/AST (operadores),
  ledger (forma de implementação por módulo).
- **Read path:** `stdlib/src` → bundle embutido → semântica → catálogo → docs/ledger;
  operadores: fonte → lexer → parser → semântica → IR → backend.
- **Write path:** `.spectra` → pipeline normal; primitiva nativa → catálogo →
  tabela de lowering gerada + registro de host call.
- **Contract boundary:** mudança de linguagem exige fixture em `tests/validation`
  **e** execução JIT+AOT (Core Language Correction Rule); símbolo novo exige
  probe + doc + catálogo; descritores só nascem do catálogo.
- **Migration/cutover:** cada tarefa converte seus consumidores internos
  (fixtures/docs/helpers) antes de remover o caminho antigo.
- **Acceptance evidence gate:** §Acceptance Evidence; nada é marcado concluído
  sem `--require-catalog` verde + fixtures JIT+AOT + baseline gravado.

**Plan Review Gate:** Requires PRE review before execution — **PRE executada**
(`Wave2PreReview`, veredito `partially aligned`); disposição em §PRE review
disposition.

---

## PRE review disposition

| # | Achado (severidade) | Correção aplicada |
|---|---|---|
| 1 | T-01 largo demais e sem todos os donos de dispatch/eval (major) | T-01 dividida em T-01a (frontend/token), T-01b (semântica + const eval), T-01c (IR/backend + fold), T-01d (docs/matriz/fixtures) com varredura obrigatória de matches exaustivos |
| 2 | Paralelismo de Waves 2–3 conflita em manifest/catálogo/docs/ledger (major) | Paralelismo restrito a **módulo-fonte + fixture**; um único *integration owner* (tooling) serializa manifest → catálogo → docs → ledger → gate da onda |
| 3 | T-02 listava arquivo gerado como editável (major) | Tabela de lowering marcada *output-only*; ordem explícita: export semântico → registro runtime → `generate_stdlib_catalog.py` → `generate_lowering_tables.py` → `--check` |
| 4 | `is_valid_utf8(text)` e NUL em `from_bytes` indefinidos (major) | API ajustada para bytes (`valid_utf8_bytes(List<int>)`), política de NUL explícita (`from_bytes` → `None`) e casos de fixture |
| 5 | Contrato de alvo citava `assert_eq`/`package test` sem API nem prova (major) | API tipada completa (`assert_eq_int/str/bool/f`, `assert_true/false`, `fail`) + prova em `spectralang package test` com projeto de falha esperada |
| 6 | APIs de T-20–T-28 sem assinatura/contrato (major) | §Wave 2 agora traz tabela com assinatura, invariantes e bordas por função |
| 7 | T-30 (CSV) sem contrato de saída/erro (major) | §T-30 define assinaturas, escrita canônica e política de citação malformada |
| 8 | T-31 (diff) sem formato estável (major) | §T-31 define formato exato, contexto, marcação e casos |
| 9 | Contagem de tarefas inconsistente; T-37 ambíguo (major) | 10 expansões (inclui `std.encoding`) + 6 módulos novos; T-37/stretch removido para a Onda 3; faixa de fixtures 808..828 corrigida |
| 10 | Cutover prometia remover workarounds não nomeados (minor) | Cutover nomeia `collatz_steps` e a extração de bits do base64 com as fixtures de regressão |
| 11 | T-03 não cobria semântica/lint que compartilham o orçamento (minor) | T-03 passa a cobrir parser + semântica + lint com helper compartilhado e regressão em thread de 1 MiB |

---

## Waves e tarefas

Convenções: owner `runtime` para módulos-fonte; `frontend`/`semantic`/`midend`/
`backend` para linguagem; `tooling` para integração (manifest/catálogo/docs/ledger
e gate). Fixtures novos: `tests/validation/808..828`, `tests/errors/720`.

**Modelo de paralelismo (Waves 1–3):** um worker por módulo escreve **apenas**
`stdlib/src/<módulo>.spectra` e `tests/validation/<N>_*.spectra`. O integration
owner coleta os módulos concluídos e atualiza, em série:
`scripts/stdlib_contract.toml` → `generate_stdlib_catalog.py` →
`generate_lowering_tables.py` (quando houver primitiva nova) →
`docs/reference/05-stdlib.md` → ledger → roda o gate completo da onda
(`check/run/AOT` de todos os fixtures + `--require-catalog`). Nenhum worker
toca arquivos compartilhados.

### Wave 0 — Pré-requisitos de linguagem

**T-01a — Frontend do bitwise/shift (tokens e AST)**
- **Owner:** frontend.
- **Arquivos:** `compiler/src/token.rs` (`Operator::{BitAnd,BitOr,BitXor,Shl,Shr,Tilde}` + `Display`),
  `compiler/src/lexer/mod.rs` (tabela de 2 caracteres: `<<`, `>>`, `&`, `|`, `^`, `~`; `is_symbol_char` ganha `^` e `~`),
  `compiler/src/ast/mod.rs` (`BinaryOperator::{BitAnd,BitOr,BitXor,Shl,Shr}`, `UnaryOperator::BitNot`),
  `compiler/src/parser/expression_precedence.rs` (níveis de precedência: shift > relacional >
  igualdade > `&` > `^` > `|` > `&&` > `||`; `~` no caminho unário).
- **Output:** `--dump-ast` mostra os operadores; nenhum diagnóstico novo nesta fatia.
- **Verificação:** `cargo test -p spectra-compiler`; `spectralang check` do fixture 808 (deve falhar
  por semântica ainda ausente — esperado até T-01b).
- **Paralelo:** com T-02/T-03 (arquivos distintos).

**T-01b — Semântica e avaliação constante do bitwise/shift**
- **Owner:** semantic.
- **Arquivos:** `compiler/src/semantic/semantic_expression_binary.rs` (operandos inteiros:
  `int`, larguras exatas e `char` com cast explícito; E036/E038 reutilizados),
  `compiler/src/semantic/semantic_expression_inference.rs` (tipo de resultado = tipo unificado,
  sem promoção a float), `compiler/src/semantic/semantic_tensor_const.rs` (fold bitwise/shift
  em `const`; shift mascara a contagem), `compiler/src/semantic/mod.rs` (mapa
  `operator_trait_and_method` — documentar que bitwise **não** é overloadável),
  `docs/diagnostics/error-code-reference.md` (semântica das mensagens reutilizadas).
- **Varredura obrigatória:** `grep -rn "BinaryOperator::" compiler/src midend/src backend/src tools`
  e tratar **todos** os `match` exaustivos restantes (inclusive `lsp`, lint e const-eval).
- **Output:** diagnóstico estável para operando não-inteiro (`tests/errors/720`).
- **Verificação:** `cargo test -p spectra-compiler`; `spectralang check tests/errors/720` → 65.

**T-01c — IR, backend e fold do midend**
- **Owner:** midend + backend.
- **Arquivos:** `midend/src/ir.rs` (`InstructionKind::{BitAnd,BitOr,BitXor,Shl,Shr}` com `unsigned`
  para `Shr`), `midend/src/builder.rs` (builders), `midend/src/lowering_expr_binary.rs`,
  `midend/src/lowering.rs` (dispatch), `midend/src/lowering_impl_core.rs` (`eval_const_binary`),
  `midend/src/lowering_impl_blocks.rs` e `lowering_impl_functions.rs` (fold/tipos),
  `midend/src/passes/verification.rs` (`binary_operand_pair` já cobre o par; adicionar os
  opcodes ao mapa de nomes), `backend/src/codegen_instruction_arithmetic.rs`
  (`band`/`bor`/`bxor`/`ishl`/`sshr`/`ushr`, com redução/extensão coerente com largura exata).
- **Semântica (documentar em T-01d):** `int`/larguras exatas; contagem de shift mascarada
  para a largura; `>>` aritmético em signed e lógico em unsigned; `~x` = complemento na largura.
- **Verificação:** `cargo test -p spectra-midend -p spectra-backend`.

**T-01d — Documentação, matriz de maturidade e fixtures 808/720**
- **Owner:** frontend + ecosystem.
- **Arquivos:** `docs/reference/06-referencia-rapida.md` (tabela de operadores),
  `docs/reference/05-stdlib.md` (nota), `docs/language-feature-maturity.md` (entrada estável com
  limites), `tests/validation/808_core_bitwise_operators.spectra`,
  `tests/errors/720_core_bitwise_non_integer_operand.spectra`.
- **Fixture 808:** cada operador em `const` e runtime; `int`, `i8..u64`; `u64` acima de 2^63;
  shift mascarado (`x << 64` ≡ `x << 0`); `~`; precedência (`a | b & c`, `a << 2 + 1`);
  combinação com `char` via cast.
- **Verificação:** `spectralang run 808` → 0; `compile --emit-exe` + execução → 0; `check 720` → 65.

**T-02 — Primitiva `std.error.panic(message: string)`**
- **Owner:** runtime + semantic (+ tooling para regenerar).
- **Ordem obrigatória:** 1) export em `compiler/src/semantic/builtin_text_system.rs`
  (`make_std_error`, retorno `unit`); 2) implementação em
  `runtime/src/stdlib/error.rs` (escreve `error: <message>` em stderr e aborta com exit **70**)
  + constante em `stdlib_bindings.rs` + registro em `registration.rs`; 3) `python
  scripts/generate_stdlib_catalog.py`; 4) `python scripts/generate_lowering_tables.py`;
  5) `--check`. A tabela `midend/src/lowering_std_host_math_io_error.rs` é **output-only**.
- **Fixtures:** `tests/validation/809_error_panic_exit_code.spectra` + sidecar `.exit` = `70`
  (o gate de execução compara stdout/exit); teste Rust em `runtime/src/stdlib/tests.rs`.
- **Verificação:** `cargo test -p spectra-runtime`; `run 809` → 70; AOT → 70;
  `spectralang package test` num projeto com `panic` → 70 (ver T-35).
- **Paralelo:** com T-01a/T-03.

**T-03 — Orçamento de stack relativo ao stack real (parser + semântica + lint)**
- **Owner:** frontend + semantic + tooling(lint).
- **Arquivos:** `compiler/src/parser/mod.rs` (helper de limites de stack com `cfg(windows)`/
  `cfg(unix)` e fallback conservador de 512 KiB; fração conservadora do stack disponível),
  `compiler/src/semantic/semantic_init.rs`, `compiler/src/lint/mod.rs` (mesma fonte de orçamento).
- **Escopo:** input patológico segue falhando com `P013`; nesting legítimo de ~30 níveis em
  thread de 1 MiB passa nos três consumidores.
- **Fixtures/testes:** `tests/validation/810_core_deep_nesting_parse.spectra`; testes Rust que
  parseiam/analisam/lintam em thread de 1 MiB; testes de `P013` existentes preservados.
- **Verificação:** `cargo test -p spectra-compiler`; `run 810` e AOT → 0;
  `cargo test -p spectra-api --test contract_drift`.
- **Paralelo:** com T-01a/T-02.

**T-04 — Contrato de bytes (L-4)**
- **Owner:** ecosystem.
- **Criar:** `docs/architecture/stdlib-bytes-contract.md`; referência em
  `docs/reference/05-stdlib.md` §17.
- **Decisões registradas:** strings são UTF-8 NUL-terminadas; bytes arbitrários são
  `List<int>`; `std.bytes` é a ponte; `from_bytes` rejeita NUL, valores fora de `0..=255`,
  overlong, surrogates e truncados; tipo `bytes` fica para a Onda 3 se houver demanda.

### Wave 1 — Camada fundacional de fonte

**T-10 — `std.unicode` (fixture 811)**
- **API (contratos completos):**
  - `rune_count(text: string) -> int` — conta scalar values (bytes de continuação não contam).
  - `rune_at(text: string, index: int) -> Option<int>` — code point do rune `index`; `None` fora do intervalo.
  - `byte_offset(text: string, rune_index: int) -> Option<int>` — offset do início do rune.
  - `from_codepoints(codes: List<int>) -> Option<string>` — `None` para qualquer código inválido
    (negativo, > 0x10FFFF, surrogate) ou **NUL** (política T-04).
  - `to_codepoints(text: string) -> List<int>` — um code point por rune.
  - `valid_utf8_bytes(bytes: List<int>) -> bool` — validação estrita de uma lista de bytes
    (overlong, surrogate, > U+10FFFF, truncado, valor fora de `0..=255`).
  - `slice_runes(text: string, start: int, end: int) -> Option<string>` — recorte por índice de
    rune, `None` fora do intervalo, sem re-codificar.
- **Fixture 811:** ASCII, 2/3/4 bytes, emoji, `valid_utf8_bytes` com overlong (`C0 80`),
  surrogate (`ED A0 80`), fora do máximo (`F4 90 80 80`), truncado (`C3`), NUL; `slice_runes`
  em fronteiras e fora delas.

**T-11 — `std.bytes` (fixture 812)**
- **API:**
  - `to_bytes(text: string) -> List<int>` — bytes UTF-8, cada um em `0..=255`.
  - `from_bytes(bytes: List<int>) -> Option<string>` — `None` para NUL, valor fora de `0..=255`
    ou sequência não-UTF-8 (mesma validação de `valid_utf8_bytes`).
  - `byte_at(text: string, index: int) -> Option<int>` — byte no offset, `None` fora do intervalo.
  - `is_ascii(text: string) -> bool` — todos os bytes < 0x80.
- **Fixture 812:** round-trip com NUL ausente, emoji, e rejeições (NUL, 256, −1, `C0 80`).

**T-12 — Dedupe: consumir `std.unicode`/`std.bytes` na Onda 1**
- **Owner:** runtime. **Arquivos:** `stdlib/src/{algorithms,encoding,text,path}.spectra`.
- **Displaced path:** deletar `collect_runes`, `rune_count`, `rune_at`, `push_rune`,
  `rune_length_at`, `decode_utf8` privados; `path.canonical` permanece (ASCII, independente).
- **Verificação:** fixtures 797–806 verdes em JIT+AOT; `grep` dos nomes removidos não retorna nada.

### Wave 2 — Expansões de módulos existentes (10 módulos)

Todas as funções abaixo têm contrato fechado: **None/None≠sentinela**, nunca mutam entradas,
e cada caso listado vira uma asserção no fixture.

| Tarefa | Módulo (fixture) | API e contratos |
|---|---|---|
| T-20 | `std.validate` (813) | `ean13_valid(text) -> bool` (mod 10, 13 dígitos); `card_brand(text) -> string` (`"visa"`,`"mastercard"`,`"amex"`,`"diners"`,`"discover"`,`"elo"`, `""` se desconhecido — Luhn não é exigido); `e164_valid(text) -> bool` (`+` e 7–15 dígitos); `pis_valid(text) -> bool` (11 dígitos, DV mod 11); `cnh_valid(text) -> bool` (9 dígitos + 2 DVs, rejeita todos iguais); `titulo_eleitor_valid(text) -> bool` (12 dígitos, DV mod 11, UF 01–28); `cnpj_alpha_valid(text) -> bool` (12 alfanuméricos + 2 DVs, valor `c - 48`) |
| T-21 | `std.iter` (814) | `zip_int(a, b) -> List<(int,int)>` (para no menor); `enumerate_int(values) -> List<(int,int)>` (índice, valor); `partition_int(values, pred) -> (List<int>, List<int>)`; `flatten_int(nested: List<List<int>>) -> List<int>`; `min_by_int`/`max_by_int(values, key: func(int) returns int) -> Option<int>`; `unique_int(values) -> List<int>` (primeira ocorrência, ordem preservada); `dedup_adjacent_int(values) -> List<int>` (sem mutar) |
| T-22 | `std.text` (815) | `title_case(value) -> string` (primeira letra ASCII de cada palavra); `split_words(value) -> List<string>` (só ASCII espaço/tab/nl/cr, sem lista vazia); `dedent(value) -> string` (remove a indentação comum não vazia); `center(value, width, pad: char) -> string` (pad equilátero, extra à direita); `jaro_winkler(a, b) -> float` (0.0–1.0, prefixo até 4, `p=0.1`); `truncate_middle(value, max_bytes) -> string` (mantém cabeça e cauda em fronteira de rune); `hard_wrap(value, width) -> List<string>` (quebra palavras longas); `normalize_newlines(value) -> string` (CRLF/CR → LF) |
| T-23 | `std.fmt` (816) | `float_scientific(value, decimals) -> Option<string>` (`1.5e3`, `decimals 0..=12`, `None` para NaN/infinito); `pad_center(value, width, pad: char) -> Option<string>`; `human_duration(millis) -> string` (`1h 02m 03s`, negativo com sinal); `percent(value, decimals) -> Option<string>` (`0.256` → `25.6%`) |
| T-24 | `std.path` (817) | `with_extension(path, ext) -> string` (substitui/adiciona, canônico `/`); `components(path) -> List<string>` (sem `.`/`..`); `is_relative(path) -> bool` (inverso de `is_absolute`); `glob_match(pattern, text) -> bool` (`*`, `?`, `[abc]`, `[!abc]`, sem `**`); `sanitize(name) -> string` (remove controle e `<>:"/\|?*`, colapsa espaços); `relative_to(path, base) -> Option<string>` (`None` se `base` não for prefixo de componentes) |
| T-25 | `std.stats` (818) | `mode_f(values) -> Option<float>` (menor valor em empate); `zscore_f(values, value) -> Option<float>` (`None` se σ=0 ou lista vazia); `histogram_f(values, bins) -> Option<List<int>>` (`bins ≥ 1`, largura `(max-min)/bins`, último bin inclui o máximo); `moving_average_f(values, window) -> Option<List<float>>` (janela completa apenas, `window ≥ 1`); `linear_regression_f(xs, ys) -> Option<(float,float)>` (slope, intercept; `None` se <2 pontos, tamanhos diferentes ou xs constante); `entropy_f(values) -> Option<float>` (Shannon base 2 sobre proporções, `None` para vazio ou soma 0) |
| T-26 | `std.calendar` (819) | `parse_iso_datetime(text) -> Option<int>` (aceita `YYYY-MM-DD[T ]HH:MM[:SS][Z]`, UTC); `format_rfc3339_utc(unix_secs) -> string` (`YYYY-MM-DDTHH:MM:SSZ`, anos negativos como em `format_iso_date`); `add_months(unix_secs, months) -> Option<int>` (`None` se o dia não existir no mês destino); `start_of_month(unix_secs) -> int`; `start_of_week(unix_secs) -> int` (**segunda-feira** como primeiro dia, coerente com ISO); `days_between(later, earlier) -> Option<int>` (`None` se `later < earlier`) |
| T-27 | `std.semver` (820) | `satisfies(text, range) -> Option<bool>` — `None` se o texto ou o range forem inválidos; ranges: exato (`1.2.3`), `=`, `^`, `~`, `x`/`*`, comparadores (`>=`, `<`), hyphen (`1.2.3 - 2.0.0`), união por `||`; **política de prerelease:** um texto com prerelease só satisfaz um range que contenha um comparador com prerelease no mesmo `major.minor.patch` (regra npm), documentada em `05-stdlib.md` |
| T-28 | `std.algorithms` (821) | `binary_search_string(sorted, target) -> int`; `kmp_find(haystack, needle) -> int` (`-1` se ausente, `""` → 0); `lcs(a, b) -> string` (runes, determinístico: escolhe o caminho com menor índice); `damerau_levenshtein(a, b) -> int` (transposição adjacente custa 1); `mod_pow(base, exponent, modulus) -> Option<int>` (`None` para expoente negativo ou `modulus ≤ 0`); `integer_sqrt(value) -> Option<int>` (`None` para negativo, truncado para baixo); `chinese_remainder(remainders, moduli) -> Option<int>` (`None` para tamanhos diferentes, módulo ≤ 1 ou sistema inconsistente) |
| T-33 | `std.encoding` (825) | `base32_encode/decode` (RFC 4648, padding, `None` para alfabeto/padding inválido ou não-UTF-8); `base58_encode/decode` (Bitcoin, sem confundíveis); `base85_encode/decode` (Ascii85 sem `<~ ~>`) — **e** conversão interna do base64 para shift/mask (Cutover) |

Cada T-20…T-33 entrega: `stdlib/src/<módulo>.spectra` (worker) + fixture (worker);
o integration owner adiciona nada ao manifest além do probe do módulo quando
necessário (namespaces já existem para todos), regenera catálogo, atualiza docs
§+ledger e roda o gate da onda.

### Wave 3 — Módulos novos (6)

| Tarefa | Módulo (fixture) | API e contratos |
|---|---|---|
| T-30 | `std.csv` (822) | `parse(text) -> Option<List<List<string>>>` — RFC 4180: `"..."` com `""` escapado; separadores de registro CRLF/LF/CR; último registro sem newline; `None` para aspas não fechadas; texto vazio → lista vazia. `parse_row(text) -> Option<List<string>>` — um único registro (newline dentro de aspas é conteúdo). `write(rows: List<List<string>>) -> string` — cita o campo **se** contiver `,` `"` CR ou LF, escapa `"` como `""`, une com `,`, termina cada registro com `\n` (LF canônico). Fixture: aspas, newline/comma embutidos, CRLF, campo vazio, `""`, round-trip `write→parse`, aspas aberta → `None` |
| T-31 | `std.diff` (823) | `same_lines(left, right: List<string>) -> bool`; `diff_lines(left, right: List<string>) -> List<string>` — formato unificado com **3 linhas de contexto**: cabeçalho `@@ -a,b +c,d @@` (1-based; `b`/`d` = contagem de linhas no hunk), linhas prefixadas ` ` (contexto), `-` (só em `left`), `+` (só em `right`) — sem cabeçalho de arquivo, linhas sem terminador, alteração = `-` seguido de `+`, hunks separados quando a distância > 6 linhas. Fixture: iguais → lista vazia, inserção, remoção, substituição, vazio vs não-vazio, dois hunks, linha final sem newline |
| T-32 | `std.vector` (824) | `dot_f(a, b) -> Option<float>` (`None` para tamanhos diferentes); `norm_f(a) -> float`; `cosine_f(a, b) -> Option<float>` (`None` para tamanhos diferentes ou norma 0); `normalize_f(a) -> Option<List<float>>` (`None` para norma 0); `add_f(a, b) -> Option<List<float>>`; `scale_f(a, k) -> List<float>`. Documentar que não substitui `std.tensor` (sem handles/GPU/autodiff) |
| T-34 | `std.uuid` (826) | `is_valid(text) -> bool` (8-4-4-4-12, hex, hífens, case-insensitive); `parse(text) -> Option<List<int>>` (16 bytes); `format(bytes) -> Option<string>` (minúsculo canônico, `None` se `len ≠ 16` ou valor fora de `0..=255`); `version(text) -> Option<int>` (dígito da versão, `None` se inválido); `v4() -> string` (híbrido via `std.random` — 122 bits aleatórios, variante RFC 4122) |
| T-35 | `std.testing` (827 + projeto) | `assert_true(cond: bool, message: string)`, `assert_eq_int(actual, expected: int, message: string)`, `assert_eq_str(actual, expected: string, message: string)`, `assert_eq_bool`, `assert_eq_f(actual, expected, tolerance: float, message: string)`, `fail(message: string)` — todos usam `std.error.panic` e imprimem `expected <e>, found <a>` + mensagem. Fixture 827: asserções que passam → exit 0. Prova de falha: `tests/projects/invalid/stdlib_testing_assertion_failure/` (`spectra.toml`, `src/lib.spectra`, `tests/failure.spectra` com `assert_eq_int(3, 4, …)`) validado por `spectralang package test --root <dir>` com exit ≠ 0 e mensagem contendo `expected 4, found 3` |
| T-36 | `std.hash` (828) | `fnv1a64_str(text) -> int` (64 bits sem sinal num `int`; offset basis `0xcbf29ce484222325`, primo `0x100000001b3`, sobre bytes); `djb2_str(text) -> int`; `crc32(text) -> int` (polinômio `0xEDB88320` refletido, resultado `int` 0..=0xFFFFFFFF). Fixture: vetores conhecidos (`""` FNV = offset basis; `"a"`; `"123456789"` CRC-32 = `0xCBF43926`), comparados com constante |

### Wave 4 — Fechamento

**T-40 — Baseline, gates e sincronização**
- `python scripts/validate_execution_coverage.py --binary target/debug/spectralang.exe --mode both --update-baseline`
  em máquina ociosa, com revisão do diff de `tests/execution-baseline.json` (inclui a
  pendência da Onda 1).
- `python scripts/validate_r3007_stdlib_contract.py --binary target/debug/spectralang.exe --require-catalog`
  → `blockers: 0`; `python scripts/generate_lowering_tables.py --check`;
  `cargo test --workspace --all-targets --no-fail-fast`; `cargo test -p spectra-api --test contract_drift`;
  `rustfmt --check`/`clippy` nos arquivos tocados (sem novas advertências).
- Atualizar `docs/architecture/stdlib-source-migration.md`, `docs/reference/{05-stdlib.md,06-referencia-rapida.md}`,
  `docs/language-feature-maturity.md` (operadores + `panic` na matriz) e — se o modo roadmap
  estiver ativo — abrir `R-3419`+ no `roadmap.toml`/backlog apontando cada `[[namespace]]`.

---

## Gate padrão por módulo (inalterado desde a Onda 1)

1. `cargo build -p spectra-cli`
2. `check` + `run` do fixture → exit 0 (bordas com código próprio)
3. AOT: `compile --emit-exe` + execução → mesmo exit
4. `validate_execution_coverage --mode both` → fixture novo `NEW … passes`
5. `[[namespace]]` + `[[probe]]` no manifest (integration owner)
6. `generate_stdlib_catalog.py` → `implementation = "spectra-source"`, `abi = "compiled Spectra function"`, `fixture` dedicado
7. `validate_r3007_stdlib_contract.py --require-catalog` → 0 blockers
8. seção em `docs/reference/05-stdlib.md` + linha no ledger

## Riscos e critérios de parada

| Risco | Mitigação |
|---|---|
| Operadores bitwise mudam a superfície da linguagem | semântica fechada por fixture (largura exata, shift mascarado, sinal, precedência), `explain`/docs/matriz atualizados, varredura de matches exaustivos em T-01b |
| `panic` com semântica de saída ambígua | exit 70 documentado e testado em JIT, AOT e `package test` |
| Novo orçamento de stack afrouxar a proteção | testes de `P013` preservados + regressão em thread de 1 MiB para parser/semântica/lint |
| Módulo grande (csv/diff/hash) estourar o teto de aninhamento | código achatado por função (lição F-09) e fixtures com timeout |
| Conflito de escrita em arquivos compartilhados | modelo de paralelismo com integration owner único e gate por onda |
| Catálogo/manifest/descritores divergirem | ordem fixa (semântica → runtime → catálogo → lowering) + `--check` no gate |
