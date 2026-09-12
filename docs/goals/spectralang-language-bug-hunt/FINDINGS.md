# Findings: SpectraLang Language Bug Hunt

## Baseline

- Branch: `fix/oop-layout-and-diagnostics`
- Binário: `target/debug/spectralang.exe`
- Gates R-207 a R-213: aprovados antes da expansão.
- `cargo test -p spectra-compiler`: aprovado.
- `cargo test -p spectra-midend`: aprovado.
- `cargo test -p spectra-backend`: aprovado.
- `python scripts/validate_test_pyramid.py`: aprovado.
- `git diff --check`: aprovado antes dos novos arquivos.

## Ledger

| Fixture | Categoria | Comando | Resultado | Fase | Classificação | Próxima ação |
|---|---|---|---|---|---|---|
| `tests/validation/258_oop_layout_permutations.spectra` | layout com scalars mistos e struct aninhado | `target/debug/spectralang.exe check/run ...` | check e execução terminam com código 0 | backend/codegen | `PASS`, R-214 validado | manter regressão |
| `tests/validation/259_oop_drop_nested_lifetimes.spectra` | `Drop` em campo aninhado e valor retornado | `target/debug/spectralang.exe check/run ...` | três execuções imprimem os três drops esperados e terminam com código 0 | lowering/runtime lifecycle | `PASS`, R-215 validado | manter regressão |
| `tests/validation/260_oop_dyn_aggregate_overwrite.spectra` | `dyn Shape` entre campos `char` e `bool` | `target/debug/spectralang.exe check/run ...` | check e execução terminam com código 0 | backend/codegen ABI | `PASS`, R-214 validado | manter regressão |
| `tests/validation/261_oop_vtable_inheritance_defaults.spectra` | herança de trait, métodos default e dispatch dinâmico | `target/debug/spectralang.exe check/run ...` | chamadas concretas e dyn terminam com código 0 | midend/backend vtable | `PASS`, R-218 validado | manter regressão |
| `tests/validation/262_oop_ufcs_receiver_matrix.spectra` | UFCS direto, default e `dyn` | `target/debug/spectralang.exe check/run ...` | matriz de receivers termina com código 0 | backend/codegen ABI | `PASS`, R-218 validado | manter regressão |
| `tests/validation/263_oop_generic_struct_multiarg.spectra` | struct genérico com dois parâmetros e aggregate aninhado | `target/debug/spectralang.exe check/run ...` | especializações e campos terminam com código 0 | semantic specialization | `PASS`, R-216 validado | manter regressão |
| `tests/validation/264_oop_generic_trait_substitution.spectra` | substituição de dois parâmetros de trait em UFCS | `target/debug/spectralang.exe check/run ...` | chamadas concretas e UFCS terminam com código 0 | semantic UFCS | `PASS`, R-216 validado | manter regressão |
| `tests/validation/265_oop_control_flow_composition.spectra` | trait mutável em `loop`, `do-while` e `switch` | `target/debug/spectralang.exe run ...` | check e execução terminam com código 0 em três tentativas | frontend/midend/backend | `PASS`, reproduzido 3/3 | manter como cobertura positiva |
| `tests/validation/266_oop_aot_aggregate.spectra` | aggregate aninhado em JIT, objeto e executável AOT | `target/debug/spectralang.exe compile ... --emit-exe ...` | check/run, emissão de objeto, link e execução do `.exe` terminam com código 0 | backend/AOT ABI | `PASS`, reproduzido no JIT e AOT | manter como cobertura positiva e repetir AOT no gate de release |
| `tests/validation/267_closure_oop_capture.spectra` | closure capturando aggregate e chamando método | `target/debug/spectralang.exe run ...` | check/run terminam com código 0 em três tentativas | frontend/midend/backend closure ABI | `PASS`, reproduzido 3/3 | manter como cobertura positiva |
| `tests/validation/268_pattern_generic_enum_oop.spectra` | construtores de enum genérico com payload record | `target/debug/spectralang.exe check/run ...` | `Item`, `Tagged` e `Empty` preservam `Packet_Score` e terminam com código 0 | semantic generic inference | `PASS`, R-216 validado | manter regressão |
| `tests/validation/269_async_trait_generic_task.spectra` | método async de trait genérico concretizado | `target/debug/spectralang.exe run ...` | check/run terminam com código 0 em três tentativas | async/semantic/midend | `PASS`, reproduzido 3/3 | manter como cobertura positiva |
| `tests/validation/270_import_alias_oop_boundary.spectra` | alias de stdlib dentro de impl inerente | `target/debug/spectralang.exe run ...` | check/run terminam com código 0 em três tentativas | semantic/module aliases | `PASS`, reproduzido 3/3 | manter como cobertura positiva |
| `tests/validation/271_stdlib_aggregate_lifecycle.spectra` | records, `Drop`, collections e stdlib string/io | `target/debug/spectralang.exe run ...` | código 0 e dois drops observados em três tentativas | runtime lifecycle/stdlib | `PASS`, reproduzido 3/3 | manter como cobertura positiva |
| `tests/validation/272_tensor_trait_lifecycle.spectra` | tensor estático em método de trait concreto e `dyn` | `target/debug/spectralang.exe run ...` | check/run terminam com código 0 em três tentativas | numerics/OOP ABI | `PASS`, reproduzido 3/3 | manter como cobertura positiva |
| `tests/validation/273_api_handler_async_trait.spectra` | `AsyncHandler` dinâmico com `await` | `target/debug/spectralang.exe check/run ...` | contrato `Task<Response>` passa check e execução termina com código 0 | web/async lowering | `PASS`, R-2113 validado | manter regressão |
| `tests/validation/274_exact_width_aggregate_boundary.spectra` | `i8/i16/u8/u16/f32/f64` em record com trait/dyn | `target/debug/spectralang.exe check/run ...` | campos exact-width e dyn terminam com código 0 | backend aggregate ABI | `PASS`, R-214 validado | manter regressão |
| `tests/projects/valid/oop_cross_module_dispatch` | inherent, trait genérico e `dyn` entre módulos | `target/debug/spectralang.exe check/run ...` | generic export, impl importado e dyn dispatch terminam com código 0 | project/module registry + codegen | `PASS`, R-217/R-219 validados | manter projeto multifile |
| `tests/errors/oop_module_qualified_unknown_type.spectra` | impl inerente com alvo `module::Type` inexistente | `target/debug/spectralang.exe check ... --json` | retorna `success=false`, `exit=65` e código `E027` | semantic/module resolution | `PASS`, R-219 validado | manter negativo de diagnóstico |

