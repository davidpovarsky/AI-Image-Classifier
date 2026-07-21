# Windows installer

The release path is a per-machine WiX/MSI or signed bootstrapper that installs the Tauri UI, supervisor service, PyInstaller onedir engine, legally approved models, production policy/updater trust roots, and protected uninstaller. All binaries and the installer are Authenticode-signed in a protected environment.

MSIX/Microsoft Store distribution requires declaring the service/full-trust capabilities accepted by Store policy; direct distribution uses an EV/organization code-signing certificate. Internal CI may emit unsigned artifacts clearly marked non-production.
