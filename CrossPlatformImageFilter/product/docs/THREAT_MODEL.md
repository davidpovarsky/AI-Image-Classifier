# Threat model

Protected assets are the administrator verifier, device private key, CA private key, license material, policy and updater trust roots, proxy snapshot, engine/model integrity, and audit chain. Adversaries include an unprivileged local user, a malicious webpage, a network attacker, a replaying client, a compromised policy CDN, and accidental installer interruption. A machine administrator remains ultimately in control.

Controls include OS ACLs and service identity, peer-checked local sockets/named pipes, bounded typed messages, nonces and replay tracking, Argon2id and exponential lockout, OS secure storage, Ed25519 and TUF verification, monotonic versions, atomic last-known-good activation, exact certificate fingerprints, proxy rollback, signed updates, redacted diagnostics, and a model-license release gate.

Residual risks include certificate-pinned clients, browser cache and `blob:`/canvas content, probabilistic model errors, platform-specific interception gaps, forced removal by an administrator, and credentials/signing infrastructure not present in Git.
