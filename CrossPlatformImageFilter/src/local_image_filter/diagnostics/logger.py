from __future__ import annotations

import json
import os
import threading
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

PRIVATE_KEYS = {
    "authorization",
    "certificatePrivateKey",
    "cookies",
    "crops",
    "host",
    "imageBytes",
    "proxyCredentials",
    "query",
    "url",
}


def _scrub(value: Any, *, store_urls: bool) -> Any:
    if isinstance(value, dict):
        return {
            key: _scrub(item, store_urls=store_urls)
            for key, item in value.items()
            if key not in PRIVATE_KEYS or (store_urls and key == "url")
        }
    if isinstance(value, list):
        return [_scrub(item, store_urls=store_urls) for item in value]
    return value


class DiagnosticsLogger:
    def __init__(self, config: dict[str, Any], directory: Path) -> None:
        self.enabled = bool(config.get("enabled", True))
        self.directory = directory
        self.max_jsonl_bytes = int(config.get("max_jsonl_bytes", 25_000_000))
        self.store_urls = bool(config.get("store_urls", False))
        self._lock = threading.Lock()
        self._summary: dict[str, Any] = {
            "totalInspected": 0,
            "totalAllowed": 0,
            "totalBlurred": 0,
            "totalReplaced": 0,
            "cacheHits": 0,
            "moduleDurations": {},
            "moduleFailureCounts": {},
            "providerSelection": {},
            "lastErrorByModule": {},
        }
        if self.enabled:
            directory.mkdir(parents=True, exist_ok=True)
            summary_path = directory / "image-filter-summary.json"
            if summary_path.is_file():
                try:
                    loaded = json.loads(summary_path.read_text(encoding="utf-8"))
                    if isinstance(loaded, dict):
                        self._summary.update(loaded)
                except (OSError, json.JSONDecodeError):
                    pass

    def append(self, event: dict[str, Any]) -> None:
        if not self.enabled:
            return
        safe = _scrub(dict(event), store_urls=self.store_urls)
        safe.setdefault("timestamp", datetime.now(UTC).isoformat())
        path = self.directory / "image-filter-events.jsonl"
        with self._lock:
            if path.exists() and path.stat().st_size >= self.max_jsonl_bytes:
                rotated = self.directory / "image-filter-events.1.jsonl"
                if rotated.exists():
                    rotated.unlink()
                os.replace(path, rotated)
            with path.open("a", encoding="utf-8") as stream:
                stream.write(json.dumps(safe, ensure_ascii=False, separators=(",", ":")) + "\n")
            self._update_summary(safe)
            self._write_summary()

    def _update_summary(self, event: dict[str, Any]) -> None:
        if event.get("event") == "imageAnalysisCacheHit":
            self._summary["cacheHits"] = int(self._summary.get("cacheHits", 0)) + 1
            return
        if event.get("event") != "imageAnalysisCompleted":
            return
        self._summary["totalInspected"] = int(self._summary.get("totalInspected", 0)) + 1
        decision = event.get("decision", {})
        action_value = decision.get("action") if isinstance(decision, dict) else None
        action = action_value if isinstance(action_value, str) else ""
        action_key = {
            "allow": "totalAllowed",
            "blur": "totalBlurred",
            "replace": "totalReplaced",
        }.get(action)
        if action_key:
            self._summary[action_key] = int(self._summary.get(action_key, 0)) + 1
        modules = event.get("modules", {})
        if isinstance(modules, dict):
            durations = self._summary.setdefault("moduleDurations", {})
            failures = self._summary.setdefault("moduleFailureCounts", {})
            last_errors = self._summary.setdefault("lastErrorByModule", {})
            for name, report in modules.items():
                if not isinstance(report, dict):
                    continue
                duration = int(report.get("durationMs", 0))
                item = durations.setdefault(name, {"totalMs": 0, "count": 0})
                item["totalMs"] = int(item.get("totalMs", 0)) + duration
                item["count"] = int(item.get("count", 0)) + 1
                item["averageMs"] = round(item["totalMs"] / item["count"], 2)
                if report.get("status") == "failed":
                    failures[name] = int(failures.get(name, 0)) + 1
                    last_errors[name] = report.get("error")
        providers = event.get("providerSelection")
        if isinstance(providers, dict):
            self._summary["providerSelection"] = providers

    def _write_summary(self) -> None:
        total = int(self._summary.get("totalInspected", 0))
        hits = int(self._summary.get("cacheHits", 0))
        self._summary["cacheHitRate"] = round(hits / (total + hits), 4) if total + hits else 0.0
        self._summary["updatedAt"] = datetime.now(UTC).isoformat()
        path = self.directory / "image-filter-summary.json"
        temporary = path.with_suffix(".json.tmp")
        temporary.write_text(
            json.dumps(self._summary, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        os.replace(temporary, path)

    def summary(self) -> dict[str, Any]:
        with self._lock:
            return json.loads(json.dumps(self._summary))
