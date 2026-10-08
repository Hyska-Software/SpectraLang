# Findings — Expansão da Standard Library em `.spectra`

Registro de defeitos encontrados, decisões tomadas e evidências coletadas
durante a execução do `PLAN.md` (S-00…S-51).

## Defeitos de linguagem/compilador corrigidos

### F-01 — `char` vs literal `char` quebrava a verificação de IR

- **Sintoma:** `let a = 65 as char; if a != 'A' { … }` abortava com
  `error[internal]: Function 'main', block 'entry' has Ne operands with
  mismatched IR types (Int vs Char)`.
- **Causa:** `emit_const_value` emitia `LoweredConstValue::Char` como constante
  **Int** (`build_const_int`), enquanto `CharLiteral` emite tipo `Char`
  (`build_const_int_typed`). `65 as char` é dobrado como constante, então a
  variável virava `Int` e o literal permanecia `Char`.
- **Correção:** `midend/src/lowering_impl_core.rs` — `Char` agora emite
  `IRType::Char`.
- **Evidência:** `tests/validation/795_core_char_literal_comparison.spectra`
  (JIT e AOT exit 0) e `tests/errors/718_core_char_int_literal_equality.spectra`
  (`int` vs `char` continua E038).

### F-02 — `phi` sem entrada para predecessor em `else if` sem `else`

- **Sintoma:** `if cond { host_call } else if cond2 { host_call }` (também
  dentro de `while`) abortava com `has a phi without an incoming entry for
  predecessor block N`.
- **Causa:** em `lowering_expr_aggregates.rs` o bloco de merge contava o
  predecessor sem valor (arela falsa do último `else if`), mas o `phi` só
  recebia as entradas com valor — um `phi` malformado quando os ramos terminam
  em chamada host.
- **Correção:** o `phi` só é construído quando as entradas cobrem **todos** os
  predecessores; caso contrário mantém-se o fallback já existente
  (constante neutra).
- **Evidência:** `tests/validation/807_core_else_if_host_call_merge.spectra`
  (JIT e AOT exit 0) + suíte `cargo test -p spectra-compiler -p spectra-midend
  -p spectra-backend`.

### F-03 — não havia caminho `code/char → string`

- **Sintoma:** `"" + c` é erro semântico (`Cannot concatenate string with
  non-string type char`) e `f"{c}"`/`println(c)` imprimem o número (`65`), não o
  caractere. Sem isso não é possível construir texto byte a byte em Spectra.
- **Correção:** nova primitiva nativa `std.string.from_scalar(code: int) ->
  Option<string>` (semântica em `builtin_text_system.rs`, runtime em
  `string_new.rs` + `stdlib_bindings.rs` + `string_registration_helpers.rs`,
  descriptor em `lowering_std_host_collections_string.rs` gerado por
  `scripts/generate_lowering_tables.py`, catálogo regenerado).
- **Evidência:** `tests/validation/796_string_from_scalar.spectra` (JIT/AOT) +
  teste Rust `from_scalar_materializes_valid_scalars_and_reports_none` em
  `runtime/src/stdlib/tests.rs`.

### F-04 — ordenação `char` rejeitada

- **Sintoma:** `a >= 'A'` → `Left operand of comparison must be numeric, found
  char`.
- **Correção:** `compiler/src/semantic/semantic_expression_binary.rs` aceita
  ordenação quando **ambos** os operandos são `char`; `char` misturado com
  numérico continua rejeitado (cast explícito).
- **Evidência:** casos de ordenação em 795 + fixture de erro
  `tests/errors/719_core_char_ordering_mixed_numeric.spectra`.

## Padrões perigosos descobertos (e evitados nos módulos)

### F-05 — `from_scalar` re-codifica; copiar bytes corrompe UTF-8

`str.from_scalar(0xC3)` produz `U+00C3` (2 bytes), não o byte `0xC3`. Laços que
percorrem **bytes** e reemitem cada byte por `from_scalar` corrompem texto
multi-byte. Ocorreu em `rot13`, `escape_json`, `unescape_json` e
`normalize_whitespace`; todos agora copiam o **rune completo** (decodificação
por faixas aritméticas) ou validam/decodificam bytes explicitamente
(`std.encoding.decode_utf8`).

