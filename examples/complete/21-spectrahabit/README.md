# SpectraHabit

SpectraHabit é um rastreador local de hábitos em SpectraLang. Cada hábito tem
uma meta semanal de 1 a 7 dias; check-ins podem ser registrados com data e nota,
e o programa calcula sequências consecutivas e progresso em períodos de sete
dias.

## Estrutura do projeto

```text
21-spectrahabit/
├── spectra.toml
└── src/
    ├── main.spectra
    ├── cli/parser.spectra
    ├── domain/model.spectra
    ├── domain/codec.spectra
    ├── reports/output.spectra
    ├── services/habits.spectra
    ├── storage/database.spectra
    └── validation/self_tests.spectra
```

O parser valida os comandos, o domínio contém regras de calendário e metas, o
serviço mantém hábitos e check-ins, o codec serializa registros, o armazenamento
publica o arquivo local e os relatórios formatam histórico, streak e semana.
Cada módulo passa records simples ou texto; handles de coleções ficam no serviço.

## Compilar e executar

Na raiz do repositório SpectraLang:

```powershell
.\target\debug\spectralang.exe check --json examples/complete/21-spectrahabit
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectrahabit.exe examples/complete/21-spectrahabit
```

Execute o binário no diretório em que deseja manter seus dados:

```powershell
.\target\spectrahabit.exe init
.\target\spectrahabit.exe add 5 "Leitura matinal"
.\target\spectrahabit.exe checkin 1 2026-09-21 "Li um capítulo"
.\target\spectrahabit.exe streak 1
.\target\spectrahabit.exe week 2026-09-21
```

Datas usam o calendário gregoriano e o fuso UTC. Check-ins retroativos são
aceitos; datas futuras são recusadas. Uma nota é opcional e pode conter espaços
quando passada entre aspas pelo terminal.

## Comandos

```text
spectrahabit init
spectrahabit add <dias-por-semana 1..7> <nome>
spectrahabit list [--all]
spectrahabit checkin <id> <AAAA-MM-DD> [nota]
spectrahabit uncheck <id> <AAAA-MM-DD>
spectrahabit history <id>
spectrahabit streak <id> [data-final]
spectrahabit week <data-inicial>
spectrahabit goal <id> <dias-por-semana 1..7>
spectrahabit archive <id>
spectrahabit remove <id>
spectrahabit self-test
```

- Cada hábito recebe um ID crescente, que não é reutilizado após a remoção.
- Só pode existir um check-in por hábito em cada dia. `uncheck` remove o registro
  daquele dia; `history` mantém os registros restantes na ordem de criação.
- `streak` conta dias consecutivos terminando na data informada. Sem data, usa o
  dia atual em UTC. Se não houver check-in na data final, a sequência é zero.
- `week` considera a data inicial e os seis dias seguintes, inclusive. A meta é
  cumprida quando o número de check-ins no período alcança os dias semanais
  definidos para o hábito.
- `archive` preserva o histórico e impede novos check-ins. `list --all` também
  mostra hábitos arquivados; `remove` apaga o hábito e seu histórico.

## Persistência e integridade

O primeiro `init` cria `.spectrahabit/habits.db`. O cabeçalho guarda a versão do
formato e o próximo ID. Registros `H` descrevem hábitos e registros `C` guardam
check-ins. Campos de texto escapam `%`, tabulações e quebras de linha.

Atualizações passam por `habits.db.tmp`, são relidas e verificadas antes da
publicação; o arquivo anterior fica em `habits.db.bak` durante a troca. Uma
execução posterior restaura o backup se o arquivo principal estiver ausente.
Cabeçalhos, hábitos, datas, IDs, referências e duplicatas são validados antes de
aceitar o estado persistido.

## Verificação

```powershell
.\target\debug\spectralang.exe fmt --check examples/complete/21-spectrahabit
.\target\debug\spectralang.exe check --json examples/complete/21-spectrahabit
.\target\debug\spectralang.exe run examples/complete/21-spectrahabit -- self-test
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectrahabit-dev.exe examples/complete/21-spectrahabit
pwsh -NoProfile -File tests/spectrahabit-integration.ps1 -Executable target/spectrahabit-dev.exe
```

A integração exercita comandos em processos AOT separados, recuperação de
backup e recusa de dados corrompidos; também faz um fluxo JIT de criação,
check-in e cálculo de sequência em um diretório temporário.

## Limites atuais

- Os dados são locais e o exemplo pressupõe um processo escritor por vez.
- Não há sincronização entre dispositivos, lembretes ou calendário visual.
- Datas e início das semanas são escolhidos pelo usuário; o relatório semanal
  percorre sete dias consecutivos a partir da data informada.
