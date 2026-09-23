# spectragit

`spectragit` é um versionador local para arquivos de texto, implementado em
SpectraLang. Ele mantém objetos endereçados por conteúdo e não invoca Git nem
depende de um repositório Git para suas operações.

## Compilar e iniciar

Na raiz do repositório SpectraLang, compile o executável:

```powershell
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectragit.exe examples/complete/18-spectragit
```

Execute-o no diretório que será versionado:

```powershell
.\target\spectragit.exe init
.\target\spectragit.exe status
```

Os exemplos abaixo usam `spectragit` como nome do comando. No Windows, use o
caminho `.\target\spectragit.exe` ou coloque uma cópia chamada
`spectragit.exe` em um diretório do `PATH`.

## Comandos

```text
spectragit init
spectragit status
spectragit add <arquivo|diretório|.> [...]
spectragit commit -m <mensagem>
spectragit log
spectragit diff [--staged]
spectragit branch [nome]
spectragit checkout <branch>
spectragit restore <arquivo> [--source <commit>]
spectragit explain <commit>
spectragit timeline
spectragit snapshot create <nome>
spectragit snapshot list
spectragit snapshot restore <nome>
spectragit inspect <hash>
spectragit self-test
```

- `status` separa alterações staged, alterações locais e arquivos não rastreados.
- `add` aceita vários caminhos, diretórios aninhados e `.`. Ele também prepara
  remoções detectadas dentro do escopo informado.
- `diff` compara o diretório de trabalho com o staging; `diff --staged` compara
  o staging com o commit atual. As diferenças exibem linhas removidas e
  adicionadas.
- `checkout` exige staging vazio e nenhuma alteração em arquivos rastreados.
  Arquivos não rastreados sem colisão são preservados; uma colisão com o
  conteúdo de destino bloqueia a troca.
- `restore <arquivo>` recupera a versão do commit atual e remove aquele caminho
  do staging. `--source <commit>` seleciona outro commit como origem.
- `snapshot create` salva uma árvore completa do diretório de trabalho, incluindo
  arquivos ainda não rastreados. Restaurar um snapshot exige staging vazio e
  arquivos rastreados sem alterações locais.
- `explain` resume os caminhos e a estimativa de linhas alteradas em um commit.
- `timeline` mostra pontas de branches e o histórico da branch atual.
- `inspect` valida e exibe um objeto blob, tree ou commit.
- `self-test` executa verificações Spectra de hash, escape, caminhos, ordenação,
  serialização, aplicação de alterações e contagem do diff por linhas.

## Persistência

O primeiro `init` cria `.spectragit/`. Os arquivos `HEAD`, `index` e
`refs/heads/` guardam a branch atual, as alterações staged e as pontas das
branches. `refs/snapshots/` guarda nomes de snapshots. `objects/` contém blobs,
trees e commits imutáveis identificados por `sg1-...`.

Blobs guardam o conteúdo UTF-8 dos arquivos. Trees são listas canônicas de
caminhos e IDs de blobs; commits registram tree, pai, autor, horário Unix e
mensagem. A serialização estável faz conteúdo igual produzir IDs iguais. Ao
carregar um objeto, o programa recalcula o ID e rejeita dados adulterados. O
hash atual combina três lanes determinísticas de 31 bits (cerca de 93 bits no
total); ele é não criptográfico e serve para identidade e detecção de corrupção,
não para proteção contra ataques de colisão.

O autor vem de `SPECTRAGIT_AUTHOR`, depois de `USERNAME` ou `USER`. O diretório
de trabalho atual é a raiz do repositório. Não há repositórios remotos, merge,
remoção de branch ou remoção de snapshot.

## Limites conhecidos

- O armazenamento suporta texto UTF-8; arquivos binários e metadados de arquivo
  como permissões e timestamps não são preservados.
- O diff é por linhas e localiza o trecho central diferente usando prefixos e
  sufixos comuns. As contagens em `explain` são aproximadas, não um diff LCS.
- Diretórios vazios não entram nas trees. O limite de profundidade da varredura é
  64 níveis.
- A API `std.fs` disponível não expõe metadados de links simbólicos. Portanto,
  links simbólicos não têm suporte garantido e devem ser evitados neste exemplo.
- A referência de tempo gravada no commit é Unix em segundos.

## Verificação

Na raiz do repositório SpectraLang:

```powershell
.\target\debug\spectralang.exe check --json examples/complete/18-spectragit
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectragit.exe examples/complete/18-spectragit
pwsh -NoProfile -File tests/spectragit-integration.ps1
```

O teste de integração cria e remove um diretório isolado sob a pasta temporária
do sistema. Ele executa o binário real em processos separados e cobre commits,
branches, alterações staged e locais, snapshots, restauração, integridade dos
objetos e preservação de arquivos não rastreados em colisões.
