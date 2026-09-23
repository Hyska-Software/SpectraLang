# SpectraLedger

SpectraLedger é um controle financeiro local em SpectraLang. Registra receitas e
despesas por data e categoria, acompanha limites mensais e calcula resumos sem
usar ponto flutuante: todo valor persistido é um número inteiro de centavos.

## Estrutura do projeto

```text
20-spectraledger/
├── spectra.toml
└── src/
    ├── main.spectra
    ├── cli/parser.spectra
    ├── domain/model.spectra
    ├── domain/codec.spectra
    ├── reports/output.spectra
    ├── services/ledger.spectra
    ├── storage/database.spectra
    └── validation/self_tests.spectra
```

O parser valida os argumentos antes de chamar o serviço. O domínio valida datas,
categorias e dinheiro; o codec serializa registros; o armazenamento grava um
ledger local; o serviço implementa as operações; e os relatórios formatam
listas, limites e resumos. O estado fica dentro do processo de serviço: entre
módulos passam records simples e texto, sem compartilhar handles de coleções.

## Compilar e executar

Na raiz do repositório SpectraLang:

```powershell
.\target\debug\spectralang.exe check --json examples/complete/20-spectraledger
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectraledger.exe examples/complete/20-spectraledger
```

Execute o binário no diretório em que deseja manter seus dados:

```powershell
.\target\spectraledger.exe init
.\target\spectraledger.exe add expense 2026-09-03 25.90 groceries "Mercado do bairro"
.\target\spectraledger.exe add income 2026-09-05 2500.00 salary "Salário mensal"
.\target\spectraledger.exe budget set 2026-09 groceries 300.00
.\target\spectraledger.exe list --month 2026-09
.\target\spectraledger.exe summary 2026-09
```

O valor recebido no terminal usa ponto decimal, por exemplo `25.90`, e aceita
uma ou duas casas decimais. Os relatórios exibem vírgula decimal. A categoria
aceita letras minúsculas sem acento, números e hífens. A descrição é um único
argumento de terminal, por isso deve ficar entre aspas quando tiver espaços.

## Comandos

```text
spectraledger init
spectraledger add <income|expense> <AAAA-MM-DD> <valor> <categoria> <descrição>
spectraledger list [--month AAAA-MM] [--kind income|expense] [--category nome]
spectraledger remove <id>
spectraledger summary [AAAA-MM]
spectraledger budget set <AAAA-MM> <categoria> <limite>
spectraledger budget list [AAAA-MM]
spectraledger self-test
```

- IDs de transações são positivos, crescentes e não são reutilizados após uma
  remoção.
- `list` combina período, tipo e categoria; sem filtros, mostra o ledger inteiro.
- `summary` pode receber um mês. Sem argumento, agrega todos os períodos e conta
  os limites mensais excedidos.
- `budget set` substitui o limite existente para a mesma combinação de mês e
  categoria; `budget list` mostra limite, gasto, restante e estado.
- Datas e meses são validados pelo calendário gregoriano, incluindo anos
  bissextos. Categorias usam o formato `alimentacao`, `transporte` ou
  `contas-casa`.
- Valores de entrada aceitam no máximo `999999999.99`; zero, negativos e mais de
  duas casas decimais são recusados.

## Persistência e integridade

O primeiro `init` cria `.spectraledger/ledger.db`. O cabeçalho guarda a versão do
formato e o próximo ID. Linhas `T` representam transações e linhas `B`
representam limites. Descrições escapam `%`, tabulações e quebras de linha antes
de serem separadas por tabulação.

Cada atualização é escrita em `ledger.db.tmp`, relida e comparada antes de
publicar o novo arquivo. O ledger anterior fica em `ledger.db.bak` durante a
troca. Na inicialização de uma operação, um backup é restaurado se o arquivo
principal estiver ausente; um banco com cabeçalho ou registros inválidos é
recusado sem ser regravado silenciosamente.

## Verificação

```powershell
.\target\debug\spectralang.exe fmt --check examples/complete/20-spectraledger
.\target\debug\spectralang.exe check --json examples/complete/20-spectraledger
.\target\debug\spectralang.exe run examples/complete/20-spectraledger -- self-test
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectraledger-dev.exe examples/complete/20-spectraledger
pwsh -NoProfile -File tests/spectraledger-integration.ps1 -Executable target/spectraledger-dev.exe
```

A integração executa o binário AOT em processos separados para conferir
gravação e leitura, filtros, totais, atualização de limites, IDs, recuperação de
backup e recusa de um arquivo corrompido. Ela também executa init, add e summary
via JIT em outro diretório temporário.

## Limites atuais

- O ledger é local e pressupõe um processo escritor por vez; não há sincronização
  remota nem bloqueio para escritores concorrentes.
- Não há transferências entre contas, recorrências ou conversão de moedas.
- A data representa um dia civil informado e não contém fuso horário.
