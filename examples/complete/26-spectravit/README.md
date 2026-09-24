# SpectraViT

SpectraViT classifica glifos monocromáticos 8×8 com um Vision Transformer
compacto. O pipeline divide cada imagem em 16 patches 2×2, projeta os patches
para embeddings, acrescenta um token de classe e posições aprendidas, aplica
atenção self-attention de duas cabeças, LayerNorm, GELU e um MLP residual. O
treino usa mini-lotes balanceados de três imagens, cross-entropy, autodiff
reverse-mode e AdamW.

O próprio projeto gera 30 imagens de treino e 15 de holdout com três padrões
simples. Os dados são determinísticos e servem como fixture de execução; não
representam um benchmark nem medem generalização para imagens reais.

## Módulos

- `data.glyphs`: gera os lotes equilibrados para treino e holdout.
- `vision.patchify`: transforma cada imagem em uma matriz de 16 patches.
- `encoder.transformer`: projeta Q/K/V e combina duas cabeças de atenção.
- `model.vit`: token de classe, posições, bloco residual e classificador.
- `training.adamw` e `training.fit`: momentos AdamW e treino mini-batch.
- `evaluation.report`: acurácia e macro-F1 por classe.

## Executar

Na raiz do repositório:

```powershell
.\target\debug\spectralang.exe fmt --check examples/complete/26-spectravit
.\target\debug\spectralang.exe check --json examples/complete/26-spectravit
.\target\debug\spectralang.exe run examples/complete/26-spectravit
pwsh -NoProfile -File tests/spectravit-integration.ps1
```
