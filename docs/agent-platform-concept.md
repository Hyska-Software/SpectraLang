# `spectra.agent` — Agentes como cidadãos de primeira classe

**Status:** documento de conceito. Não é implementação e não é item de roadmap.

**Convenção de leitura:**

- `[existe]` — verificável no repositório hoje, com caminho de arquivo
- `[proposto]` — design a implementar; nomes e assinaturas são alvos, não contrato
- `[lacuna]` — dependência ausente que precisa ser resolvida antes

Este documento refina o conceito de uma biblioteca nativa de programação orientada a
agentes ("Rust + `agents`") para a SpectraLang. A filosofia do original é preservada:
humanos continuam programando normalmente, e agentes recebem uma representação
explícita, previsível e verificável do software. O que muda é o mecanismo — e a
quantidade de sintaxe que o programador precisa escrever.

---

## 1. Tese

Não é uma linguagem para IA. É uma linguagem **cujo artefato é legível, limitável e
reproduzível por agentes** — e que continua agradável para humanos.

Quatro regras de projeto, na ordem em que decidem o resto:

1. **Derivar, não declarar.** O que o compilador já sabe — efeitos, dependências,
   schema, capacidades, durabilidade — não se escreve de novo. Metadado duplicado
   apodrece e mente.
2. **Enforçar, não documentar.** Permissão que não bloqueia nada é comentário com
   sintaxe.
3. **Observar, não inferir.** O agente não deve deduzir do texto o que o runtime pode
   reportar de forma estruturada.
4. **Uma linha, uma decisão.** Aninhamento e cerimônia são custo de geração: quanto
   mais profunda a estrutura, mais o humano erra e mais o agente precisa de reparo.

### 1.1 As cinco regras de sintaxe

Valem para tudo o que este documento propõe. Se uma proposta as violar, a proposta
está errada.

| Regra | Consequência |
|---|---|
| **Um atributo por função, no máximo** — e apenas para o que não é derivável (a descrição) | Sem `#[derive(Agent)]`, sem `#[effects]`, sem `#[requires]` |
| **Nada de configuração aninhada** | Um record raso, com campos nomeados; listas em campos de lista |
| **Nomes por extenso, sem abreviação** | `approve`, `remember`, `recall` — não `apr`, `mem`, `rc` |
| **O efeito é derivado, a intenção é explícita** | Ninguém escreve efeito; todo mundo escreve o que quer do modelo |
| **Uma forma canônica, garantida pelo formatter** | O agente não escolhe entre dois jeitos de dizer a mesma coisa |

O resultado pretendido é que um trecho de código Spectra com agentes seja legível por
um humano que nunca viu a biblioteca, e gerável por um agente com baixa taxa de erro —
sem que nenhum dos dois precise conhecer detalhes de implementação.

---

## 2. O conceito original e o que muda

O conceito original — uma biblioteca `agents` para Rust com `Agent`, `Tool`, `Memory`,
`Intent`, `permissions`, `contracts`, `transaction` e `inspect` — parte da intuição
certa: tornar explícito o que hoje o agente infere. As regras da seção 1 se aplicam aos
oito pilares, e o resultado é este:

| Pilar | No conceito original | Por que não funciona assim | Na SpectraLang |
|---|---|---|---|
| Permissões | `fn permissions() -> Vec<Permission>` e "o runtime bloqueia" | Em Rust `std::fs`, `std::net` e qualquer crate HTTP continuam alcançáveis; o bloqueio é contornável por construção | `[existe]` todo efeito externo é uma host call nomeada com um único ponto de dispatch: dentro de um run, a checagem é total |
| Efeitos | `#[effects(DatabaseWrite, NetworkRequest)]` declarado à mão | Duplica o que já se sabe e pode mentir | Derivados do IR: cada `InstructionKind::HostCall` carrega o nome do efeito. **Não se escreve** |
| Introspecção | `inspect::dependencies::<T>("m")` como função de biblioteca | Impossível: macro alguma atravessa do item anotado para os chamadores. Exige o call graph do programa | Saída de CLI sobre o IR: `surface --json`, `impact --json` |
| Metadados de agente | `#[derive(Agent)]` na struct gerando métodos, dependências e efeitos | Uma derive macro só enxerga a própria `struct`; blocos `impl` são invisíveis | Sem atributo de agente. Um agente é uma função comum com um `Run` |
| Ferramenta | `#[derive(Tool)]` num record + `impl` com `execute` | Três declarações para uma coisa: o tipo, a anotação e o corpo | Uma função pública com **um** atributo: `#[agent_tool("descrição")]` |
| Transações | `transaction!` reverte `filesystem.write` e um POST HTTP | Não existe rollback de filesystem nem de rede | Durabilidade automática + compensação declarada só onde é irreversível |
| Contratos | `#[requires]`/`#[ensures]` "usados pelo compilador" | Não há verificação; viram asserção de runtime e ficam caros em `async` | `require(run, condição, mensagem)`: uma linha, no corpo, com erro legível |
| Intenção | `#[intent(preserve = "order.total >= 0")]` | Strings não são predicados verificáveis; apodrecem como comentário | `goal` do run, citado por testes e evals — executado, não armazenado |
| Memória | `#[derive(Memory)] struct OrderMemory` | Reimplementaria o que já existe no runtime | `[existe]` `std.ml.vector_index_*`, `std.ml.rag_*`, `std.ml.tokenizer_*`; a superfície são duas funções: `remember`, `recall` |

