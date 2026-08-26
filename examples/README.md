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
