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

## examples/agent — runtime de agentes (`std.agent`)

Projetos executáveis com o provedor mock determinístico (sem credenciais nem
rede), em JIT (`spectralang run`) e AOT (`compile --debug-info=none --emit-exe`):

| Diretório | O que demonstra |
|---|---|
| `01-tool-and-run` | `#[agent_tool]`, `agent_start`/`ask`/`act`/`tool_call`/`agent_end` |
| `02-approval-and-budget` | negação padrão de aprovação, `require` e teto de tokens |
| `03-mcp-and-memory` | `mcp_serve`/`mcp_connect`/`tool_call` pelo loopback em processo e `remember`/`recall` |
| `04-durable-replay` | journal e replay com o mesmo `run_id`, sem repetir o efeito |
| `05-streaming-and-schema` | `ask_stream`/`stream_next`/`stream_close`, `ask_json` com o `json_schema` derivado e os dois desfechos do validador |
| `06-capabilities-and-taint` | grant de namespace, aprovação negada por padrão e `untrusted`/`trust` com razão obrigatória |
| `07-protocol-surface` | `a2a_card`/`a2a_handle` (`message/send`, `tasks/get`) e `acp_handle` (`initialize`, `session/new`) em processo |
| `08-memory-and-recall` | `remember`/`recall` entre duas runs com o mesmo goal, com a proveniência apontando o `run_id` do escritor |
| `09-compensation-saga` | `compensate`/`rollback` em ordem LIFO, com contador provando execução única e replay pelo journal |
| `10-list-payloads` | `List` escalar como argumento, como retorno e como campo derivado (`json_schema`, encode e decode) |
| `11-tool-call-budget` | teto de `max_tool_calls` no `act`: chamada recusada sem executar, run cancelada com erro tipado e report nomeando o teto |
| `12-structured-output` | `ask_json` com o `json_schema` derivado, falha tipada de validação e o reparo alimentado de volta ao modelo |
| `13-embeddings-and-ranking` | `embed` como primitiva de ranking (similaridade de cosseno calculada no programa), `remember`/`recall` e o embedding reconstruído pelo journal sem nova ida ao provider |
| `14-acp-and-permissions` | a superfície ACP (`initialize`, `session/new`, `session/prompt`, `session/cancel`, método não servido) e a ponte de permissão `acp_permission` negando por padrão, com as duas decisões distintas no journal (`sem cliente ACP` vs `sem approver`) |
| `15-mcp-service-surface` | a superfície MCP servida (`mcp_handle`): `initialize`, notificação, `ping`, `tools/list` com as anotações derivadas, `tools/call` com sucesso, falha de tool (`isError`) e recusa de run (`-32001`), além do método não servido e do documento inválido |
| `16-a2a-task-lifecycle` | o ciclo de vida das tarefas A2A: `completed` com poll idempotente, `failed` nomeando o teto, `rejected` pela grant e as recusas codificadas (`-32002`, `-32001`, `-32600`, `-32602`, `-32601`) |
| `17-token-budgeting` | `token_count` como primitiva de orçamento: sentinela sem teto, embedding que não é turno, a identidade `token_count(prompt) + token_count(resposta)` e as duas metades da regra (o caller decide o que cabe; o runtime recusa depois de gasto) |
| `18-complex-tool-payloads` | payload de tool com record aninhado, lista, float, booleano e UTF-8, incluindo schema derivado e rejeição de tipo interno |
| `19-schema-recovery` | violação de `ask_json` por limite de schema, erro tipado com caminho e continuação da mesma run até uma resposta final |
| `20-journal-payload-policy` | `journal_payloads=false/true`: output sempre durável para replay e input capturado somente por opt-in |
| `21-replay-tool-effects` | resultado de tool replayado pelo journal sem reexecutar o corpo, provado por contador de invocações |
| `22-multiple-streams` | dois `ChunkStream` intercalados, fechamento independente e leitura completa do handle sobrevivente |
| `23-memory-tie-order` | empates de embedding ordenados pelo ordinal de inserção e limitados por `top_k` |
| `24-capability-boundaries` | grant de sink totalmente qualificado, grant de namespace pai e recusa de prefixo parecido (`fsx`) |
| `25-compensation-failure` | rollback LIFO tolerante a compensation inválida, com sucesso e falha duráveis e sem duplicação em replay |
| `26-mcp-replay` | descoberta MCP, chamada remota pelo loopback e replay sem reabrir o listener |
| `27-protocol-negative-matrix` | `a2a_serve` real, lifecycle ACP e recusas para sessão errada, ação vazia e documentos malformados |
| `28-taint-policy-matrix` | matriz de taint em sink real: `allow`, trust explícito e bloqueio fail-closed |
| `29-cost-ceiling` | teto `max_cost_micros`, charge de custo e report de orçamento |
| `30-unsafe-run-id` | sanitização de `run_id` no nome do journal e replay |
| `31-corrupt-journal` | registros JSON malformados/incompletos rejeitados como `journal_error` |
| `32-stream-replay` | chunks de `ask_stream` duráveis e replayados sem nova chamada |
| `33-schema-booleans` | schemas booleanos `true`/`false` e violação de `enum` |
| `34-option-enum-tool` | campo opcional, enum unitário renomeado e erro de wire value |
| `35-nested-compensation` | compensation declarada dentro de tool e restaurada no replay externo |
| `36-mcp-remote-errors` | envelope MCP `isError` convertido em `ToolFailed` sem perder a próxima chamada |
| `37-a2a-idempotency` | replay por `messageId` e recusa de conflito por `-32600` |
| `38-cross-module-tools` | tools declaradas em módulo importado, dispatch direto do wrapper e cadeia `act` |
| `39-remote-scope-isolation` | descoberta MCP remota isolada na run de origem; uma run local posterior continua chamável |
| `40-empty-stream-boundary` | stream vazio do provider, marcador de fim persistente e close idempotente |
| `41-a2a-card-defaults` | defaults de identidade A2A, URL omitida e rejeição tipada de campo authored |
| `42-unicode-tool-payload` | round-trip JSON derivado para Unicode, aspas, novas linhas e barras invertidas |
| `43-remote-act-and-card` | ferramenta descoberta via MCP compartilhada pelo cartão A2A e pelo loop `act` |
| `44-stream-after-run-end` | stream mantém seu buffer após `agent_end` e só libera o handle em `stream_close` |
| `45-concurrent-runs` | duas runs vivas intercalam tools sem misturar contadores nem turnos do provider |

