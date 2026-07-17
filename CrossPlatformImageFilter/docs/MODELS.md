# Model contract and supply chain

Model weights are build artifacts, never Git source. `models/model-sources.json`
pins each URL, release/revision, source hash, and licensing warning.
`runtime-manifest.json` records runtime files, sizes, SHA-256 hashes, tensor
contracts, color layouts, and auxiliary artifacts. The runtime rejects missing,
unexpected, path-traversing, or hash-mismatched files before inference.

## MobileCLIP2-S2

- Source: `apple/MobileCLIP2-S2`
- Revision: `72424e7025436db18f15c3eff6ee8c7c15ad4481`
- Checkpoint SHA-256: `37c2d839a856491f2fcc82c40dc28672dbd0907235b4cd4c38dfff6457f0c09f`
- ml-mobileclip: `aecfb5453d022e9deff12f81a150ea8f35194baa`
- open_clip: `54c9754a94baacac5b2d9b1c76318078d48912af`

The exporter derives classes and prompt text from the pinned iOS converter. It
exports static-batch `encode_image(..., normalize=True)` at opset 18. Input is
float32 RGB NCHW `[1,3,256,256]`, scaled by `1/255`, zero mean, unit standard
deviation. Output is an L2-normalized `[1,512]` embedding.

The NPZ stores `class_names` and normalized `[4,512]` prompt embeddings for
`woman`, `man`, `uncertain`, and `notPerson`. Parity uses ten deterministic
inputs and checks embedding cosine similarity, logits, ranks, and top class.

## NudeNet320n

The reproducible source is the `nudenet==3.4.2` wheel. Its wheel hash and the
bundled `320n.onnx` hash are pinned. The dedicated adapter owns the YOLOv8
channels-first contract, RGB preprocessing, square padding, coordinate mapping,
finite checks, and per-class NMS. It preserves the exact 18-label order in
`nudenet_labels.json`.

The package metadata and files contain conflicting MIT/AGPL licensing signals;
the workflow preserves both license files. Review distribution rights before
publishing the model. This is not legal advice.

## YOLOX Nano

The checkpoint is the official `0.1.1rc0` release asset and export uses source
commit `419778480ab6ec0590e5d3831b3afb3b46ab2aa3`. Source code is Apache-2.0;
the checkpoint states no separate terms. The dedicated adapter expects decoded
`[1,boxes,85]` cx/cy/width/height, objectness, and COCO class scores. It uses
BGR float32 values in the original 0-255 range, top-left placement in a
416-square canvas filled with 114,
then filters COCO class 0 and applies NMS.

## Commands

```bash
python tools/model_download/acquire_nudenet.py --output models/nudenet320n.onnx --licenses models/licenses
python tools/model_download/acquire_yolox.py --output .model-cache/yolox/yolox_nano.pth
local-image-filter models inspect --config config/default.toml
local-image-filter models verify --config config/default.toml
```

MobileCLIP and YOLOX export require pinned source checkouts; CI is the reference
procedure. Do not redistribute weights until all applicable terms are reviewed.
