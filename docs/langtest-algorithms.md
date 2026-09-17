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
arquivos: `check`, `run -O0`, `run -O3`, `lint` e `fmt --check` passam nos
28 arquivos.

## Quarta leva (22–28): correção no próprio teste

- `24_kmp_search.spectra` — a primeira versão reutilizava a tabela `pi` de
  outro padrão entre buscas (falha `exit 6`, não bug da linguagem: KMP exige
  o prefixo do padrão buscado). Correção no `.spectra`: `prefix_fn` do
  padrão correto antes de cada `kmp_find`. Nenhuma mudança no compilador
  nesta leva; o gate `-O0` do validador (introduzido no R527) passou em
  todos os 28 arquivos de primeira, confirmando que o fix anterior segura.
Por isso não há alteração em `compiler/`, `midend/`, `backend/` ou `runtime/`
neste change — apenas arquivos novos sob `examples/langtest/` mais este doc e
o validador. Se um bug real aparecer no futuro, a correção deve entrar no crate
responsável com regressão em `tests/validation/` ou `tests/errors/`.
