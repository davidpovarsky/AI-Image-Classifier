# External credentials and approvals

The software paths are fail-closed. Private CI uses generated development signing material; production mode never accepts it.

## Control plane runtime

- `CONTROL_PLANE_MODE=production` and `CONTROL_PLANE_DEVELOPMENT_AUTH=false` select production authentication.
- `CONTROL_PLANE_ENROLLMENT_TOKEN`: random one-time enrollment secret, at least 32 characters, supplied through the deployment secret manager.
- `CONTROL_PLANE_DATABASE_URL`: PostgreSQL connection URI with `sslmode=require` or `sslmode=verify-full`.
- `CONTROL_PLANE_OIDC_ISSUER`: exact HTTPS issuer claim.
- `CONTROL_PLANE_OIDC_AUDIENCE`: exact API audience claim.
- `CONTROL_PLANE_OIDC_JWKS_URL`: HTTPS JWKS endpoint for the issuer.
- `CONTROL_PLANE_KEYGEN_ACCOUNT_ID`: Keygen account identifier.
- `CONTROL_PLANE_KEYGEN_ED25519_PUBLIC_KEY_BASE64`: raw 32-byte Keygen webhook public key encoded as standard Base64.
- `CONTROL_PLANE_PUBLIC_HOST`: lowercase public webhook host, without scheme or path.
- `CONTROL_PLANE_SIGNER_URL`: HTTPS KMS/HSM signing-gateway endpoint.
- `CONTROL_PLANE_SIGNER_BEARER_TOKEN`: signer-gateway bearer credential.
- `CONTROL_PLANE_POLICY_SIGNING_KEY_ID`: public identifier of the Ed25519 assignment key.
- `CONTROL_PLANE_POLICY_SIGNING_PUBLIC_KEY_BASE64`: raw 32-byte assignment public key encoded as standard Base64.

## Policy publication

- `POLICY_TRUSTED_KEYS_JSON`: JSON object mapping approved bundle key IDs to standard-Base64 Ed25519 public keys.
- `POLICY_PUBLISH_ENDPOINT`: allowlisted HTTPS control-plane origin.
- `POLICY_PUBLISH_TOKEN`: short-lived production OIDC access token accepted by the policy publisher.
- `PRODUCTION_TUF_ROOT_JSON`: signed TUF root JSON that contains no development key ID.

## Public release environment

The `commercial-release` GitHub Environment must hold:

- `MODEL_DISTRIBUTION_APPROVAL_JSON`, `MODEL_APPROVAL_KEY_ID`, and `MODEL_APPROVAL_PUBLIC_KEY_BASE64` for the signed per-model redistribution approval.
- `WINDOWS_SIGNING_CERTIFICATE_BASE64`: Base64 PKCS#12 code-signing identity. A signing job also requires its password and trusted RFC 3161 timestamp URL before public Windows publication.
- `MACOS_SIGNING_IDENTITY`: Apple Developer ID Application identity. A signing job also requires the certificate/keychain import material and App Store Connect issuer, key ID, and `.p8` API key before notarization.
- `APPLE_NOTARY_KEY`: protected App Store Connect notarization key material.
- `TAURI_SIGNING_PRIVATE_KEY`: minisign-compatible Tauri updater private key; `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is required when encrypted.
- `PRODUCTION_TUF_ROOT_JSON`: production signed TUF root.

The absence of any item above is an external credential boundary, not a reason to substitute a test key in a public artifact.
