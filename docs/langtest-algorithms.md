# Algoritmos de teste de linguagens em SpectraLang

Suíte executável dos principais algoritmos usados no teste de linguagens de
programação, implementada em `.spectra` puro em `examples/langtest/`.
Cada arquivo é autoverificável: `main` retorna `0` quando todas as
invariantes passam e um código distinto por estágio quando alguma falha.

## Arquivos

| Arquivo | Algoritmo | Ideia central |
|---|---|---|
| `01_dfa_lexer.spectra` | DFA léxico (Thompson + subset construction + Hopcroft) | Estados S0/S1/S2 por classes alpha/digit/outro; `count_idents`/`count_ints` com maximal munch, dígitos dentro de identificador não contam como inteiro |
| `02_ll1_table_parser.spectra` | Parser preditivo LL(1) | Gramática E/T/F sobre tokens com 1 lookahead; decisão explícita por kind, sem backtracking |
| `03_pratt_precedence.spectra` | Pratt / precedence climbing | `parse_expr(min_prec)` com tabela `prec`, associatividade à esquerda, `%` no mesmo nível de `*`/`/` |
| `04_hm_unification.spectra` | Unificação Hindley-Milner (toy) | Tags 10/11/12 + variáveis 100+id, union-find com `parent`/`binding`, resolve após união |
| `05_dataflow_liveness.spectra` | Liveness (dataflow backward) | CFG diamante B0→B1→B2\|B3→B3, ponto fixo iterativo (máx. 10 iterações), um array 0/1 por variável |
| `06_fuzz_differential.spectra` | Fuzzing + diferencial + metamórfico + cobertura | LCG determinístico (seed 42) gera 40 expressões; avaliadores recursivo × Pratt precisam concordar; mutação de operador; EMI `x+0==x`, `x*1==x`; contadores por operador com fallback dirigido |
| `07_ddmin_reduction.spectra` | Delta debugging (ddmin) | Predicado monótono "contém 1, 2 e 3"; remoção de chunks com granularidade dobrada; `[9,1,9,2,9,3,9]` → `[1,2,3]` |
| `08_nfa_thompson.spectra` | Simulação de NFA (Thompson) | Regex `a(b|c)*` com epsilon-closure iterativa; aceita `a`, `abcbcb`, rejeita `""`, `aa`, `abd` |
| `09_cyk_parser.spectra` | CYK (programação dinâmica, CNF) | Gramática S→AB\|BC, A→BA\|a, B→CC\|b, C→AB\|a; tabelas 5×5 achatadas por não-terminal; `baaba` aceita, `bb` rejeitada |
| `10_lr0_closure.spectra` | Autômato LR(0): closure/goto | Itens `prod*4+dot` para `S'→E, E→E+T\|T, T→id\|(E)`; closure(I0)={0,4,8,12,16}, goto(E)={1,5}, goto(T)={9}, goto(id)={13} |
| `11_reaching_definitions.spectra` | Reaching definitions (forward) | Dual do liveness: OUT=GEN∪(IN−KILL), IN=união dos predecessores; redefine `y` em B2 mata d2; converge e valida matança |
| `12_mutation_testing.spectra` | Mutation testing | 4 mutantes (troca de operador, off-by-one, inversão min/max, paridade); suite mata 4/4, score 100 |
| `13_quickcheck_properties.spectra` | Propriedades QuickCheck | Roundtrip int↔string (incl. negativos), `reverse(reverse(x))==x` (string e array), sort idempotente com soma preservada, busca binária como invariante |
| `14_interval_abstract.spectra` | Interpretação abstrata (intervalos) | Transfers sound para `+,-,*`, join de branches, sanidade concreto-dentro-do-abstrato (`contains`) em cada assert |
| `15_earley_parser.spectra` | Earley (predict/scan/complete) | Gramática ambígua `S→SS\|(S)\|()`; itens `rule*64+dot*8+origin`, chart achatado 5×20; `()()`, `(())` aceitas, `(()`, `)(` rejeitadas |
| `16_slr_table_parser.spectra` | SLR(1) com ACTION/GOTO | Tabelas do dragão para `E→E+T\|T, T→T*F\|F, F→(E)\|id` (72+36 entradas) com pilha semântica; `2+3*4=14`, entrada inválida retorna sentinela |
| `17_constprop_lattice.spectra` | Constant propagation (must, forward) | Reticulado plano TOP>CONST>BOT com meet de predecessores; `y=6` constante até o encontro, `x` vira BOT após entrada desconhecida |
| `18_dominators.spectra` | Dominadores iterativos (base do SSA) | `DOM(n)={n}∪∩DOM[p]` sobre CFG com laço; conjuntos exatos `DOM3={0,1,3}`, `DOM4={0,1,3,4}` |
| `19_levenshtein_diff.spectra` | Levenshtein (base do diff) | DP com 2 linhas sobre char codes; `kitten/sitting=3`, `saturday/sunday=3` — valida snapshots/diagnósticos |
| `20_concolic_paths.spectra` | Teste concolico estilo DART | Driver sistemático cobre os 4 paths de branches aninhados com entradas dirigidas `{0,1,10,11,20,21}` |
| `21_grammar_fuzzer.spectra` | Geração guiada por gramática | LCG deriva `NUM(OP NUM)*`; `wellformed` checa alternância e o diferencial recursivo×Pratt é o oráculo |
| `22_dpll_sat.spectra` | DPLL (unit+pure+splitting) | CNF 3-literal com pad -1, literal `var*2+sign`; decide SAT com modelo verificável e UNSAT `(x)∧(¬x)` |
| `23_tarjan_scc.spectra` | Tarjan SCC (ciclo em call-graph) | index/lowlink recursivo com single-elem out-params; `{0,1,2}` e `{3,4,5}` isolados |
| `24_kmp_search.spectra` | KMP prefixo + busca | Sem retrocesso sobre o texto; base de busca de símbolos/diagnósticos do tooling |
| `25_graph_coloring.spectra` | Chaitin-Briggs (regalloc do backend) | simplify/select com spill guess, K=3: amostra colorível + K4 que exige spill |
| `26_mark_sweep.spectra` | Mark-and-sweep (runtime) | Mark iterativo com worklist desde as raízes; sweep libera a ilha `5→6` |
| `27_packrat_memo.spectra` | PEG com memoização | Tabela `(regra,pos)` com MISS sentinel; 2ª passada só dá cache hit (contrato linear) |
| `28_topo_sort.spectra` | Kahn (deps do package manager) | Ordem válida com `pos[dep]<pos[pkg]` + ciclo `0→1→2→0` detectado (`count<n`) |
| `29_shunting_yard.spectra` | Shunting-yard (Dijkstra) | Infixa→RPN com pilha de operadores + avaliação com pilha de valores; `2+3*4=14`, `(1+2)*3=9` |
| `30_first_follow.spectra` | FIRST/FOLLOW + tabela LL(1) | Construção por ponto fixo sobre produções como dados; 6 células verificadas (`M[E',+]=P1`, `M[E',$]=P2`…) |
| `31_peephole_opt.spectra` | Passes de midend (fold+alg+DCE) | IR brinquedo com folding, `x+0`/`x*0`/`x*1`, DCE por roots + diferencial antes/depois |
| `32_stlc_bidir.spectra` | Bidirecional STLC (De Bruijn) | `infer` falha em lambda nua, `check` anota; `(λx.x:Int→Int) 5 : Int`, `Int→Bool` rejeitado |
| `33_json_parser.spectra` | Parser JSON (subset tooling) | Extrai soma dos números + conta strings; rejeita `{"a":}` e `[1,]` |
| `34_domfrontiers.spectra` | DF + phis de Cytron (SSA) | Recomputa DOM, deriva idom, DF e posiciona phis para defs `{2,3}` ⇒ `{1,3}` |
| `35_brzozowski.spectra` | Derivadas de regex | Matching por derivação com pool fixo; casa `a(b|c)*` sem autômatos |
| `36_bmh_search.spectra` | Boyer-Moore-Horspool | Bad-character sobre a-z, comparação reversa; tabela verificada + 4 buscas |
| `37_astar_grid.spectra` | A* com Manhattan admissível | Grade 5×5: ótimo 8 com parede + sem-caminho (-1), cadeia de parentes válida |
| `38_gvn_numbering.spectra` | GVN (midend) | Partição com operandos canônicos; redundância fundida em 4 classes |
| `39_lru_cache.spectra` | LRU por timestamps | Despejo do menos recente + update in-place (teste de dispatch-cache) |
| `40_error_recovery.spectra` | Panic-mode (diagnósticos) | Sync=FIRST∪FOLLOW, totaliza erros: `2+*3`→5/1 erro, `(2+3`→5/1 erro |
| `41_scope_resolution.spectra` | Grafo de escopos | Shadowing, resolve por profundidade, redeclare e indefinido |
| `42_taint_analysis.spectra` | Taint may-analysis | Bypass vulnerável (1) vs sanitizado seguro (0) no mesmo CFG |
| `43_aho_corasick.spectra` | Aho-Corasick | Trie + failure BFS com herança de saídas; `he`/`she`/`hers` em 1 passada sobre `ushers` |
| `44_dfa_minimization.spectra` | Table-filling (Myhill-Nerode) | DFA `*ab` com estado duplicado; só o par (0,3) sobrevive ⇒ 3 classes |
| `45_model_checking.spectra` | BFS explícito (TOCTOU) | Check-then-act acha `(2,2)`; variante com flag prova segurança por exaustão |
| `46_program_slice.spectra` | Slice backward (Weiser) | Walk use-def reverso; critério `d` isola o stmt 5 morto |
| `47_short_circuit.spectra` | Demo de curto-circuito | Guardas, skips observáveis, `or`, aninhados e loop-guard anti-OOB |
| `48_escape_analysis.spectra` | Escape p/ stack | Global/arg/store propagam; só o local fica na stack |
| `49_inline_cost.spectra` | Heurística de inline | Limiar + call-site único + vetos (recursivo/raiz/gigante) |
| `50_corpus_minimization.spectra` | afl-cmin guloso | 6 candidatos × 8 arestas ⇒ `{A,B}` ótimo |
| `51_string_interning.spectra` | Hash-cons (lexer) | Buffer bump + (hash,off,len); dedup por id, cheio ⇒ -1 |
| `52_burs_tiling.spectra` | BURS bottom-up (codegen) | DP com SHL p/ `x*2`; `(a*2)+b` com custo ótimo 4 |
| `53_abcd_elimination.spectra` | ABCD-lite (bounds) | Fatos de range removem 2 checks, mantêm 3 |
| `54_glob_matching.spectra` | fnmatch `*?**` (CLI) | Recursão com memo; 7 casos incl. `a**b` |
| `55_left_recursion_elim.spectra` | Eliminação (gramáticas) | `E→E+T\|T` ⇒ `E→TE'`, `E'→+TE'\|ε`, forma verificada |
| `56_callrank_pagerank.spectra` | PageRank (PGO) | d=0.85, 20 iterações; soma~1, sink no topo |
| `57_minimax_ab.spectra` | Minimax + alpha-beta | Nim 7: mesmo valor, poda estrita, lance ótimo 3 |
| `58_rope_buffer.spectra` | Rope (buffers LSP) | Peças paralelas; split/flatten/char_at coerentes |