Os casos serão adicionados uma linha por vez. Falhas de compilação, crashes,
timeouts e resultados incorretos devem preservar o output relevante em uma
seção própria abaixo antes de qualquer promoção ou correção.

## Reproductions

### 258: mixed scalar layout reaches a codegen verifier failure

O programa é aceito pelo frontend e o IR mostra offsets distintos para os
campos: `flag@0`, `code@4`, `narrow@8`, `wide@16`, `count@24` e `label@32`.

As três execuções produziram o mesmo resultado:

```text
error[codegen]: Failed to define function 'Mixed_checksum': Compilation error: Verifier errors
exit=65
```

O caso ainda não foi promovido para `tests/validation/` porque o comportamento
esperado é executar com sucesso. Nenhuma correção de produção foi feita.

### 259: nested and returned Drop values are not destroyed

O fixture cria três `Token` com `Drop`: um local direto, um dentro de
`Bundle` e um retornado por `make_returned`. As três execuções terminaram com
`exit=0`, mas produziram somente:

```text
[R-259 drop] 1:local
```

Os drops `2:bundle` e `3:returned` não foram observados. O caso permanece em
`tests/regressions/pending/`.

### 260: dyn aggregate with mixed scalar fields fails codegen verification

O mesmo tipo `dyn Shape` usado com sucesso no fixture R-210 foi colocado entre
um `char` e um `bool` em `Holder`. O frontend aceita a fonte, mas todas as
execuções terminam com:

