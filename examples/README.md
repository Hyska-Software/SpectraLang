# SpectraLang — Exemplos

Exemplos executáveis da linguagem. Rode qualquer um com:

```
spectralang run <arquivo>.spectra
```

## examples/ (raiz)

Demonstrações gerais da linguagem e fixtures de regressão históricas:

- `console_demo.spectra` — tour completo da stdlib (io, math, string, convert, random, env) com f-strings
- `type_system_demo.spectra`, `traits_demo.spectra` — sistema de tipos e traits
- `test_*.spectra` — micro-exemplos históricos de construtos específicos

## examples/complete — projetos integrados

O índice dos 30 projetos integrados está em [complete/README.md](complete/README.md).
O exemplo `18-spectragit` é um CLI local de controle de versão escrito em
SpectraLang; os comandos e limites estão em seu
[README](complete/18-spectragit/README.md).
O exemplo `19-spectraboard` é um organizador local de tarefas em módulos; seu
CLI, persistência e verificações estão no
[README](complete/19-spectraboard/README.md).
O exemplo `20-spectraledger` é um controle financeiro local em módulos; seu
CLI, ledger em centavos inteiros, orçamentos mensais e verificações estão no
[README](complete/20-spectraledger/README.md).
O exemplo `21-spectrahabit` é um rastreador local de hábitos em módulos, com
metas semanais, check-ins datados, sequências e relatórios; veja o
[README](complete/21-spectrahabit/README.md).
O exemplo `22-spectravision` treina uma CNN para classificar imagens 6×6 com
convolução, pooling, dropout, autodiff, AdamW, métricas e checkpoint; veja o
[README](complete/22-spectravision/README.md).
O projeto `27-spectradiffuse` treina um denoiser DDPM tensorial para uma
mistura de quatro modos 2D; treino, holdout, amostragem reversa e comparação
JIT/AOT estão no [README](complete/27-spectradiffuse/README.md).
O projeto `28-spectrarelease` combina treino tensorial com autodiff/AdamW,
detecção de drift, rollback e uma pipeline assíncrona modular; veja o
[README](complete/28-spectrarelease/README.md).
O projeto `29-spectraquant` calcula preços de opções e Greeks com Monte Carlo
tensorial e autodiff, agrega VaR/CVaR e aplica políticas de risco por trait
dinâmico; o harness compara JIT e AOT.
O projeto `30-spectragrid` treina em dados sintéticos um MLP tensorial para
previsão de carga e usa a previsão em despacho de microrrede com bateria,
políticas dinâmicas e validação de balanço; o exemplo cobre seis períodos e
até três geradores, e o harness compara JIT e AOT.

## examples/stdlib — biblioteca padrão

Sessenta exemplos executáveis que exercitam as APIs `std.*`; cada um
tem um fixture correspondente em `tests/validation/436_stdlib_collections_snapshot.spectra`
até `tests/validation/495_stdlib_serve_multi_model_routing.spectra`.

