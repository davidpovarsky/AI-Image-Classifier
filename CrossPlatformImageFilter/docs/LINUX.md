# Linux

Use Python 3.12 with the pinned `onnxruntime==1.27.0`. The default package uses
the CPU provider; a separately managed compatible CUDA package can be selected
when installed, but is not part of the reproducible default bundle.

## GNOME regular proxy

```bash
local-image-filter doctor --config config/default.toml --load-models
local-image-filter run --config config/default.toml --mode regular
./scripts/linux/enable-system-proxy.sh
./scripts/linux/trust-mitm-ca.sh
```

When `gsettings` and GNOME proxy schemas are available, the enable script saves
the exact prior state and switches HTTP/HTTPS to loopback. Restore it with:

```bash
./scripts/linux/disable-system-proxy.sh
```

Linux environments are not assumed to use GNOME. For other desktops, configure
HTTP and HTTPS proxy settings manually or set application-scoped variables:

```bash
export http_proxy=http://127.0.0.1:8080
export https_proxy=http://127.0.0.1:8080
```

The CA helper installs into the current user's NSS database when supported. If
the application uses another trust store, follow that application's process.
Local capture is experimental and support varies by kernel, desktop, and
permissions.
