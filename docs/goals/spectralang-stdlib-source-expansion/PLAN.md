# SpectraLang — Expansão da Standard Library em `.spectra` (Phase 34)

**Plano de implementação sequencial com verificação por etapa.**

**Intent:** crescer a biblioteca-fonte da standard library de 1 módulo
(`std.algorithms`, 2 funções) para 11 módulos validados, mantendo uma única
fonte de verdade por símbolo (catálogo tipado), evidência de execução JIT+AOT e
o processo incremental do R-3406.

**Current Behavior (verificado em 2026-10-08):**

- `stdlib/src/` contém apenas `algorithms.spectra` → `std.algorithms` com
  `gcd_nonnegative` e `is_prime`.
- O compilador embute as fontes (`compiler/build.rs`), resolve o fechamento de
  imports em ordem de dependência (`compiler/src/semantic/mod.rs:44`) e compila
  o corpo como código Spectra normal.
- Auditoria R-3007 verde no baseline: `0 blockers`, catálogo
  `complete (1355/1141)`, todos os probes `passed`
  (`target/r3007-stdlib-contract/before.json`).
- Módulo-fonte **pode importar módulo nativo e outro módulo-fonte** (validado
  com probe temporário `std.probe_hybrid` → `std.string`; `check`/`run` OK;
  arquivos removidos após o teste).
- Lacunas de linguagem encontradas que bloqueiam parte da implementação:
  1. `char` comparado a literal `'A'` em `==`/`!=` gera **erro interno de
     verificação IR** (`... has Ne/Eq operands with mismatched IR types (Int vs Char)`).
  2. Não existe caminho `code/char → string`: `"" + c` é erro semântico
     (`Cannot concatenate string with non-string type char`) e `f"{c}"` /
     `println(c)` imprimem o número (`65`), não o caractere.
  3. Ordenação `char` vs `char` é rejeitada
     (`Left operand of comparison must be numeric, found char`).

**Expected Outcome:** 10 módulos novos + 1 módulo estendido, cada um com fixture
import-only executando em JIT e AOT, probe registrado, catálogo regenerado,
documentação e ledger atualizados, sem duplicar símbolos nativos existentes.

**Truth Owner:**

- `stdlib/src/` — implementação-fonte.
- `scripts/stdlib_contract.toml` — classificação (`[[namespace]]`) e cobertura
  (`[[probe]]`).
- `packages/spectra-contract/catalog/stdlib.toml` — catálogo tipado (gerado por
  `scripts/generate_stdlib_catalog.py`; nunca editar à mão).
- `docs/architecture/stdlib-source-migration.md` — ledger de migração.
- `tests/execution-baseline.json` — baseline de execução JIT+AOT.

**Contract Boundary (por símbolo novo):** declaração semântica (fonte ou tabela
nativa) ↔ implementação única ↔ descriptor de lowering (só quando nativo) ↔
entrada no catálogo com `abi`, `implementation` (`spectra-source` ou nativo),
`binding`, `docs`, `fixture` e `maturity`.

**Cutover / Displaced Path:** os módulos novos só entram como `.spectra` puro
(ou híbrido com primitivas nativas já existentes). Nenhum símbolo nativo
existente é renomeado ou removido. Duplicatas de comportamento já coberto
(`std.math.gcd/lcm`, `std.collections.list_sort/list_sort_by/list_reduce`,
`std.string.pad_left/pad_right/repeat_str/split_by`) são proibidas nas novas
APIs.

**Value Density:** os pré-requisitos (S-01/S-02) removem um erro interno de
compilador e destravam toda a construção de texto byte-a-byte; os lotes 1–2 são
puros e cobrem lacunas reais (codificação, validação, caminhos, estatística);
os lotes 3–4 provam genéricos, closures e o seam híbrido dentro da std.

**Acceptance Evidence:** fixture por módulo (`tests/validation/`), execução JIT
(`spectralang run`) e AOT (`compile --emit-exe` + execução), gate
`scripts/validate_execution_coverage.py --mode both`, auditoria R-3007 com
`--require-catalog`, `cargo test` dos crates tocados, `cargo fmt --check`,
`cargo clippy -- -D warnings`.

**Evidence Lane:** `target/debug/spectralang.exe`, `tests/validation`,
`tests/errors`, `tests/execution-baseline.json`,
`target/r3007-stdlib-contract/*.json`, `packages/spectra-contract/catalog/stdlib.toml`.

**Kill Criteria:** parar e reportar (não improvisar) se:

- um módulo exigir primitiva nativa não prevista neste plano;
- JIT e AOT divergirem em qualquer fixture;
- a auditoria R-3007 acusar `unclassified_symbol`, `production_symbol_without_documentation`,
  `production_symbol_without_probe` ou `production_probe_failed`;