Relação com o já existente: `tests/validation/524_recursion_recursive_descent_parser.spectra`
cobre descida recursiva sobre strings; esta suíte complementa com DFA tabular,
LL(1) sobre tokens, Pratt iterativo, unificação, dataflow, fuzzing diferencial e
ddmin — nenhum duplica o 524.

## Como executar

```powershell
.\target\debug\spectralang.exe check examples\langtest
.\target\debug\spectralang.exe run examples\langtest\01_dfa_lexer.spectra
.\target\debug\spectralang.exe fmt --check examples\langtest
.\target\debug\spectralang.exe lint examples\langtest\01_dfa_lexer.spectra
python scripts\validate_langtest_algorithms.py --binary .\target\debug\spectralang.exe
```

## Correções aplicadas durante a implementação

1. `06_fuzz_differential.spectra` — `rec_f` ignorava o parâmetro `kinds`
   (`unused-binding` no lint). Correção real no `.spectra`: valida
   `kinds[pos] == NUM` antes de consumir o valor, o que usa o parâmetro e
   ainda fortalece o teste. Lint volta a `no findings`.
3. `11_reaching_definitions.spectra` — declarava `succ_first`/`succ_second`
   sem uso (análise forward só consulta predecessores; `unused-binding` no
   lint). Correção real no `.spectra`: removidas as duas declarações mortas.
   Lint volta a `no findings`.
