# Licensing

The licensing crate defines a provider boundary for a mock provider and Keygen-compatible production adapter. A device generates an asymmetric keypair; activation binds signed claims to its public identity. The client never contains a provider administrator token.

Offline licenses are Ed25519-signed, device-bound, time-limited, and verified locally. Grace mode preserves filtering and network safety while management changes are restricted. Revocation does not abruptly strand proxy traffic: the supervisor retains safe filtering and guides renewal or protected deactivation.