Os contratos que os exemplos demonstram ficam fixados nos fixtures
`tests/validation/384_agent_stream_lifecycle.spectra`,
`385_agent_capabilities_in_practice.spectra`,
`386_agent_journal_artifact.spectra`, `387_agent_introspection.spectra`,
`388_async_aggregate_result_lifetime.spectra`,
`389_string_and_container_payload_lifetime.spectra`,
`390_agent_nested_dispatch.spectra`, `391_agent_concurrent_runs.spectra`,
`392_agent_payload_scale.spectra`, `393_agent_tool_call_ceiling.spectra`,
`394_agent_structured_output.spectra`, `395_agent_memory_limits.spectra` e
`396_agent_stream_in_tool.spectra` e
`397_agent_nested_dispatch_stress.spectra` — esta última repete o dispatch
aninhado 50 vezes numa run, a forma em que um worker de tool em background e o
chamador que espera compartilham a mesma árvore de tasks.

Os fixtures `398_agent_dead_handle_matrix.spectra` (todo entry point num
`Run` já encerrado responde `unknown_handle`),
`399_agent_spec_rejection.spectra` (a validação estrita do spec, campo a campo),
`400_agent_stream_teardown.spectra` (o ciclo de vida do `ChunkStream` na
fronteira de teardown da run), `401_agent_provider_routing.spectra` (qual
provider cada spec seleciona e o que um provider não configurado responde),
`402_agent_script_directives.spectra` (a gramática das diretivas do mock) e
`403_async_scalar_slots_and_float_payloads.spectra` (as regressões de codegen
`Result<float, _>`/slot escalar em corrotina/reload encadeado),
`404_agent_memory_edges.spectra` (o insert vazio, o `top_k` negativo e a
fronteira exata do orçamento de payload: 512 tokens entram, 513 não),
`405_agent_ceiling_combinations.spectra` (qual teto vence quando vários são
declarados, o teto de tool cruzado fora do `act` e o sentinela após um
cancelamento por relógio), `406_agent_replay_divergence.spectra` (a divergência
tipada, o prefixo retomado para a frente e o uso regravado no orçamento),
`407_agent_a2a_task_states.spectra` (os estados de tarefa e as recusas
codificadas), `408_agent_taint_ledger_edges.spectra` (tags obrigatórias, o corte
em 256 caracteres e a proveniência endereçada por conteúdo) fecham a matriz de
cobertura da superfície. Todos rodam em JIT e AOT pelo gate de certificação
R-3221.

Os fixtures `409_agent_nested_payloads.spectra`, `410_agent_schema_recovery.spectra`,
`411_agent_journal_payloads.spectra`, `412_agent_replay_tool_once.spectra` e
`413_agent_stream_interleave.spectra` cobrem payloads aninhados, recuperação de
schema, privacidade do input no journal, replay idempotente de tool e três
streams independentes. `414_agent_memory_ties.spectra` verifica a ordem de
empates vetoriais; `415_agent_capability_boundary.spectra` verifica a fronteira
de namespace; `416_agent_compensation_failure.spectra` verifica rollback com
falha de wrapper; `417_agent_mcp_replay.spectra` verifica descoberta e chamada
MCP replayadas; `418_agent_protocol_negatives.spectra` verifica recusas A2A/ACP.

Os fixtures `419`–`428` espelham esses limites: taint e custo, identidade
insegura e integridade do journal, chunks em replay, schemas booleanos e
enums em payloads de tool, compensation aninhada, erro remoto MCP e
idempotência A2A.

Os fixtures `429_agent_remote_scope.spectra` até
`432_agent_unicode_tool_payload.spectra` fixam, respectivamente, isolamento de
descoberta MCP entre runs, o marcador de fim de stream vazio, defaults e
validação de campos do cartão A2A e round-trip de payload JSON com escapes. O
fixture 429 também protege contra a falha corrigida em que um descriptor remoto
registrado por uma run contaminava o grant check de outra.

Os fixtures `433_agent_remote_act.spectra`, `434_agent_stream_after_run.spectra`
e `435_agent_concurrent_runs.spectra` fecham o follow-up com, respectivamente,
descoberta remota compartilhada entre cartão A2A e `act`, ownership de stream
após `agent_end` e isolamento de contadores/budget entre runs concorrentes.

Os exemplos `43`–`45` ampliam a matriz para o caminho remoto orientado pelo
modelo, a separação entre handles de run e stream e a atribuição de trabalho
entre runs concorrentes.

Ver `docs/book/11-agents.md`.
