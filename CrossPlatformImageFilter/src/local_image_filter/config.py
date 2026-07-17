from __future__ import annotations

import copy
import os
import tomllib
from dataclasses import dataclass
from importlib import resources
from pathlib import Path
from typing import Any


class ConfigurationError(ValueError):
    pass


def _merge(base: dict[str, Any], overlay: dict[str, Any]) -> dict[str, Any]:
    result = copy.deepcopy(base)
    for key, value in overlay.items():
        if isinstance(value, dict) and isinstance(result.get(key), dict):
            result[key] = _merge(result[key], value)
        else:
            result[key] = copy.deepcopy(value)
    return result


def _expanded(path: str, root: Path) -> Path:
    value = os.path.expandvars(os.path.expanduser(path))
    candidate = Path(value)
    return candidate.resolve() if candidate.is_absolute() else (root / candidate).resolve()


def _read_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as stream:
            return tomllib.load(stream)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ConfigurationError(f"Unable to load TOML configuration {path}: {error}") from error


def _packaged_default() -> dict[str, Any]:
    resource = resources.files("local_image_filter.resources").joinpath("default.toml")
    try:
        return tomllib.loads(resource.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ConfigurationError(
            f"Unable to load packaged default configuration: {error}"
        ) from error


@dataclass(frozen=True, slots=True)
class Settings:
    data: dict[str, Any]
    source_path: Path | None
    source_description: str
    project_root: Path

    def section(self, name: str) -> dict[str, Any]:
        value = self.data.get(name, {})
        if not isinstance(value, dict):
            raise ConfigurationError(f"Section {name!r} must be a table")
        return value

    def path(self, dotted_key: str) -> Path:
        value: Any = self.data
        for part in dotted_key.split("."):
            if not isinstance(value, dict) or part not in value:
                raise ConfigurationError(f"Missing path setting: {dotted_key}")
            value = value[part]
        if not isinstance(value, str):
            raise ConfigurationError(f"Path setting {dotted_key} must be a string")
        return _expanded(value, self.project_root)

    def to_dict(self) -> dict[str, Any]:
        return copy.deepcopy(self.data)


def load_settings(
    path: str | Path | None = None,
    overlays: list[str | Path] | None = None,
) -> Settings:
    selected: Path | None
    if path is not None and str(path).strip():
        selected = Path(path).expanduser().resolve()
    elif os.environ.get("LOCAL_IMAGE_FILTER_CONFIG"):
        selected = Path(os.environ["LOCAL_IMAGE_FILTER_CONFIG"]).expanduser().resolve()
    else:
        selected = None

    if selected is None:
        data = _packaged_default()
        configured_home = os.environ.get("LOCAL_IMAGE_FILTER_HOME")
        project_root = (
            Path(configured_home).expanduser().resolve()
            if configured_home
            else Path.cwd().resolve()
        )
        description = "package:local_image_filter.resources/default.toml"
    else:
        data = _merge(_packaged_default(), _read_toml(selected))
        project_root = (
            selected.parent.parent if selected.parent.name == "config" else selected.parent
        )
        description = str(selected)

    for overlay_path in overlays or []:
        overlay = Path(overlay_path).expanduser().resolve()
        data = _merge(data, _read_toml(overlay))

    settings = Settings(
        data=data,
        source_path=selected,
        source_description=description,
        project_root=project_root,
    )
    validate_settings(settings)
    return settings


def validate_settings(settings: Settings) -> None:
    action_values = {"allow", "blur", "replace", "error"}
    proxy = settings.section("proxy")
    policy = settings.section("policy")
    for key, value in {
        "proxy.fail_action": proxy.get("fail_action"),
        "policy.block_action": policy.get("block_action"),
    }.items():
        if value not in action_values:
            raise ConfigurationError(f"{key} must be one of {sorted(action_values)}")

    processing = settings.section("processing")
    positive_values = {
        "processing.maximum_person_crops": processing.get("maximum_person_crops"),
        "processing.maximum_dimension": processing.get("maximum_dimension"),
        "processing.maximum_total_pixels": processing.get("maximum_total_pixels"),
        "proxy.max_parallel_images": proxy.get("max_parallel_images"),
        "runtime.max_concurrent_inference": settings.section("runtime").get(
            "max_concurrent_inference"
        ),
    }
    for key, value in positive_values.items():
        if not isinstance(value, int) or isinstance(value, bool) or value < 1:
            raise ConfigurationError(f"{key} must be a positive integer")

    for key in ("horizontal_crop_padding", "vertical_crop_padding"):
        value = processing.get(key)
        if not isinstance(value, (int, float)) or isinstance(value, bool) or not 0 <= value <= 1:
            raise ConfigurationError(f"processing.{key} must be between 0 and 1")

    if processing.get("animated_image_mode") not in {"replace", "sample"}:
        raise ConfigurationError("processing.animated_image_mode must be replace or sample")
    if proxy.get("mode") not in {"regular", "local"} and not str(proxy.get("mode", "")).startswith(
        "local:"
    ):
        raise ConfigurationError("proxy.mode must be regular, local, or local:<process>")

    for model_name in ("person", "mobileclip", "nudenet"):
        model = settings.section("models").get(model_name)
        if not isinstance(model, dict) or not isinstance(model.get("path"), str):
            raise ConfigurationError(f"models.{model_name}.path must be configured")