4. Formatação — 5 dos 7 arquivos novos foram normalizados com
   `spectralang fmt` (sem mudança semântica; `run` revalidado após o `fmt`).

## Bug real encontrado e corrigido (R527)

O `15_earley_parser.spectra` expôs um bug genuíno do midend: `run -O0` e
`run -O1` falhavam com `error[codegen]: Value N not found during backend code
generation`, enquanto `-O2`/`-O3` passavam (o DCE escondia o problema).

- **Causa raiz:** `build_call(..., has_return=true)` criava um valor SSA
  fantasma para chamadas a funções `unit`; o lowering do `if` o colocava no
  phi de merge quando um braço terminava em chamada unit. Qualquer `if/else`
  como statement com tails unit (`if c { noop(1) } else { noop(2) }`)
  reproduzia em 12 linhas.
- **Correção (`midend/`):** chamadas a funções com retorno `Void` agora
  emitem `build_call(..., has_return=false)` e devolvem `const 0` — o mesmo
  contrato já usado por host calls Void e chamadas de closure. Aplicado nos
  3 sites com o padrão: `lowering_expr_calls.rs` (chamada direta),
  `lowering_expr_method.rs` (função qualificada e método `Type_method`),
  mais o helper `user_function_returns_unit` em `lowering_impl_core.rs`.
