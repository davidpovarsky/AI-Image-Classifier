# Process boundaries

The WebView owns presentation only and receives true supervisor state. Tauri commands translate a small allowlisted operation set into versioned IPC. They cannot manipulate proxy, certificates, services, policy files, password hashes, or executables.

The supervisor is the sole privileged authority. The Python engine owns image eligibility, decoding, local inference, evidence merging, policy evaluation, rewriting, caching, and privacy-preserving inference diagnostics. The control plane cannot receive content or classification data.
