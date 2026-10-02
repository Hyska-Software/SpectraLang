# Auditoria do estado da SpectraLang

**Snapshot:** 29/09/2026 — branch main, commit 90346409

**Tipo:** revisão estática de implementação, contratos, testes, exemplos e documentação

**Escopo:** linguagem, compilador, execução, biblioteca padrão, AI/ML, API, banco de dados, tooling e interop

## Resumo executivo

A SpectraLang tem um compilador e um runtime reais: o caminho padrão da CLI passa por análise semântica, lowering e backend Cranelift; JIT e AOT produzem e executam código nativo. A biblioteca inclui implementações reais para muitas operações numéricas, async, HTTP e bancos de dados. A linguagem não é, em conjunto, uma fachada simulada.

O estado, porém, é desigual. O núcleo mais usado tem um contrato estável e testes executáveis; outras áreas são beta, dependem de features Cargo ou serviços externos, implementam apenas um subconjunto do protocolo, ou oferecem explicitamente uma simulação para testes e demonstrações. Há também lacunas de análise semântica em sintaxes que chegam ao lowering.

As constatações de maior impacto são:

1. Guards de match são baixados para branches, mas a análise semântica não verifica a expressão guard e a análise de exaustividade a ignora.
2. O operador ? tem parser e lowering, mas a inferência e a validação semântica não aplicam o contrato esperado de Option/Result.
3. A documentação afirma que let mut é aceito; o parser o rejeita. Ao mesmo tempo, bindings locais comuns podem ser reatribuídos e não há modelo geral de imutabilidade ou ownership.
4. TensorGraph é construído e validado, mas o resultado do grafo não é consumido pelo codegen JIT/AOT. Isso não prova execução fused de um grafo nativo.
5. A feature blas está vazia. GPU/WGPU e ONNX são opções reais, porém opcionais; o suporte é restrito ao conjunto de operações e modelos implementado.
6. O projeto mantém APIs antigas de treinamento distribuído simuladas e APIs mais novas que fazem treinamento real via threads/TCP. As classificações e a documentação precisam distinguir esses grupos.
7. Falhas de alocação sob pressão podem resultar em ponteiro nulo sem diagnóstico Spectra antes de operações de memória.

Na CLI local consultada, release-info informa versão 0.4.2, canal nightly e compatibilidade spectralang-0.1. --list-experimental retorna “Experimental language features: none”. --enable-experimental é compatibilidade no-op.

## Como ler as classificações

- **Implementado no escopo atual:** existe caminho de código correspondente ao contrato descrito e há fixtures, testes ou gates no repositório. Isso não quer dizer que a suíte tenha passado nesta auditoria.
- **Parcial / beta:** existe comportamento utilizável, mas o próprio contrato, a implementação, o suporte de operações ou a certificação externa é limitado.
- **Condicional:** implementação real que só fica disponível com feature de build, dispositivo, modelo, credencial ou serviço externo.
- **Simulado:** fluxo determinístico ou didático que usa valores fornecidos pelo chamador, respostas artificiais ou dados sintéticos. Não é equivalente a treinamento, inferência ou integração de produção.
- **No-op / reservado / adiado:** flag sem efeito, sintaxe rejeitada ou capacidade futura sem contrato executável.
- **Não certificado neste snapshot:** há código ou testes, mas os testes, gates ou serviços externos não foram executados durante esta revisão.

O rótulo stable da política de maturidade significa que a sintaxe está habilitada, documentada e tem cobertura positiva. Não equivale a uma certificação de todos os targets, integrações e cenários de produção.

## Escopo e evidências

Foram revisados o frontend e a semântica, midend e backend, runtime, CLI, spectra-interop, os pacotes spectra-api e spectra-db, fixtures .spectra, validadores e documentação relacionada. A inspeção abrangeu 881 arquivos .spectra dentro de tests/ e 175 scripts validate_*.py; esses números representam arquivos encontrados, não testes aprovados. Três revisões paralelas cobriram frontend/semântica, execução/runtime e bibliotecas AI/ML/API/DB.

Foram consultados diretamente os comandos --list-experimental e --help, além de release-info em JSON usando a raiz do repositório. Não foram executados cargo test, run_tests.ps1, validadores Python, exemplos JIT/AOT, benchmarks nem serviços externos. Portanto, as referências a testes abaixo indicam cobertura existente no checkout, não resultado de execução nesta auditoria.

