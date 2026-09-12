# Plano de execução

## Protocolo por caso

1. criar um único candidato em `tests/regressions/pending/`;
2. executar `check` e `run` pelo binário `target/debug/spectralang.exe`;
3. repetir três vezes os casos de lifetime, async, dyn, hostcall e runtime;
4. executar AOT nos casos ABI/codegen selecionados;
5. se houver falha, minimizar a fonte antes de tocar na implementação;
6. corrigir somente o estágio implicado e adicionar a regressão local;
7. registrar a classificação e a evidência em `FINDINGS.md`;
8. promover o caso aprovado para `tests/validation/` ou para
   `tests/projects/valid/`.

## Lotes

| Lote | IDs | Cobertura |
| --- | --- | --- |
| 1 | 275–281 | lexer, parser, aliases, closures, controle, padrões, arrays |
| 2 | 282–288 | generics, traits, UFCS, dyn, Drop, receivers mutáveis |
| 3 | 289–292 | imports, reexports, visibilidade, projetos multifile |
| 4 | 293–297 | async, Task, spawn/join, streams, channels, Send |
| 5 | 298–307 | stdlib, filesystem, JSON, tempo, runtime, tensor, autodiff, ML |
| 6 | 308–316 | API, middleware, SQLite, AOT, otimização, hostcalls, CLI |

## Validação final

```text
cargo test --workspace
python scripts/validate_test_pyramid.py
python scripts/validate_language_bug_hunt_v2.py --binary target/debug/spectralang.exe
.\run_tests.ps1 -Phase language_bug_hunt_v2
.\run_tests.ps1
git diff --check
```

Roadmap e backlog não fazem parte desta execução: a rodada é uma validação
ordinária do código e dos testes existentes.
