# Proxy and certificate lifecycle

The safe lifecycle is ordered and reversible:

1. Verify models and configuration with `doctor --load-models`.
2. Start the local filter and confirm its status.
3. Save and enable system proxy settings.
4. Trust the generated CA in the current-user store.
5. Test an HTTP and HTTPS image through the proxy.
6. During shutdown, restore proxy settings before or together with stopping.
7. On uninstall, remove only the recorded CA fingerprint.

Regular mode is the default. `ignore_hosts` is passed to mitmproxy at startup
for pinned or incompatible destinations. Entries should be narrow host regular
expressions; ignoring a host means its images are not classified.

Eligibility has three states:

- `ignore`: definitely not an image response, so bytes are untouched;
- `inspect`: supported image bytes enter the pipeline;
- `failClosedImage`: it appears to be an image but cannot be safely analyzed,
  such as a range, oversized, unsupported, animated (by policy), or corrupt body.

Rewritten responses remove stale compression, range, validator, and length
headers; set the new content type/length; and use `Cache-Control: no-store`.
Failure placeholders are valid PNG responses with HTTP 200, preventing broken
image icons and preventing accidental passthrough of unclassified bytes.

Never expose the listener beyond loopback unless the threat model, access
control, and CA handling have been redesigned.
