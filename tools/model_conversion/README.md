# NudeNet 320n to Core ML

This conversion-only toolchain downloads the official NudeNet `v3.4-weights`
`320n.pt`, verifies its committed SHA-256, and exports a 320×320, batch-one,
Float16 Core ML ML Program with non-maximum suppression enabled.

Run on macOS with Python 3.11 and Xcode installed:

```bash
python3.11 -m venv .venv
source .venv/bin/activate
python -m pip install --upgrade pip
python -m pip install -r tools/model_conversion/requirements.txt
tools/model_conversion/download_model.sh
python tools/model_conversion/convert_nudenet_320n.py
python tools/model_conversion/verify_model.py
```

The output is `AI-Image-Classifier/Models/NudeNet320n.mlpackage`. Normal iOS
builds consume the committed package and never install Python, PyTorch,
Ultralytics, ONNX Runtime, or coremltools.

The successful export environment was macOS 26.4, Xcode 26.6 (17F113), and
Python 3.11.9. Exact tested conversion pins are Ultralytics 8.4.95,
coremltools 9.0, PyTorch 2.13.0, torchvision 0.28.0, Pillow 12.3.0, and NumPy
2.3.5. The workflow prints the environment and installed versions on every run.

## License review

- The NudeNet repository and its `v3.4-weights` tag contain an AGPL-3.0 license,
  while NudeNet's PyPI metadata and `setup.py` declare MIT. The release does not
  provide an unambiguous separate weights license. Treat this discrepancy as a
  distribution blocker until the copyright holder or legal counsel clarifies it.
- Ultralytics is AGPL-3.0 and is used only by this conversion toolchain.
- coremltools is BSD-3-Clause; torchvision and Pillow are BSD-style; NumPy is
  BSD-3-Clause; PyTorch uses its bundled BSD-style license and notices.

This is a technical inventory, not legal advice.
