# Updates

Tauri updater handles application binaries with mandatory signature verification and a compiled allowlisted HTTPS endpoint. Production builds require the real updater public key; private keys remain release secrets.

Application, policy, and model update channels are separate. Models require provenance, hashes, tensor contracts, compatibility, licenses, regression results, signed targets, and legal approval. Every update stages and verifies before activation and retains a last-known-good version where platform packaging supports rollback.
