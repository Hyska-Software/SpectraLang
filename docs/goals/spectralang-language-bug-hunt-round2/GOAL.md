# SpectraLang language bug hunt — round 2

## Objetivo

Executar uma segunda rodada determinística de testes de linguagem contra o
pipeline público do SpectraLang, cobrindo os candidatos 275–316. Cada caso
deve atravessar `check`, `run` e, quando aplicável, emissão AOT, sem depender de
rede, GPU ou PostgreSQL externo.

O objetivo técnico é encontrar regressões reais nas fronteiras lexer → parser →
semântica → midend → backend → runtime/CLI e deixar cada descoberta com uma
reprodução mínima e uma regressão permanente.

## Critérios de saída

- os 42 IDs têm classificação explícita (`PASS`, `BUG`, `HARNESS_GAP` ou
  `NONDETERMINISTIC`);
- casos positivos aprovados são promovidos sem duplicação para
  `tests/validation/` e projetos multifile para `tests/projects/valid/`;
- cada positivo promovido passa `check` e `run`;
- casos selecionados passam emissão, link e execução AOT;
- lifetime, async, dyn, hostcall e runtime têm três repetições quando a área
  for exercitada;
- falhas corrigidas mantêm regressão no estágio proprietário da causa;
- o validador produz o relatório `spectralang.language_bug_hunt.v2`;
- a fase dedicada e os gates completos do repositório passam.

## Limites

Os fixtures usam a extensão `.spectra` minúscula e permanecem sem rede externa,
GPU obrigatória ou banco PostgreSQL obrigatório. SQLite, quando usado, deve
escrever somente em `target/`. Recursos que a implementação ainda não suporta
não serão artificialmente classificados como bugs.
