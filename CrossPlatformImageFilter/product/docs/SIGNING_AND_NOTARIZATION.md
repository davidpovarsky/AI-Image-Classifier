# Signing and notarization

Private signing keys exist only in protected release environments. Windows signs every executable and installer for direct distribution; an MSIX/Store route is documented separately. macOS signs nested engine binaries before the app/helper/package, enables hardened runtime, submits with `notarytool`, checks the notarization log, and staples the ticket.

Missing credentials produce a precise release-job failure. They are never replaced with test keys in production mode.