| Arquivo | O que demonstra |
|---|---|
| `01-collections-snapshot.spectra` | snapshots de `List`, `Map`, `Set` e `Iterator` |
| `02-option-result-flow.spectra` | transformação e propagação de `Option`/`Result` |
| `03-filesystem-tree.spectra` | criação, listagem, rename, cópia e limpeza de arquivos |
| `04-error-recovery.spectra` | `Error`, códigos, contexto e recuperação de falhas |
| `05-environment-options.spectra` | variáveis de ambiente, argumentos e opções |
| `06-typed-collections.spectra` | coleções genéricas tipadas e acesso opcional |
| `07-unicode-text.spectra` | texto UTF-8, caracteres e limites de substring |
| `08-string-builder-report.spectra` | `StringBuilder`, padding, repetição e relatórios |
| `09-range-iterator.spectra` | ranges inclusivos/exclusivos e exaustão de iterador |
| `10-random-convert.spectra` | aleatoriedade determinística e conversões |
| `11-time-deadlines.spectra` | `Duration`, `Instant`, relógio e UTC |
| `12-concurrent-fanout.spectra` | tasks, canais, contadores e pipeline concorrente |
| `13-numeric-widths.spectra` | inteiros de largura exata e floats `f32`/`f64` |
| `14-serve-lifecycle.spectra` | modelo linear multicamada, guardrails e monitoramento |
| `15-standard-pipeline.spectra` | pipeline integrado de arquivo, texto, coleções e erros |
| `16-text-normalizer.spectra` | normalização de texto, prefixos, caracteres e `Option` |
| `17-math-geometry.spectra` | trigonometria, potências, logaritmos, arredondamento e inteiros |
| `18-convert-config.spectra` | parsing com fallback e conversões de tipos |
| `19-collection-mutation.spectra` | mutações de lista, ordenação, remoção e limpeza |
| `20-collection-hof.spectra` | `map`, `filter`, `reduce` e `sort_by` com closures |
| `21-map-set-lifecycle.spectra` | atualização, ausência, remoção e ciclo de vida de `Map`/`Set` |
| `22-iterator-values.spectra` | consumo tipado, contagem restante e exaustão de iteradores |
| `23-tensor-views.spectra` | reshape, transpose, slice, concat e stack |
| `24-tensor-determinism.spectra` | RNG semeado, distribuições e política de tolerância |
| `25-tensor-autodiff.spectra` | `requires_grad`, `backward`, gradientes e `zero_grad` |
| `26-tensor-lifecycle.spectra` | reutilização de buffers, lifetimes e relatório de memória |
| `27-ml-transformer-cache.spectra` | embedding, attention, KV-cache e sampling seeded |
| `28-ml-dataset-module.spectra` | datasets tensor-backed, splits, dataloader e módulos |
| `29-concurrent-batch.spectra` | batches, canais FIFO, contadores e reset |
| `30-time-clocks.spectra` | clocks monotônicos, durações, instantes e UTC |
| `31-char-classifier.spectra` | classificação de caracteres e composição de strings |
| `32-string-search-report.spectra` | busca textual, índices e relatório formatado |
| `33-filesystem-append-copy.spectra` | append, cópia, rename e erros estruturados de filesystem |
| `34-numeric-checked.spectra` | conversões e aritmética numérica checked por largura |
| `35-tensor-algebra.spectra` | reduções, `dot`, `matmul` e `matmul_batched` |
| `36-tensor-device-precision.spectra` | residência CPU, `sync` e conversão de precisão |
| `37-tensor-runtime-stats.spectra` | métricas de alocação, kernels, autograd e memória |
| `38-ml-evaluation-report.spectra` | métricas ML e round-trip do relatório JSON |
| `39-ml-artifact-roundtrip.spectra` | artefatos, metadados, tensores e validação persistida |
| `40-ml-experiment-repro.spectra` | tracking, manifests reprodutíveis e comparação |
| `41-ml-distributed-checkpoint.spectra` | passos de workers, checkpoint e resume distribuído |
| `42-ml-tokenizer-training.spectra` | treinamento BPE/WordPiece, encode, decode e vocabulário |
| `43-ml-vector-index.spectra` | índice HNSW, consulta, métricas e persistência |
| `44-ml-optimizer-schedule.spectra` | gradiente escalado, SGD momentum, Adam, AdamW e schedule |
| `45-serve-named-model.spectra` | inferência linear nomeada, vetor de resultado e monitoramento |
| `46-collections-iterator-cleanup.spectra` | iteradores, aliases opcionais e limpeza de coleções |
| `47-environment-argument-options.spectra` | variáveis de ambiente e argumentos como `Option` |
| `48-math-integer-rounding.spectra` | `gcd`, `lcm`, clamp, sinais, arredondamento e valores especiais |
| `49-random-stream.spectra` | sequência determinística de booleanos, floats e inteiros |
| `50-time-utc-calendar.spectra` | conversão Unix/UTC e campos de calendário, incluindo bissexto |
| `51-tensor-elementwise.spectra` | aritmética elementwise e ativações transcendentais |
| `52-tensor-shape-views.spectra` | dimensões, `permute`, `concat` e `stack` |
| `53-tensor-diagnostics.spectra` | estratégia de kernel, tolerâncias, residência e relatório |
| `54-ml-modules-layers-losses.spectra` | módulos, camadas, pooling, dropout e losses |
| `55-ml-dataset-files.spectra` | datasets CSV/JSONL/diretório, dataloader e dataframe |
| `56-ml-rag-metrics.spectra` | chunking/prompt RAG e métricas de ranking, geração e serving |
| `57-ml-onnx-contract.spectra` | exportação, validação, resumo e round-trip ONNX |
| `58-serve-http-lifecycle.spectra` | bind HTTP efêmero, stop/restart e benchmark real |
| `59-ml-tokenizer-direct.spectra` | WordPiece direto, encode/decode e tokens desconhecidos |
| `60-serve-multi-model-routing.spectra` | roteamento por nome e métricas por modelo |

