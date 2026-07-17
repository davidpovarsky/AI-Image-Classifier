from pathlib import Path

from PIL import Image

from local_image_filter.config import load_settings
from local_image_filter.runtime.container import build_runtime


def test_mock_pipeline(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[1]
    settings = load_settings(root / "config" / "default.toml")
    settings.data["runtime"]["mock_models"] = True
    settings.data["cache"]["sqlite_path"] = str(tmp_path / "cache.sqlite3")
    settings.data["diagnostics"]["directory"] = str(tmp_path / "diagnostics")
    image_path = tmp_path / "sample.png"
    Image.new("RGB", (128, 128), "white").save(image_path)
    runtime = build_runtime(settings)
    outcome = runtime.pipeline.filter_bytes(image_path.read_bytes(), "image/png")
    assert outcome.decision.action.value == "allow"
    assert outcome.response["pipeline"]["status"] == "success"
