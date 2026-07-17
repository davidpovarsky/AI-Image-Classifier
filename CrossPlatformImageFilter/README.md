# CrossPlatformImageFilter

`CrossPlatformImageFilter` is a self-contained desktop image-response filter for
Windows, macOS, and Linux. It is isolated from the Swift/Xcode application and
does not change the iOS pipeline.

The runtime is local: mitmproxy intercepts eligible HTTP(S) image responses and
ONNX Runtime executes the person, MobileCLIP2-S2, and NudeNet models in the same
process. There is no cloud classifier, telemetry service, or model server.

## What it filters

The filter inspects network image responses only. It does not inspect page text,
video, audio, DNS, website categories, or content created locally in a canvas or
`blob:` URL. Browser-cached content may not cross the proxy again. Certificate-
pinned applications can bypass or reject interception. Animated images are
replaced by default; this is controlled by `processing.animated_image_mode`.

Visual classification is probabilistic. False positives and false negatives are
expected, and no result should be treated as a statement about gender identity
or biological sex.

## Install from source

Python 3.12 is the supported development runtime.

```bash
cd CrossPlatformImageFilter
python -m venv .venv
# Windows PowerShell: .venv\Scripts\Activate.ps1
# macOS/Linux: source .venv/bin/activate
python -m pip install --upgrade pip
python -m pip install -e ".[dev]"
local-image-filter doctor --config config/mock.toml
```

Release artifacts contain a wheel, platform scripts, model files, and an
`offline-wheels/` directory. Install from an extracted artifact without the
network with:

```bash
python -m pip install --no-index --find-links offline-wheels local_ai_image_filter-*.whl
```

## Models

Model weights are intentionally absent from Git. Production requires:

```text
models/person_detector.onnx
models/mobileclip2_s2_image_encoder.onnx
models/mobileclip2_s2_prompt_embeddings.npz
models/nudenet320n.onnx
models/runtime-manifest.json
```

Use the model artifact from the desktop workflow or the pinned tools described
in [Model contract](docs/MODELS.md). `local-image-filter models verify` checks
file sizes, SHA-256 hashes, safe paths, and auxiliary artifacts before loading.

## Regular proxy mode

Regular mode is the stable default and listens only on `127.0.0.1:8080`.

```bash
local-image-filter run --config config/default.toml --mode regular
```

In another terminal, enable the OS proxy using the platform guide. Browse to
`http://mitm.it` through the running proxy or use the CA helper to trust the
current-user certificate. CA trust lets the local process decrypt proxied HTTPS
traffic; protect its private key and remove trust during uninstall.

- [Windows](docs/WINDOWS.md)
- [macOS](docs/MACOS.md)
- [Linux](docs/LINUX.md)
- [Proxy and certificate lifecycle](docs/PROXY_LIFECYCLE.md)

## Experimental local capture

```bash
local-image-filter run --config config/local-capture.toml --mode local
local-image-filter run --config config/local-capture.toml --mode local:chrome
```

Local capture has different OS, process-selection, and privilege constraints
and is not equivalent across platforms. Start with regular mode.

## Commands

```text
local-image-filter run [--mode regular|local|local:<process>]
local-image-filter doctor [--load-models]
local-image-filter classify IMAGE [--output-image PATH] [--report PATH]
local-image-filter print-config
local-image-filter cache stats|clear
local-image-filter models inspect|verify
local-image-filter diagnostics summary
```

Configuration precedence is `--config`, then `LOCAL_IMAGE_FILTER_CONFIG`, then
the default TOML embedded in the wheel. Relative paths resolve against
`LOCAL_IMAGE_FILTER_HOME` when set, otherwise the external config directory or
current directory.

## Policy, cache, and diagnostics

- [Policy](docs/POLICY.md) explains thresholds and failure behavior.
- `policy-balanced.toml` and `policy-strict.toml` are configuration overlays.
- [Diagnostics schema](docs/DIAGNOSTICS_SCHEMA.md) documents privacy-preserving
  JSONL events and atomic summaries.
- `cache stats` reports memory and SQLite state; `cache clear` removes only this
  filter's entries.
- Cache namespaces include the policy, model manifest, and processing settings,
  preventing stale decisions after configuration or model changes.

## Uninstall

1. Stop the filter.
2. Restore the exact saved proxy state with the platform disable script.
3. Remove only the CA fingerprint recorded by the trust script.
4. Remove the virtual environment or extracted artifact if desired.
5. Optionally remove `~/.local-ai-image-filter/` after saving needed reports.

Do not leave the OS pointing at `127.0.0.1:8080` after stopping the process.

## Development and verification

```bash
python -m compileall -q src tools
python -m ruff check .
python -m ruff format --check .
python -m mypy src
python -m pytest
python -m build
```

Real model export, MobileCLIP parity, provider reports, smoke/benchmark tests,
and platform bundles run in `.github/workflows/build-desktop-image-filter.yml`.
See [Architecture](docs/ARCHITECTURE.md), [Security and limitations](docs/SECURITY_AND_LIMITATIONS.md),
and [upstream integration](docs/INTEGRATION_WITH_EXISTING_REPO.md).