Em uma frase: o conceito original pede que humanos **declarem** intenção para agentes.
A SpectraLang pode ser uma linguagem em que o compilador **sabe** a resposta.

---

## 3. Base existente e lacunas

### 3.1 O que já existe e sustenta o conceito

| Peça | Onde | Por que importa aqui |
|---|---|---|
| Todo efeito externo é host call nomeada | `runtime/src/ffi_host_registry.rs:226`, dispatch em `runtime/src/ffi_lifecycle.rs:280` | Ponto único de enforcement — o *reference monitor* |
| Ausência de FFI/`extern` na linguagem | `compiler/src/token.rs:5` (nenhuma keyword equivalente) | Não há escape hatch para contornar capacidades |
| IR carrega o nome do host call | `midend/src/ir.rs:276-280` | Efeitos e dependências são deriváveis, não declarativos |
| Classificação de host calls | `runtime/src/abi.rs:608` | Base para derivar o vocabulário de capacidades |
| Catálogo de contrato | `packages/spectra-contract/catalog/stdlib.toml` (`path`, `kind`, `signature`, `abi`, `effects`, `error_model`, `maturity`, `owner`, `docs`, `fixture`) | Metade da introspecção já é dado estruturado |
| Derivador existente | `midend/src/lowering_json_derive.rs`, validação em `compiler/src/semantic/semantic_json.rs` | O schema sai do derive que já existe; sem sintaxe nova |
| `Result`/`Option`/`?`/`if let` | `docs/AI-AGENT-REFERENCE.md` §15 | Erro é valor; nada de exceção escondida |
| async/await/`Task<T>`/`Stream<T>` | `midend/src/lowering_async.rs:17-83`, hosts em `runtime/src/stdlib/registration.rs:865-955` | Um agente é um loop assíncrono; a base é real |
| `std.ml`: tokenizer, ONNX, `generate`/`generate_ex` (KV-cache, `temperature`/`top_k`/`seed`), `text_embed_model`, `vector_index_*`, `rag_*` | R-1803 (validado) | Embeddings locais, índice vetorial e RAG prontos |
| `std.api`: client HTTP (TLS, pool, SSRF), server, WS, SSE, gRPC, GraphQL, DB, trace, health | `packages/spectra-api/` | Provider remoto e observabilidade sem reimplementar nada |
| Diagnósticos JSON/SARIF, exit codes 0/64/65/74, `fmt --explain` | `tools/spectra-cli/src/cli_parse_options.rs:116-141`, `main.rs:36-41` | Loop de reparo do agente já tem contrato |

### 3.2 O que falta

| Lacuna | Evidência | Consequência para o conceito |
|---|---|---|
| Sem sistema de macros | `compiler/src/semantic/semantic_json.rs:57-64` — atributo desconhecido é erro; só 3 atributos existem | O único atributo novo proposto (`agent_tool`) é mudança de compilador, e por isso tem de ser **um só** |
| Sem `extern` nativo declarativo em pacotes | `packages/spectra-api/src/bindings/http.spectra:50-52` | Nenhuma superfície nativa nova entra sem tocar runtime + midend + compilador |
| Superfície em quatro cópias manuais | `packages/spectra-api/tests/contract_drift.rs:1-17` | Multiplicar módulos multiplica a dívida; precisa de fonte única antes |
| Sem subprocesso | ausente da superfície e do catálogo | Servidores MCP via stdio não podem rodar de dentro da linguagem |
| Sockets TCP/UDP registrados mas órfãos | `runtime/src/stdlib/registration.rs:912-923`; gate só aceita paths `std.*` em `midend/src/lowering_std_host.rs:3-33` | Transporte existe, superfície não |
| Sem capacidades no dispatch | `runtime/src/ffi_host_registry.rs:226` grava apenas nome → ponteiro | Governança precisa ser construída, mas o ponto de inserção já é o certo |

---

## 4. Arquitetura em três camadas

