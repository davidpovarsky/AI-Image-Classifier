# Policy

Model adapters only decode evidence. They never decide whether an image is
blocked. The policy engine evaluates full-image NudeNet detections, person-crop
MobileCLIP scores, crop NudeNet detections, module status, and exact configured
thresholds.

The default successful block action is `blur`. `policy-balanced.toml` and
`policy-strict.toml` are small overlays that can be passed with `--config`; all
unspecified fields inherit from the wheel's packaged default.

NudeNet thresholds are keyed by the unmodified 18 raw labels. MobileCLIP uses
the pinned four-class prompt contract and configurable score/margin rules.
Same-label boxes are merged transitively at the configured IoU while preserving
module and crop provenance.

Pipeline/module errors are distinct from a policy block. `proxy.fail_action`
chooses `allow`, `blur`, or `replace` for failure. The shipped default is
`replace`, because passing an unclassified image through is unsafe. Select
`allow` only as an explicit availability tradeoff.

Use mock configuration to test mechanics, not classification quality:

```bash
local-image-filter classify tests/fixtures/example.png --config config/mock.toml
```

Every decision is based only on visual classification. No page text, URL,
website category, identity database, or cloud result participates.
