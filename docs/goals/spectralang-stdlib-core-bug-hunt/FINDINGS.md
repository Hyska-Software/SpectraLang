# Findings — stdlib principal

## Resultado por fixture

| Fixture | Área | Classificação final | Evidência | Resultado |
| --- | --- | --- | --- | --- |
| 317 | string | `PASS` | `check --json` + `run` ×3 | promovido |
| 318 | collections | `PASS` após `BUG` | verifier + regressão de lowering + `run` ×3 | promovido |
| 319 | math/convert | `PASS` | `check --json` + `run` ×3 | promovido |
| 320 | numeric | `PASS` após `HARNESS_GAP` | oracle de wrapping corrigido + `run` ×3 | promovido |
| 321 | io/fs/env | `PASS` após `BUG` | contratos Bool corrigidos + `run` ×3 | promovido |
| 322 | io input | `PASS` após `BUG` | hostcall ausente corrigido + stdin + `run` ×3 | promovido |
| 323 | option/result | `PASS` após `BUG` | tags, Bool e payload inferido corrigidos + `run` ×3 | promovido |
| 324 | char/range | `PASS` | `check --json` + `run` ×3 | promovido |
| 325 | random | `PASS` | seed determinístico + `run` ×3 | promovido |
| 326 | collections HOF | `PASS` | closures e HOF + `run` ×3 | promovido |
| 327 | concurrent | `PASS` após `BUG` | ABI Bool do fast path corrigido + `run` ×3 | promovido |
| 328 | serve | `PASS` após `HARNESS_GAP` | oracle de timeout/processing alinhado + `run` ×3 | promovido |
| 329 | time | `PASS` | relógio/duration/UTC + `run` ×3 | promovido |
| 293–297 | async/runtime | `PASS` | `check` + `run` ×3 | cobertura suplementar |

## Bugs corrigidos

### BUG-318 — `list_contains` declarava `Int` no lowering

O frontend aceitou o caso, mas o backend encontrou incompatibilidade entre o
resultado nativo e o tipo IR booleano durante a execução. O descriptor de
`std.collections.list_contains` foi corrigido para `IRType::Bool`, com teste
em `midend/tests/lowering_tests.rs`.

### BUG-321 — resultados booleanos de `std.fs` e `std.env` tinham ABI IR errado

`fs_write`, `fs_append`, `fs_exists`, `fs_remove` e `env_set` eram expostos
semanticamente como `bool`, mas alguns descriptors do midend ainda usavam
`IRType::Int`. A correção alinha lowering e backend; a regressão Rust verifica
explicitamente os resultados de `fs_exists` e `env_set`.

### BUG-322 — `std.io.input` sem descriptor no midend

O símbolo existia no contrato semântico/runtime, mas a chamada não tinha
descriptor de hostcall no lowering. `check` passava e `run` terminava com erro.
O descriptor `spectra.std.io.input` agora retorna `IRType::String`, com
regressão Rust e fixture que fornece duas linhas reais pelo stdin.

### BUG-323 — Option/Result divergiam em tags, Bool e payload inferido

O registro genérico de `Option` no midend tinha `None` antes de `Some`, embora
runtime e `?` usem `Some=0` e `None=1`. Além disso, predicados
`is_some`/`is_none`/`is_ok`/`is_err` declaravam `Int`, e unwraps sem anotação
perdiam o tipo do payload. A correção alinha a ordem das variantes, usa
`IRType::Bool` e especializa o descriptor pelo enum inferido; quatro testes de
lowering cobrem esses contratos.

### BUG-327 — fast path de channel send retornava `i32` para IR `Bool`

O ABI nativo de `channel_send` retorna `i32`, enquanto a superfície Spectra
declara `bool`. O caminho rápido do backend agora reduz o resultado para `i8`
quando o tipo IR é Bool. O fixture de concorrência executa tasks, channels,
batch e pipeline três vezes.

## Ajustes de harness/oracle

- O comprimento observado do `StringBuilder` 317 é 11 para a sequência usada;
  o oracle foi corrigido sem alteração de produção.
- A subtração wrapping de `u32` 3−10 produz `4294967289`; o oracle 320 foi
  corrigido para refletir a semântica de largura exata.
- Em `std.serve`, timeout zero cancela o item pendente; portanto o processamento
  esperado é zero. O fixture 328 foi alinhado a esse contrato determinístico.

## Validação de escopo

Nenhum fixture novo referencia `std.api`, `std.tensor` ou `std.ml`. Não houve
necessidade de classificar recursos não suportados como bugs, nem de alterar
sintaxe pública ou contrato CLI.
