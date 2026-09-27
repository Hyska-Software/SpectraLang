# SpectraRelease

Projeto multifile de pipeline de release orientado por qualidade e telemetria.
Ele treina um calibrador residual com tensores e autodiff, avalia holdout e
drift, aplica políticas de segurança e executa etapas assíncronas por trait
dinâmico. Os dados são sintéticos e servem para exercitar a linguagem e a
pipeline; não representam um modelo de produção nem uma integração real com um
registry externo.

## O que o projeto demonstra

- módulos independentes para domínio, contrato, pipeline, estágios e ML;
- `Envelope<T>` e `advance<T>` genéricos exportados entre módulos;
- bounds de trait importados e casts para `dyn ReleaseStage`;
- três estágios assíncronos — build, segurança e publicação — com auditoria;
- calibrador MLP residual com GELU, LayerNorm e dropout;
- treino com autodiff e AdamW, minibatches, holdout e baseline zero;
- monitoramento de drift e rejeição de risco, com rollback do artefato;
- paridade JIT/AOT no projeto completo.

## Executar

Na raiz do repositório:

```powershell
.\target\debug\spectralang.exe run examples\complete\28-spectrarelease
```

A saída confirma três estágios concluídos, oito tensores de parâmetros
treináveis, 240 atualizações e melhoria sobre a baseline. Também exercita a
rejeição por risco, a rejeição por mudança de distribuição e o rollback.

## Verificação integrada

```powershell
cargo build -p spectra-cli
.\tests\spectrarelease-integration.ps1
```

O harness confere formatação, análise semântica, lint, execução JIT e compilação
e execução AOT. Ele compara a saída dos dois backends e valida os campos de
treino, holdout, drift, rejeição e rollback.
