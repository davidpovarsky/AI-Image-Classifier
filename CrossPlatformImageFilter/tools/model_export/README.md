# Desktop model export

These scripts are deliberately separate from the existing Core ML conversion.
They should be wired into a new desktop-model CI workflow after Codex verifies
the current branch.

## MobileCLIP2-S2

Install export dependencies in a disposable environment and install Apple's
official `ml-mobileclip` repository plus the MobileCLIP2 OpenCLIP patch exactly
as required by the upstream project. Then run:

```bash
python export_mobileclip2_onnx.py \
  --checkpoint /path/to/mobileclip2_s2.pt \
  --output ../../models/mobileclip2_s2_image_encoder.onnx \
  --prompts-output ../../models/mobileclip2_s2_prompt_embeddings.npz
```

The exporter uses the same prompts, 256x256 input, 1/255 scaling, no external
mean/std normalization, reparameterization, and normalized embeddings as the
existing iOS conversion.

## NudeNet and person detector

Do not convert a compiled Core ML package back to ONNX. Obtain the original,
licensed ONNX model or reproduce it from its original source. Record its source,
license, hash, input/output names, opset, and label order in
`models/runtime-manifest.json`.