- um fixture novo nascer com status diferente de `NEW`/`passed` no gate de
  execução;
- a implementação exigir duplicar símbolo nativo existente.

---

## 0. Baseline congelado

| Fato | Evidência |
|---|---|
| CLI construído | `cargo build -p spectra-cli` |
| Auditoria R-3007 verde | `python scripts/validate_r3007_stdlib_contract.py --binary target/debug/spectralang.exe --report target/r3007-stdlib-contract/before.json` → exit 0, `blockers: 0` |
| Catálogo completo | `catalog: complete (1355/1141)` no mesmo relatório |
| Único módulo-fonte | `stdlib/src/algorithms.spectra` |
| Números livres de fixture | `tests/validation` → a partir de `795`; `tests/errors` → a partir de `718` |
| Baseline de execução | `python scripts/validate_execution_coverage.py --binary target/debug/spectralang.exe --mode both --report target/execution-coverage/report.json` |
| Capacidades de linguagem verificadas | probe temporário JIT+AOT exit 0: closure como parâmetro (`func(int) returns int`), `identity<T>`, `echo_list<T>(List<T>)`, `List<List<int>>`, `Option<T>` retornado de função com `Option::Some/None`, `list_get -> Option<int>` (usar `option_unwrap_or`), `-7 % 2 == -1`, `len("é") == 2` |

## 1. Gate obrigatório por módulo (não negociável)

Cada tarefa de módulo só é aceita quando os 9 itens passam, nesta ordem:

1. `cargo build -p spectra-cli`
2. `./target/debug/spectralang.exe check tests/validation/<N>_<fixture>.spectra`
3. `./target/debug/spectralang.exe run tests/validation/<N>_<fixture>.spectra` → exit 0
   (o fixture mapeia cada caso de borda para um código de retorno distinto)
4. AOT: `./target/debug/spectralang.exe compile --emit-exe target/<mod>.exe --debug-info=none tests/validation/<N>_<fixture>.spectra`
   e executar `target/<mod>.exe` → mesmo exit code
5. `python scripts/validate_execution_coverage.py --binary target/debug/spectralang.exe --mode both --report target/execution-coverage/report.json`
   → fixture novo aparece como `NEW` (não falha); registrar no baseline no fim do lote
6. `scripts/stdlib_contract.toml`: `[[namespace]]` do módulo (owner, `classification = "production"`, `roadmap`) + `[[probe]]` com `covers` do módulo
7. `python scripts/generate_stdlib_catalog.py` → entrada nova com
   `implementation = "spectra-source"`, `abi = "compiled Spectra function"`,
   `effects = []`, `binding = "spectra.std.<mod>.<fn>"`, `docs`, `fixture`
8. `python scripts/validate_r3007_stdlib_contract.py --binary target/debug/spectralang.exe --require-catalog --report target/r3007-stdlib-contract/<mod>.json` → `0 blockers`
9. Docs: seção nova em `docs/reference/05-stdlib.md` citando o namespace e cada
   função pública; linha nova em `docs/architecture/stdlib-source-migration.md`
   (form `Source`, total de funções, migradas)

## 2. Pré-requisitos descobertos (Fase 0)

| ID | Defeito | Evidência | Impacto |
|---|---|---|---|
| BUG-1 | `char ==/!= literal` → erro interno `Int vs Char` | repro mínimo: `let a = 65 as char; if a != 'A' { return 1 }` → `error[internal]: Function 'main', block 'entry' has Ne operands with mismatched IR types (Int vs Char)` | ICE em padrão básico; `std.text`/`std.algorithms` não podem comparar contra literais |
| GAP-2 | sem conversão `code/char → string` | `"" + c` → erro semântico; `f"{c}"` e `println(c)` imprimem `65` | bloqueia `std.encoding`, `std.fmt`, `std.text` e `to_base` |
| GAP-3 | `char` sem ordenação | `a >= 'A'` → `Left operand of comparison must be numeric, found char` | apenas ergonomia: workaround `(code as int)` funciona |

## 3. Mapa de tarefas

