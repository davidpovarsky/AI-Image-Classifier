# macOS daemon

The supervisor is a signed, hardened launch daemon or supported privileged helper installed through Apple-supported service-management/installer mechanisms. IPC uses a root-owned Unix-domain socket with restrictive mode and audit-token/code-signature peer validation.

Keychain stores secrets. System proxy and trust changes require explicit consent and exact rollback. A future native NetworkExtension remains behind the capture trait and requires its own entitlement/review; no private API is used.
