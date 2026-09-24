# SpectraGraph

SpectraGraph treina uma rede convolucional de grafos (GCN) para classificar nós
em duas comunidades. A topologia e os atributos são tensores; cada camada
projeta atributos com `ml.linear` e propaga mensagens pela matriz de adjacência
normalizada por linha. A rede adiciona LayerNorm e GELU, e aprende por
cross-entropy, autodiff reverse-mode e AdamW.

O treino usa um grafo sintético e a avaliação usa outro grafo com a mesma
topologia e variações determinísticas nos atributos. O programa exige redução
da loss de treino e classificação correta de todos os oito nós do holdout. Esse
fixture demonstra o fluxo de execução e não mede generalização em dados reais.

## Módulos

- `graph.topology`: constrói a adjacência com auto-conexões e normalização
  `D^-1(A + I)`.
- `data.graph_sample`: materializa grafos, atributos e rótulos em tensores.
- `model.gcn`: duas propagações de mensagem com LayerNorm e GELU.
- `training.adamw` e `training.fit`: estados do otimizador e treino full-batch.
- `evaluation.metrics`: predições e métricas de classificação no holdout.

## Executar

Na raiz do repositório:

```powershell
.\target\debug\spectralang.exe fmt --check examples/complete/25-spectragraph
.\target\debug\spectralang.exe check --json examples/complete/25-spectragraph
.\target\debug\spectralang.exe run examples/complete/25-spectragraph
pwsh -NoProfile -File tests/spectragraph-integration.ps1
```
