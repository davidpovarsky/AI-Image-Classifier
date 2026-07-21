# Legal model review

No commercial redistribution approval is represented in this repository. NudeNet 3.4.2 metadata says MIT while bundled license files contain AGPL-3.0; YOLOX source is Apache-2.0 but the checkpoint lacks a clear separate redistribution statement; MobileCLIP2 uses Apple model terms requiring formal review.

Counsel must produce a real, signed document matching `legal/model-distribution-approval.schema.json`, including exact runtime hashes and obligations. The example is deliberately unapproved and cannot pass the release gate. If a model is rejected, preserve its adapter boundary, tensor/evidence contract, class ordering, preprocessing, and regression fixtures while substituting an approved model.
