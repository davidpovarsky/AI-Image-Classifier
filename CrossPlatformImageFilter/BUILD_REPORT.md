# Build report

This file records pre-commit Windows verification. GitHub Actions results and
artifact hashes are reported by the workflow artifacts and final task report.

- Package: `local-ai-image-filter==0.1.0`
- Python: 3.12.5 x64
- mitmproxy: 12.2.3
- ONNX Runtime DirectML: 1.24.4
- Available Windows providers: DirectML, CPU
- Ruff: passed
- Ruff formatting: passed
- Mypy: passed for 37 source files
- Pytest: 54 passed in 29.79 seconds, including a real `mitmdump` subprocess
- PowerShell scripts: parser passed; mock start/status/stop passed
- macOS/Linux shell scripts: `bash -n` passed with Git Bash
- Source distribution and wheel: built successfully
- Clean wheel install: passed
- Packaged default configuration: loaded from the installed wheel
- Wheel smoke: `print-config`, mock `doctor`, and synthetic classify passed
- Source manifest: generated and verified
- MobileCLIP/iOS contract check: passed
- Swift/desktop response field check: passed

Pinned NudeNet 3.4.2 was acquired and its wheel/model SHA-256 hashes verified.
On Windows it loaded with provider order DirectML then CPU. A measured local run
reported approximately 11,966 ms model load, 4,392 ms warm-up, and 92 ms warm
inference on a synthetic safe image. These figures describe this machine only.

The complete three-model export, ten-case MobileCLIP parity test, real pipeline
smoke/benchmarks, cross-platform test matrix, and platform bundles remain
pending until the separate desktop GitHub Actions workflow runs. The existing
iOS workflow also remains pending at this pre-commit stage.