O harness configura validate_execution_coverage.py --mode both para fixtures de tests/validation e tests/control_flow que têm main, exercitando JIT e AOT. Isso é a cobertura planejada pelo runner, não um resultado desta execução.

O checkout já continha alterações fora do escopo desta revisão:

- modificações em scripts/stdlib_contract.toml e scripts/validate_r3308_collections.py;
- novos fixtures tests/validation/622_stdlib_collection_growth.spectra até 629_stdlib_collections_graph_search.spectra.

Essas alterações foram preservadas. Como não foram executadas, não são tratadas como evidência de aprovação nem como comportamento validado.

## Matriz geral

| Área | Estado observado |
|---|---|
| Sintaxe e fluxo de controle básicos | Núcleo estável amplo; alguns recursos presentes no parser têm lacunas semânticas concretas |
| Módulos, imports, records, enums, traits e generics | Implementados na superfície validada; class e várias extensões do sistema de tipos não fazem parte do contrato |
| JIT e AOT | Implementações nativas reais via Cranelift; AOT é para a máquina host, sem target cruzado constatado |
| Async e reactor | Implementação real de tarefas, I/O e timers; maturidade beta e limites de falha/integração |
| Tensor e autodiff CPU | Baseline real e extenso; kernels SIMD em algumas operações |
| Tensor GPU | WGPU real, opcional e parcial, com fallback CPU |
| BLAS | Não implementado; feature Cargo vazia |
| ONNX | Inferência real quando compilada com onnx; formatos/modelos não são universais |
| RAG | Índice HNSW real sobre vetores fornecidos; embeddings e avaliação dos exemplos não demonstram relevância semântica |
| Treinamento distribuído | API antiga simulada; novas APIs treinam de verdade, mas cluster externo e hardening têm limites |
| Serving | Inferência de modelo real em fila local; benchmark é sintético e não mede carga de endpoint remoto |

| API HTTP | Implementações reais de sockets/protocolos; o gate de conformidade cobre somente parte da superfície |
| Banco de dados | SQLite local real; PostgreSQL/Redis dependem de serviços configurados para prova de integração |
| Packages e tooling | CLI/LSP e fluxos locais reais; registry central hospedado, auth e resolução ampla permanecem ausentes |
| Interop | C ABI e NPY estreitos existem; não há mecanismo geral de extern/bindings nativos constatado |

## 1. Linguagem, parser e semântica

### Implementado no núcleo

A política de maturidade lista como estáveis módulos e projetos multifile; imports por caminho, alias e reexport; visibilidade public/internal; funções e métodos; records, enums, traits, impl; a superfície validada de generics e dyn Trait; tipos primitivos, tuplas e tipos de função; const e static; e o fluxo de controle com condicionais, loops, switch, match, retorno, break e continue.

Há cobertura de padrões de tupla, struct, enum e OR-patterns. Closures por valor têm cobertura validada; capturas mutáveis e ambientes mais gerais não têm o mesmo contrato.

O lexer implementa strings, chars, escapes, f-strings, comentários e formas numéricas hexadecimais, octais, binárias, com _ e expoente. Existem fixtures executáveis para parte significativa dessas construções. Identificadores Unicode e strings raw continuam fora do contrato.

### Lacunas semânticas em recursos que já têm sintaxe

| Recurso | Código encontrado | Limite observado |
|---|---|---|
| Guards de match | Parser cria MatchArm.guard (expression_primary.rs:514-540); lowering avalia a expressão e gera branch condicional (midend/src/lowering_expr_match.rs:130-145) | analyze_expression_match não analisa nem tipa o guard (semantic_expression_matches.rs:7-45). A exaustividade considera _/binding como catch-all sem verificar se o arm tem guard (semantic_exhaustiveness.rs:15-32). Não foram encontrados fixtures .spectra para essa forma |
| Operador ? | Parser cria ExpressionKind::Try (expression_precedence.rs:603-609); lowering separa sucesso/erro e extrai o payload (midend/src/lowering_expr_tail.rs:71-105) | A semântica analisa apenas o operando (semantic_expression_tail.rs:83) e a inferência retorna seu tipo original (semantic_expression_inference.rs:657). Não valida que seja Option/Result, não extrai o tipo de sucesso nem valida o tipo de retorno da função. Não foram encontrados fixtures .spectra para ? |
| Mutabilidade local | A AST de LetStatement não contém flag de mutabilidade (compiler/src/ast/mod.rs:395-404); assignment verifica identificador e tipo (semantic_statements.rs:108) | let mut x é rejeitado com erro genérico de parser P019 (statement.rs:234-263). Bindings locais sem mut são reatribuíveis por padrão; não há checagem de imutabilidade |
| Ownership e lifetimes | Há análise de alguns usos após free/free_all conhecidos (semantic_use_after_free.rs) | A análise é local/restrita e baseada em nomes de chamadas; não equivale a borrow checker ou sistema geral de ownership. Não há sintaxe geral de referências/lifetimes |

