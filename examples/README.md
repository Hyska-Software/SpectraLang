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

Ver `docs/book/11-agents.md`.