### F-06 — laço infinito por falta de avanço de índice

`escape_json` acumulava o escape sem avançar `index` nos ramos de 1 byte; o
fixture `std.text` travou (`timeout`) e só foi detectado porque o gate usa
timeout. Correção: passo de avanço único por iteração. Lição: fixtures novos
devem rodar com timeout.

### F-07 — `returns unit` não existe em código-fonte

Funções sem valor de retorno **omitindo** a anotação é a única forma válida;
`returns unit` falha com `Unknown type 'unit'`. Módulos-fonte devem omitir o
`returns` em helpers void.

### F-08 — tipo de ramo em `if` deve unificar

Um ramo cujo último statement é uma chamada que retorna `Option<string>`
enquanto o outro retorna `unit` falha com
`Incompatible branch types in if expression`. Helpers void que descartam
resultados precisam de `let` explícito para manter o ramo unitário.

## Decisões de contrato

- **`std.encoding` cobre texto UTF-8** (não bytes arbitrários): decodificadores
  validam UTF-8 estritamente (overlong, surrogates e > U+10FFFF rejeitados).
  `percent_encode` usa hexadecimal **maiúsculo** (`%C3%A9`) e `percent_decode`
  preserva `+` (form encoding é `std.api.query`).
- **`std.path` v1:** separadores `/` e `\` na entrada, saída canônica com `/`,
  drive `X:` reconhecido, prefixos UNC não resolvidos.
- **`std.stats`** opera em `List<float>` (cópia para ordenar; `median`/`percentile`
  não mutam a entrada).
- **`std.iter`** não muta entradas e usa genéricos de nível único
  (`take<T>`, `skip<T>`, `reverse<T>`), verificados no baseline.
- **`std.calendar`** exige tempo Unix explícito; "agora" vem de
  `std.time.time_now_secs()`.
- **`std.fmt`** arredonda half-away-from-zero após escalar por `10^decimals`;
  `NaN`/infinito e overflow de `i64` retornam `None`.

## Limites de plataforma encontrados

### F-09 — orçamento de stack do parser (P013) limita o aninhamento de módulos

- **Sintoma:** `cargo test -p spectra-api --test contract_drift` falhava com
  `embedded std sources must have valid semantic exports: [Parse(... "nesting
  too deep" ...)]` para `stdlib/src/path.spectra` e, depois, `text.spectra`,
  embora `spectralang check/run` compilasse os dois normalmente.
- **Causa:** `compiler/src/parser/mod.rs` limita o parse a
  `MAX_STACK_USE_BYTES = 512 KiB` medidos desde `Parser::new`. Em builds debug
  cada nível de recursão custa ~17 KiB (medido com instrumentação temporária:
  `depth=30 stack=528432 over_stack=true`), então funções com ~30 níveis
  sintáticos (cadeias `if/else if` aninhadas + argumentos de chamada aninhados)
  estouram o orçamento **em threads de teste**, mesmo sendo código legítimo.
- **Correção (lado do módulo):** `std.path.normalize` foi dividido em
  `collect_segments`/`append_segment` e `std.text.unescape_json` em
  `apply_escape`/`simple_escape_code`/`apply_unicode_escape`/`push_scalar`.
  Ambos ficaram mais legíveis e abaixo do orçamento.
- **Recomendação (lado da linguagem):** transformar o orçamento em fração do
  stack disponível da thread (limites reais) ou reduzir o custo por frame; o
  valor absoluto de 512 KiB é sensível a builds debug e a threads com stack
  pequena, e o erro P013 não diz *qual* módulo falhou.
- **Evidência:** `cargo test -p spectra-api --test contract_drift` → 7 passed;
  fixtures 801/802 seguem verdes em JIT e AOT.

## Revisão independente (gate krypton) — achados e correções

Revisor independente (`reviewer`) auditou o diff completo; 9 achados, todos
corrigidos com caso de fixture dedicado:

| ID | Severidade | Achado | Correção | Fixture |
|---|---|---|---|---|
| F-10 | alta | `std.algorithms.binomial` usava `k` no numerador mesmo com a malha iterando `min(k, n-k)` → `binomial(5, 4)` devolvia 2 em vez de 5 | numerador passa a usar o mesmo limite da malha (`n - steps + step`) | 797 códigos 58–60 |
| F-11 | alta | `std.semver.is_valid` rejeitava hífens que só aparecem em build metadata (`1.0.0+build-1`), contaminando `compare`/`major`/`minor`/`patch`/`prerelease` | novo `prerelease_dash` acha só o `-` antes do primeiro `+` | 806 códigos 40–45 |
| F-12 | média | `std.fmt.float_fixed` arredondava errado a um ulp do empate (`0.49999999999999994 + 0.5 == 1.0` em IEEE) | compara a fração descartada com o empate em vez de somar 0.5 | 805 códigos 35–37 |
| F-13 | baixa | `std.stats.percentile_f` aceitava `NaN` (comparações com NaN são falsas) | guarda `math.is_nan_f(percent)` | 799 código 32 |
| F-14 | baixa | `std.fmt.bytes_si` negava `i64::MIN` (sinal duplo) e parava em PB | caminho dedicado para `i64::MIN` + unidade `EB` | 805 códigos 38–39 |
| F-15 | baixa | `std.validate.url_is_valid` aceitava `http://?q=1` (host vazio) | rejeita autoridade vazia/iniciada por `?` ou `#` | 800 códigos 41–42 |
| F-16 | baixa | `std.path.parent`/`join` preservavam `\` apesar do contrato canônico `/` | `canonical()` aplicado às saídas | 801 códigos 42–43 |
| F-17 | baixa | `std.calendar.padded` colocava o sinal dentro do campo de ano (`00-1-12-31`) | sinal fora do padding (`-0001-12-31`) | 803 código 40 |
| F-18 | baixa | `'a' < 'b'` em `const` era aceito pela semântica mas o folder não tinha braço `Char` (erro de lowering) | braço `Char` adicionado nos dois folders (semântico e midend) | probe local + suíte do compilador |

Observação do revisor mantida como não-achado: `\u0000` não fecha round-trip
por `str.from_scalar(0)` (strings são NUL-terminadas no ABI empacotado) — limite
de plataforma documentado, herdado pelos helpers novos.

## Ferramentas ajustadas (não só conteúdo)
- `scripts/generate_stdlib_catalog.py`:
  - `signature_ir_return` agora mapeia `Option_<T>`/`Result_<T>_Error` para a
    gramática IR (`Option<string>`, `Result<string>`), igual aos descritores.
  - `fixture_for`/`catalog_fixture` preferem o probe **dedicado** ao probe
    genérico de namespace, para o catálogo apontar o fixture que realmente
    exercita o símbolo.
- `scripts/generate_lowering_tables.py`: `from_scalar` adicionado ao `LAYOUT` de
  `std.string`.
- `compiler/src/semantic/mod.rs`: teste que fixava `functions.len() == 2` para
  `std.algorithms` foi reescrito para checar visibilidade/ausência de
  `stdlib_path` (contagem era incidental).

## Ambiente

- Três processos `spectralang.exe` presos (laço infinito de F-06) bloquearam o
  link do binário (`Access denied`, os error 5); foram encerrados e o build
  refeito antes de cada verificação.

## Dívida pré-existente do repositório (não introduzida por este change set)

- `cargo fmt --all -- --check` já é vermelho em `HEAD` (ex.: `backend/src/aot.rs`,
  `compiler/src/semantic/mod.rs` linhas 319/466/811, `midend/src/passes/verification.rs`).
  Os arquivos editados aqui passam `rustfmt --check` exceto por trechos
  pré-existentes fora das regiões alteradas.
- `cargo clippy -p spectra-compiler -p spectra-midend -p spectra-runtime
  --all-targets -- -D warnings` também é vermelho em `HEAD`
  (`verification.rs`, `lint/mod.rs`, `stdlib/map.rs`, `vector_index.rs`, etc.);
  nenhum aviso aponta para arquivos/regiões alterados neste change set.
- `python scripts/validate_execution_coverage.py --update-baseline` (corpus
  completo) não foi concluído nesta sessão — a gravação em massa do baseline
  deve ser feita em máquina ociosa com revisão do diff; os fixtures novos já
  passam como `NEW` em JIT e AOT (`target/execution-coverage/*.json`).
