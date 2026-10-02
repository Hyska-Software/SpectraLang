# Goal: corrigir lacunas verificáveis do estado da SpectraLang

## Objetivo

Transformar os achados da [auditoria de estado da linguagem](../../language-state-review-2026-09.md)
em correções de código, regressões executáveis e documentação sincronizada. O resultado deve
preservar a distinção entre implementação validada, capacidade parcial, integração opcional,
simulação intencional e recurso reservado.

## Baseline

- Snapshot auditado: `main`, commit `90346409`, em 29/09/2026.
- CLI local observada: `0.4.2`, canal `nightly`, compatibilidade `spectralang-0.1`.
- `--list-experimental` informa que não há gates ativos; `--enable-experimental` permanece
  opção de compatibilidade sem efeito.
- A auditoria foi estática. Nenhum teste foi considerado aprovado até ser executado neste goal.
- O checkout já tinha alterações locais em `scripts/stdlib_contract.toml`,
  `scripts/validate_r3308_collections.py` e fixtures `tests/validation/622–629`; elas devem
  permanecer preservadas.

## Escopo de implementação

### A. Semântica do núcleo

1. Validar a expressão guard de cada braço de `match` no escopo que contém as bindings do
   padrão; exigir `bool` e emitir diagnóstico localizado para tipo inválido.
2. Corrigir exaustividade: braço com guard não cobre incondicionalmente a variante ou o
   wildcard. Testar enum, `bool`, padrões alternativos e wildcard guardado.
3. Fazer `?` aceitar somente operandos `Option<T>`/`Result<T, E>`, inferir o payload de
   sucesso e verificar se o contexto de retorno aceita o caminho de erro/ausência que o
   lowering propaga. Diagnosticar operandos e contextos incompatíveis.
4. Consumir `mut` em `let` conforme a referência atual. O contrato existente torna bindings
   locais reatribuíveis por padrão; portanto `mut` será aceito como marcador redundante e não
   introduzirá uma regra de imutabilidade incompatível com o código existente.
5. Registrar testes `.spectra` positivos e negativos. Casos com execução devem atravessar
   `check`, JIT e AOT pelos validadores apropriados.

### B. Segurança e observabilidade de runtime

1. Fazer a falha de alocação rastreada chegar ao programa como falha controlada antes de
   qualquer leitura/escrita por ponteiro nulo.
2. Tornar a contabilização explícita: bytes vivos e bytes retidos na quarentena devem poder
   ser distinguidos, e o limite deve refletir a política de memória documentada.
3. Remover o fallback de literal JIT que escapa do rastreamento, substituindo-o por erro
   propagado do codegen ou por uma alocação rastreada cujo erro seja tratado.
4. Fazer falha ao iniciar o worker de timers ser observável pelo chamador; identificar
   corretamente o backend efetivamente ativo quando `mio::Poll` usa fallback.
5. Incluir testes determinísticos de alocação/telemetria e de falha de inicialização, sem
   depender de esgotar a memória real da máquina.

### C. TensorGraph e otimização

1. Fechar a divergência entre construção/otimização do TensorGraph e os caminhos JIT/AOT:
   o grafo otimizado precisa participar da execução gerada, e não apenas ser validado e
   descartado.
2. Preservar fallback correto para nós não suportados, sem rotular esse caminho como fusão.
3. Adicionar um programa de referência com operações fusíveis, comparar o resultado com a
   rota não otimizada e verificar que JIT e AOT executam a transformação observável.
4. Manter métricas e relatórios limitados ao trabalho realmente executado. Não registrar
   fusão ou economia de buffers se o backend não as aplicou.

### D. Contratos das capacidades parciais e simuladas

1. Classificar APIs de treinamento distribuído por símbolo: os helpers legados de contadores
   continuam identificados como simulação; `distributed_train_*` continua descrito como
   treinamento real com limites de cluster e transporte documentados.
2. Manter explícitos os limites de fixtures de RAG, benchmark local de
   serving, BLAS reservado e integrações condicionais. Corrigir qualquer texto que atribua
   a esses fluxos uma evidência que não produzem.
3. Não elevar maturidade por documentação. GPU, ONNX, serviços externos e protocolos
   opcionais só recebem status verificado quando seus gates correspondentes forem executados.

### E. Sincronização documental

Revisar, conforme as alterações de código, os nove pontos listados na auditoria:

- `docs/language-feature-maturity.md`;
- `docs/frontend/frontend-coverage-audit.md`;
- `docs/frontend/parser-coverage-audit.md`;
- `docs/semantic/semantic-coverage-audit.md`;
- `docs/AI-AGENT-REFERENCE.md`;
- `docs/reference/04-avancado.md`;
- `compiler/src/parser/README.md`;
- `scripts/language_stability_contract.toml` quando seus contratos afetados precisarem
  mudar;
