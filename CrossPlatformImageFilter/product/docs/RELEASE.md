# Release

Internal CI builds unsigned engine, desktop, supervisor, and combined qualification archives with mock licensing and test policy keys. It also produces checksums and a CycloneDX SBOM. These archives are not native signed installers and are not redistributable commercial releases.

A commercial release additionally requires a signed tag, protected environment, native installer jobs, platform signing/notarization credentials, updater key, production TUF root, signed model-distribution approval, matching model hashes, signed checksums, SBOM, provenance, and the manual lifecycle qualifications listed in `MANUAL_PLATFORM_QUALIFICATION.md`.

The release workflow is manual and fail-closed. Its current release-gate job rejects the checked-in example legal approval, development keys, placeholder updater key, and placeholder TUF root. Credential-gated native signing, notarization, deb/rpm, MSI/NSIS/MSIX, updater publication, and release publication jobs still need to be completed and exercised after real release infrastructure is provisioned.
