# Findings and Corrections

## ML training to serving tensor layout

The first end-to-end attempt exposed a real interoperability contract between
the two standard-library surfaces:

- `std.ml.linear` validates weights as `[in_features, out_features]`.
- `std.serve.server_register_named_model_linear` validates weights as
  `[out_features, in_features]`, matching its `x @ W^T + b` serving kernel.

The trained checkpoint was therefore valid for the ML training path but was
rejected by the serving registry when passed unchanged. The correction is now
implemented in `src/serving_registry.spectra`: the loaded `[2, 1]` checkpoint
tensor is reshaped to `[1, 2]` at the serving boundary before registration.
This preserves both existing library contracts and keeps the adapter explicit.

Evidence: the original registration failed with
`spectra.std.serve.server_register_named_model_linear`; the corrected adapter is
covered by the complete application's JIT and AOT runs.

No compiler defect was found in this investigation. The other observed failures
were application contract mistakes: tokenizer vocabulary capacity and the
vector-index requirement for `model_version` metadata. Both are now corrected
in the example and exercised by its validation flow.

## Cleanup ownership for generated sub-artifacts

The first cleanup audit found that the experiment host writes
`experiment/experiment-manifest.json` and the checkpoint host writes
`model/weights.spar`; removing only their parent directories is not enough
because `std.fs.remove_dir` intentionally handles empty directories only.
`src/cleanup.spectra` now removes both concrete files before their directories,
and JIT/AOT reruns confirm that the isolated application roots are absent after
success.
