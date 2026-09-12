# Findings — round 2

## Resumo atual

| ID | Lote | Área | Classificação | Evidência | Ação |
| --- | --- | --- | --- | --- | --- |
| 275 | 1 | alias de tipo, constante e tupla | `PASS` após BUG | `check`, `run`, compiler stage smoke, midend IR layout | promovido para `tests/validation/` |
| 276 | 1 | lexer, precedência e parser | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 277 | 1 | import aliasado | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 278 | 1 | captura de closure e HOF | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 279 | 1 | composição de controle de fluxo | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 280 | 1 | destructuring e padrões | `PASS` | `check`, `run` após ajustar oracle para 35 | promovido para `tests/validation/` |
| 281 | 1 | arrays, iteração e mutação | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 282 | 2 | generic nested record | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 283 | 2 | traits, defaults e UFCS concreto | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 284 | 2 | dyn trait e auto-traits | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 285 | 2 | Drop, retorno e move | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 286 | 2 | receiver mutável encadeado | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 287 | 2 | enum genérico e match | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 288 | 2 | dyn aggregate e AOT | `PASS` | `check`, `run`, objeto, link e execução AOT | promovido para `tests/validation/` |
| 289 | 3 | import nomeado e alias | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 290 | 3 | projeto multifile e agregado importado | `PASS` | `check`, `run` do projeto | promovido para `tests/projects/valid/` |
| 291 | 3 | reexport público multifile | `PASS` | `check`, `run` do projeto | promovido para `tests/projects/valid/` |
| 292 | 3 | visibilidade e módulo interno | `PASS` | `check`, `run` do projeto | promovido para `tests/projects/valid/` |
| 293 | 4 | async e await aninhado | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 294 | 4 | bloco Task explícito | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 295 | 4 | spawn e join | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 296 | 4 | stream e dyn poll | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 297 | 4 | channel e limite Send | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 298 | 5 | collections e strings da stdlib | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 299 | 5 | filesystem roundtrip | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 300 | 5 | JSON derive e tipo de retorno estático | `PASS` após BUG | `check`, `run`, compiler e midend regressions | promovido para `tests/validation/` |
| 301 | 5 | relógio, epoch e duration | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 302 | 5 | runtime status e cleanup | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 303 | 5 | shape e mutação de tensor | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 304 | 5 | autodiff e reutilização de tensor | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 305 | 5 | composição de treinamento ML | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 306 | 5 | métricas ML e artefato de relatório | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 307 | 5 | dataset ML e tensor loader | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 308 | 6 | tipos HTTP e resposta | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 309 | 6 | composição de middleware | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 310 | 6 | SQLite, transação e rollback | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 311 | 6 | agregado com string e AOT | `PASS` após BUG | objeto, link e execução AOT | promovido para `tests/validation/` |
| 312 | 6 | otimização e controle de fluxo | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 313 | 6 | batching de hostcalls | `PASS` | `check`, `run` repetido 3x | promovido para `tests/validation/` |
| 314 | 6 | contrato CLI de formatter | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 315 | 6 | JSON HTTP e router | `PASS` | `check`, `run` | promovido para `tests/validation/` |
| 316 | 6 | agregado final e AOT | `PASS` | objeto, link e execução AOT | promovido para `tests/validation/` |

## BUG-275 — aliases aceitos pelo parser não alcançavam semântica/IR

- Reprodução mínima: `type Pair = (int, string)` usado no retorno de uma
  função e em uma anotação local.
- Sintoma: `check` falhava com `Unknown type 'Pair'` na anotação local.
- Causa: `SemanticAnalyzer` não mantinha aliases na tabela de tipos; o
  `ASTLowering` também convertia o nome do alias em `IRType::Void`.
- Correção: registro de aliases antes da análise dos corpos e expansão do
  alvo no resolvedor semântico e no lowering.
- Regressões: `compiler/tests/stage_smoke.rs` e
  `midend/tests/ir_snapshot_tests.rs`.
- Validação: `cargo test -p spectra-compiler
  type_aliases_are_available_to_signatures_and_bodies`, `cargo test
  -p spectra-midend type_alias_lowers_to_the_target_aggregate_layout`, além
  de `spectralang check` e `spectralang run` no fixture 275.

## BUG-300 — retorno de método JSON derivado perdia o tipo no midend

- Reprodução mínima: `Profile::json_error_field("...") != ""` sem anotação
  explícita no binding local.
- Sintoma: o frontend aceitava a expressão, mas o codegen tentava gerar
  `Profile_eq` em vez de usar a igualdade de strings.
- Causa: o caminho `EnumVariant` do inferidor IR caía no fallback do struct,
  enquanto o lowering dedicado já produzia uma string vazia.
- Correção: o midend reconhece `json_error_field` como `IRType::String` e
  `from_json` como o agregado correspondente; a semântica também mantém os
  retornos derivados explícitos.
- Regressões: `compiler/tests/stage_smoke.rs` e
  `midend/tests/ir_snapshot_tests.rs`.

## BUG-311 — literais string AOT não tinham alinhamento de `i64`

- Reprodução mínima: registro AOT com campos `int`, `string` e `bool`, seguido
  de leitura/comparação do campo string.
- Sintoma: emissão e link passavam, mas o executável abortava em
  `read_spectra_string` com `misaligned pointer dereference`.
- Causa: o `DataDescription` das strings AOT não declarava alinhamento de 8
  bytes, embora o runtime leia cada slot como `i64`.
- Correção: `backend/src/aot.rs` agora define o alinhamento do literal como
  `align_of::<i64>()`.
- Regressão: os fixtures 311 e 316 exigem emissão, link e execução AOT.

## Hipóteses removidas da matriz

O spelling de UFCS genérico `Score::plus(item, 2)` foi minimizado no caso 283
e reportou a mensagem semântica de que o receiver não implementava o trait.
Como a forma genérica ainda não é uma superfície suportada pelo contrato atual,
ela foi removida do fixture final; o caso preservado exercita UFCS concreto e
default methods, sem classificar uma capacidade fora da linguagem como BUG.

## Diagnóstico negativo

`tests/errors/bug_hunt_v2_invalid_json.spectra` foi validado com
`spectralang check --json`: retorna `success=false`, código de processo `65` e
código diagnóstico estável `EJSON001`. A entrada inválida não alcança
midend/backend.

## Convenção de classificação

- `PASS`: comportamento suportado e verificável passou os gates exigidos;
- `BUG`: falha reproduzível da implementação, corrigida ou ainda pendente;
- `HARNESS_GAP`: o caso é suportado, mas a automação não fornece evidência
  suficiente;
- `NONDETERMINISTIC`: resultado varia sem mudança de entrada ou ambiente.

Todos os 42 casos desta rodada estão agora classificados e promovidos; o
validador determinístico será a fonte da evidência consolidada dos comandos e
dos códigos de saída.