| ID | Entrega | Depende | Owner | Fixture |
|---|---|---|---|---|
| S-00 | Baseline e números congelados | — | tooling | — |
| S-01 | Fix BUG-1 (comparação com literal) + regressões | S-00 | frontend/midend | 795, 718 |
| S-02 | Primitiva `std.string.from_scalar` (destrava GAP-2) | S-00 | runtime + semantic + midend | 796 |
| S-03 | Ordenação `char` (GAP-3) — opcional, decisão explícita | S-01 | frontend/midend/backend | 795 (estende) |
| S-10 | `std.algorithms` estendido | S-02 | runtime | 797 |
| S-11 | `std.encoding` | S-02 | runtime | 798 |
| S-12 | `std.stats` | S-02 | runtime | 799 |
| S-20 | `std.validate` | S-02 | runtime | 800 |
| S-21 | `std.path` | S-02 | runtime | 801 |
| S-30 | `std.text` (1º híbrido formal) | S-02, S-10 | runtime | 802 |
| S-31 | `std.calendar` (híbrido `std.time`) | S-10 | runtime | 803 |
| S-40 | `std.iter` (genéricos + closures) | S-02 | runtime | 804 |
| S-41 | `std.fmt` | S-02 | runtime | 805 |
| S-42 | `std.semver` | S-02 | runtime | 806 |
| S-50 | Gates de lote: baseline de execução + `cargo test/fmt/clippy` | S-10…S-42 | tooling | — |
| S-51 | Sincronização de docs/ledger/roadmap (`R-3407`…`R-3418`) | S-50 | ecosystem | — |

## 4. Especificação de API

Convenções: `Option<T>`/`Result<T, E>` são os tipos de ausência/erro (sem
sentinelas). `List<T>` é a coleção vetorial tipada. Nenhuma função pode
duplicar símbolo nativo existente. Onde aparece "None", `None` também cobre
argumento inválido documentado.

### S-10 `std.algorithms` (estendido; importa `std.string`)

| Função | Contrato |
|---|---|
| `binary_search_int(sorted: List<int>, target: int) -> int` | índice 0-based ou `-1`; exige ordem não decrescente |
| `to_base(value: int, radix: int) -> Option<string>` | `value >= 0`, `2 <= radix <= 36`, dígitos `0-9a-z`; `None` fora do domínio |
| `from_base(text: string, radix: int) -> Option<int>` | aceita `0-9a-z`/`0-9A-Z`, sem sinal; `None` em dígito inválido/overflow |
| `mod_inverse(value: int, modulus: int) -> Option<int>` | `value >= 0`, `modulus > 1`; `None` quando `gcd != 1` |
| `factorial(value: int) -> Option<int>` | `None` para negativo ou overflow de `i64` (`21!`) |
| `binomial(n: int, k: int) -> Option<int>` | `None` para `n < 0`, `k < 0`, `k > n` ou overflow |
| `collatz_steps(value: int) -> Option<int>` | `None` para `value < 1`; passos até 1 |
| `digit_sum(value: int) -> int` | soma dos dígitos decimais de `abs(value)` |
| `roman_encode(value: int) -> Option<string>` | `1..=3999` |
| `roman_decode(text: string) -> Option<int>` | símbolos `I,V,X,L,C,D,M`; `None` com símbolo inválido |
| `levenshtein(left: string, right: string) -> int` | distância em **Unicode scalars** (decodifica UTF-8 por ranges, sem bitwise) |

Bordas obrigatórias no fixture: radix 2/36, overflow (`factorial(21)`),
`mod_inverse` não coprimo, `collatz_steps(1) == 0`, roman `3999`/`MMXXIV`,
`levenshtein("", "") == 0`, `levenshtein("café", "cafe") == 1`.

### S-11 `std.encoding` (importa `std.string`)

Escopo explícito: **payloads de texto** (UTF-8). Round-trip binário arbitrário
fica fora do escopo até existir tipo `bytes`.

| Função | Contrato |
|---|---|
| `hex_encode(value: string) -> string` | minúsculo, 2 dígitos por byte UTF-8 |
| `hex_decode(text: string) -> Option<string>` | aceita maiúsc./minúsc.; `None` para comprimento ímpar, dígito inválido ou bytes que não formem UTF-8 |
| `base64_encode(value: string) -> string` | RFC 4648 com padding `=` |
| `base64_decode(text: string) -> Option<string>` | valida alfabeto, padding e UTF-8 resultante |
| `percent_encode(value: string) -> string` | preserva `A-Za-z0-9-._~`; resto `%XX` maiúsculo |
| `percent_decode(text: string) -> Option<string>` | `%XX`; `+` **não** vira espaço (form é `std.api.query`); valida UTF-8 |
| `rot13(value: string) -> string` | apenas `A-Za-z`; involução |

Bordas: `""`, `"é"` → hex `c3a9` / base64 `w6k=`, emoji de 4 bytes, `%` solto,
`%ZZ`, padding inválido, `rot13(rot13(x)) == x`.

### S-12 `std.stats` (importa `std.collections`; `std.math.sqrt_f` para desvio)