Arquivos centrais para os dois primeiros casos: compiler/src/parser/expression_primary.rs, compiler/src/ast/mod.rs, compiler/src/semantic/semantic_expression_matches.rs, compiler/src/semantic/semantic_exhaustiveness.rs, compiler/src/semantic/semantic_expression_tail.rs, compiler/src/semantic/semantic_expression_inference.rs e midend/src/lowering_expr_match.rs / lowering_expr_tail.rs.

Há inconsistência documental adicional para mutabilidade: docs/AI-AGENT-REFERENCE.md:219 diz que let mut é aceito sintaticamente; a implementação atual não consome mut no parser de let.

### Reservado ou fora do contrato

- class, herança, override, super, layout de classes e ABI são reservados; o parser rejeita declaração class com P007. O modelo suportado é baseado em struct/enum/trait/impl.
- Identificadores Unicode, raw strings, repeat/until, foreach, goto, yield, lifetimes, higher-kinded types e slice patterns não têm implementação/contrato identificado.
- O sistema de atributos não é um sistema geral de macros: há tratamento dedicado para derives JSON; atributos desconhecidos em funções podem ser ignorados.
- O protocolo tipado de iteradores ainda é beta; ranges e coleções selecionadas não passam todos por uma única rota tipada.
- Arrays e coleções tipadas também são beta, com gaps de ergonomia/performance e cobertura em expansão.
- Option/Result são beta. A propagação estruturada de erro cobre a fatia atual de filesystem, não toda a stdlib/API.

CompilationPipeline::new() pode usar NoopBackend (compiler/src/pipeline.rs:97) para checagem semântica e parar antes de lowering/execução. Isso é uma rota de análise da biblioteca; a CLI constrói o backend completo. Não se deve confundir um pipeline de frontend com a rota executável normal.

## 2. Backend, execução e memória

### O que executa de verdade

O JIT e o AOT usam Cranelift. A CLI gera e executa o main JIT (tools/spectra-cli/src/compiler_integration_core.rs:568); AOT emite objeto/executável nativo e chama um linker instalado (backend/src/aot.rs:175; tools/spectra-cli/src/linker.rs:164). O runtime de host calls é real, com registry, despacho e integração no JIT/AOT (runtime/src/ffi_host_registry.rs:130). Não é um interpretador/mock.

O AOT observado seleciona a arquitetura nativa e o linker da máquina; não foi constatada seleção geral de target cruzado. O contrato de retorno do main também não é idêntico: o caminho JIT aceita void, int e bool, enquanto o shim AOT converte uma faixa mais ampla de retornos para exit code. A semântica não demonstrou nesta revisão um contrato unificado para esses casos.

### Memória: rastreada, mas não GC nem ownership seguro

O runtime usa alocações rastreadas por frames e limpeza manual; não foi encontrado coletor de lixo. O diagnóstico de use-after-free existente cobre casos reconhecidos pelo analisador, não substitui ownership/lifetimes no sistema de tipos.

Risco estático importante: spectra_rt_manual_alloc pode retornar nulo ao exceder o limite (runtime/src/ffi_fast_paths.rs:82), e o codegen associa o resultado sem checar nulo antes de operações subsequentes de memória (backend/src/codegen_instruction_memory.rs:47). Isso pode transformar pressão de memória em acesso inválido em vez de diagnóstico de runtime Spectra.

Há ainda dois limites de contabilização/comportamento:

- a quarentena de blocos liberados retém memória para detectar double-free, mas a contabilização de bytes vivos é reduzida antes do descarte da quarentena; o indicador não mede toda a memória residente;
- se alocação rastreada de literal JIT falha, o fallback armazena o texto em Box não rastreado, com ressalva do próprio código para comparação de chaves de coleções.

### TensorGraph e alegações de otimização