- **Regressão:** `tests/validation/527_unit_call_branch_merge.spectra`
  (passa em `-O0`/`-O1`/`-O2`/`-O3`) e o validador agora roda toda a suíte
  também com `-O0`, mantendo o gate sensível a essa classe de bug.
- **Validação da correção:** `cargo test -p spectra-midend` (78 passed),
  `cargo test -p spectra-compiler` (94 passed), suíte langtest 21/21 em
  default e `-O0`, `fmt --check` limpo, regressão pontual (524, 211, 206,
  276, 155) em `-O0` e default.

Nenhum outro defeito de compilador/runtime foi encontrado nos demais
arquivos daquela leva: `check`, `run -O0`, `run -O3`, `lint` e `fmt --check`
passavam nos 28 arquivos então existentes (a suíte hoje tem 58).

## Quarta leva (22–28): correção no próprio teste

- `24_kmp_search.spectra` — a primeira versão reutilizava a tabela `pi` de
  outro padrão entre buscas (falha `exit 6`, não bug da linguagem: KMP exige
  o prefixo do padrão buscado). Correção no `.spectra`: `prefix_fn` do
  padrão correto antes de cada `kmp_find`. Nenhuma mudança no compilador
  nesta leva; o gate `-O0` do validador (introduzido no R527) passou em
  todos os 28 arquivos de primeira, confirmando que o fix anterior segura.

## Oitava leva (51–58): dois bugs reais numéricos

O `56_callrank_pagerank` (aritmética mista `int*float`) expôs dois bugs
encadeados de tipos numéricos.

1. **Widening ausente (`midend/src/lowering_expr_binary.rs`).** A semântica
   permite `int`+`float` (`can_auto_promote`, resultado float), mas o
   lowering repassava operandos mistos e o backend emitia `fmul.i64`
   (verifier) ou panica em local promovido. Agora
   `widen_mixed_int_float_operands` insere `Cast` tipado (com signedness)
   para o lado float — cobrindo Add/Sub/Mul/Div/Rem, comparações e `==`
   (que antes truncava `1 == 1.5` para `true` ao forçar rhs→Int!). Backend
   `Cast` já tratava sint/uint; só faltava emitir.
