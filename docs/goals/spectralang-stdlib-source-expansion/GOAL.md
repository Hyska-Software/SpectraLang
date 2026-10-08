# Goal Handoff — Expansão da Standard Library em `.spectra`

Objetivo: implementar, com verificação por etapa, a expansão da biblioteca-fonte
da std (`stdlib/src/*.spectra`) definida em
`docs/goals/spectralang-stdlib-source-expansion/PLAN.md`.

A entrega só é considerada completa quando todos os módulos do plano passam:

- fixture import-only por módulo executando em **JIT e AOT**;
- `python scripts/validate_r3007_stdlib_contract.py --binary target/debug/spectralang.exe --require-catalog` com **0 blockers**;
- catálogo regenerado (`packages/spectra-contract/catalog/stdlib.toml`) sem drift;
- ledger `docs/architecture/stdlib-source-migration.md` e `docs/reference/05-stdlib.md` atualizados.

Nenhum módulo pode ser marcado como concluído com critério de aceite não
verificado, nem pode conter primitiva nativa duplicada sem registro no catálogo.
