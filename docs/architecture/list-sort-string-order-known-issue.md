# Correção: ordenação tipada de `List<T>`

**Status:** corrigido em 2026-09-23 e validado em JIT e AOT.

## Resumo do defeito

Antes da correção, `std.collections.list_sort` não ordenava strings pelo
conteúdo. O contrato dizia que a função ordenava a lista em ordem crescente,
mas o runtime aplicava uma comparação numérica ao valor `i64` armazenado para
cada elemento. Para `List<string>`, esse valor era o endereço do buffer. Em
processos JIT distintos, a mesma entrada produziu ordens diferentes; no AOT
reproduzido, a lista permaneceu na ordem errada em cinco execuções.

Isso impedia usar `list_sort` para produzir uma ordem lexical estável de nomes,
caminhos ou chaves string. O exemplo `spectragit` contornava o problema com
`codec.sort_paths`, que comparava o conteúdo por `string.char_at`.

## Reprodução antes da correção

Salve este arquivo como `repro/main.spectra`:

```spectra
module list_sort_repro

from std.collections import List
import std.collections as collections
import std.option as option
import std.io as io

public func main() returns int {
    let values: List<string> = collections.list_new()
    collections.list_push(values, "z")
    collections.list_push(values, "a")
    collections.list_push(values, "m")

    collections.list_sort(values)
    io.println(option.option_unwrap(collections.list_get_option(values, 0)))
    io.println(option.option_unwrap(collections.list_get_option(values, 1)))
    io.println(option.option_unwrap(collections.list_get_option(values, 2)))
    collections.list_free(values)
    return 0
}
```

Execute pelo JIT e gere a versão AOT:

```powershell
1..5 | ForEach-Object { .\target\debug\spectralang.exe run .\repro }
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe .\target\list-sort-repro.exe .\repro
1..5 | ForEach-Object { .\target\list-sort-repro.exe }
```

Para a entrada `z`, `a`, `m`, a ordem esperada é `a`, `m`, `z`. Antes da
correção, as cinco execuções JIT produziram `a,m,z`, `a,m,z`, `z,m,a`, `m,z,a`
e `a,m,z`; o AOT produziu `z,a,m` nas cinco execuções. O resultado JIT às
vezes coincidia com a ordem esperada, mas variava entre processos. Como
controle, `List<int>` com `30`, `10`, `20` produziu `10`, `20`, `30`.

## Causa confirmada

Os elementos da lista são armazenados como `VecDeque<SpectraHostValue>`, e
`SpectraHostValue` é uma palavra `i64`. A ordenação antiga chamava `.sort()`
diretamente sobre essas palavras. Como a chamada recebia apenas o handle, o
runtime não conhecia o tipo de `T`; em `List<string>`, comparava os endereços
dos buffers em vez dos bytes do texto.

## Correção aplicada

- A assinatura de origem continua `list_sort<T>(list: List<T>)`. Durante o
  lowering, a linguagem resolve o tipo concreto do elemento e acrescenta uma
  tag interna ao host call.
- O ABI rápido recebe `(handle, sort_kind)`. O dispatcher genérico também
  encaminha a tag. Chamadas sem tag são recusadas, então nenhuma rota ordena
  palavras `i64` sem conhecer o tipo.
- O runtime compara `int`, inteiros exatos com e sem sinal, floats, booleanos,
  strings e caracteres pelo valor. Inteiros exatos respeitam sinal e largura;
  floats usam ordem total IEEE 754; strings usam bytes UTF-8 sem locale; chars
  usam seu valor Unicode.
- Records, tuplas, handles e outros tipos sem ordem escalar implícita geram um
  diagnóstico no lowering. O runtime também rejeita tags desconhecidas.
- `examples/complete/18-spectragit` agora chama `collections.list_sort` para
  normalizar entries, combinar caminhos e listar branches/snapshots. As funções
  manuais `codec.sort_paths` e `codec.lexically_before` foram removidas.
- `docs/reference/05-stdlib.md` e `docs/AI-AGENT-REFERENCE.md` descrevem os
  tipos aceitos e as regras de comparação.

## Regressões e validação

`tests/validation/618_stdlib_list_sort_typed.spectra` cobre lista vazia,
strings criadas em runtime, duplicatas, prefixos e UTF-8, inteiros com sinal,
`f32`, booleanos, `u8` e chars. O fixture passou cinco execuções JIT e cinco
AOT em processos separados. Testes Rust também verificam comparações de
conteúdo, sinal e largura, a tag no lowering e no dispatcher genérico, e a
recusa de aggregates sem ordem e de chamadas sem tag.

Validações executadas após a correção:

```text
cargo build -p spectra-cli                                      passou
cargo test -p spectra-midend                                    115 passaram
cargo test -p spectra-runtime                                   242 passaram + 1 gate
cargo test -p spectra-backend                                   76 passaram
python scripts/generate_lowering_tables.py --check             passou (1035 arms)
python scripts/generate_host_calls.py --check                   passou (560 bindings)
spectralang check --json tests/validation/618_stdlib_list_sort_typed.spectra  passou
spectralang check --json examples/complete/18-spectragit       passou (9 módulos)
spectralang fmt --check tests/validation/618_stdlib_list_sort_typed.spectra   passou
spectralang fmt --check examples/complete/18-spectragit         passou (9 arquivos)
spectralang run examples/complete/18-spectragit -- self-test    passou (14 verificações)
tests/spectragit-integration.ps1                                passou
```

Também foram gerados e executados executáveis AOT para o fixture e para o
SpectraGit. A integração AOT validou commits, branches, alterações staged e
locais, remoções, snapshots, restauração e integridade de objetos.

`cargo fmt --all -- --check` ainda aponta diferenças de formatação em arquivos
preexistentes fora deste conjunto de mudanças; esses trechos foram mantidos sem
alteração. As adições Rust foram formatadas com `rustfmt`, e os arquivos
SpectraLang alterados passaram `spectralang fmt --check`.

## Limitação restante

Records e outros aggregates não têm ordem implícita. `list_sort` informa um
diagnóstico em vez de produzir uma ordem baseada em layout ou endereço. Uma
evolução futura pode oferecer uma API genérica de comparador para qualquer
`T`; até lá, o código que conhece os campos do aggregate deve definir a ordem.
