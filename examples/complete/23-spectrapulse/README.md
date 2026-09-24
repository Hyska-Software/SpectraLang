# SpectraPulse

SpectraPulse prevê a próxima leitura de quatro sensores a partir de uma janela
de quatro instantes. O fluxo passa por um codificador temporal de duas cabeças
de atenção, LayerNorm, GELU e conexão residual, seguido por um MLP residual
treinado com autodiff, MSE e AdamW.

O dataset contém 24 janelas sintéticas geradas pelo próprio programa. Ele serve
para exercitar o pipeline de ponta a ponta e não representa telemetria real,
qualidade de previsão em produção ou benchmark de desempenho. A divisão
determinística usa 18 exemplos para treino e 6 para teste.

## Estrutura

O projeto organiza a implementação nestes módulos:

- app: coordena preparação, treino e avaliação
- data.synthetic: gera janelas multivariadas e alvos do próximo passo
- encoder.temporal_attention: projeta Q/K/V e calcula dois heads treináveis com LayerNorm, GELU e residual
- model.forecaster: MLP residual de oito para doze unidades e quatro saídas
- model.optimizer: estado e atualizações AdamW dos 14 parâmetros
- training.fit: minibatches, autodiff e agenda de learning rate
- evaluation.evaluate: MSE, MAE e RMSE no holdout

O codificador divide os canais em dois grupos e aplica projeções Q/K/V treináveis
em cada cabeça. Cada uma resume uma consulta no instante mais recente contra as
quatro chaves/valores da janela. A autodiff atravessa projeções, atenção,
LayerNorm, GELU, conexão residual e MLP para prever os quatro sensores no passo
seguinte.

## Correção do runtime nesta rodada

O caminho inicial fazia ml.attention, ml.layer_norm e ml.gelu apenas no
forward; esses resultados não tinham nós no grafo de autodiff. O runtime agora
registra os criadores dessas operações e propaga gradientes para Q, K, V,
entrada, escala e bias de LayerNorm, além da derivada da GELU aproximada por
tanh. O bloco `diff` também falhava com E3004 porque a análise semântica, o
midend, o backend e o fast path do runtime não registravam as três regras
reversas; esses pontos agora concordam sobre os códigos das operações. A
comparação por diferenças finitas revelou ainda que o gradiente da entrada de
LayerNorm não ponderava o gradiente recebido por `gamma` nas reduções; essa
derivada foi corrigida.

Uma regressão do runtime compara os gradientes compostos com diferenças
finitas. O projeto exercita o mesmo caminho em JIT e AOT, com a atenção dentro
do grafo de treinamento. O teste de integração exige MSE abaixo de 0.01 no
fixture sintético.

## Executar

Na raiz do repositório, rode:

    .\target\debug\spectralang.exe fmt --check examples/complete/23-spectrapulse
    .\target\debug\spectralang.exe check --json examples/complete/23-spectrapulse
    .\target\debug\spectralang.exe run examples/complete/23-spectrapulse
    .\target\debug\spectralang.exe compile --debug-info=none --emit-exe target/spectrapulse.exe examples/complete/23-spectrapulse
    .\target\spectrapulse.exe

O programa encerra com erro se o conjunto de teste exceder MSE 0.01.
