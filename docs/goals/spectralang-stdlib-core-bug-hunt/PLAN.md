# Plano de execução

## Protocolo vertical

Para cada hipótese:

1. manter um único caso reproduzível em `tests/regressions/pending/`;
2. executar `check --json` e `run` com `target/debug/spectralang.exe`;
3. repetir três vezes os casos com hostcalls, runtime, lifetime ou async;
4. minimizar a fonte antes de alterar produção;
5. corrigir somente o estágio implicado;
6. adicionar a regressão Rust ou `.spectra` apropriada;
7. promover o caso aprovado, sem duplicação, para `tests/validation/`;
8. registrar a classificação e a evidência em `FINDINGS.md`.

## Matriz

| Fixture | Área | Cobertura |
| --- | --- | --- |
| 317 | `std.string` | bordas de strings, builder, split, padding e índices |
| 318 | `std.collections` | listas, mapas, mutação, índices e contains |
| 319 | `std.math` / `std.convert` | funções matemáticas, NaN/Inf e conversões |
| 320 | `std.numeric` | larguras exatas, wrapping e checked float |
| 321 | `std.io` / `std.fs` / `std.env` | saída, arquivos, ambiente e argumentos |
| 322 | `std.io` | `read_line` e `input` com stdin real |
| 323 | `std.option` / `std.result` | tags, predicados, unwraps e payloads genéricos |
| 324 | `std.char` / `std.range` | classificação Unicode/ASCII e ranges |
| 325 | `std.random` | seed, inteiros, floats e bools |
| 326 | `std.collections` | map/filter/reduce/sort com closures |
| 327 | `std.concurrent` | tasks, batch, channels, counters e pipeline |
| 328 | `std.serve` | ciclo do servidor, políticas, filas, métricas e diagnóstico |
| 329 | `std.time` | relógios, duration, instant, sleep e UTC |
| 293–297 | async/runtime | await, Task, dyn Stream, channels e Send |

Os caminhos de API, tensor e ML não aparecem na matriz. SQLite local, rede,
GPU e PostgreSQL não são necessários para esta validação.

## Comandos do gate

```text
cargo test -p spectra-midend --test lowering_tests
cargo test --workspace
python scripts/validate_test_pyramid.py
python scripts/validate_stdlib_core_bug_hunt.py --binary target/debug/spectralang.exe
.\run_tests.ps1 -Phase stdlib_core_bug_hunt
.\run_tests.ps1
git diff --check
```

Roadmap e backlog não são alterados: esta é uma execução de validação
ordinária, não a ativação de um item de roadmap.
