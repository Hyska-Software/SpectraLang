# Goal: SpectraLang Language Bug Hunt

Execute `docs/goals/spectralang-language-bug-hunt/PLAN.md` e o aditivo de
implementação aprovado após a triagem.

Regras principais:

- Use o CLI compilado em `target/debug/spectralang.exe`.
- Crie um fixture por vez e valide o comportamento público antes de avançar.
- Preserve reproductions atuais em `tests/regressions/pending/` até a classificação; depois promova cada caso verde sem duplicar a fonte.
- Corrija bugs confirmados no pipeline Rust e deixe uma regressão pública para cada correção.
- Diferencie bug confirmado, comportamento suportado, recurso ausente e lacuna do harness.
- Registre cada comando, código de saída e diagnóstico em `FINDINGS.md`.
- Sincronize os itens R-214 a R-219 e R-2113 no backlog humano e no TOML
  estruturado após validar os critérios de aceite.