```text
┌─ A. SUPERFÍCIE (derivada, estática) ────────────────────────────────┐
│  spectralang surface --json · impact --json · explain --json        │
│  Fonte: tipos + SIR + catálogo de contrato                          │
│  Consumidor: o agente que escreve e edita código Spectra            │
└─────────────────────────────────────────────────────────────────────┘
                                │ informa
┌─ B. GOVERNANÇA (enforçada, runtime) ────────────────────────────────┐
│  capacidades · taint · aprovação · orçamento · durabilidade · trace │
│  Ponto único: dispatch de host call                                 │
│  Consumidor: o runtime que executa o agente                         │
└─────────────────────────────────────────────────────────────────────┘
                                │ autoriza
┌─ C. EXECUÇÃO (biblioteca `std.agent`) ──────────────────────────────┐
│  13 funções rasas, todas com nome por extenso                        │
│  Consumidor: o código Spectra escrito por humano ou agente           │
└─────────────────────────────────────────────────────────────────────┘
```

Regra de dependência: **A informa B, B autoriza C, C nunca decide por si.** Um
componente da camada C que precise de um efeito pede à camada B; se a camada B não
tiver a capacidade, o efeito falha com erro estruturado — não com exceção opaca.

---

## 5. Camada A — Superfície derivada

Nada aqui é biblioteca. É o compilador descrevendo o programa em formato que um agente
consome sem ler todos os arquivos.

### 5.1 Comandos `[proposto]`

```text
spectralang surface --json [--tokens N] [--package X]
spectralang impact  --json <symbol>
spectralang explain --json <error-code>
```

`surface` emite a superfície pública — funções, tipos, assinaturas, efeitos derivados,
capacidades exigidas, schemas derivados e fixtures associadas — em JSON, com filtro por
orçamento de tokens. `impact` responde "o que quebra se eu mudar isto" a partir do grafo
de chamadas, não de busca textual. `explain` devolve o plano de reparo de um código de
erro.

### 5.2 Por que derivado e não declarado

O catálogo já carrega `effects`, `abi`, `error_model`, `maturity` e `fixture` por
entrada, e o IR já carrega o nome de cada host call. Pedir ao humano para redigitar isso
na assinatura cria exatamente a classe de defeito que o repositório já combate com teste
anti-drift. A superfície é **gerada**; a única entrada humana é o que não é derivável:
objetivo, descrição e política.

### 5.3 Vocabulário de capacidades é derivado, não inventado

Capacidades não são strings novas. São prefixos do namespace de host calls que já
existe, e a ferramenta valida contra ele:

```text
spectra.api.client.request               capacidade de namespace
spectra.api.client.request:host=a.com    com escopo (opcional)
network:payments.api                     E04xx: desconhecida
                                         did-you-mean: spectra.api.client.request
```

Capacidade que não corresponde a nenhuma host call registrada é erro de compilação ou
lint, com o mesmo padrão de sugestão do E033. Isso elimina o modo de falha mais comum de
sistemas de permissão: a permissão que não corresponde a nada e ninguém percebe.

---

## 6. Camada B — Governança

### 6.1 Capacidades

`[existe]` um único ponto de dispatch: `runtime/src/ffi_host_registry.rs:226` registra
nome → ponteiro; `runtime/src/ffi_lifecycle.rs:280` invoca. `[proposto]` interpor a
checagem nesse ponto cobre tudo o que o programa chama, incluindo código de terceiros —
o que Rust não consegue fazer nem dentro de um escopo controlado.

A regra, em uma frase: **sem run ativo vale o comportamento de sempre; com run ativo, o
teto é o run.** Isso preserva todo programa Spectra existente e torna a adoção
incremental.

- **Default deny dentro do run.** Sem capacidade concedida, a host call falha.
- **Concedida na abertura do run**, não por função. Tudo o que o run chama herda esse
  teto e não pode ampliá-lo.
- **Run filho só reduz.** Delegação entre agentes passa um subconjunto estrito.

### 6.2 Taint e declassificação

Conteúdo vindo de fora — resultado de ferramenta, saída de modelo, arquivo, resposta
HTTP — entra marcado com `untrusted`; sinks sensíveis exigem `trust`. É o que impede que
uma instrução embutida num documento vire uma chamada de ferramenta legítima.

**Limite honesto:** a ABI atual transporta valores em slots de 64 bits
(`runtime/src/ffi_core.rs:316-317`), com handles generacionais em `runtime/src/handles/`.
Taint é viável para valores com handle — tensores, conexões, erros, sessions — mas não
para strings e inteiros crus sem trabalho de ABI. `[lacuna]` Portanto o taint começa no
nível de mensagem e de handle (o transcript carrega proveniência por mensagem), não como
fluxo de informação completo. Prometer mais que isso seria mentir.

### 6.3 Aprovação humana

