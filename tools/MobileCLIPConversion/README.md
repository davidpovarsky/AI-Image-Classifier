# MobileCLIP2-S2 Core ML conversion

No official MobileCLIP2-S2 Core ML package was present when this pipeline was
written. Apple's `apple/coreml-mobileclip` repository contains v1 encoders; the
v2 source of truth is `apple/MobileCLIP2-S2/mobileclip2_s2.pt`.

The GitHub Actions workflow is the supported environment: macOS, Python 3.11,
coremltools 9.0, PyTorch 2.8.0, Apple's ml-mobileclip commit
`aecfb5453d022e9deff12f81a150ea8f35194baa`, and OpenCLIP commit
`54c9754a94baacac5b2d9b1c76318078d48912af`. The latter is pinned because
Apple's official inference patch applies to that revision and not current
OpenCLIP `main`.

The converter uses Apple's documented `image_mean=(0,0,0)` and
`image_std=(1,1,1)` for MobileCLIP2-S2, switches to evaluation mode, folds the
reparameterizable branches, exports a Float16/iOS 17 image ML Program, and
generates category embeddings directly in PyTorch from every configured prompt.
The text encoder is never saved or bundled. No quantization is performed. The
verifier runs both frameworks on ten deterministic images and rejects cosine,
score, finiteness, or top-category drift outside the acceptance thresholds.

CI also converts trace/Float16/iOS 18 and trace/Float32/iOS 17 diagnostic
variants. It attempts torch.export/Float16/iOS 17 and records an explicit
unsupported result when the pinned toolchain cannot convert that path.

`model_manifest.json` is a source template. `download_model.sh` writes the
actual checkpoint SHA-256 to the repository-root `model_manifest.json` used by
the build artifact. Generated `.mlpackage` files and checkpoints belong in
`.model-cache` or the workflow workspace and should not be committed.

The model is governed by Apple's research-only terms in `MODEL_LICENSE.md`.