| Função | Contrato |
|---|---|
| `sum_f(values: List<float>) -> float` | vazio → `0.0` |
| `mean_f(values: List<float>) -> Option<float>` | populacional; `None` se vazio |
| `median_f(values: List<float>) -> Option<float>` | não muta a entrada; média dos centrais quando `n` par |
| `variance_f(values: List<float>) -> Option<float>` | populacional; `None` se vazio |
| `variance_sample_f(values: List<float>) -> Option<float>` | `n-1`; `None` se `n < 2` |
| `stddev_f` / `stddev_sample_f(values: List<float>) -> Option<float>` | `sqrt` das variâncias |
| `percentile_f(values: List<float>, percent: float) -> Option<float>` | interpolação linear, `rank = p/100 * (n-1)`; `None` fora de `0..=100` |
| `covariance_f(left: List<float>, right: List<float>) -> Option<float>` | populacional; `None` se tamanhos diferentes ou vazio |
| `correlation_f(left, right) -> Option<float>` | Pearson; `None` se tamanhos diferentes, vazio ou variância zero |

Bordas: lista vazia, `n=1`, valores negativos, `percentile` 0/50/100,
correlação perfeita `1.0`, séries constantes.

### S-20 `std.validate` (importa `std.string`)

| Função | Contrato |
|---|---|
| `luhn_valid(text: string) -> bool` | ignora espaços e hífens; exige ≥ 2 dígitos |
| `isbn10_valid(text: string) -> bool` | aceita `X` final; normaliza separadores |
| `isbn13_valid(text: string) -> bool` | mod 10 |
| `cpf_valid(text: string) -> bool` | dígitos verificadores; rejeita sequências repetidas |
| `cnpj_valid(text: string) -> bool` | idem, pesos de 12/13 dígitos |
| `iban_valid(text: string) -> bool` | remove espaços, uppercase, `15..=34`, mod 97 == 1 |
| `email_is_valid(text: string) -> bool` | heurística documentada (`local@dominio.tld`, sem espaços) |
| `url_is_valid(text: string) -> bool` | heurística: `http`/`https` + `://` + host não vazio |

Bordas: entradas com máscara (`529.982.247-25`), todos os dígitos iguais,
`IBAN` válido de teste, e-mails com `+`, `..`, domínio sem ponto.

### S-21 `std.path` (importa `std.string`)

Semântica v1 documentada: separadores `/` e `\`; saída canônica com `/`; sem
resolução de drive/UNC além de `X:` e `\\` em `is_absolute`.

| Função | Contrato |
|---|---|
| `join(left: string, right: string) -> string` | `left` vazio → `right`; `right` absoluto → `right`; insere `/` quando necessário |
| `normalize(path: string) -> string` | colapsa `//`, resolve `.`/`..` sem atravessar a raiz; preserva `/` final? (não: remove) |
| `file_name(path: string) -> string` | último segmento; `""` para raiz/terminação |
| `parent(path: string) -> Option<string>` | `None` quando não há pai |
| `extension(path: string) -> Option<string>` | sem ponto; `None` se ausente; `"a.tar.gz"` → `"gz"` |
| `stem(path: string) -> string` | `file_name` sem extensão |
| `is_absolute(path: string) -> bool` | `/…`, `\\…`, `X:/…`, `X:\…` |

Bordas: `""`, `"/"`, `"a/"`, `"a/./b/../c"`, `"C:\\dir\\file.txt"`, mistos `a/b\c`.

### S-30 `std.text` (importa `std.string`; 1º híbrido)

