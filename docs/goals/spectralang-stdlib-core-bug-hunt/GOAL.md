# Bug hunt executável da stdlib principal

## Objetivo

Validar a superfície principal da standard library do SpectraLang por meio do
pipeline público `check --json` → `run`, identificar falhas reproduzíveis,
minimizá-las antes da correção e deixar uma regressão `.spectra` permanente
para cada comportamento suportado.

Esta rodada cobre os 16 namespaces diretos fora de API, tensor e ML:

`std.io`, `std.math`, `std.numeric`, `std.collections`, `std.string`,
`std.convert`, `std.random`, `std.fs`, `std.env`, `std.option`, `std.result`,
`std.char`, `std.time`, `std.range`, `std.concurrent` e `std.serve`.

A superfície assíncrona é validada separadamente pelos fixtures 293–297, que
exercitam `async`, `Task`, `await`, streams, channels e o limite `Send`. O
contrato atual registra `std.async`, mas não há um módulo semântico importável
com esse nome; a matriz não inventa uma API pública para preencher essa lacuna.

API (`std.api`), tensor (`std.tensor`) e ML (`std.ml`) ficam explicitamente
fora desta rodada e continuam cobertos por seus próprios gates.

## Critérios de saída

- os 13 novos fixtures 317–329 passam `check --json` com `success=true`;
- cada fixture passa `run` três vezes, com stdin determinístico no caso 322;
- os fixtures assíncronos 293–297 passam `check` e `run` três vezes;
- cada falha encontrada tem causa e correção registradas em `FINDINGS.md`;
- regressões de lowering ficam em `midend/tests/lowering_tests.rs` quando o
  contrato de tipo/ABI é a causa;
- o gate produz relatório determinístico
  `spectralang.stdlib_core_bug_hunt.v1`;
- a fase dedicada e os gates completos do repositório passam.

## Classificação

Cada hipótese é classificada como `PASS`, `BUG`, `HARNESS_GAP` ou
`NONDETERMINISTIC`. Recursos fora do contrato suportado não são promovidos nem
classificados artificialmente como bugs.
