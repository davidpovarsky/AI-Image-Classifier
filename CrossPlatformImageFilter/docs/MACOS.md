# macOS

Use Python 3.12. The standard `onnxruntime==1.27.0` package is pinned; the
runtime reports the provider actually selected and falls back to CPU when an
accelerated provider is unavailable.

## Enable

```bash
local-image-filter doctor --config config/default.toml --load-models
local-image-filter run --config config/default.toml --mode regular
./scripts/macos/enable-system-proxy.sh
./scripts/macos/trust-mitm-ca.sh
```

The proxy script identifies the active network service and saves its web,
secure-web, and automatic-proxy state before changing it. The CA helper uses the
login keychain by default and records the exact SHA-256 fingerprint.

## Restore and uninstall

```bash
./scripts/macos/disable-system-proxy.sh
./scripts/macos/remove-mitm-ca.sh
```

The disable script restores the saved network-service state. The removal script
targets only the recorded certificate. It does not modify the System keychain.

Local capture is experimental and may need OS permissions. Certificate pinning,
cached responses, and local canvas/blob content remain coverage gaps.
