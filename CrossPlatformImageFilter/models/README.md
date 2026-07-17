# Runtime model assets

Weights are excluded from Git. CI produces and verifies:

- `person_detector.onnx`: YOLOX Nano, decoded `[1,boxes,85]` output;
- `mobileclip2_s2_image_encoder.onnx`: normalized 512-dimensional encoder;
- `mobileclip2_s2_prompt_embeddings.npz`: `class_names` plus `[4,512]` embeddings;
- `nudenet320n.onnx`: NudeNet YOLOv8 output with the exact 18-label order;
- `runtime-manifest.json`: source and runtime hashes, byte sizes, tensor metadata,
  preprocessing, adapter identity, auxiliary artifacts, and license references.

`model-sources.json` is the pinned source-of-truth used to create the runtime
manifest. `coco_labels.json` and `nudenet_labels.json` are auxiliary artifacts
whose hashes are also recorded.

Do not copy Core ML packages here: they cannot provide the required Windows and
Linux runtime. Use the pinned acquisition/export workflow. Before distribution,
review every file under `licenses/`; NudeNet metadata says MIT while both license
files bundled in its wheel contain AGPL-3.0, and the YOLOX checkpoint does not
state terms separate from the Apache-2.0 source repository. This is not legal
advice.
