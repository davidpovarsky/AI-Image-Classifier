# Windows

Use Python 3.12 on x64 Windows. The pinned runtime dependency is
`onnxruntime-directml==1.24.4`; it exposes DirectML and CPU providers. The
runtime selects the first available configured provider, always keeps CPU as a
fallback, and serializes DirectML `Run` calls with memory patterns disabled.

## Start and enable

From an activated environment or extracted artifact:

```powershell
local-image-filter doctor --config config/default.toml --load-models
.\scripts\windows\start-filter.ps1 -Config config\default.toml
.\scripts\windows\status-filter.ps1
.\scripts\windows\enable-system-proxy.ps1
```

The enable script writes the pre-existing `ProxyEnable`, `ProxyServer`,
`ProxyOverride`, `AutoConfigURL`, and `AutoDetect` values to
`%LOCALAPPDATA%\LocalAIImageFilter\proxy-backup.json`. Re-running it does not
overwrite that original backup.

After the first start, trust the generated mitmproxy CA for the current user:

```powershell
.\scripts\windows\trust-mitm-ca.ps1
```

The script records the exact thumbprint and will not add a duplicate.

## Stop and restore

```powershell
.\scripts\windows\disable-system-proxy.ps1
.\scripts\windows\stop-filter.ps1
.\scripts\windows\remove-mitm-ca.ps1
```

Disable restores the original registry values, including absent values; CA
removal uses only the stored exact thumbprint. These scripts default to the
current user and do not request system-wide certificate changes.

Local capture is experimental: use `--mode local` or `--mode local:chrome` only
after regular mode works. Certificate-pinned applications can still fail or
bypass inspection.