## examples/projects/multi_file — projetos stdlib multi-arquivo

Dez projetos executáveis que atravessam módulos de usuário e a biblioteca
padrão. O tamanho varia deliberadamente entre 2 e 5 arquivos `.spectra`; cada
projeto possui uma verificação correspondente em
`tests/projects/valid/stdlib_multifile_01_*` até `stdlib_multifile_10_*`.

| Projeto | Arquivos `.spectra` | Superfície exercitada |
|---|---:|---|
| `p5_collections_pipeline` | 2 | `collections`, `option`, iteradores e namespaces de módulos |
| `p6_filesystem_report` | 3 | filesystem, `Result`, erros e composição de caminhos |
| `p7_time_random_config` | 4 | ambiente, opções, aleatoriedade determinística e UTC |
| `p8_tensor_shapes` | 5 | aritmética tensorial, views, concatenação, stack e cleanup |
| `p9_ml_dataset_pipeline` | 2 | datasets CSV/JSONL/diretório e dataframe |
| `p10_ml_metrics_report` | 3 | métricas ML e relatório persistido |
| `p11_ml_tokenizer_rag` | 4 | WordPiece, RAG, ranking, geração e serving |
| `p12_ml_artifact_experiment` | 5 | artefatos, manifests e reprodutibilidade |
| `p13_concurrent_tasks` | 2 | tasks, canais FIFO, contadores e reset |
| `p14_serve_models` | 4 | modelos nomeados, políticas e monitoramento |

## examples/api — servidor HTTP e banco

| Arquivo | O que demonstra |
|---|---|
| `00_hello_http.spectra` | Servidor HTTP mínimo (`block_on` embutido da linguagem) |
| `01_rest_crud.spectra` | REST CRUD completo com router e middleware |
| `02_jwt_auth_crud.spectra` | Autenticação JWT (HS256) protegendo rotas |
| `03_middleware_composition.spectra` | Composição de middlewares (logging, rate-limit, CORS) |
| `04_sse_progress.spectra` | SSE com progresso em tempo real e replay |
| `05_websocket_echo.spectra` | WebSocket echo com permessage-deflate |
| `06_rest_sqlite_crud.spectra` | REST + SQLite (prepared statements) |
| `07_request_validation.spectra` | Validação de requests com schemas e problem+json |
| `08_sessions_login.spectra` | Sessões com login, lookup e revogação |
| `09_migrations.spectra` | Migrations com checksum e rollback |
| `10_otel_prometheus.spectra` | Observabilidade: OpenTelemetry traces + Prometheus |

## examples/ai — pipeline de IA

- `rag_retrieval_pipeline.spectra` — pipeline RAG completo: chunking →
  embedding → busca vetorial (HNSW) → montagem de prompt. A resposta é
  fornecida externamente no "model-call boundary" — a geração propriamente
  dita usa `ml.generate` com um modelo causal-LM via ONNX Runtime
  (ver `std.ml.generate` na referência da stdlib).

## Regressões gerais do pipeline assíncrono

Os fixtures ~368_async_await_loop.spectra~, ~370_async_aggregate_across_await.spectra~,
~373_record_reassign_in_loop.spectra~ e ~403_async_float_slot.spectra~ cobrem
suspensão e retomada efetivas enquanto preservam, respectivamente, variáveis de
loop, agregados, reatribuição de records e valores escalares nos frames.