2. **Slots com tipo errado (`lowering_impl_blocks.rs` +
   `lowering_impl_methods.rs`).** O pré-pass de slots rodava sem env:
   `let y = x + 1.0` (tipo visível só via `let` anterior) e
   `let y = p + 1.0` (params fora de `variable_types` em `lower_function`,
   ao contrário de `lower_method`) caíam no fallback `Int` — e `float`/`bool`
   mutado através de blocos panica no frontend (slot I64 + store F64).
   Também havia `tf1` passando por acidente (bits de `1.0` lidos como int
   dão ~4.6e18, e `> 0.0` continuava verdadeiro!). Fix: escopo scratch com
   push/pop no pré-pass + seed sequencial first-wins de `let`s e params;
   sem vazamento para o lowering real.
- **Regressões:** `tests/validation/530_promoted_slot_types.spectra`
  (cadeia de lets, param float, bool, valores exatos que pegariam a
  reinterpretação) em `-O0`–`-O3`. O 56 virou o teste de sistema da
  aritmética mista (soma~1 com tolerância só fecha com floats exatos).
- **Correções nos testes da leva:** `orhs`/`olen` conferidos à mão no 55;
  memo 48 slots dimensionado pelo `(ti,pi)` máximo no 54; `left_lens` e
  `right_lens` do 58 passaram a ser asseridos via `rope_len`.
- **Validação dos fixes:** `cargo test -p` midend/backend/compiler verdes,
  suíte langtest 58/58 em default e `-O0`, sweeps abaixo.

## Sétima leva (43–50): três bugs reais de compilador

O `15_earley_parser`/`02_ll1_table_parser` (com `and`) e os probes de guarda
exigiram investigar fundo. Resultado: `and`/`or` eram **eager** (sem
curto-circuito) e, ao implementar o curto-circuito, dois bugs latentes
apareceram.

1. **Curto-circuito (`midend/src/lowering_expr_binary.rs`).** `and`/`or`
   avaliavam ambos os lados + 1 instrução eager, então
   `i >= 0 and a[i] == v` avaliava o OOB (o bounds-check da leva passada o
   expôs). Agora baixam para `rhs/short/merge` com phi bool/bool, espelhando
   o lowering do `if`. Semântica garante operandos bool.
2. **Tipos dos params de bloco phi (`backend/`).** Params eram sempre I64:
   phis bool/float quebravam o verifier (`arg has type i8, expected i64`).
   `if` com valor bool já era quebrado antes (`probe_boolphi`). Agora
   `get_phi_args` declara params preguiçosamente com o tipo do primeiro
   jump (JIT+AOT), com fallback I64 p/ blocos nunca saltados.
3. **Inliner (`midend/src/passes/function_inlining.rs`, 2 pontos).**
   (a) Clonagem remapeava valores mas não os blocos dos phis; (b) ao dividir
   o bloco do call, phis continuavam nomeando o bloco original em vez da
   continuação. Ambos davam `PHI for target block N is missing incoming`
   só em O2 (ex.: `max` puro inlineado!). Corrigidos com remap + rewire.
4. **Robustez (`backend/src/codegen_core.rs`, `aot.rs`).** Erro de backend
   deixava o `FunctionBuilderContext` sujo e o próximo `define` panica
   (`debug_assert func_ctx.is_empty`). Reset no início de cada `define`.
- **Regressões:** `tests/validation/528_short_circuit_and_or.spectra`
  (guardas, skips observáveis, bool-phi), `529_inlined_branch_merge`
  (phi no callee + call em pred de phi), teste unitário de backend
  (`backend_error_does_not_poison_builder_context`) e
  `examples/langtest/47_short_circuit.spectra`.
- **Correções nos testes da leva:** aresta `her→s` faltante, cópia de
  outputs que sobrescrevia o próprio padrão e `fail[hers]=3` no 43;
  `defs[5]=-1` no 46; `46` usa `if`s aninhados porque `and` não tem
  curto-circuito na guarda (o que, junto ao bounds-check, armou o trap 101
  e revelou o bug nº 1 acima); `40` trocado por parêntese faltante;
  `leave.dname` removido no 41.
- **Validação dos fixes:** `cargo test -p` midend/backend/compiler verdes,
  suíte langtest 50/50 em default e `-O0`, sweep amplo abaixo.