TensorGraph extrai e valida operações tensor do IR e há infraestrutura de fusão/dumps/snapshots. Porém, JIT/AOT chamam validate_tensor_ir e descartam o grafo retornado (backend/src/codegen.rs:205, codegen_core.rs:65 e aot.rs:257); a emissão segue pelo IR convencional. Assim, a existência de grafo fundido/validado não demonstra que esse grafo seja compilado em kernels nativos fusionados. As operações tensor individuais continuam reais no runtime, mas a execução otimizada do grafo precisa de evidência separada.

## 3. Async e concorrência

Async/await não é apenas sintaxe: existem frames de coroutine, poll/drop, lowering para suspend/resume, tarefas/streams e reactor baseado em I/O readiness. O reactor usa mio e o backend de plataforma correspondente quando suportado.

A maturidade oficial é beta. A existência de epoll/IOCP/kqueue não significa por si só um executor completo ou suporte idêntico em todos os sistemas. O reactor entrega prontidão; as outras camadas realizam a semântica de tasks/streams.

Foi encontrada uma falha de robustez no caminho de timers: erro ao iniciar a thread de timers é absorvido e register_timer não retorna falha, então um timer pode não disparar (runtime/src/reactor/mod.rs:388). Se criação de mio::Poll falha, há fallback, mas o rótulo do backend ainda reflete o sistema operacional selecionado (runtime/src/reactor/mod.rs:354).

O harness contém validadores de frontend, lowering, reactor, structured concurrency, streams e async stdlib. Esses gates não foram executados nesta auditoria.

## 4. Tensor, numerics e AI/ML

### Tensores e autodiff

A API CPU de tensor implementa handles, metadados de shape, operações elementwise, reduções, transformações, matmul e partes de batched matmul. Há autodiff reverse-mode para o contrato atual, além de métricas e kernels SIMD seletivos (AVX2/NEON onde disponíveis).

As seguintes limitações impedem tratar o domínio inteiro como “GPU/BLAS completo”:

- runtime/Cargo.toml declara blas = []; não há dependência BLAS nem call sites de kernel que usem essa flag.
- WGPU é opcional (runtime/Cargo.toml, feature gpu) e cobre um subconjunto de operações em tensor_helpers_kernels.rs e tensor_autograd_gpu.rs. Outras rotas recorrem à CPU; o caminho GPU trabalha com precisões/conversões e operações suportadas, não com todos os kernels/autograd.
- A validação de TensorGraph não prova codegen do grafo no backend.
- CUDA, ROCm, Metal, DirectML e Vulkan não estão no baseline citado pela política.
- Sintaxe tensor tipada e shape metadata já existem, mas tensor syntax/shape types de produção mais gerais permanecem adiados.

### ONNX, geração e embeddings

Quando compilado com onnx, o runtime usa ONNX Runtime real para sessões e inferência (runtime/src/stdlib/ml_onnx.rs); sem essa feature, os caminhos retornam indisponibilidade em vez de fabricar uma resposta. ONNX é opcional e pode buscar bibliotecas nativas no build. O código cobre subconjuntos/formatos de tensor específicos; não prova compatibilidade com qualquer grafo ONNX ou modelo arbitrário.

std.ml.text_embed_model também pode usar inferência ONNX real. A disponibilidade depende da mesma configuração de build e de modelo/tokenizer.

### RAG

O índice vetorial HNSW, persistência e busca são implementação real (runtime/src/vector_index.rs), mas consomem vetores fornecidos. Helpers de chunking e montagem de prompt operam sobre janelas de texto (runtime/src/stdlib/ml_tokenization_retrieval.rs); métricas de resposta usam sobreposição lexical de tokens. Os exemplos demonstrativos usam vetores constantes e resposta fixa.

Logo, há uma caixa de ferramentas de retrieval e adaptadores de embedding, mas os fixtures não provam um pipeline semântico completo nem qualidade de recuperação/geração. Para demonstrar semântica real, é necessário usar um modelo de embedding/geração real e avaliar contra um conjunto rotulado.

### Treinamento distribuído: APIs diferentes

| Família | Comportamento |
|---|---|
| API antiga: distributed_session_start, distributed_worker_step, distributed_global_step e checkpoint/resume | Simulação de contadores. O chamador fornece amostras/perda; a API não calcula gradientes, não atualiza pesos e não comunica workers pela rede |
| APIs novas: distributed_train_multithread, distributed_train_tcp e versões com dataset | Treinamento real: forward/backward, agregação de gradientes e atualização SGD em workers |