- `docs/language-state-review-2026-09.md`, acrescentando resultado das correções e evidência
  executada sem apagar a distinção entre status estático e status validado.

## Fora deste goal

Este trabalho corrige comportamento já exposto, falhas de segurança/contrato e alegações
desalinhadas. Não implementa novas superfícies que a auditoria classifica como reservadas ou
adiadas (por exemplo, classes, lifetimes, identificadores Unicode, registry hospedado ou
bindings `extern` gerais). Essas capacidades devem continuar descritas como ausentes até
terem um objetivo próprio. Serviços externos, GPU e modelos locais só podem ser validados se
estiverem disponíveis neste host; a indisponibilidade será registrada como limite de
verificação, não como aprovação.

## Sequência de execução

| Fase | Entrega | Critério de saída |
|---|---|---|
| 0. Plano e baseline | Este goal e mapa dos arquivos/alterações preexistentes | Goal revisado antes de editar implementação; mudanças anteriores preservadas |
| 1. Núcleo | Guards, `?`, `let mut`, diagnósticos e fixtures | Check/frontend e execução JIT/AOT aprovados para os casos aplicáveis |
| 2. Runtime | OOM controlado, contabilidade, literais e timers | Testes determinísticos provam erro controlado e métricas coerentes |
| 3. TensorGraph | Grafo otimizado consumido pelos caminhos nativos | Equivalência de resultados e evidência de caminho otimizado em JIT/AOT |
| 4. Contratos e documentos | Maturidade, APIs simuladas/condicionais e referências alinhadas | Nenhuma divergência auditada permanece sem status ou explicação |
| 5. Certificação local | Gates afetados e suíte central | Resultados registrados; limitações de serviços opcionais separadas |

## Critérios de aceitação finais

- Cada defeito de semântica e runtime acima tem regressão permanente e passa os gates
  aplicáveis.
- Operadores com propagação/controle de fluxo têm validação semântica coerente com o
  lowering e com a execução nativa.
- Falha de alocação nunca vira acesso a ponteiro nulo, e a telemetria diferencia memória
  viva da memória retida na quarentena.
- Um caminho reportado como TensorGraph otimizado é realmente usado pelo JIT/AOT; as saídas
  coincidem com a execução de referência.
- A documentação distingue com precisão implementação real, parcial, condicional,
  simulada, reservada e ainda não certificada.
- Gates locais definidos para as mudanças passam. Dependências externas ausentes são
  enumeradas nominalmente e não são declaradas aprovadas.
- `run_tests.ps1` passa no estado final, ou cada falha preexistente/alheia é reproduzida,
  isolada e relatada sem removê-la do checkout.

## Estado

- Fases 0–5: concluídas. Guards/`?`/`let mut`, caminhos de falha do runtime,
  fusão unária consumida pelo JIT/AOT e documentação foram implementados.
- Evidência central: no baseline anterior, compiler (133 testes), midend (115),
  backend (77), runtime (258 unitários + 1 integração) passaram; o corpus chegou
  a 1.010 aprovações, 3 falhas e 1 ignorado por ambiente. A validação R-3308
  também passou naquele baseline.
- Triagem dos 3 resultados negativos do run_tests.ps1 no baseline: duas falhas
  eram a mesma expectativa antiga de kernels em
  tensor_graph_reduction_fusion, já corrigida; R-2001 foi recertificado com
  22/22 gates e 21/21 exemplos de IA, e R-2013 com 8/8 projetos. O gate R-3208
  foi validado contra o catálogo reduzido: 1.352 entradas, 560 host calls da
  API (532 sempre ativos e 28 condicionados por feature) e 1.115 braços de
  lowering em 7 tabelas geradas.
- Limite de repetição: `run_tests.ps1` não foi reexecutado por inteiro após a
  última correção. Os gates que falharam foram reexecutados isoladamente com
  sucesso; portanto, não se declara um resultado zero de falhas para uma
  execução integral posterior.
- Limites externos: R-2904 registrou WGPU ignorado por falta de adaptador. GPU
  em hardware e integrações com serviços/endpoint externos não são declaradas
  certificadas sem seus recursos configurados.
- Higiene do diff: `git diff --check` passou. `cargo fmt --all -- --check`
  retorna 494 diferenças de formatação espalhadas pelo workspace; não foi feita
  reformatagem ampla. `rustfmt --check` passou individualmente em
  `backend/src/codegen.rs`, `backend/src/tensor_graph_codegen.rs`,
  `runtime/src/stdlib/tensor_autograd.rs` e
  `runtime/src/stdlib/tensor_ops_reductions.rs`.
