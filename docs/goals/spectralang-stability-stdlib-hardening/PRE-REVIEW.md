# PRE Review — SpectraLang Stable Core e STD Tipada

MODE: PRE

## Escopo revisado

`PLAN.md` foi revisado contra o template `plan-reviewer-prompt.md`, o código
atual do repositório e os gates/artefatos já existentes. A revisão foi feita
como auditoria local do agente; não houve aprovação de execução nem alteração
de código de produção.

## Checklist

- Intent, comportamento atual, resultado esperado e saída do ponto de vista do
  usuário estão explícitos.
- As fronteiras fonte → AST → semântica → IR → JIT/AOT → runtime/serviços têm
  owner e contrato definidos.
- `class`, `static`, `Type::Unknown`, placeholders, catálogo, ADTs/STD,
  handles, exact-width, arrays/iteradores, async e gates externos estão
  mapeados diretamente para tarefas K-01…K-12.
- Cada tarefa declara arquivos, escopo, output, verificação, aceite e
  paralelismo; K-03 e K-05 foram divididas em unidades menores.
- O caminho deslocado e o cutover estão definidos; `std.compat` e
  `scripts/stdlib_contract.toml` não podem permanecer como autoridades
  concorrentes.
- O plano preserva a regra de não promover `skipped_environment`, baseline ou
  simulation a evidência de estabilidade.
- A alteração de planejamento não toca `roadmap/roadmap.toml` nem altera
  código durante a execução do PRE.

## Verdict: aligned

### Findings ordered by risk

- **minor:** `--require-redis` ainda não existe no validador atual. A menor
  correção já está registrada em K-11: adicionar a flag antes de executar o
  comando required de K-09/K-11, preservando o modo local permissivo.
- **minor:** a evidência local atual não certifica PostgreSQL, Redis e TLS
  externo. O plano trata isso como lacuna explícita e desloca a certificação
  para as lanes CI/endpoint required; nenhum resultado local será contado como
  prova final.

Não há blocker para iniciar K-00. A execução não deve começar pelas tarefas de
código antes de o baseline e os dois manifests passarem pela revisão dos owners.

## Recommended next gate

Executar somente K-00, gerar `target/stability/baseline.json` e revisar o schema
de `scripts/language_stability_contract.toml` e
`packages/spectra-contract/catalog/stdlib.toml`. Depois disso, abrir K-01 e
K-02 em paralelo com integração serial antes de K-03.