O runner TCP implementa framing, mensagens, retry/idempotência e autenticação opcional por token. O caminho embutido cria workers internos em loopback; isso não torna o gerenciamento de um cluster externo automático. Há teste Rust para dois processos e um script de namespaces de rede Linux que exige permissões/ferramentas específicas. Não executei esses gates.

O checkpoint legado ainda lê o formato antigo de topologia simulada. Maturidade e contrato devem ser classificados por função/API, não pelo prefixo std.ml.distributed_* inteiro.

### Serving e benchmark

std.serve executa inferência de um modelo registrado; o benchmark chama essa inferência real. A entrada de benchmark, entretanto, é gerada sinteticamente e processada localmente em sequência/batches. Não mede tráfego HTTP/gRPC, concorrência externa, latência sob rede ou residência distribuída de modelos.

## 5. API, protocolos e banco de dados

### API HTTP e protocolos

- HTTP/1.1 parser, server e client usam sockets reais; há TLS real via rustls.
- HTTP/2 usa h2 e tem cobertura local. Extended CONNECT/WebSocket sobre H2 é recusado como não implementado.
- HTTP/3 usa h3/QUIC e está ligado pela feature padrão http3 do crate spectra-api; build sem defaults pode removê-lo.
- GraphQL usa async-graphql com schema/resolvers e limites de profundidade/complexidade.
- gRPC implementa runtime H2, envelopes e chamadas unary/streaming. Compressão de mensagens é rejeitada; não foi encontrada geração completa de cliente/serviço a partir de .proto.
- WebSocket, SSE, routing, middleware, auth e REST têm código/fixtures dedicados; isso não torna todas as certificações externas completas.

O gate de conformidade API v0 cobre um recorte de HTTP/1, JSON e routing; não incorpora toda a matriz de H2, H3, TLS, GraphQL e gRPC. Portanto, um gate v0 aprovado não equivaleria à conformidade integral da plataforma.

### Banco de dados e observabilidade

- SQLite usa rusqlite bundled. Há integração local com CRUD, DDL, transações e casos REST+SQLite.
- PostgreSQL tem driver/pool/transactions/COPY/notifications reais; os testes saem sem validar integração se SPECTRA_POSTGRES_URL não estiver configurado. A prova estrita requer o serviço esperado e sua versão.
- Redis tem driver/comandos/TTL/pub-sub/pool reais; de forma semelhante, os testes podem sair sem validação quando SPECTRA_REDIS_URL não existe.
- A camada de query observada cobre SQLite e PostgreSQL; não foi encontrado dialeto MySQL.
- Health, métricas e tracing têm implementação local. Exportação a OTLP precisa de endpoint/collector de verdade.

Não houve serviço PostgreSQL, Redis, OTLP ou endpoint TLS externo conectado durante esta auditoria. O código real e os testes existentes não devem ser reportados como integração externa aprovada sem executar os gates estritos com os serviços configurados.

## 6. Tooling, packages e interop

### CLI e LSP

A CLI fornece compile/check/run/lint/bench/repl/new/package/db/fmt, release-info, surface, impact, docs e explain. LSP implementa hover, definition, references, rename, completion, diagnostics, formatting, inlay hints, quick fixes e semantic tokens. Esta revisão inspecionou a superfície e os contratos; não executou validação de cliente/editor.

O conjunto de comandos do CLI não significa que cada subcomando tenha uma integração remota de registry ou cobertura operacional de produção.

### Packages

Há suporte real para lock/build/check/run/test/bench/doc/add/update, workspace, dependências por path, registry local com checksum e dependências Git com commit pinado/catalog. O contrato ainda é estreito: registry central hospedado, autenticação, sync de catálogo remoto, enforcement de --locked e resolução de ranges semver estão adiados.

### Interop

Existe crate de interop e ABI C com operações numéricas e troca .npy v1 little-endian de vetor f64; existe bridge Python. Isso é um baseline útil, não bindings gerais. Não foi encontrado mecanismo Spectra de extern para declarar/importar bibliotecas nativas arbitrárias nem opção geral de link para bibliotecas estrangeiras.

## 7. Simulações, mocks e no-ops encontrados

