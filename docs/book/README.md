# The Spectra Book

This book is the production adoption path for SpectraLang as an AI/ML-oriented
language. It is intentionally tied to checked-in examples and validation
commands so users can follow the docs and reproduce results locally.

## Reading Path

1. [Language Basics](01-language-basics.md)
2. [Numerics](02-numerics.md)
3. [Tensors](03-tensors.md)
4. [Autodiff](04-autodiff.md)
5. [Model Authoring](05-model-authoring.md)
6. [Deployment And Export](06-deployment-export.md)
7. [Standard Library, Runtime, And Packages](07-stdlib-runtime-packages.md)
8. [Benchmarks And Comparisons](08-benchmarks-and-comparisons.md)
9. [Hello HTTP](09-hello-http.md)
10. [Middleware Chain](10-middleware-chain.md)
11. [Agents](11-agents.md)

## Verified AI Examples

The examples in `examples/ai/` are part of the book contract:

- `linear_regression_train_export.spectra`
- `logistic_regression_train_export.spectra`
- `mlp_training_serving.spectra`
- `cnn_image_classifier.spectra`
- `toy_transformer_inference.spectra`
- `data_preprocessing_pipeline.spectra`

Run all examples:

```powershell
cargo build -p spectra-cli
New-Item -ItemType Directory -Force -Path target\ai-examples | Out-Null
Get-ChildItem examples\ai -Filter *.spectra | Sort-Object Name | ForEach-Object {
  .\target\debug\spectralang.exe run $_.FullName
}
```

Run the full repository validation:

```powershell
.\run_tests.ps1
```

`run_tests.ps1` includes the AI examples as Phase 13 checks.

## Verified API Examples

The first API chapter is validated against a checked-in executable example:

- `examples/api/00_hello_http.spectra`
- `tests/validation/148_api_middleware_chain.spectra`

Run the Hello HTTP example:

```powershell
.\target\debug\spectralang.exe run examples\api\00_hello_http.spectra
.\target\debug\spectralang.exe run tests\validation\148_api_middleware_chain.spectra
```

## Verified Agent Examples

The agent chapters are validated against checked-in projects that run with the
deterministic mock provider (no credentials):

- `examples/agent/01-tool-and-run`
- `examples/agent/02-approval-and-budget`
- `examples/agent/04-durable-replay`

Run them in JIT and through AOT:

```powershell
.\target\debug\spectralang.exe run examples\agent\01-tool-and-run
.\target\debug\spectralang.exe compile --debug-info=none --emit-exe target\example-01-agent.exe examples\agent\01-tool-and-run
.\target\example-01-agent.exe
```

`run_tests.ps1` includes the Phase 32 agent validators, which exercise the same
projects in both execution modes.
