# SpectraRoute

SpectraRoute planeja caminhos de menor duração em uma rede de transporte. O
grafo guarda conexões dirigidas, duração em minutos e estado aberto/fechado em
um tensor inteiro. O módulo `planner.dijkstra` executa Dijkstra, mantém vetor de
distâncias e predecessores e reconstrói a rota; `network.topology` monta a rede
e atualiza fechamentos; `report.format` transforma o caminho em uma saída
legível.

O exemplo valida quatro situações na rede determinística: uma origem inválida,
a menor rota inicial (16 min), o redirecionamento após fechar uma conexão
bidirecional (18 min) e a resposta para um aeroporto isolado. É um fixture local
para exercitar o planejador e não representa dados ou horários reais de
transporte.

## Regressão de tipo importado

O registro do projeto chama-se `Route`, o mesmo nome curto de um handle opaco de
`std.api.routing`. O lowering preserva o layout concreto do registro do projeto
ao importar o tipo e seu retorno entre módulos. O script de integração exercita
o caso em `check`, JIT e AOT.

## Executar

Na raiz do repositório:

```powershell
.\target\debug\spectralang.exe fmt --check examples/complete/24-spectraroute
.\target\debug\spectralang.exe check --json examples/complete/24-spectraroute
.\target\debug\spectralang.exe run examples/complete/24-spectraroute
pwsh -NoProfile -File tests/spectraroute-integration.ps1
```