| Item | O que de fato faz | O que não prova |
|---|---|---|
| --enable-experimental feature | Aceita compatibilidade; não há gates ativos | Habilitação de uma feature experimental |
| Feature Cargo blas | Está vazia; nenhum kernel a consome | BLAS, GEMM otimizado ou aceleração por biblioteca externa |
| API legada distributed_worker_step | Persiste contadores/perda entregues pelo chamador | Treinamento/gradientes/cluster |
| Provider mock/echo | Resposta determinística e embedding de hash | Inferência, similaridade semântica ou qualidade de LLM |
| Fixtures de RAG | Vetores constantes e resposta fixa em exemplos selecionados | Recuperação semântica avaliada |
| Benchmark de serving | Mede inferência real sobre input sintético local | Throughput/latência de endpoint de rede sob carga externa |
| Fallback CPU | Mantém operações disponíveis sem WGPU ou sem kernel GPU aplicável | Execução GPU ou ganho de desempenho |
| Testes com HTTP transport/servidor mock | Exercitam tratamento de protocolo e governança de modo determinístico | Disponibilidade/conformidade do serviço remoto |

Simulação não significa necessariamente que a API toda seja falsa: há famílias antigas simuladas ao lado de novos caminhos reais. O relatório distingue as unidades para não classificar um namespace completo pela implementação de um único símbolo.

## 8. Documentação e contratos que precisam de sincronização

No snapshot estático inicial foram encontradas afirmações fora de sincronia com o código então observado. As correções e o estado posterior estão registrados na seção 12:

1. docs/language-feature-maturity.md tem data de 19/08/2026. A seção adiada sobre números ainda fala em formas avançadas além das formas decimais atuais, mas o lexer e fixtures já cobrem hexadecimal, octal, binário, separadores e expoentes.
2. A mesma política descreve somente a antiga simulação distribuída; a nova família distributed_train_* executa gradientes e transporte real. A atualização deve separar os dois contratos.
3. docs/frontend/frontend-coverage-audit.md contém claims antigos sobre números, class, OR-patterns e gates P004, contrariados pelo lexer/parser/maturidade atuais.
4. docs/frontend/parser-coverage-audit.md é datado de 2025 e está desatualizado em comentários, escapes, OR-patterns, guards e generics.
5. docs/semantic/semantic-coverage-audit.md marca pattern guards como suportados, embora o analisador semântico atual não os valide nem os considere corretamente para exaustividade.
6. docs/AI-AGENT-REFERENCE.md:219 diz que let mut é aceito, enquanto o parser atual o rejeita.
7. docs/reference/04-avancado.md:547-574 promete para ? um contrato que a inferência/análise semântica não implementa hoje.
8. compiler/src/parser/README.md mostra sintaxe antiga como fn, ->, pub e semicolons.
9. scripts/language_stability_contract.toml é útil como contrato tipado, mas descreve só uma fração das superfícies do produto; não é uma matriz completa do estado da linguagem.

Roadmap e documentos de maturidade são contexto, não prova executável. Os testes e o código atual prevalecem para este snapshot; a correção destas afirmações documentais não foi feita nesta tarefa.

## 9. Prioridades sugeridas

1. **Corrigir a semântica de match guard e ?:** analisar/validar tipo e bindings, ajustar exaustividade e acrescentar fixtures que atravessem check, JIT e AOT.
2. **Definir o contrato de mutabilidade:** decidir se o design aceita mutabilidade implícita ou exige mut; alinhar parser, AST, semântica, docs e testes. Se ownership não for objetivo, documentar claramente o limite e reforçar a verificação de UAF onde o runtime exige segurança.
3. **Fechar o caminho de alocação:** propagar falha de alocação como erro controlado antes do uso de ponteiro; revisar contabilidade de memória/fallback de literais.
4. **Separar validação e execução tensor:** se houver claim de graph fusion acelerando runtime, integrar o grafo ao codegen ou limitar explicitamente o claim a análise/validação.
5. **Sincronizar maturidade e docs** conforme as nove divergências desta auditoria, sem fundir APIs simuladas e reais sob prefixos amplos.
6. **Executar os gates condicionais** em ambientes com GPU, PostgreSQL, Redis, OTLP e endpoints/modelos configurados antes de elevar maturidade ou declarar integração certificada.
7. **Distinguir benchmarks locais de carga de serviço** e exemplos com mock de avaliação de modelo real nos relatórios de performance/AI.

## 10. Referências principais

