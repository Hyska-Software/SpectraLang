# Goal: SpectraLang Stable Core e STD Tipada

Use Krypton Execution para executar
`docs/goals/spectralang-stability-stdlib-hardening/PLAN.md`.

Regras centrais:

- tratar `PLAN.md` como o plano fonte;
- preservar intent, ownership, contracts, cutover, evidence e kill criteria;
- manter `class` reserved/deferred; não criar uma implementação superficial;
- não adicionar um registry, catálogo, API ou sistema de handles dominante sem
  remover, redirecionar ou despromover o caminho antigo;
- não permitir `Type::Unknown`, Value sintético, zero de erro ou skip externo
  como sucesso;
- capturar evidência do ponto de vista do usuário, incluindo JIT, AOT,
  diagnostics, docs e relatório required;
- dizer “implemented but unproven” sempre que a implementação existir sem a
  evidência correspondente;
- não atualizar roadmap/backlog até a evidência final justificar a mudança;
- não criar commit, push ou PR sem solicitação explícita.

Conclusão exige todos os critérios de K-12; falha em qualquer gate mantém o
objetivo `in_progress` e registra a lacuna concreta.