`approve(run, ação)` devolve `false` se negado e `true` se aprovado; o autor decide o que
fazer com a resposta, normalmente com um `require` na linha seguinte. O canal é o mesmo
que os protocolos de agente já padronizam — `allow`/`deny`/`allow-once`/`allow-always` —
e o resultado é registrado, para que o replay não pergunte de novo.

### 6.4 Orçamento

Toda execução tem teto de tokens, custo, tempo de parede e chamadas de ferramenta.
Estourado o teto, a execução é cancelada de forma cooperativa — o cancelamento
assíncrono já existe no runtime. Orçamento é padrão, não opção: sem ele, um loop de
agente é um incidente financeiro esperando acontecer.

### 6.5 Durabilidade

Cada passo com efeito externo é registrado com entrada, saída, seed e idempotency key.
**O autor não anota nada** — a durabilidade é consequência de rodar dentro de um run.
Replay reproduz a execução sem repetir efeitos. Isto é o que torna um agente testável e
comparável, e é o que o conceito original não menciona apesar de ser mais decisivo na
prática do que contratos formais.

Compensação é declaração explícita, uma linha, e só onde o efeito é irreversível: o
runtime não inventa como desfazer um `POST`, mas registra que o autor disse como.

### 6.6 Trace

Spans seguem as convenções GenAI do OpenTelemetry (`invoke_agent`, `invoke_workflow`,
`plan`, `execute_tool`, `chat {model}`), com `gen_ai.agent.name`,
`gen_ai.conversation.id` e captura de conteúdo opt-in. `std.api.trace` já existe; o
trabalho é mapear, não construir.

---

## 7. Camada C — a biblioteca `std.agent`

### 7.1 Forma

Pacote nativo no mesmo molde de `spectra.api` (ADR-0011): manifesto `spectra.toml`,
crate Rust com `crate-type = ["rlib", "staticlib"]`, prefixo de host call
`spectra.agent.`, importável como `std.agent`.

Quatro módulos, e não dez. O trabalho do dia a dia está inteiro em `std.agent`; o resto
é interop.

```text
std.agent            o run, o modelo, as ferramentas, a memória, a política
std.agent.mcp        cliente e servidor MCP
std.agent.eval       casos, graders, pass^k
std.agent.protocol   exposição A2A e ACP
```

### 7.2 A superfície inteira

Treze funções. Todas rasas, todas com nome por extenso, todas impossíveis de confundir
com outra coisa.

| Função | Assinatura | O que faz |
|---|---|---|
| `agent_start` | `(spec: AgentSpec) returns Result<Run, Error>` | abre o run: objetivo, modelo, teto, capacidades |
| `agent_end` | `(run: Run) returns Result<Report, Error>` | fecha o run e devolve tokens, custo e passos |
| `ask` | `(run: Run, prompt: string) returns Result<string, Error>` | uma volta no modelo |
| `ask_stream` | `(run: Run, prompt: string) returns Result<Stream<string>, Error>` | a mesma volta, em pedaços |
| `ask_json` | `(run: Run, prompt: string, schema: string) returns Result<string, Error>` | resposta presa ao schema (decoding restrito) |
| `act` | `(run: Run, prompt: string) returns Result<string, Error>` | o loop de ferramentas até a resposta final |
| `embed` | `(run: Run, text: string) returns Result<List<float>, Error>` | vetor do texto |
| `remember` | `(run: Run, text: string) returns Result<bool, Error>` | grava na memória |
| `recall` | `(run: Run, query: string, top_k: int) returns Result<string, Error>` | recupera da memória |
| `approve` | `(run: Run, action: string) returns Result<bool, Error>` | pede decisão humana |
| `require` | `(run: Run, condition: bool, message: string) returns Result<bool, Error>` | asserção governada |
| `untrusted` | `(value: string, origin: string) returns string` | marca dado externo |
| `trust` | `(value: string, reason: string) returns string` | declassifica dado externo |

Duas funções de leitura completam o quadro sem virar ruído: `token_count(text) returns
int` e `json_schema`, esta última como função associada a acrescentar ao derive que já
existe — `Cobranca::json_schema()`, ao lado de `Cobranca::from_json(..)`.

### 7.3 O que **não** existe nessa lista, por decisão

- **`Agent` como tipo.** Um agente é uma função comum mais um `Run`. Criar um tipo
  especial obrigaria cerimônia de anotação sem acrescentar garantia alguma.
- **`#[derive(AgentSchema)]`.** O schema sai de `#[derive(Serialize, Deserialize)]`,
  que já existe e já produz `to_json`/`from_json`. Um derive, dois produtos.
- **`#[derive(Tool)]` + `#[tool(...)]`.** Uma função pública com um atributo é a
  ferramenta. Nome, schema dos argumentos, efeitos e capacidades são derivados; a
  descrição é o único texto que o humano escreve, e é o único que não é derivável.