```text
error[codegen]: Failed to define function 'main': Compilation error: Verifier errors
exit=65
```

O caso permanece pendente para preservar a interação entre fat pointer e
layout de aggregate.

### 261: inherited trait dispatch is wrong through a dynamic receiver

O fixture verifica primeiro `base`, `extended` e `label` no valor concreto
`Item`; essas chamadas passam. Em seguida repete as três operações através de
`dyn Extended`. As três execuções terminaram com código 2, indicando que
`dynamic.base()` não retornou os 40 esperados. Não houve erro de compilação ou
crash; o caso permanece pendente para investigar a montagem da vtable herdada.

### 262: receiver matrix reaches a codegen verifier failure

O fixture combina chamadas concretas, UFCS, método default, `dyn` e UFCS sobre
`dyn`. O frontend aceita a fonte, mas as três tentativas de `run` terminam com:

```text
error[codegen]: Failed to define function 'main': Compilation error: Verifier errors
exit=65
```

O caso permanece pendente porque não há resultado executável para comparar os
oito caminhos de receiver.

### 263: multi-argument generic aggregate lookup is incomplete

O construtor de `Envelope<int, string>` é aceito, porém os acessos
`envelope.marker` e `envelope.payload.first` falham nas três tentativas com:

```text
error[semantic]: Struct 'Envelope_int_string' is not defined
```

O resultado é determinístico e o caso permanece pendente.

### 264: generic trait arguments are lost in UFCS return types

As chamadas por método de `IntText` e `TextFlag` são aceitas. As chamadas UFCS
`PairView::first/second(...)` são analisadas como retornando os parâmetros
abstratos `T` e `U`; as três tentativas rejeitam as quatro comparações com os
tipos concretos (`int`, `string` e `bool`).

### 265: OOP mutation composes with stable control flow

`check` passou e `run` terminou com código 0 nas três tentativas. O caso foi
promovido para `tests/validation/`.

### 266: nested aggregate survives the AOT path

O caso passou no JIT (`check` e `run`, código 0), na emissão de objeto e na
emissão de executável. O executável gerado em `target/bug-hunt/r266.exe`
terminou com código 0. Os artefatos ficam fora da árvore versionada; a fonte
foi promovida para `tests/validation/`.

## Cross-feature reproductions

### 268: generic enum constructors lose the expected specialization

`Packet::Item(Score { ... })` is accepted as `Packet_Score`, but the named
payload variant and the unit variant are inferred as bare `Packet` when passed
to `score(Packet<Score>)`. The three checks report:

```text
error[semantic]: Argument 1 of function 'score' has type Packet, expected Packet_Score
```

O fixture permanece pendente para separar inferência pelo payload de inferência
pelo tipo esperado da chamada.

### 273: dynamic AsyncHandler loses its task result

O `AsyncHandler` implementado por `StatusHandler` compila até o lowering, mas
as três tentativas de `check` abortam com:

```text
error[internal]: await operand must lower to Task<T>, found Int
```

Isso contrasta com `AsyncWorker` dinâmico já existente e indica que o caminho
API não preserva a natureza async da chamada de trait.

### 274: exact-width aggregate float read returns the wrong branch

O check passa e o IR mostra os offsets `i8@0`, `i16@2`, `u8@4`, `u16@6`,
`f32@8` e `f64@16`. Ainda assim, as três execuções retornam código 1 na
comparação de `narrow`/`wide`, antes do checksum inteiro e do dispatch `dyn`.
O caso permanece pendente como resultado incorreto de backend/ABI.

