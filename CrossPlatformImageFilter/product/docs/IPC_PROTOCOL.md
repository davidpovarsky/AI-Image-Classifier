# IPC protocol

Protocol version 1 uses one bounded JSON envelope per local connection: `protocolVersion`, UUID `requestId`, a 32–256 character nonce, and a tagged request from the compile-time allowlist. Maximum request and response size is 64 KiB. There is no execute, shell, filesystem, environment, or arbitrary command request.

Windows uses a local named pipe and Unix systems use a Unix-domain/local socket. The service must validate peer OS identity, executable identity, ACL membership, nonce freshness, operation authorization, and message size before dispatch. Authorization values are never accepted on a command line or logged.