- **`#[effects(...)]`, `#[requires]`, `#[ensures]`, `#[intent]`.** Todos deriváveis ou
  substituídos por uma linha de código.
- **`journal_step`.** Durabilidade não é anotação.
- **`inspect::*`.** É a camada A, não a camada C.

### 7.4 A forma de uma ferramenta

```spectra
#[agent_tool("Cria uma cobrança no provedor de pagamentos")]
public async func cobrar(run: Run, cobranca: Cobranca) returns Result<Recibo, Error> {
```

Dessa única linha derivam-se:

| Derivado | De onde |
|---|---|
| nome `cobrar` | nome da função |
| `inputSchema` | tipo do parâmetro que não é o `run` |
| efeitos | host calls alcançáveis no corpo |
| capacidades exigidas | das host calls, pelo vocabulário de 5.3 |
| invocabilidade pelo modelo | o `run` na assinatura |

---

## 8. Exemplos

Os blocos abaixo ilustram a superfície alvo. `agent_tool`, `Run`, `AgentSpec` e
`std.agent.*` são `[proposto]`; o resto é `[existe]` — sintaxe, `Result`, `?`, `await`,
`if let`, f-strings e `#[derive(Serialize, Deserialize)]`. Os nomes de `std.api` são
ilustrativos do que o pacote já oferece (`packages/spectra-api/src/bindings/`), não uma
lista fechada.

### 8.1 A ferramenta inteira

```spectra
module pagamentos

import std.agent
import std.api.client
import std.error as error

#[derive(Serialize, Deserialize)]
record Cobranca {
    amount: float,
    currency: string,
}

#[derive(Serialize, Deserialize)]
record Recibo {
    transaction_id: string,
}

#[agent_tool("Cria uma cobrança no provedor de pagamentos")]
public async func cobrar(run: Run, cobranca: Cobranca) returns Result<Recibo, Error> {
    let request = request_new("POST", "https://payments.api/pay", cobranca.to_json())
    let response = await client_request(request)?
    return Recibo::from_json(client_body(response))
}
```

Nada aqui declara efeito, capacidade, schema ou nome. Tudo isso é derivado. As duas
primeiras linhas do corpo são as que executam o efeito, e o efeito é
`spectra.api.client.request` — conhecido pelo compilador, checado contra o run.

### 8.2 O programa inteiro

```spectra
public async func main() returns int {
    let run = agent_start(AgentSpec {
        goal: "Processar pedidos pendentes sem duplicar cobrança",
        model: "anthropic/claude-sonnet",
        allow: [
            "spectra.api.client.request:host=payments.api",
            "spectra.api.db.postgres.query:table=orders",
        ],
        max_seconds: 120,
        max_tool_calls: 20,
    })?

    let pedidos = await carregar_pedidos(run)?
    let processados = 0

    for pedido in pedidos {
        let resposta = await act(run, f"Processe o pedido {pedido.id} e responda OK ou FALHA")?
        if resposta == "OK" {
            processados += 1
        }
    }

    let report = agent_end(run)?
    println(f"processados: {processados} de {report.steps} passos")
    return 0
}
```

Sem `journal_step`, sem bloco de política separado, sem `#[derive(Agent)]`. A
durabilidade e o orçamento valem porque existe um run; as capacidades valem porque foram
declaradas uma vez, na abertura.

### 8.3 Aprovação e asserção, em quatro linhas

```spectra
#[agent_tool("Reembolsa uma cobrança já paga")]
public async func reembolsar(run: Run, id: string) returns Result<bool, Error> {
    let aprovado = await approve(run, f"reembolsar a cobrança {id}")?
    require(run, aprovado, "reembolso exige aprovação humana")?
    return estornar(run, id)
}
```

### 8.4 O que o agente lê

```bash
$ spectralang surface --json --tokens 2000
```

Saída ilustrativa — `<fixture>` e `<caso>` apontam para arquivos reais quando existirem.

```json
{
  "package": "pagamentos",
  "tools": [
    {
      "name": "cobrar",
      "description": "Cria uma cobrança no provedor de pagamentos",
      "input_schema": { "type": "object", "required": ["amount", "currency"] },
      "effects": ["spectra.api.client.request"],
      "capabilities": ["spectra.api.client.request:host=payments.api"]
    }
  ]
}
```

```bash
$ spectralang impact --json Cobranca.amount
```

```json
{
  "symbol": "Cobranca.amount",
  "affected_functions": ["cobrar"],
  "affected_schemas": ["Cobranca"],
  "fixtures": ["<fixture>"],
  "eval_cases": ["<caso>"]
}
```

O agente não infere: está derivado, com ponteiro para o teste que exercita o símbolo.

---

## 9. Determinismo, replay e evals

Um programa de agente é não determinístico por natureza. A resposta não é tentar tipar o
não determinismo — é **registrar e reproduzir**.

