from pathlib import Path

from local_image_filter.config import load_settings


def test_default_config_loads() -> None:
    root = Path(__file__).resolve().parents[1]
    settings = load_settings(root / "config" / "default.toml")
    assert settings.section("proxy")["listen_port"] == 8080
    assert settings.path("models.mobileclip.path").name.endswith(".onnx")
