# SpectraBoard

SpectraBoard é um organizador local de tarefas com interface de terminal. O
aplicativo é escrito em módulos `.spectra` organizados por responsabilidade e
persiste cada operação em `.spectraboard/tasks.db`.

## Estrutura do projeto

```text
19-spectraboard/
├── spectra.toml
└── src/
    ├── main.spectra
    ├── cli/parser.spectra
    ├── domain/model.spectra
    ├── domain/codec.spectra
    ├── reports/output.spectra
    ├── services/tasks.spectra
    ├── storage/database.spectra
    └── validation/self_tests.spectra
```

Os nomes de módulo acompanham os diretórios (`domain.model`,
`storage.database`, `services.tasks`). O parser converte os argumentos em um
comando validado; o serviço aplica as regras de tarefas; o módulo de domínio
valida datas e serializa campos; a camada de armazenamento recupera gravações
interrompidas e substitui o banco por arquivo temporário; o módulo de relatório
formata a saída. Handles de `List<Task>` ficam dentro do módulo de serviço e
atravessam módulos somente registros escalares ou texto serializado.

## Compilar e executar

Na raiz do repositório SpectraLang:

```powershell
.\target\debug\spectralang.exe check --json examples/complete/19-spectraboard
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectraboard.exe examples/complete/19-spectraboard
```

Execute o binário no diretório em que deseja manter o quadro:

```powershell
.\target\spectraboard.exe init
.\target\spectraboard.exe add --title "Revisar imports aninhados" --priority high --due 2026-10-01 --tag compiler
.\target\spectraboard.exe list --status open
.\target\spectraboard.exe complete 1
.\target\spectraboard.exe summary
```

No Windows, o shell passa cada texto entre aspas como um argumento; o título
pode conter espaços e caracteres UTF-8.

## Comandos

```text
spectraboard init
spectraboard add --title <texto> [--priority low|normal|high] [--due AAAA-MM-DD] [--tag nome]
spectraboard list [--status all|open|done] [--priority low|normal|high] [--tag nome]
spectraboard show <id>
spectraboard edit <id> [--title <texto>] [--priority low|normal|high] [--due AAAA-MM-DD|none] [--tag nome|none]
spectraboard complete <id>
spectraboard reopen <id>
spectraboard remove <id>
spectraboard summary
spectraboard self-test
```

- IDs são positivos, monotônicos e não são reutilizados após remover tarefas.
- Prioridades aceitas: `low`, `normal` e `high`.
- Prazos opcionais usam `AAAA-MM-DD`; o modelo valida mês, quantidade de dias
  e anos bissextos.
- `edit --due none` e `edit --tag none` removem prazo e tag.
- `list` filtra por estado, prioridade e tag. O resultado mantém a ordem dos IDs.
- `complete` e `reopen` são idempotentes quando a tarefa já está no estado
  solicitado.
- `remove` exclui somente o registro indicado; IDs já usados permanecem
  reservados pelo contador persistido.

## Persistência e integridade

O banco é texto UTF-8 versionado. O cabeçalho armazena a versão do formato e o
próximo ID; cada linha restante contém ID, estado, prioridade, prazo, tag,
instantes de criação/atualização e título. Campos de texto escapam `%`, tabs,
quebras de linha e retorno de carro antes de serem separados por tabulações.
Ao carregar, o programa valida todas as linhas, a ordem dos IDs, as datas, os
estados e o contador antes de aceitar os dados.

Uma gravação é preparada em `tasks.db.tmp`, relida e comparada com a entrada. O
banco anterior passa para `tasks.db.bak` antes de publicar o arquivo novo. Se o
processo parar nessa janela, a próxima execução restaura o backup quando o banco
principal estiver ausente; se o banco novo já estiver presente, ele é mantido e
os temporários antigos são descartados.

## Verificação

```powershell
.\target\debug\spectralang.exe fmt --check examples/complete/19-spectraboard
.\target\debug\spectralang.exe check --json examples/complete/19-spectraboard
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectraboard-dev.exe examples/complete/19-spectraboard
spectralang run tests/validation/619_stdlib_utf8_program_arguments.spectra -- "título Ω — café" next-argument
pwsh -NoProfile -File tests/spectraboard-integration.ps1
```

O teste de integração chama o executável em processos separados, exercitando
persistência entre execuções, filtros, edição, conclusão, reabertura, remoção,
validação de dados, recuperação de um backup de gravação e argumento UTF-8 em
uma chamada AOT.
O problema de codificação que motivou essa cobertura está descrito em
[windows-utf8-command-line-arguments.md](../../../docs/runtime/windows-utf8-command-line-arguments.md).

## Limites atuais

- O banco é local, texto UTF-8 e adequado a um único processo escritor por vez;
  não há sincronização remota nem bloqueio entre escritores concorrentes.
- Datas representam apenas o dia civil informado e não carregam fuso horário.
- `remove` é permanente; não há lixeira ou trilha de auditoria de alterações.
- O formato tem versão `1`; migrações entre versões ainda não fazem parte do
  exemplo.
