# 5. Model Authoring

Spectra model authoring uses `std.tensor` for data and parameters and `std.ml`
for layers, losses, optimizers, datasets, and dataloaders. The current
production path is explicit and handle-based: tensors are runtime handles, model
modules are runtime handles, and examples validate behavior through the CLI.

## Linear Model

```spectra
import std.tensor as tensor
import std.ml as ml

func train_step(x: int, target: int, weight: int, bias: int) returns int {
    let prediction = ml.linear(x, weight, bias)
    let loss = ml.mse_loss(prediction, target)
    tensor.backward(loss)
    ml.sgd_step(weight, 0.1)
    ml.sgd_step(bias, 0.1)
    return 0
}
```

Run the complete checked-in version:

```powershell
.\target\debug\spectralang.exe run examples\ai\linear_regression_train_export.spectra
```

That example trains a toy linear regression model and writes
`target/ai-examples/linear_regression_model.txt`.

## Classification

Use binary cross entropy for logistic-style examples:

```spectra
let probs = tensor.requires_grad(tensor.full_f(4, 0.5), true)
let target = tensor.full_f(4, 1.0)
let loss = ml.bce_loss(probs, target)
tensor.backward(loss)
ml.sgd_step(probs, 0.1)
```

Validated example:

```powershell
.\target\debug\spectralang.exe run examples\ai\logistic_regression_train_export.spectra
```

For multiclass classification, `cross_entropy_loss` consumes a rank-2 logits
tensor and one class index per row. Labels can be integer tensors or numeric
float tensors whose values are finite whole numbers; CSV dataset readers return
numeric labels as floats, so this lets their dataloader batches feed the loss
directly. Fractional, non-finite, negative, or out-of-range class indices are
rejected.

## Modules And Layers

`std.ml` module handles allow examples to express a model boundary while keeping
the current compiler/runtime contract explicit.

```spectra
let model_handle = ml.module_new()
let weights = tensor.requires_grad(tensor.reshape(tensor.full_f(8, 0.1), 4, 2), true)
let bias = tensor.requires_grad(tensor.full_f(2, 0.0), true)
ml.module_add_parameter(model_handle, weights)
ml.module_add_parameter(model_handle, bias)
let logits = ml.linear(tensor.full2_f(3, 4, 1.0), weights, bias)
```

Validated example:

```powershell
.\target\debug\spectralang.exe run examples\ai\mlp_training_serving.spectra
```

## Datasets And Dataloaders

Use tensor-backed datasets for reproducible AI examples:

```spectra
let features = tensor.reshape(tensor.full_f(4, 1.0), 4, 1)
let labels = tensor.full_f(4, 2.0)
let dataset = ml.dataset_from_tensors(features, labels, 4)
let loader = ml.dataloader_new(dataset, 2, 7)
let batch_features = ml.dataloader_batch_features(loader, 0)
let batch_labels = ml.dataloader_batch_labels(loader, 0)
```

The third dataloader argument is a deterministic shuffle seed; `0` preserves
row order.

## Convolutional Example

[`examples/complete/22-spectravision`](../../examples/complete/22-spectravision/README.md)
is an executable CPU example that composes `std.ml.conv2d`, ReLU, max pooling,
linear layers, dropout, cross-entropy, autodiff and AdamW, then evaluates and
reloads a checkpoint in both JIT and AOT execution.

## Production Rules For Examples

- Call `tensor.free_all()` before and after long-running examples.
- Set deterministic seeds or use deterministic constants.
- Write export artifacts under `target/ai-examples/`.
- Keep examples executable with `spectralang run`, not pseudo-code.
