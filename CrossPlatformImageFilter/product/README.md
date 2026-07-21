# Commercial desktop product

This additive workspace turns the existing local Python/ONNX image filter into a service-owned desktop product for Windows x64, macOS universal distributions, and Linux x64. The shared React UI runs in an unprivileged Tauri process. Privileged lifecycle changes belong to a Rust supervisor reached only through authenticated local IPC.

The repository contains no production signing secret, policy private key, license-provider administrator token, certificate private key, or model-distribution approval. Development builds and explicitly unsigned internal qualification archives are possible; native signed installers, public model redistribution, and signed releases remain blocked until those external materials and the documented platform qualifications are provisioned.

See [commercial architecture](docs/COMMERCIAL_ARCHITECTURE.md), [threat model](docs/THREAT_MODEL.md), [release process](docs/RELEASE.md), and [manual qualification](docs/MANUAL_PLATFORM_QUALIFICATION.md).

The `services/supervisor` binary is the service entry point. It exposes only
the framed methods in `supervisor-ipc`, validates peer process identity where
the OS provides it, applies timestamp and nonce replay checks, and requires a
short-lived caller- and scope-bound authorization for protected operations.
On Windows it dispatches through the Service Control Manager and creates an
explicitly secured named pipe; on macOS and Linux it uses a mode-restricted
Unix-domain socket.

The packaged Python engine invokes its bundled mitmproxy runtime directly and
does not require system Python. Model weights remain separate, versioned
resources and continue to be verified through the existing runtime manifest.
