# SpectraLang Language Bug Hunt

**Intent:** ampliar a cobertura executável da linguagem com fixtures `.spectra` determinísticos, priorizando OOP e as fronteiras entre parser, semântica, lowering, backend, runtime, módulos e AOT.

**Current Behavior:** a branch `fix/oop-layout-and-diagnostics` possui os gates R-207 a R-213 aprovados e uma suíte interna limpa, mas os fixtures recentes exercitam principalmente caminhos felizes isolados.

**Expected Outcome:** cada comportamento novo será classificado como aprovado, bug reproduzível, recurso não suportado ou lacuna do harness; bugs confirmados terão correção de produção, regressão executável e evidência de validação.

**Target-Perspective Output:** comandos reais de `target/debug/spectralang.exe` (`check`, `compile` e `run`) com saída, código de retorno e diagnóstico observáveis.

**Truth Owner:** o pipeline Rust do compilador e o CLI; os arquivos `.spectra` são entradas comportamentais versionadas, não testes de detalhes internos.

**Contract Boundary:** programa `.spectra` -> CLI -> diagnóstico, IR/artefato ou resultado de execução.

**Cutover:** novos candidatos começam em `tests/regressions/pending/`; casos aprovados migram para `tests/validation/` ou `tests/errors/` somente após validação.

**Displaced Path:** não duplicar um reproducer em `tests/validation/`, `tests/errors/` e `tests/regressions/pending/`; o caso pendente é a fonte durante a investigação.

**Acceptance Evidence:** execução JIT, compilação, diagnóstico JSON, repetição de casos de lifetime e emissão AOT quando aplicável.

**Evidence Lane:** `target/debug/spectralang.exe` é o binário local autoritativo; a execução parte da raiz do repositório.

**Kill Criteria:** interromper um caso se ele depender de sintaxe explicitamente ausente, produzir apenas falha intermitente sem reprodução estável ou exigir alteração de produção para ser observável.

**Non-goals:** novos snapshots ou fuzz targets fora dos casos reproduzíveis desta matriz; a correção Rust, o validator reproduzível e a sincronização do roadmap passaram a fazer parte do escopo explícito de implementação aprovado após a triagem.

**Plan Review Gate:** manter a execução em fatias verticais; revisar cada lote antes de integrar o próximo.

## Aditivo de implementação aprovado

1. Corrigir ABI/layout, fat pointers, ownership/drop e dispatch OOP no pipeline
   completo, preservando o CLI como contrato.
2. Corrigir especialização genérica, UFCS, vtables herdadas e registro
   intermodular de traits/templates.
3. Corrigir o contrato `Task<Response>` de `AsyncHandler` dinâmico.
4. Transformar a aceitação indevida de `impl módulo::Tipo` inexistente em
   diagnóstico semântico estável `E027`.
5. Promover fixtures verdes para `tests/validation/`/`tests/errors/`, adicionar
   `scripts/validate_language_bug_hunt.py`, atualizar evidências e sincronizar
   `roadmap.toml` com `docs/roadmap-backlog.md`.

## Lotes

1. OOP: layout, drop, fat pointers, vtables, UFCS e generics.
2. Projeto multifile com impl qualificado e dispatch entre módulos.
3. Composição com closures, padrões, async, stdlib, tensor, API e tipos numéricos.
4. Classificação, promoção/quarentena e relatório final.

## Regras de classificação

- `PASS`: o caso produz exatamente o comportamento esperado.
- `BUG`: há crash, timeout, resultado incorreto ou diagnóstico ausente/incorreto reproduzível.
- `UNSUPPORTED_EXPECTED`: a forma é ausente/diferida pela linguagem atual.
- `HARNESS_GAP`: o comportamento é observável, mas o runner padrão não o cobre.
- `NONDETERMINISTIC`: a falha não é estável após três tentativas.