- Política de maturidade: docs/language-feature-maturity.md
- Contrato de estabilidade parcial: scripts/language_stability_contract.toml
- Harness central e cobertura executável: run_tests.ps1
- Parser/AST/semântica: compiler/src/parser/, compiler/src/ast/mod.rs, compiler/src/semantic/
- Lowering e TensorGraph: midend/src/lowering_expr_match.rs, midend/src/lowering_expr_tail.rs, midend/src/
- Backend/runtime: backend/src/, runtime/src/
- Tensor/ML: runtime/src/stdlib/tensor_helpers_kernels.rs, tensor_autograd_gpu.rs, ml_onnx.rs, ml_tokenization_retrieval.rs, ml_experiments_distributed.rs, ml_distributed_tcp.rs, serve_api.rs

- API/DB: packages/spectra-api/src/, packages/spectra-db/src/
- Interop: tools/spectra-interop/src/

**Conclusão do snapshot estático:** a base do compilador e muitos subsistemas são implementação real, com um núcleo de linguagem amplo. O estado observado não justificava chamar a plataforma inteira de completa ou totalmente certificada: havia recursos beta, integrações condicionais, simulações deliberadas, gaps semânticos em operadores já baixados e inconsistências entre documentação e comportamento. Naquele ponto esta auditoria não havia executado testes, então identificava a implementação e a cobertura disponível sem declarar que os gates passavam. A evidência posterior consta na seção 12.

## 11. Correções e evidências executadas em 29/09/2026

Esta seção complementa o snapshot estático acima; não reclassifica uma capacidade como certificada apenas por ter sido documentada. Os resultados abaixo foram produzidos no checkout local após as correções e devem ser lidos junto às limitações de hardware, serviços e recursos opcionais descritas nas seções anteriores.

### 11.1 Semântica do núcleo

| Superfície | Estado implementado | Evidência |
|---|---|---|
| Guards em `match` | O guard é analisado no escopo das bindings do padrão e precisa ser `bool` (`E040`). Um braço guardado não cobre uma variante ou wildcard incondicionalmente (`E031`). | `tests/validation/630_language_core_guards_try_mut.spectra`; `tests/errors/match_guard_non_bool.spectra`, `match_guard_exhaustiveness.spectra` e `match_guarded_wildcard_exhaustiveness.spectra`; `check`, JIT e AOT aprovados. |
| Propagação `?` | Aceita apenas `Option<T>`/`Result<T, E>`, infere o valor de sucesso e valida o contexto de retorno; operandos/contextos inválidos produzem `E049`/`E050`. | `tests/errors/try_invalid_operand.spectra`, `try_incompatible_context.spectra` e caso positivo em fixture 630; verificações semânticas e execução aprovadas. |
| `let mut` | O parser consome `mut` como marcador redundante. Bindings locais já são reatribuíveis por padrão; o marcador não cria uma segunda regra de mutabilidade. | Caso positivo em `tests/validation/630_language_core_guards_try_mut.spectra`, aprovado pelo checker, JIT e AOT. |

### 11.2 Alocação, memória e timers

- Os caminhos FFI de alocação tratam falhas e resultados nulos antes de expor um ponteiro utilizável; reservas que podem falhar propagam erro controlado.
- O limite de `ManualHeap` contabiliza bytes vivos e bytes mantidos na quarentena sob a política configurada. A telemetria separa as duas parcelas, preservando `ManualStats.bytes` como bytes vivos.
- A falha ao alocar string literal no JIT retorna `BackendErrorKind::AllocationFailed`; não há fallback de `Box` fora do rastreamento.
- O registro de timer informa falha de startup ao chamador. O backend de reactor efetivo é reportado corretamente em fallback, e timeouts que não puderam ser registrados retornam erro com rollback dos recursos parcialmente registrados.
- Evidência: `cargo test -p spectra-runtime` passou com 258 testes unitários e 1 integração de reactor; `cargo test -p spectra-backend` passou com 77 testes, incluindo a falha de alocação de literal injetada deterministicamente.

### 11.3 TensorGraph: grafo otimizado versus execução