| Função | Contrato |
|---|---|
| `slugify(value: string) -> string` | ASCII lowercase; não alfanumérico vira `-` colapsado; trim `-`; bytes não-ASCII descartados (documentado) |
| `normalize_whitespace(value: string) -> string` | colapsa ` \t\n\r` em um espaço + trim |
| `truncate(value: string, max_bytes: int) -> string` | corta sem quebrar rune; `max_bytes <= 0` → `""` |
| `wrap(value: string, width: int) -> List<string>` | quebra em espaços; palavra maior que `width` fica inteira |
| `word_count(value: string) -> int` | sequências não-espaço |
| `escape_json(value: string) -> string` | escapa `"` `\` e controles (`\n` `\r` `\t`, `\u00XX` para demais) |
| `unescape_json(value: string) -> Option<string>` | aceita `\uXXXX` (BMP) e os escapes acima; `None` em escape inválido |
| `similarity_ratio(left: string, right: string) -> float` | `1.0 - levenshtein/max(runes)`; strings vazias → `1.0` |

Bordas: `"Olá, Mundo!"`, emoji, `"\u0000"`/`\u00e9`, larguras 1 e 0, texto vazio.

### S-31 `std.calendar` (importa `std.string`, `std.time`, `std.convert`)

Todas as funções recebem tempo Unix (UTC) explícito; "agora" é
`std.time.time_now_secs()` no chamador.

| Função | Contrato |
|---|---|
| `is_leap_year(year: int) -> bool` | calendário gregoriano |
| `days_in_month(year: int, month: int) -> Option<int>` | `None` para `month` fora de `1..=12` |
| `day_of_week(unix_secs: int) -> int` | `0=domingo..6=sábado` (1970-01-01 = 4) |
| `add_days(unix_secs: int, days: int) -> int` | aritmética inteira |
| `diff_days(later: int, earlier: int) -> int` | dias de calendário; documentar divisão para negativos |
| `iso_year(unix_secs: int) -> int` / `iso_week(unix_secs: int) -> int` | ISO-8601 (semanas `1..=53`) |
| `format_iso_date(unix_secs: int) -> string` | `YYYY-MM-DD` |
| `format_iso_timestamp(unix_secs: int) -> string` | `YYYY-MM-DDTHH:MM:SSZ` (via `std.time.unix_to_utc`) |
| `parse_iso_date(text: string) -> Option<int>` | aceita `YYYY-MM-DD`; `None` para data inválida |

Bordas: 2000-02-29 (válido), 1900-02-29 (inválido), 1970-01-01 (quinta-feira,
ISO semana 1), 2020-12-31 (ISO ano 2020 semana 53), 2021-01-01 (ISO ano 2020).

### S-40 `std.iter` (importa `std.collections`; genéricos + closures)

Genéricos de nível único e `List<List<int>>` já foram verificados no baseline
(§0: probe JIT+AOT exit 0 com `identity<T>`, `echo_list<T>(List<T>)`,
`List<List<int>>` e closure como parâmetro). Se algum deles regredir no gate,
a tarefa para e o caso é registrado como blocker (sem fallback improvisado).

| Função | Contrato |
|---|---|
| `take<T>(items: List<T>, count: int) -> List<T>` | `count <= 0` → lista vazia; `count > len` → cópia |
| `skip<T>(items: List<T>, count: int) -> List<T>` | idem, pelo fim |
| `reverse<T>(items: List<T>) -> List<T>` | não muta a entrada |
| `sum_int(values: List<int>) -> int` | vazio → `0` |
| `count_if_int(values: List<int>, predicate: func(int) returns bool) -> int` | — |
| `position_if_int(values: List<int>, predicate: func(int) returns bool) -> Option<int>` | primeiro índice |
| `chunk_int(values: List<int>, size: int) -> List<List<int>>` | `size >= 1`; último chunk menor |
| `window_sum_int(values: List<int>, window: int) -> List<int>` | `window >= 1`; saída com `len - window + 1` itens |

Bordas: listas vazias, `count = 0`, `size` maior que a lista, `window = 1`,
predicado sempre falso/verdadeiro.

### S-41 `std.fmt` (importa `std.string`, `std.math`)

| Função | Contrato |
|---|---|
| `int_padded(value: int, width: int, pad: char) -> Option<string>` | sinal à esquerda do padding; `pad` apenas `' '` ou `'0'`; `width < 0` → `None` |
| `thousands(value: int, separator: string) -> string` | separador a cada 3 dígitos; sinal preservado |
| `float_fixed(value: float, decimals: int) -> Option<string>` | `0 <= decimals <= 12`; arredondamento half-away-from-zero; sem notação científica |
| `bytes_si(count: int) -> string` | base 1000, 1 decimal (`B`,`KB`,`MB`,`GB`,`TB`); negativo com sinal |

Bordas: `0`, negativos, `0.5`/`-0.5`, `1.005`, `999999`, `2.675`,
`float_fixed(1e18, 2)`, `decimals = 12`.

### S-42 `std.semver` (importa `std.string`)

| Função | Contrato |
|---|---|
| `is_valid(text: string) -> bool` | semver 2.0.0 (prerelease/build opcionais; zeros à esquerda inválidos) |
| `compare(left: string, right: string) -> Option<int>` | `-1/0/1`; precedência completa com prerelease; `None` se inválido |
| `major(text: string) -> Option<int>` / `minor` / `patch` | `None` se inválido |
| `prerelease(text: string) -> string` | `""` quando ausente; identifica pré-release |

Bordas: `1.0.0-alpha < 1.0.0-alpha.1 < 1.0.0-alpha.beta < 1.0.0-beta < 1.0.0-beta.2 < 1.0.0-beta.11 < 1.0.0-rc.1 < 1.0.0`;
`1.2` inválido, `01.2.3` inválido, build metadata ignorada na comparação.

## 5. Tarefas executáveis

### S-00 — Baseline e construção

- **Owner:** tooling.
- **Arquivos:** nenhum de produção.
- **Escopo:** `cargo build -p spectra-cli`; capturar R-3007 (`before.json`);
  capturar baseline de execução; registrar revisão git e bundle id embutido
  (`spectralang release-info` se disponível).
- **Output:** `target/r3007-stdlib-contract/before.json`,
  `target/execution-coverage/report.json`, `target/zzz-baseline/notes.md`.
- **Verificação:** auditoria exit 0 com `blockers: 0` (confirmado em 2026-10-08);
  gate de execução executado sem `FAIL`.
- **Aceite:** baseline verde arquivado; qualquer regressão posterior é
  atribuível ao lote.
- **Paralelismo:** nenhum (porta de entrada).

### S-01 — Fix da comparação `char` vs literal (BUG-1)

- **Owner:** frontend + midend.
- **Arquivos:** `compiler/src/semantic/semantic_expression_binary.rs`
  (caminho de igualdade, ~linha 139), `midend/src/lowering_impl_blocks.rs`
  (`609` = `IRType::Char`, `1161` = fold `CharLiteral → i64`),
  `midend/src/lowering_impl_expression.rs`, `midend/src/lowering_expr_tail.rs`.
- **Escopo:** garantir que `c == 'A'` / `c != 'A'` produzam operandos IR do
  mesmo tipo (`Char` vs `Char`) em todos os caminhos (variável, literal,
  const-fold, ternário/match guard). Não alterar a regra estrita
  `int` vs `char` (permanece erro semântico com mensagem estável).
- **Fixtures:** `tests/validation/795_core_char_literal_comparison.spectra`
  (positivo; exit codes por caso) e
  `tests/errors/718_core_char_int_literal_equality.spectra`
  (`check` deve falhar com a mensagem/código estável do mismatch `int`/`char`).
- **Verificação:**
  `cargo test -p spectra-compiler -p spectra-midend -p spectra-backend`;
  `spectralang run tests/validation/795_…` → 0;
  `spectralang compile --emit-exe target/s01.exe --debug-info=none tests/validation/795_…` → executa 0;
  `spectralang check tests/errors/718_…` → exit 65 com diagnóstico esperado.
- **Aceite:** ICE desaparece; regressões adicionadas; suíte existente sem
  regressão.
- **Paralelismo:** não (bloqueia S-03 e o estilo dos módulos de texto).

### S-02 — Primitiva `std.string.from_scalar(code: int) -> Option<string>`

- **Owner:** runtime (implementação) + semantic/midend (contrato).
- **Arquivos:** `compiler/src/semantic/builtin_text_system.rs` (`make_std_string`;
  padrão `Option<string>` copiado de `env_get_option`),
  `runtime/src/stdlib/stdlib_bindings.rs` (constante `spectra.std.string.from_scalar`),
  `runtime/src/stdlib/string_new.rs` (implementação),
  `runtime/src/stdlib/registration.rs` (registro),
  `midend/src/lowering_std_host_collections_string.rs` (descriptor com
  `ir_return = "Option<string>"`),
  `scripts/stdlib_contract.toml` (probe `string-from-scalar`),
  `packages/spectra-contract/catalog/stdlib.toml` (regeneração),
  `docs/reference/05-stdlib.md` (§2).
- **Escopo:** retorna string de 1 rune para scalar válido
  (`0..=0x10FFFF` excluindo `0xD800..=0xDFFF`); `None` fora do domínio.
  É a única primitiva nativa nova do plano; não deve duplicar `char_at`
  (leitura) nem `pad_left`.
- **Fixture:** `tests/validation/796_string_from_scalar.spectra`
  (`65` → `"A"`, `0xE9` → `"é"`, `0x20AC` → `"€"`, `0x1F600` → emoji 4 bytes,
  `-1`/`0xD800`/`0x110000` → `None`; `std.string.len` confere 1/2/3/4).
- **Verificação:** `cargo test -p spectra-runtime -p spectra-midend`;
  `spectralang run tests/validation/796_…` → 0; AOT idem;
  `python scripts/generate_stdlib_catalog.py` → entrada `host(...)`;
  R-3007 `--require-catalog` → 0 blockers.
- **Aceite:** primitiva disponível em JIT e AOT, catalogada, documentada,
  coberta por probe.
- **Paralelismo:** pode rodar em paralelo com S-01 (arquivos distintos) desde
  que a integração final rode os dois gates juntos.

### S-03 — Ordenação `char` vs `char` (decisão explícita)

- **Owner:** frontend + midend + backend.
- **Escopo:** aceitar `<`, `<=`, `>`, `>=` entre `char` e `char`/literal
  (ordem de scalar Unicode), preservando o erro atual para `char` vs `int`.
- **Decisão:** recomendada; se recusada, registrar em FINDINGS que o estilo
  `(code as int)` é obrigatório e não incluir o caso no fixture 795.
- **Verificação:** estender 795 (`'A' < 'B'`, `'a' > 'Z'`), reexecutar
  `cargo test` e o gate AOT; atualizar `docs/reference/06-referencia-rapida.md`
  (tabela de operadores) e `docs/diagnostics/error-code-reference.md` se a
  mensagem mudar.

### S-10 … S-42 — Módulos (execução padrão do gate §1)

Cada tarefa de módulo segue exatamente o mesmo contrato de execução:

- **Arquivos:** `stdlib/src/<módulo>.spectra`; fixture
  `tests/validation/<N>_std_source_<módulo>.spectra`; `scripts/stdlib_contract.toml`
  (`[[namespace]] prefix = "std.<módulo>"`, `owner = "runtime"`,
  `classification = "production"`, `roadmap = "R-341x"`, e `[[probe]]` com
  `covers = ["std.<módulo>.*"]`); `docs/reference/05-stdlib.md`;
  `docs/architecture/stdlib-source-migration.md`.
- **Escopo:** apenas as funções da seção 4; o fixture exercita **todas** as
  funções públicas e cada borda listada, com código de retorno próprio por caso.
- **Verificação:** os 9 passos do gate §1 + `cargo fmt --all -- --check`.
- **Aceite:** exit 0 em JIT e AOT, `0 blockers` na auditoria, catálogo com
  `implementation = "spectra-source"`, docs/ledger atualizados.
- **Ordem:** S-10 → S-12 → S-20 → S-21 → S-30 → S-31 → S-40 → S-41 → S-42.
  S-30 depende de S-10 (reusa `levenshtein`); S-11/S-12 podem rodar em paralelo
  com S-10 após S-02 (arquivos distintos, um fixture cada).
- **Notas de implementação por módulo:**
  - S-10: primeiro módulo-fonte que importa `std.string` (seam híbrido
    validado no baseline) — registrar no ledger como `Hybrid`;
    `to_base` usa `std.string.from_scalar` + concatenação;
    `levenshtein` implementa `utf8_rune_len` interno por comparação de ranges.
  - S-11: cada bloco de 3 bytes → 4 caracteres base64 via divisão/módulo por 64
    (sem bitwise); decodificação valida UTF-8 estruturalmente.
  - S-12: para `median`/`percentile`, copiar para uma nova `List<float>` via
    loop `list_get`/`list_push` e ordenar com `list_sort` (o `list_map` nativo
    aceita apenas `fn(int) -> int`, não serve para `float`); `median` não muta
    a entrada.
  - S-30: importa `std.string` e `std.algorithms` (primeiro caso fonte→fonte em
    produção; a ordem de dependência do resolver já foi validada no baseline);
    `wrap` usa `split_by`/`substring`.
  - S-31: usa `std.time.unix_to_utc` (nativo) + `std.convert.int_to_string` +
    `std.string.pad_left`; `parse_iso_date` valida sem `convert.string_to_int`
    (usa `%`/comparações) ou reusa o nativo com verificação de erro.

### S-50 — Gates de lote e baseline de execução

- **Owner:** tooling.
- **Escopo:** após cada lote (1, 2, 3, 4): rodar
  `python scripts/validate_execution_coverage.py --binary target/debug/spectralang.exe --mode both --update-baseline`
  e commitar `tests/execution-baseline.json` junto do lote; rodar
  `cargo test --workspace --all-targets --no-fail-fast`,
  `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`.
- **Verificação:** todos os fixtures novos com status `passed` no relatório
  `target/execution-coverage/report.json`; nenhum fixture antigo muda de status.
- **Aceite:** baseline atualizado, sem regressões de execução.

### S-51 — Sincronização final

- **Owner:** ecosystem.
- **Escopo:** atualizar `docs/architecture/stdlib-source-migration.md`
  (tabela de módulos + contagem de funções públicas migradas),
  `docs/reference/05-stdlib.md` (sumário), `docs/reference/06-referencia-rapida.md`
  (se S-03 for aceita), `docs/language-feature-maturity.md` (apenas se alguma
  classificação de maturidade mudar). Se o modo roadmap estiver ativo, abrir
  `R-3407`…`R-3418` em `roadmap/roadmap.toml` + `docs/roadmap-backlog.md`
  (Phase 34) com owner/priority/risk/dependencies/acceptance e apontar cada
  `[[namespace]].roadmap` para o item correspondente.
- **Verificação:** `python -c "import tomllib,pathlib;tomllib.loads(pathlib.Path('roadmap/roadmap.toml').read_text(encoding='utf-8'))"`
  (parse TOML), IDs únicos, dependências existentes; auditoria R-3007 final.
- **Aceite:** catálogo, docs, ledger e (se ativo) roadmap consistentes.

## 6. Gate final (definição de pronto)

```bash
cargo build -p spectra-cli
cargo test --workspace --all-targets --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python scripts/generate_stdlib_catalog.py            # regenera o catálogo
git diff --exit-code packages/spectra-contract/catalog/stdlib.toml || true   # inspecionar diff
python scripts/validate_r3007_stdlib_contract.py --binary target/debug/spectralang.exe --require-catalog --report target/r3007-stdlib-contract/after.json
python scripts/validate_execution_coverage.py --binary target/debug/spectralang.exe --mode both --report target/execution-coverage/report.json
```

Critério: `after.json` com `blockers: 0` e `catalog: complete`, cobertura de
execução sem `FAIL`, todos os 11 fixtures de módulo em `passed`.

## 7. Decisões abertas

| Decisão | Opções | Recomendação |
|---|---|---|
| Ordenação `char` (S-03) | implementar agora / manter erro | implementar antes do lote 3 (custo baixo, evita casts espalhados) |
| `std.stats` sobre `List<float>` vs `Vector<float>` | List (beta) / Vector (beta) | List: HOFs e `list_sort` já existem |
| `std.path` Windows | POSIX-only documentado / suporte a drive | v1 com `X:` e `\\` em `is_absolute` e tratamento de `\` como separador; sem UNC |
| `std.encoding` binário | só UTF-8 (documentado) / tipo `bytes` futuro | só texto; `bytes` vira item separado |
| `std.json`/`std.regex`/`std.hash`/`std.uuid`/`std.testing` | incluir / adiar | adiar: conflito com `std.api.json`/derive (json), tamanho (regex), bitwise ausente (hash), RNG nativo (uuid v4), sem `panic` (testing) |

---

## 8. Execução — estado final

Todas as tarefas de implementação (S-00…S-42) concluídas e verificadas; o
detalhamento de defeitos, decisões e evidências está em
`docs/goals/spectralang-stdlib-source-expansion/FINDINGS.md`.

| Entrega | Estado | Evidência |
|---|---|---|
| S-01 char vs literal (ICE) | concluído | fixture 795 (JIT+AOT), erro 718, suíte do compilador |
| S-02 `std.string.from_scalar` | concluído | fixture 796 (JIT+AOT), teste Rust, catálogo + docs |
| S-03 ordenação `char` | concluído | casos em 795, erro 719 |
| S-10 `std.algorithms` (13 fns) | concluído | fixture 797 (JIT+AOT) |
| S-11 `std.encoding` (7) | concluído | fixture 798 |
| S-12 `std.stats` (10) | concluído | fixture 799 |
| S-20 `std.validate` (8) | concluído | fixture 800 |
| S-21 `std.path` (7) | concluído | fixture 801 |
| S-30 `std.text` (8) | concluído | fixture 802 |
| S-31 `std.calendar` (10) | concluído | fixture 803 |
| S-40 `std.iter` (8) | concluído | fixture 804 |
| S-41 `std.fmt` (4) | concluído | fixture 805 |
| S-42 `std.semver` (6) | concluído | fixture 806 |
| Regressão do `phi` (achado) | concluído | fixture 807 (JIT+AOT) |
| Catálogo + manifest + ledger + docs | concluído | `packages/spectra-contract/catalog/stdlib.toml` (1444 entradas), `scripts/stdlib_contract.toml`, `docs/reference/05-stdlib.md` §§16–25, `docs/architecture/stdlib-source-migration.md` |
| Cobertura de execução (JIT+AOT) | concluído para os fixtures novos | `target/execution-coverage/new-modules.json` (10 módulos), `lang795.json`, `lang807.json` |
| Auditoria R-3007 com `--require-catalog` | **verde** | `target/r3007-stdlib-contract/after.json`: `blockers: 0`, `catalog: complete (1444/1221)` |
| `cargo test -p spectra-api --test contract_drift` | verde após F-09 | 7 passed |
| Revisão independente (reviewer) | 9 achados corrigidos (2 altos) | `FINDINGS.md` §"Revisão independente" |
| `cargo test --workspace --all-targets` | executado (ver resultado do job final) | — |

**Pendência registrada:** `python scripts/validate_execution_coverage.py --update-baseline`
(corpus completo JIT+AOT) para gravar os 13 fixtures novos em
`tests/execution-baseline.json`; os fixtures já passam como `NEW` no gate.

**Dívida técnica assumida (documentada):** helpers privados de UTF-8
(`push_code`/decodificação de rune) repetidos em `std.encoding`, `std.text`,
`std.path` e `std.algorithms` (~40 linhas). Mantidos por módulo para preservar
autonomia do bundle-fonte; extrair um `std.bytes` interno quando surgir um
quinto consumidor.

S-51 (itens `R-3407`…`R-3418` no `roadmap.toml` + backlog) permanece **não
executado**: o modo roadmap não foi ativado nesta sessão; os namespaces usam
`roadmap = "R-3406"` (gate de migração por módulo) como âncora válida.

