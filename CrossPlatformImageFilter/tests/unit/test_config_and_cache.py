import threading
from pathlib import Path

import pytest

from local_image_filter.cache.store import AnalysisCache
from local_image_filter.config import ConfigurationError, load_settings
from local_image_filter.domain.models import FilterAction, FilterDecision


def cache(path: Path, namespace: str = "one", ttl: int = 60) -> AnalysisCache:
    return AnalysisCache(
        True,
        4,
        path,
        ttl,
        namespace=namespace,
        policy_fingerprint="policy",
        manifest_fingerprint="manifest",
    )


def test_packaged_default_works_from_arbitrary_directory(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    monkeypatch.delenv("LOCAL_IMAGE_FILTER_CONFIG", raising=False)
    settings = load_settings()
    assert settings.source_path is None
    assert settings.section("proxy")["fail_action"] == "replace"
    assert settings.project_root == tmp_path


def test_environment_config_and_overlay_precedence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = tmp_path / "config.toml"
    overlay = tmp_path / "overlay.toml"
    config.write_text("[proxy]\nlisten_port = 9000\n", encoding="utf-8")
    overlay.write_text("[proxy]\nlisten_port = 9001\n", encoding="utf-8")
    monkeypatch.setenv("LOCAL_IMAGE_FILTER_CONFIG", str(config))
    assert load_settings(overlays=[overlay]).section("proxy")["listen_port"] == 9001


@pytest.mark.parametrize(
    "contents,message",
    [
        ("[proxy]\nfail_action='invalid'\n", "proxy.fail_action"),
        ("[processing]\nmaximum_dimension=0\n", "maximum_dimension"),
        ("[processing]\nhorizontal_crop_padding=2\n", "horizontal_crop_padding"),
    ],
)
def test_invalid_config_is_rejected(tmp_path: Path, contents: str, message: str) -> None:
    path = tmp_path / "invalid.toml"
    path.write_text(contents, encoding="utf-8")
    with pytest.raises(ConfigurationError, match=message):
        load_settings(path)


def test_memory_and_sqlite_hit_clear_and_stats(tmp_path: Path) -> None:
    first = cache(tmp_path / "cache.sqlite3")
    decision = FilterDecision(FilterAction.ALLOW, ("safe",))
    first.put("hash", {"ok": True}, decision)
    assert first.get("hash") is not None
    second = cache(tmp_path / "cache.sqlite3")
    assert second.get("hash").response == {"ok": True}
    assert second.stats()["sqliteEntries"] == 1
    assert second.clear() == 1
    assert second.get("hash") is None


def test_namespace_change_invalidates_cache(tmp_path: Path) -> None:
    first = cache(tmp_path / "cache.sqlite3", "one")
    first.put("hash", {"ok": True}, FilterDecision(FilterAction.ALLOW, ("safe",)))
    assert cache(tmp_path / "cache.sqlite3", "two").get("hash") is None


def test_corrupted_row_is_deleted(tmp_path: Path) -> None:
    store = cache(tmp_path / "cache.sqlite3")
    store.put("hash", {"ok": True}, FilterDecision(FilterAction.ALLOW, ("safe",)))
    with store._connect() as connection:
        connection.execute(
            "UPDATE analysis_cache_v2 SET response_json=? WHERE content_hash=?",
            ("not-json", "hash"),
        )
    store._memory.clear()
    assert store.get("hash") is None


def test_concurrent_reads_and_writes(tmp_path: Path) -> None:
    store = cache(tmp_path / "cache.sqlite3")
    errors: list[Exception] = []

    def worker(index: int) -> None:
        try:
            decision = FilterDecision(FilterAction.ALLOW, ("safe",))
            store.put(f"hash-{index}", {"index": index}, decision)
            assert store.get(f"hash-{index}") is not None
        except Exception as error:
            errors.append(error)

    threads = [threading.Thread(target=worker, args=(index,)) for index in range(12)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert not errors
    assert store.stats()["sqliteEntries"] == 12
