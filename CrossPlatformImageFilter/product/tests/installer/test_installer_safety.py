from pathlib import Path


def test_linux_unit_is_hardened_and_capture_is_not_enabled_by_installer() -> None:
    root = Path(__file__).resolve().parents[3]
    unit = (root / "product/installers/linux/local-ai-image-filter.service").read_text()
    postinstall = (root / "product/installers/linux/postinst").read_text()
    assert "NoNewPrivileges=yes" in unit
    assert "ProtectSystem=strict" in unit
    assert "enable capture" not in postinstall.lower()


def test_uninstaller_never_accepts_plaintext_password_argument() -> None:
    root = Path(__file__).resolve().parents[3]
    scripts = [
        root / "product/installers/windows/uninstall-service.ps1",
        root / "product/installers/linux/prerm",
    ]
    for script in scripts:
        source = script.read_text(encoding="utf-8")
        assert "param([string]$Password" not in source
        assert "--password" not in source