- Tudo o que o run faz é registrado: entrada, saída, seed, custo e idempotency key.
- Replay reconstrói o histórico sem repetir efeitos externos.
- `eval_run` executa uma suíte de casos com grader determinístico ou juiz, medindo
  `pass@1` e `pass^k`, e falha o build em regressão.

Evals aqui são o análogo do teste de regressão: existem para pegar regressão, não para
provar que o trabalho foi feito.

---

## 10. Interop

| Protocolo | Papel da SpectraLang | Observação |
|---|---|---|
| MCP | cliente (HTTP; stdio depois) e servidor | `[lacuna]` stdio exige subprocesso, que a linguagem não tem |
| A2A | servidor: agente Spectra recebe delegação | agent card gerado do record de descrição |
| ACP | servidor: a Spectra como agente dentro do editor | reaproveita o pedido de permissão da camada B |
| OTel GenAI | emissor de spans | `std.api.trace` já existe; o trabalho é mapear nomes e atributos |

Interop é adaptador. Nenhum desses protocolos define a arquitetura; eles entram porque a
camada B já tem as primitivas que eles exigem.

---

## 11. Segurança: o que se promete e o que não se promete

**Prometido:**

- Dentro de um run, efeitos fora das capacidades concedidas são bloqueados no dispatch,
  em qualquer ponto do programa — inclusive em código de terceiros.
- Aprovação humana obrigatória para o que o autor marcar como irreversível.
- Orçamento estourado cancela a execução.
- Todo efeito externo de um run é auditável.

**Não prometido:**

- Imunidade a prompt injection. Capacidades limitam o **dano**, não a persuasão: um
  agente pode ser induzido a dizer coisa errada ou a agir de forma prejudicial *dentro*
  das capacidades concedidas. A mitigação é o teto ser pequeno.
- Verificação estática de pré e pós-condições. `require` é asserção de runtime.
- Rollback de filesystem ou de rede. Compensação é declarada pelo autor, não inferida.
- Fluxo de informação completo. Ver o limite de ABI em 6.2.
- Descrição de ferramenta confiável. Descrição é texto que o modelo lê e pode conter
  instrução hostil; é dado, nunca política.

---

## 12. Métricas de aceitação

| Métrica | Como medir |
|---|---|
| Fricção de geração | corpus de tarefas Spectra: tokens por tarefa, taxa de sintaxe válida, iterações de reparo, `pass@1` e `pass^k` |
| Legibilidade por agente | tokens necessários para responder "o que este projeto expõe" com e sem `surface --json` |
| Bloqueio de efeito não autorizado | suíte de injeção estilo AgentDojo com e sem capacidades; percentual de sinks bloqueados |
| Retomada | taxa de conclusão de run após interrupção; ausência de efeito duplicado em replay |
| Enforcement de orçamento | nenhuma execução ultrapassa o teto declarado, com evidência de cancelamento |
| Conformidade de trace | spans validados contra a versão pinada das convenções GenAI do OTel |
| Custo | tokens e custo por tarefa resolvida; p50 e p95 |

Critério sem comando é aspiração.

---

## 13. Não objetivos

- **Sintaxe de fornecedor de modelo.** Nada de `llm claude "..."` na linguagem. Um
  provider é um campo de `AgentSpec`, não uma keyword.
- **Primitivizar agentes no compilador antes das bibliotecas provarem as formas.**
  Biblioteca → padrão repetido → sintaxe. Nunca o contrário.
- **Um modo "linguagem de agente" separado.** Uma semântica, duas renditions.
- **Piorar a experiência humana para agradar o agente.** Se a mudança incomoda o humano
  e não bloqueia nada, ela não entra.
- **Default permissivo.** Capacidade começa vazia dentro do run.
- **Confiar em decoding restrito.** Schema garante forma, não verdade; validar sempre.
- **Reimplementar HTTP, banco, embeddings ou tracing.** Já existem e são validados.
- **Virar framework de agentes genérico.** A história do produto é o artefato ser
  legível, limitável e reproduzível — não competir em quantidade de integrações.
- **Mais de um atributo novo.** Se um segundo for necessário, o desenho está errado.

---

## 14. Sequência de adoção

| Etapa | Entrega | Depende de |
|---|---|---|
| **E0 — Superfície** | `surface --json`, `impact --json`, `explain --json`; referência da versão instalada gerada pelo CLI | nada; maior valor imediato, menor custo |
| **E1 — Fonte única da superfície nativa** | gerar tabelas de compilador, midend e runtime a partir do catálogo de contrato | E0 |
| **E2 — Núcleo** | `#[agent_tool]`, `Run`, `ask`/`act`/`embed`, `remember`/`recall` | E1, senão cada módulo nasce como dívida |
| **E3 — Governança** | capacidades no dispatch, taint de mensagem e handle, `approve`, orçamento, durabilidade, trace | E2 |
| **E4 — Interop e evals** | `mcp`, `protocol`, `eval` | E3 |
| **E5 — Linguagem** | só se E2–E4 revelarem forma repetida: `extern` nativo declarativo, açúcar sintático | E2–E4 |

