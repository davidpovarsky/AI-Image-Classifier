# Installers

The repository currently produces an explicitly marked unsigned internal product archive on Windows, macOS, and Linux CI. The archive combines the self-contained engine, Tauri binary, supervisor, `filterctl`, platform installation metadata, trusted policy root, and base policy. It is a qualification artifact, not a signed public installer.

The intended production routes are a per-machine signed Windows installer, a hardened and notarized macOS app/package using `notarytool`, and signed deb/rpm packages around the hardened systemd unit. Those public routes remain release-blocked until the platform signing credentials, production updater key, production TUF root, approved model resources, and platform lifecycle qualification are supplied.

The checked-in Windows, macOS, and Linux lifecycle scripts are reviewed inputs for the eventual installers. They do not enable capture during installation. A production installer must stage files before system changes, verify hashes and signatures, register the service, complete health checks, and invoke exact proxy/certificate recovery on failure. Completion of those native installer integrations is tracked as a release blocker; the internal archive must not be presented to end users as installed-product proof.