### Multifile project: imported trait implementations are not uniformly visible

No projeto `oop_cross_module_dispatch`, o caminho apenas inerente e
`evaluate_runtime(Account)` passa isoladamente com `check`/`run` código 0.
Reintroduzindo a chamada genérica exportada, o backend emite:

```text
error[codegen]: Function 'evaluate' not found during backend code generation
```

Com o cast dinâmico presente, a análise semântica também emite:

```text
Cannot cast `Account` to `dyn Valued`: type `Account` does not implement trait `Valued`
```

As falhas foram reproduzidas três vezes no projeto completo. O projeto fica
intencionalmente em `tests/projects/valid/` para que a falha de integração não
seja convertida em uma expectativa positiva falsa.

## Negative coverage

Os seguintes candidatos foram executados com `check --json`, terminaram com
código 65 e foram promovidos para `tests/errors/` por representarem rejeições
esperadas. Os códigos observados permanecem na saída JSON e formam a cobertura
negativa desta rodada:

| Arquivo | Contrato exercitado | Resultado observado |
|---|---|---|
| `api_handler_wrong_response.spectra` | `Handler::call` deve retornar `Response` | `E023`, retorno `int` rejeitado |
| `async_oop_send_boundary.spectra` | `dyn Worker + Send` exige evidência `Send` | `E2104`, cast rejeitado |
| `oop_duplicate_impl_method.spectra` | método inerente duplicado | `E013`, duplicata rejeitada |
| `oop_dyn_generic_trait.spectra` | trait genérico sem argumentos não vira `dyn` | `E026`, cast rejeitado |
| `oop_generic_trait_arity.spectra` | aridade de impl genérico | `E025`, aridade rejeitada |
| `oop_generic_trait_parameter_mismatch.spectra` | parâmetro concreto do trait | `E023`, tipo rejeitado |
| `oop_generic_trait_return_mismatch.spectra` | retorno concreto do trait | `E023`, tipo rejeitado |
| `oop_inherent_impl_wrong_arity.spectra` | aridade de impl inerente | `E025`, aridade rejeitada |
| `oop_missing_trait_method.spectra` | método obrigatório sem default | `E016`, impl rejeitado |
| `oop_self_not_first_trait.spectra` | `self` como primeiro parâmetro | `E024`, declaração rejeitada |
| `oop_ufcs_missing_method.spectra` | UFCS para método inexistente | `E017`, chamada rejeitada |
| `oop_ufcs_nonimplementor.spectra` | UFCS sem implementação do trait | `E016`, chamada rejeitada |
| `pattern_generic_non_exhaustive.spectra` | exaustividade após substituição genérica | match não exaustivo |
| `tensor_oop_shape_mismatch.spectra` | shape estático em método OOP | `E1403`, `matmul` rejeitado |

### Negative acceptance bug: qualified inherent impl is not resolved

`oop_module_qualified_unknown_type.spectra` declara `impl missing::Foreign`
sem qualquer módulo ou tipo correspondente. O contrato negativo esperado era
um diagnóstico semântico, mas as três tentativas de `check --json` produziram:

```json
{"success":true,"diagnostics":[]}
```

O arquivo permanece em `tests/regressions/pending/negative/` como BUG. A
aceitação ocorre mesmo sem usar o método; a próxima triagem deve adicionar uma
instanciação/call mínima para distinguir apenas a validação ausente do alvo de
um problema posterior de lowering.

## Implementation map used for triage

Na triagem inicial não houve alteração Rust; a leitura dos pontos responsáveis restringiu
as hipóteses aos seguintes contratos:

- `compiler/src/parser/item.rs`: o parser coleta o prefixo `module::` de impl
  inerente, mas entrega o alvo separado para a fase semântica. O reproducer de
  tipo qualificado aceito indevidamente deve ser comparado com essa passagem.