E1 antes de E2 porque a alternativa é multiplicar por N o problema que o teste
anti-drift existe para conter.

---

## 15. Como isso se usa durante o desenvolvimento

### 15.1 Dois loops, dois consumidores

Antes do walkthrough, a distinção que evita a maior parte da confusão:

| | Loop de desenvolvimento | Loop de produção |
|---|---|---|
| Quem é o agente | Codex, Claude Code | o agente do produto |
| O que ele produz | código Spectra | uma resposta ao usuário |
| O que ele consome | **camada A** (superfície derivada) | **camadas B e C** (governança e execução) |
| Onde ele para | no diff, para revisão humana | no orçamento, na aprovação, no fim do run |
| O que garante o limite | a revisão humana | o runtime |

A biblioteca `std.agent` serve o segundo loop. O que faz o primeiro funcionar é a
camada A — e o mesmo substrato sustenta os dois: a superfície derivada descreve os
efeitos que a governança vai limitar em produção.

### 15.2 Preparo do repositório, uma vez

```markdown
<!-- AGENTS.md -->
## Comandos de validação
- Tipos:      spectralang check --json
- Estilo:     spectralang fmt --check
- Lint:       spectralang lint --deny unused-import --json
- Testes:     spectralang package test --json
- Comportamento do agente: spectralang agent eval --json

## Superfície
Antes de editar, consulte `spectralang surface --json --tokens 2000`.
Não edite `.spectra/reference.json` (gerado).

## Limites
- Não altere o campo `allow` do `AgentSpec` sem aprovação humana explícita.
- Não edite o journal em `.spectra/journal/`.
```

O mesmo arquivo serve Codex e Claude Code; se o segundo esperar `CLAUDE.md`, mantenha
um apontando para o outro. Nada disso é específico da SpectraLang — o que é específico
é que os comandos retornam JSON estável e exit codes fixos (0/64/65/74), então o agente
não precisa interpretar prosa.

### 15.3 A tarefa

O walkthrough abaixo assume E0–E3 concluídos. A seção 15.9 separa o que já existe hoje
do que ainda é proposta.

Ana pede a Claude Code: *"reembolso acima de R$ 500 precisa de aprovação humana"*.

Primeiro movimento do agente — orientar-se sem ler o repositório inteiro:

```bash
$ spectralang surface --json --tokens 2000
{"tools":[{"name":"reembolsar","description":"Reembolsa uma cobrança já paga",
           "input_schema":{"type":"object","required":["id"]},
           "effects":["spectra.api.client.request","spectra.std.fs.append"],
           "capabilities":["spectra.api.client.request:host=payments.api"]}]}

$ spectralang impact --json estornar
{"symbol":"estornar","affected_functions":["reembolsar"],
 "fixtures":["<fixture>"],"eval_cases":["<caso>"]}
```

Duas chamadas substituem a leitura de dez arquivos: o agente sabe quem existe, o que
toca o mundo externo e o que quebra se mexer.

### 15.4 O diff que ele produz

```spectra
// antes
#[agent_tool("Reembolsa uma cobrança já paga")]
public async func reembolsar(run: Run, id: string) returns Result<bool, Error> {
    return estornar(run, id)
}
```

```spectra
// depois
#[agent_tool("Reembolsa uma cobrança já paga; acima de 500 exige aprovação")]
public async func reembolsar(run: Run, id: string) returns Result<bool, Error> {
    let valor = await consultar_valor(run, id)?
    if valor > 500.0 {
        let aprovado = await approve(run, f"reembolsar {valor} da cobrança {id}")?
        require(run, aprovado, "reembolso acima de 500 exige aprovação humana")?
    }
    return estornar(run, id)
}
```

Três observações que valem como teste do desenho:

1. O agente **atualizou a descrição**, porque ela é contrato com o modelo — não é
   comentário. Se a descrição e o comportamento divergirem, o modelo erra.
2. Nenhum efeito, capacidade, schema ou nome foi declarado. Continuam derivados.
3. A mudança inteira cabe num diff que um humano revisa em dez segundos.

### 15.5 O loop de reparo

```bash
$ spectralang check --json
{"diagnostics":[{"code":"E0311","severity":"error",
  "message":"expected bool, found Result<bool, Error>",
  "span":{"file":"src/pagamentos.spectra","line":14},
  "expected":"bool","actual":"Result<bool, Error>",
  "fix":"propague com `?` ou trate com `if let`"}]}
```

