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
chamador que espera compartilham a mesma árvore de tasks (rodados em JIT e AOT
pelo gate de certificação R-3221).

Ver `docs/book/11-agents.md`.
