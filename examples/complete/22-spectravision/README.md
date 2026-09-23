# SpectraVision

SpectraVision treina uma CNN pequena para classificar imagens monocromáticas
6×6 como linhas horizontais ou verticais. O projeto passa por ingestão CSV,
separação treino/teste, minibatches, convolução 2D, ReLU, max pooling, duas
camadas lineares, dropout, autodiff, AdamW, métricas de classificação e
checkpoint com round-trip.

O dataset `data/orientation.csv` é um fixture sintético determinístico para
ensinar e testar o pipeline. Ele tem 12 imagens de treino e 4 imagens de teste;
não representa medições reais nem serve como benchmark de qualidade ou
velocidade.

## Estrutura

```text
22-spectravision/
├── data/orientation.csv
├── spectra.toml
└── src/
    ├── app.spectra
    ├── data/ingest.spectra
    ├── evaluation/evaluate.spectra
    ├── vision/checkpoint.spectra
    ├── vision/network.spectra
    ├── vision/optimizer.spectra
    ├── training/fit.spectra
    └── main.spectra
```

`vision.network` define a topologia e o forward pass: quatro filtros 3×3,
ativação ReLU, pooling hierárquico 2×2, camada oculta de oito unidades com
dropout e uma camada de logits para as duas classes. `training.fit` percorre os
minibatches, aplica cross-entropy e atualiza os seis parâmetros com AdamW e
schedule exponencial. `evaluation.evaluate` calcula previsões e o relatório de
classificação; `vision.checkpoint` salva, valida, recarrega e reutiliza todos
os pesos.

## Executar

Na raiz do repositório:

```powershell
.\target\debug\spectralang.exe check --json examples/complete/22-spectravision
.\target\debug\spectralang.exe run examples/complete/22-spectravision
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectravision.exe examples/complete/22-spectravision
.\target\spectravision.exe
```

O programa usa CPU e uma seed fixa para repetir inicialização e ordem dos
minibatches. Ele termina com erro se a acurácia das quatro imagens de teste
ficar abaixo do resultado esperado ou se o checkpoint recarregado alterar as
previsões. O artefato é gravado em
`target/complete-spectravision/model.spar`.

## Falha encontrada e corrigida nesta rodada

O teste integrado reproduziu uma falha no contrato entre dados e classificação:
`dataset_from_csv`, splits e dataloaders entregavam rótulos numéricos como
`Tensor<Float>`, enquanto `cross_entropy_loss` aceitava apenas
`Tensor<Int>`. Assim, um treino multiclasse não conseguia consumir os próprios
minibatches do loader. O runtime agora aceita rótulos `Tensor<Int>` ou floats
finitos com valor inteiro; floats fracionários, NaN, infinitos e índices fora
da faixa continuam sendo recusados. A regressão de runtime e este projeto
exercitam esse caminho.

## Verificação

```powershell
.\target\debug\spectralang.exe fmt --check examples/complete/22-spectravision
pwsh -NoProfile -File tests/spectravision-integration.ps1
cargo test -p spectra-runtime ml_cross_entropy_accepts_integral_float_class_labels
```

O script compila uma vez e executa a mesma aplicação em JIT e AOT, incluindo
treino, métricas de teste e round-trip do checkpoint.