Exit code 65, código estável, span e correção sugerida. O agente não precisa adivinhar
o que o compilador quis dizer — e o mesmo JSON alimenta o editor do humano.

### 15.6 O segundo agente como falsificador

Codex entra com contexto limpo, recebe o diff e a instrução de **tentar derrubar** a
mudança. O que ele escreve são duas coisas diferentes, porque são dois riscos
diferentes:

```spectra
// Teste determinístico: a governança bloqueia. Não depende de modelo nenhum.
#[spectra_async_test]
async func reembolso_acima_de_500_sem_aprovacao_e_bloqueado() returns int {
    let run = agent_start(AgentSpec { goal: "teste", model: "mock", allow: [] })?
    let resultado = await reembolsar(run, "cobranca-600")
    return match resultado {
        when Result::Err(e) then 0,
        when Result::Ok(_) then 1,
    }
}
```

```json
// Caso de eval: o modelo pede aprovação. Depende de comportamento, então é eval.
{ "nome": "reembolso-grande-pede-aprovacao",
  "entrada": "reembolse a cobrança 600",
  "exige_aprovacao": true,
  "recusa_se_negado": true }
```

Regra: **governança se testa, comportamento se avalia.** Misturar os dois produz suíte
que falha por variação de modelo e teste que passa por sorte.

### 15.7 O que o humano revisa

Só três coisas, porque todo o resto é derivado:

```text
  src/pagamentos.spectra   +/- 5 linhas   (comportamento)
  main.spectra             + 1 linha     (allow: "spectra.api.client.request:host=notify.api")
  tests/validation/4xx_reembolso_aprovacao.spectra   +18 linhas
  evals/pagamentos/reembolso-grande-pede-aprovacao.json  +4 linhas
```

**Toda ampliação de poder do agente de produção é uma linha no `allow`.** Não existe
caminho em que um agente de código conceda capacidade nova em silêncio, escondida
dentro do corpo de uma função — e é por isso que o `allow` fica no `AgentSpec`, longe
de onde se escreve lógica.

### 15.8 O que vai para produção

```bash
$ spectralang --emit-exe build/atendimento
$ ./build/atendimento --config prod.toml
```

Um binário nativo, sem runtime de framework. Em produção: capacidades valendo como
teto, orçamento cortando o que passar, aprovação humana aparecendo na fila, journal
permitindo replay de qualquer atendimento, spans indo para o coletor OTel e o custo
por atendimento saindo no relatório.

### 15.9 O que já dá para fazer hoje

| Peça | Estado |
|---|---|
| `check --json`, `--sarif`, exit codes, spans e códigos estáveis | `[existe]` — o loop de reparo já é confiável |
| `fmt --check --explain=json`, `lint --deny`, `package test --json` | `[existe]` |
| Referência da linguagem em Markdown, versionada no repositório | `[existe]` (`docs/AI-AGENT-REFERENCE.md`) |
| `surface --json`, `impact --json`, `explain --json` | `[proposto]` — E0, e é o que mais muda o loop de desenvolvimento |
| Referência gerada da versão instalada | `[proposto]` — E0 |
| `#[agent_tool]`, `Run`, `AgentSpec`, governança, `agent eval` | `[proposto]` — E2 e E3 |

Ou seja: **a maior parte do ganho para o loop de desenvolvimento está em E0**, que não
depende de nenhuma decisão de design da biblioteca. É o primeiro passo justamente por
isso.

---

## Anexo — mapa do conceito original para a SpectraLang

| Original (Rust + `agents`) | SpectraLang |
|---|---|
| `#[derive(Agent)] struct OrderAgent` | `agent_start(AgentSpec { goal, model, allow, ... })` |
| `#[memory] memory: OrderMemory` | `remember(run, texto)` e `recall(run, consulta, top_k)` |
| `#[derive(Tool)] #[tool(name, description, permission)]` | `#[agent_tool("descrição")]` numa função pública |
| `#[derive(Intent)] #[intent(goal = ...)]` | campo `goal` do `AgentSpec`, citado por testes e evals |
| `fn permissions() -> Vec<Permission>` | campo `allow` do `AgentSpec`; default deny dentro do run |
| `Context<DatabaseRead, DatabaseWrite>` | capacidade no run; checagem no dispatch |
| `#[requires(...)]` / `#[ensures(...)]` | `require(run, condição, mensagem)` |
| `#[effects(DatabaseWrite, NetworkRequest)]` | derivado do IR; não se escreve |
| `transaction!(ctx, { ... })` | durabilidade automática + compensação declarada |
| `inspect::agent::<OrderAgent>()` | `spectralang surface --json` |
| `inspect::dependencies::<T>("m")` | `spectralang impact --json <symbol>` |
| `inspect::impact::<Order>("total")` | `spectralang impact --json Order.total` |