## Sexta leva (36–42): só correções no próprio teste

- `36_bmh_search.spectra` — tabela esperada corrigida (`a→1`, não 4) e
  removida linha morta.
- `40_error_recovery.spectra` — o caso "lixo trailing" não gerava erro
  (o loop de `E` só consome `+`/`-`); trocado por parêntese faltante, que
  exercita o caminho de erro de verdade.
- `41_scope_resolution.spectra` — typo `depth[0]]` (erro de sintaxe) e
  param morto `leave.dname` removido; lint limpo.
- `42_taint_analysis.spectra` — removido loop morto no início de `analyze`.
- Nenhum bug de compilador/runtime nesta leva: `check`, `run -O0`,
  `run -O3`, `lint` e `fmt --check` passam nos 42 arquivos; o gate `-O0`
  segue verde em tudo (R527 continua segurando).

## Quinta leva (29–35) + fix de safety pendente (bounds-check dinâmico)

Correções nos próprios testes:

- `33_json_parser.spectra` — contagem manual errada do cursor (`pos != 22`,
  o correto é 25 para `{"a":[1,2,true],"b":null}`); corrigido o assert.
- `35_brzozowski.spectra` — `nullable` da ALT olhava `node-1` em vez dos
  filhos; corrigido para `nullable(l) or nullable(r)` (e `and` no CONCAT).
- `31_peephole_opt.spectra` — `is_const`/`cval`/`used` com tamanho 6 para
  registradores `0..6`; ampliados para 8.
- `31`/`32` — params mortos removidos (`alg_pass.dst`, `dom_of.ty`); lint
  limpo.

**Fix implementado: índice dinâmico OOB em array agora é fail-fast.**
Antes, índice estático OOB era erro de compilação
(`error[semantic]: Array index 10 out of bounds`), mas `a[i]` dinâmico fora
da faixa era silencioso (`a[10]=99; return a[10]` devolvia 99): o backend
(`GetElementPtr` em `backend/src/codegen_instruction_memory.rs`) calculava
`ptr+index*size` sem checagem e sem length no site.

- **Implementação:** `GetElementPtr` ganhou `bound: Option<usize>` (mede
  `midend/src/ir.rs`); o midend anexa o tamanho nos dois sites de indexação
  do usuário (leitura em `lowering_expr_aggregates.rs`, escrita em
  `lowering_impl_statements.rs`) quando a inferência conhece o tamanho
  (`size > 0`); o backend emite `icmp ult(index, len)` com bloco de panic
  via `spectra_rt_panic` (`runtime error: array index out of bounds`, exit
  101) — o mesmo contrato do `integer division by zero`. Unsigned também
  pega índices negativos. O construtor antigo `build_getelementptr` delega
  com `None` (zero mudança nos demais GEPs internos) e o `bound` é
  preservado pelo inliner.
- **Regressões:** `tests/errors/array_index_oob_read.spectra`,
  `array_index_oob_write.spectra` e `array_index_oob_copy.spectra`
  (registrados em `run_tests.ps1` `runtimeErrorFixtures`, falham com 101 em
  `-O0`–`-O3`) + 2 testes de backend (`bounded_gep_lowering_passes_verifier`,
  `aot_bounded_gep_references_spectra_rt_panic`, cobrindo JIT e AOT).
- **Limites honestos (não cobertos):** params `[T]` baixam para `[0 x int]`
  (tamanho desconhecido no callee) — seguem silenciosos e exigem fat
  pointers (trabalho futuro); escritas em `String` têm length dinâmico
  (leituras já vão por `char_at`, que retorna -1). Cópias (`let b = a`)
  **são** cobertas: o frontend propaga o tipo dimensionado (`[int; 3]`),
  então o bound acompanha o valor (travado pelo fixture `_copy`).
- **Validação do fix:** `cargo test -p spectra-backend` (61 passed, incl. os
  2 novos), `-p spectra-midend` (78), `-p spectra-compiler` (94), suíte
  langtest 35/35 em default e `-O0`, slice array-heavy (10 arquivos) em
  default e `-O0`, fixtures com mensagem exata em `-O0`–`-O3`.
