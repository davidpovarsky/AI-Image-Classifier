# Signed policy bundles

Bundles follow `policy/schemas/policy-bundle.schema.json`, use RFC 8785 canonical JSON, and carry an Ed25519 signature. Only filtering and processing allowlist fields are accepted. Paths, executables, IPC endpoints, service identity, environment variables, shell commands, arbitrary proxy targets, trust keys, and uncompiled update endpoints are forbidden.

Production precedence is immutable packaged defaults, signed vendor policy, signed tenant policy, signed device policy, then non-security UI preferences. Revision rollback, future/expired metadata, unknown keys, wrong devices, unsupported algorithms, duplicates, non-finite numbers, oversized files, unexpected fields, and invalid ranges fail closed.

TUF supplies root rotation, timestamp/snapshot/targets metadata, consistent snapshots, expiry, target size/hash verification, and rollback protection. Bundle signatures remain defense in depth and support offline import. The checked-in root is an intentionally unusable placeholder until production public keys are supplied.
