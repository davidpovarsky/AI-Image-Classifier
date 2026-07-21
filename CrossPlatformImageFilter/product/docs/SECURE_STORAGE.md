# Secure storage

Secrets are stored through a supervisor-only abstraction backed by DPAPI/credential protection on Windows, Keychain on macOS, and Secret Service with a root-owned encrypted fallback on Linux. The fallback requires restrictive ownership/mode checks and never stores plaintext.

Administrator passwords use Argon2id v19 with 64 MiB memory, three iterations, one lane, a random salt, and a 32-byte output. Verification is rate-limited with exponential lockout. Passwords and recovery authorizations are never command-line arguments or audit fields.
