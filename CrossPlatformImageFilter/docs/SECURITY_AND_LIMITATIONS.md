# Security and limitations

## Local TLS interception

There is no cloud service or model server. Trusting a mitmproxy CA still gives
the local proxy the ability to decrypt traffic sent through it. Treat the CA
private key as sensitive, use the current-user store/keychain, never copy it,
and remove only the recorded fingerprint during uninstall.

The proxy binds to loopback by default. Do not expose it on a LAN. Diagnostics
remove URL-like fields recursively and never store image bytes or crops.

## Coverage

This is an image-response filter only. It cannot guarantee coverage for:

- certificate-pinned applications, which may bypass or reject interception;
- applications that ignore OS proxy settings;
- QUIC/HTTP3 or other paths not routed through the proxy;
- images already in browser/application caches;
- local `canvas`, `blob:`, or generated data;
- DRM, PDFs, video, audio, text, DNS, or site categories;
- SVG, which is fail-closed instead of rasterized;
- animation beyond configured behavior (default: replace).

Range responses, oversized or corrupt images, and unsupported image types are
`failClosedImage`, never silently passed through as safe images.

## Accuracy and policy

All decisions use visual classification. Models can miss people or nudity and
can misclassify presentation, illustrations, children, occlusion, or groups.
Scores are evidence, not truth. Do not use this tool to infer gender identity
or biological sex.

Partial failures remain visible in module reports. The default runtime failure
action is replacement; successful policy blocks default to blur. Explicit
fail-open settings trade safety for availability.

## Operational boundary

Version 0.1 is a user-space proxy. It has no GUI, service/daemon, installer,
administrator policy, password, or anti-tamper protection. A user can stop it,
change settings, remove trust, or bypass it.