- `compiler/src/semantic/mod.rs`: `validate_trait_impl` já substitui os
  argumentos concretos ao validar assinaturas, enquanto `types_match` ainda
  trata `TypeParameter` como compatível com qualquer tipo. Isso explica por que
  os negativos básicos são rejeitados, mas UFCS genérico e inferência de
  especialização continuam alvos distintos.
- `midend/src/lowering.rs`: especializações de métodos genéricos são acumuladas
  em `generic_impl_methods`/`pending_method_specializations`; o mesmo lowering
  valida que `await` recebe `Task<T>`. Os achados de `evaluate` intermodule e de
  `AsyncHandler` dinâmico passam por esse ponto.
- `backend/src/codegen.rs` e `midend/src/layout.rs`: `FieldPtr`, offsets de
  aggregate e `MakeDynFatPtr` formam a superfície comum aos verifier failures
  de 258/260/262, ao resultado incorreto de 274 e aos caminhos de vtable/dyn.

Essas associações foram hipóteses de triagem baseadas no código inicial. A
implementação abaixo registra as causas confirmadas somente quando a correção
passou pelo CLI e pela regressão pública correspondente.

## Corrections implemented

- `R-214`: coercion of exact scalar values, cumulative aggregate field access,
  dyn fat-pointer allocation/escape, and receiver forwarding were corrected in
  `midend/src/lowering.rs`, `backend/src/codegen.rs`, and related ABI paths.
- `R-215`: recursive drop/escape glue now tracks moved identifiers and nested
  aggregate ownership across scope, return, method, and lambda paths.
- `R-216`/`R-217`: generic record/enum/trait substitutions, imported generic
  templates, and cross-module specialized calls are preserved through AST,
  semantic registry, and lowering.
- `R-218`: inherited trait metadata is registered parent-first, so concrete,
  default, UFCS, and dyn calls share the same slots.
- `R-219`: exported trait declarations/impls are imported for downstream
  vtable construction; `impl missing::Foreign` now emits `E027`.
- `R-2113`: builtin API trait metadata exposes `AsyncHandler::call` as a
  `Task<Response>` contract, allowing dynamic `await` lowering.

The former pending sources `258`--`264`, `268`, and `273`--`274` were promoted
to `tests/validation/`; the qualified-target negative was promoted to
`tests/errors/oop_module_qualified_unknown_type.spectra`. The executable gate
is `scripts/validate_language_bug_hunt.py`.

## Final gates

- 17 positivos promovidos (`258`--`267`, `268`--`274`, com os casos existentes
  `265`--`267` e `269`--`272`): `check=0` e `run=0` no caminho final.
- R-266: emissão de objeto, link AOT e execução do executável final: `0/0/0`.
- 15 negativos promovidos: todos terminaram com `exit=65` e
  `success=false` no JSON; a matriz teve `expected_negative_failures=0`.
- `oop_module_qualified_unknown_type.spectra` agora retorna `E027`, com hint
  acionável; a aceitação indevida não permanece.
- `python scripts/validate_language_bug_hunt.py`: aprovado, incluindo três
  repetições do caso de lifetime e o projeto multifile.
- R-266 AOT: emissão do executável e execução final terminaram com `0/0`.
- Validadores `R-207`, `R-208`, `R-209`, `R-210`, `R-211`, `R-212` e `R-213`:
  todos `exit=0`.
- `python scripts/validate_test_pyramid.py`: aprovado.
- `cargo test -p spectra-compiler`, `cargo test -p spectra-midend` e
  `cargo test -p spectra-backend`: aprovados.
- `cargo test -p spectra-cli`: 44 testes unitários e 6 testes de integração
  aprovados.
- `git diff --check`: aprovado.

O worktree contém as correções Rust, regressões promovidas, o projeto multifile,
o validator e os artefatos de objetivo/roadmap desta implementação. Relatórios
gerados ficam em `target/` e não fazem parte da fonte versionada.