- O backend JIT/AOT aplica fusão real de cadeias unárias de CPU suportadas (`neg`, `relu`, `sigmoid_f`, `tanh_f`, `sqrt_f`, `log_f`), limitadas a oito operações e a um caminho SSA com uso único comprovado. Uso observável do intermediário, efeitos ou caminhos não reconhecidos impedem a fusão; a compilação não afirma uma fusão que não conseguiu aplicar.
- A operação fusionada preserva valores, tipo, forma, dispositivo, contabilidade de kernel e autodiff. A fixture `tests/validation/640_tensor_graph_fused_unary.spectra` verifica `relu → tanh_f`, `requires_grad`, gradiente e exatamente um kernel fusionado; JIT e AOT mais a execução do binário passaram.
- No exemplo `relu → tanh_f → sum_t`, a cadeia unária é executada em um kernel e a redução em outro. A fusão `FusedReduction` continua disponível na otimização de grafo, mas não é emitida como kernel único pelo backend. `examples/ai/tensor_graph_reduction_fusion.spectra` agora exige exatamente os dois kernels executados.
- `planned_buffers`, `peak_live_buffers` e `reusable_edges` são estimativas/metadados do planner; não comprovam por si só alocação menor nem economia de memória em runtime. O relatório R-2904 passou no caminho CPU/JIT/AOT, mas registrou WGPU como `skipped_environment` porque não havia adaptador. Isso não certifica execução GPU neste host.
- Evidência: `cargo test -p spectra-midend` passou com 115 testes; `cargo test -p spectra-backend` passou com 77; o fixture 640 passou em JIT e AOT; os exemplos elementwise e reduction passaram após alinhar asserções com a quantidade de kernels realmente executada.

### 11.4 Maturidade e documentação

Os contratos e referências abaixo foram alinhados com o comportamento observado: `docs/language-feature-maturity.md`, `docs/frontend/frontend-coverage-audit.md`, `docs/frontend/parser-coverage-audit.md`, `docs/semantic/semantic-coverage-audit.md`, `docs/AI-AGENT-REFERENCE.md`, `docs/reference/04-avancado.md`, `compiler/src/parser/README.md`, `scripts/language_stability_contract.toml` e `docs/architecture/r1602-graph-optimization-fusion.md`. A distinção entre APIs distribuídas simuladas e `distributed_train_*`, mocks, vetores RAG fixos, BLAS reservado, provedores condicionais, planner versus execução e conformance de API limitada foi mantida. As nove divergências da seção 8 são históricas do baseline, não uma lista de mudanças ainda pendentes.

### 11.5 Validação integrada e limites

| Verificação | Resultado observado |
|---|---|
| `run_tests.ps1` no estado integrado antes dos ajustes finais do exemplo | 1.010 passaram, 3 falharam, 1 foi ignorado por ambiente (1.013 com resultado esperado; 99,7%). |
| Exemplo `tensor_graph_reduction_fusion.spectra` | A falha era uma asserção antiga (`>= 3`) incompatível com a fusão unária real; alterada para exigir exatamente 2 kernels e `spectralang run` passou. |
| R-2001 conformance | Reexecutado depois da correção; certificação passou, incluindo os 21 exemplos de IA. |
| R-2013 release candidate | Reexecutado depois da correção; status passou, 8/8 projetos integrados passaram e 0 falhas não rastreadas. |
| R-3208 host-call generation | The generated catalog contains 1,352 entries; the API table has 560 bindings (532 always-on and 28 feature-gated), with 1,115 lowering arms across 7 generated tables. |
| R-3308 collections | Passou na suíte integrada, incluindo build release, JIT/AOT e benchmark independente. O relatório de performance foi atualizado em `docs/performance/phase33/r3301-collections.json`. |

`run_tests.ps1` não foi reexecutado por inteiro após corrigir a asserção do exemplo. Portanto, a execução central citada acima terminou com código diferente de zero, embora cada gate que falhou tenha sido corrigido ou reexecutado isoladamente com sucesso. O WGPU não tinha adaptador disponível; GPU em hardware, PostgreSQL/Redis/OTLP externos, e endpoints/modelos opcionais não são certificados por esta execução. Testes cujo contrato permite omissão sem a configuração de serviço continuam sendo evidência local, não integração externa.

`git diff --check` passou. `cargo fmt --all -- --check` ainda falha com 494 diferenças de formatação espalhadas pelo workspace; não foi feita reformatagem ampla para evitar alterações fora do escopo. `rustfmt --check` individual passou em `backend/src/codegen.rs`, `backend/src/tensor_graph_codegen.rs`, `runtime/src/stdlib/tensor_autograd.rs` e `runtime/src/stdlib/tensor_ops_reductions.rs`.
