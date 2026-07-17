# Integration with the existing iOS repository

The repository is upstream-derived. The desktop implementation is additive and
lives in the sibling `CrossPlatformImageFilter/` folder. It is not part of the
Xcode synchronized root group and requires no Xcode reference, target, package,
entitlement, or build-setting change.

The only repository-level additions are the separate desktop workflow and a
short README link. The Swift pipeline, Xcode project, iOS workflow, MobileCLIP
converter, and model download script remain unchanged.

Shared behavior is checked without coupling the runtimes:

- `check_mobileclip_contract.py` compares checkpoint identity, image contract,
  classes, and prompts with the existing iOS converter source.
- `check_swift_schema.py` compares required response fields with Swift models.
- desktop CI exports ONNX separately and never inserts it into the iOS target.
- the existing iOS workflow is run after desktop CI as a regression check.

This isolates future upstream merges. New desktop features should remain below
the sibling directory. Modify upstream-owned files only for a narrow, documented
bridge that cannot be expressed as a source-level parity check.
