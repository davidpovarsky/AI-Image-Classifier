# Diagnostics schema

Diagnostics are rotating JSONL plus an atomically replaced `summary.json`.
They contain decisions and timings, not content. URL/host/request/header-like
keys are recursively redacted, even when nested. Image bytes, crops, prompt
inputs, and model tensors are never persisted.

An event can include:

```json
{
  "timestamp": "RFC3339 UTC",
  "contentHash": "SHA-256",
  "decision": "allow|blur|replace|error",
  "cacheHit": false,
  "providers": {"person": "CPUExecutionProvider"},
  "moduleDurationsMs": {"personDetection": 12.5},
  "moduleFailures": {"nudeNetFullImage": "message"},
  "outputFormat": "PNG"
}
```

The summary aggregates total processed/allowed/blurred/replaced/errors, cache
hit rate, average duration per module, failure counts, provider selection, and
the last sanitized error per module. It is operational evidence, not a record
of browsed URLs.

```bash
local-image-filter diagnostics summary --config config/default.toml
```

Rotation is controlled by `diagnostics.max_jsonl_bytes`. Setting
`diagnostics.enabled=false` disables event writes. `store_urls`,
`store_image_bytes`, and `store_crops` must remain false.
