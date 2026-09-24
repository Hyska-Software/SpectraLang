# SpectraDiffuse

SpectraDiffuse trains a small denoising diffusion model to generate points from
a four-mode 2D Gaussian mixture. It uses a linear beta schedule, sinusoidal
timestep features, a residual MLP, reverse-mode autodiff, and AdamW. The sampler
runs the reverse DDPM update from Gaussian noise back to the data space.

The train and holdout points are generated locally with fixed tensor seeds.
They are small synthetic fixtures for validating the complete training and
sampling path; they are not a benchmark or evidence of image-generation quality.

## Modules

- `data.mixture`: builds balanced train and holdout point clouds.
- `diffusion.schedule`: defines the beta schedule and forward noising process.
- `model.denoiser`: embeds the timestep and predicts noise with a residual MLP.
- `training.adamw` and `training.fit`: update model parameters and track a fixed
  denoising loss.
- `sampling.ddpm`: generates points with the reverse diffusion process.
- `evaluation.report`: measures holdout denoising error and synthetic mode
  coverage.

## Run

From the repository root:

```powershell
.\target\debug\spectralang.exe fmt --check examples/complete/27-spectradiffuse
.\target\debug\spectralang.exe check --json examples/complete/27-spectradiffuse
.\target\debug\spectralang.exe run examples/complete/27-spectradiffuse
pwsh -NoProfile -File tests/spectradiffuse-integration.ps1
```
