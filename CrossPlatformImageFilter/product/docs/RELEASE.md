# Release

Internal CI builds real-model engine packages, supervisor services, shared desktop applications, native unsigned Windows/macOS/Linux installers, signed development updater artifacts, checksums, dependency reports, and a CycloneDX SBOM. Test keys are confined to private CI qualification and are rejected by the public-release gate.

A commercial release additionally requires a signed tag, protected environment, native installer jobs, platform signing/notarization credentials, updater key, production TUF root, signed model-distribution approval, matching model hashes, signed checksums, SBOM, provenance, and the manual lifecycle qualifications listed in `MANUAL_PLATFORM_QUALIFICATION.md`.

The release workflow is manual and fail-closed. It rejects the checked-in example legal approval, development policy/TUF keys, the development updater key, missing platform identities, and incomplete CI results. Exact protected-environment secret names are declared in the workflow. Public signing, notarization, updater publication, and release publication cannot execute until those external credentials and legal approvals are provisioned.
